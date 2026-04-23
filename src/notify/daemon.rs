/// Notification daemon D-Bus service.
///
/// Claims `org.freedesktop.Notifications` on the session bus and handles
/// incoming notification requests from applications.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::time::SystemTime;

use zbus::object_server::SignalEmitter;
use zbus::{connection, interface};
use zbus::zvariant::OwnedValue;

use super::types::{Notification, NotifyEvent};

/// The notification daemon D-Bus object, served at /org/freedesktop/Notifications.
pub struct NotificationDaemon {
    next_id: Arc<Mutex<u32>>,
    tx: mpsc::Sender<NotifyEvent>,
    /// Maps x-canonical-private-synchronous hint values to notification IDs,
    /// so that volume/brightness notifications replace the previous one.
    sync_keys: Arc<Mutex<HashMap<String, u32>>>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationDaemon {
    /// Returns the capabilities of this notification server.
    fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".to_string(),
            "body-hyperlinks".to_string(),
            "body-images".to_string(),
            "body-markup".to_string(),
            "icon-static".to_string(),
            "persistence".to_string(),
            "actions".to_string(),
        ]
    }

    /// Sends a notification to the server.
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        // Check for x-canonical-private-synchronous hint (used by volume/brightness scripts).
        // If present, reuse the previous notification ID for that sync key.
        let sync_key = hints.get("x-canonical-private-synchronous")
            .and_then(|v| <&str>::try_from(v).ok())
            .map(|s| s.to_string());

        let effective_replaces = if replaces_id > 0 {
            replaces_id
        } else if let Some(ref key) = sync_key {
            let sync_keys = self.sync_keys.lock().unwrap();
            sync_keys.get(key).copied().unwrap_or(0)
        } else {
            0
        };

        let id = if effective_replaces > 0 {
            effective_replaces
        } else {
            let mut next = self.next_id.lock().unwrap();
            let id = *next;
            *next = next.wrapping_add(1);
            if *next == 0 {
                *next = 1;
            }
            id
        };

        // Update sync key mapping
        if let Some(key) = sync_key {
            self.sync_keys.lock().unwrap().insert(key, id);
        }

        // Parse actions from flat [key, label, key, label, ...] into pairs
        let action_pairs: Vec<(String, String)> = actions
            .chunks(2)
            .filter_map(|chunk| {
                if chunk.len() == 2 {
                    Some((chunk[0].clone(), chunk[1].clone()))
                } else {
                    None
                }
            })
            .collect();

        let notification = Notification {
            id,
            app_name: app_name.to_string(),
            app_icon: app_icon.to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            actions: action_pairs,
            hints,
            expire_timeout,
            timestamp: SystemTime::now(),
        };

        log::info!(
            "Notification #{}: [{}] {} - {}",
            id,
            notification.app_name,
            notification.summary,
            notification.body
        );

        let _ = self.tx.send(NotifyEvent::New(notification));
        id
    }

    /// Closes a notification by ID.
    fn close_notification(&self, id: u32) {
        log::info!("CloseNotification #{}", id);
        let _ = self.tx.send(NotifyEvent::Close(id));
    }

    /// Returns server information.
    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "i3more".to_string(),
            "i3more".to_string(),
            "0.1.0".to_string(),
            "1.2".to_string(),
        )
    }

    /// Signal emitted when a notification is closed.
    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    /// Signal emitted when a notification action is invoked.
    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

/// Start the notification daemon on a background thread.
/// Returns an action sender for emitting ActionInvoked D-Bus signals
/// and a close sender for emitting NotificationClosed D-Bus signals.
pub fn start_notification_daemon(
    tx: mpsc::Sender<NotifyEvent>,
) -> (mpsc::Sender<(u32, String)>, mpsc::Sender<u32>) {
    let (action_tx, action_rx) = mpsc::channel::<(u32, String)>();
    let (close_signal_tx, close_signal_rx) = mpsc::channel::<u32>();
    std::thread::spawn(move || {
        async_io::block_on(async {
            if let Err(e) = run_daemon(tx, action_rx, close_signal_rx).await {
                log::error!("Notification daemon failed: {}", e);
            }
        });
    });
    (action_tx, close_signal_tx)
}

async fn run_daemon(
    tx: mpsc::Sender<NotifyEvent>,
    action_rx: mpsc::Receiver<(u32, String)>,
    close_signal_rx: mpsc::Receiver<u32>,
) -> zbus::Result<()> {
    let daemon = NotificationDaemon {
        next_id: Arc::new(Mutex::new(1)),
        tx,
        sync_keys: Arc::new(Mutex::new(HashMap::new())),
    };

    let conn = connection::Builder::session()?
        .name("org.freedesktop.Notifications")?
        .serve_at("/org/freedesktop/Notifications", daemon)?
        .build()
        .await?;

    log::info!("Notification daemon active on session bus");

    // Poll for action invocations and emit D-Bus signals
    loop {
        if i3more::shutdown_requested() {
            log::info!("Notification daemon: shutdown requested, exiting");
            break Ok(());
        }
        async_io::Timer::after(std::time::Duration::from_millis(50)).await;
        while let Ok((id, action_key)) = action_rx.try_recv() {
            log::info!("ActionInvoked #{}: {}", id, action_key);
            let iface_ref = conn
                .object_server()
                .interface::<_, NotificationDaemon>("/org/freedesktop/Notifications")
                .await
                .expect("interface not found");
            let emitter = iface_ref.signal_emitter();
            if let Err(e) = NotificationDaemon::action_invoked(&emitter, id, &action_key).await {
                log::error!("Failed to emit ActionInvoked signal: {}", e);
            }
        }

        // Emit NotificationClosed for dismissed popups (enables notify-send --wait)
        while let Ok(id) = close_signal_rx.try_recv() {
            log::info!("NotificationClosed #{}", id);
            let iface_ref = conn
                .object_server()
                .interface::<_, NotificationDaemon>("/org/freedesktop/Notifications")
                .await
                .expect("interface not found");
            let emitter = iface_ref.signal_emitter();
            // reason 2 = dismissed by user, reason 1 = expired
            if let Err(e) = NotificationDaemon::notification_closed(&emitter, id, 2).await {
                log::error!("Failed to emit NotificationClosed signal: {}", e);
            }
        }
    }
}
