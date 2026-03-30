/// Notification daemon D-Bus service.
///
/// Claims `org.freedesktop.Notifications` on the session bus and handles
/// incoming notification requests from applications.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::SystemTime;

use linbus::{Connection, Dispatcher, Value};

use super::types::{Notification, NotifyEvent};

/// Start the notification daemon on a background thread.
/// Returns an action sender and a join handle for graceful shutdown.
pub fn start_notification_daemon(
    tx: mpsc::Sender<NotifyEvent>,
    shutdown: Arc<AtomicBool>,
) -> (mpsc::Sender<(u32, String)>, JoinHandle<()>) {
    let (action_tx, action_rx) = mpsc::channel::<(u32, String)>();
    let handle = std::thread::spawn(move || {
        if let Err(e) = run_daemon(tx, action_rx, shutdown) {
            eprintln!("Notification daemon failed: {}", e);
            log::error!("Notification daemon failed: {}", e);
        }
    });
    (action_tx, handle)
}

fn run_daemon(
    tx: mpsc::Sender<NotifyEvent>,
    action_rx: mpsc::Receiver<(u32, String)>,
    shutdown: Arc<AtomicBool>,
) -> Result<(), linbus::LinbusError> {
    let mut conn = Connection::session()?;
    conn.request_name("org.freedesktop.Notifications")?;

    log::info!("Notification daemon active on session bus");

    let next_id = Arc::new(Mutex::new(1u32));
    let sync_keys: Arc<Mutex<HashMap<String, u32>>> = Arc::new(Mutex::new(HashMap::new()));

    let mut dispatcher = Dispatcher::new(conn);
    dispatcher.track_name("org.freedesktop.Notifications");

    // Link external shutdown flag to dispatcher stop
    let stop = dispatcher.stop_handle();
    let shutdown_check = shutdown.clone();


    // GetCapabilities
    dispatcher.add_handler(
        "org.freedesktop.Notifications",
        "GetCapabilities",
        Box::new(|_msg| {
            Ok(vec![Value::Array(vec![
                Value::String("body".into()),
                Value::String("body-hyperlinks".into()),
                Value::String("body-images".into()),
                Value::String("body-markup".into()),
                Value::String("icon-static".into()),
                Value::String("persistence".into()),
                Value::String("actions".into()),
            ])])
        }),
    );

    // GetServerInformation
    dispatcher.add_handler(
        "org.freedesktop.Notifications",
        "GetServerInformation",
        Box::new(|_msg| {
            Ok(vec![
                Value::String("i3more".into()),
                Value::String("i3more".into()),
                Value::String("0.1.0".into()),
                Value::String("1.2".into()),
            ])
        }),
    );

    // CloseNotification
    {
        let tx = tx.clone();
        dispatcher.add_handler(
            "org.freedesktop.Notifications",
            "CloseNotification",
            Box::new(move |msg| {
                let id = msg.body.first().and_then(|v| v.as_u32()).unwrap_or(0);
                log::info!("CloseNotification #{}", id);
                let _ = tx.send(NotifyEvent::Close(id));
                Ok(vec![])
            }),
        );
    }

    // Notify
    {
        let tx = tx.clone();
        let next_id = next_id.clone();
        let sync_keys = sync_keys.clone();
        dispatcher.add_handler(
            "org.freedesktop.Notifications",
            "Notify",
            Box::new(move |msg| {
                let body = &msg.body;
                let app_name = body.first().and_then(|v| v.as_str()).unwrap_or("").to_string();
                let replaces_id = body.get(1).and_then(|v| v.as_u32()).unwrap_or(0);
                let app_icon = body.get(2).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let summary = body.get(3).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let body_text = body.get(4).and_then(|v| v.as_str()).unwrap_or("").to_string();

                // Actions: flat array [key, label, key, label, ...]
                let actions_raw: Vec<String> = body.get(5)
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();

                // Hints: a{sv}
                let hints: HashMap<String, linbus::Value> = body.get(6)
                    .and_then(|v| v.to_string_dict())
                    .unwrap_or_default();

                let expire_timeout = body.get(7).and_then(|v| v.as_i32()).unwrap_or(-1);

                // Check for x-canonical-private-synchronous hint
                let sync_key = hints.get("x-canonical-private-synchronous")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let effective_replaces = if replaces_id > 0 {
                    replaces_id
                } else if let Some(ref key) = sync_key {
                    sync_keys.lock().unwrap().get(key).copied().unwrap_or(0)
                } else {
                    0
                };

                let id = if effective_replaces > 0 {
                    effective_replaces
                } else {
                    let mut next = next_id.lock().unwrap();
                    let id = *next;
                    *next = next.wrapping_add(1);
                    if *next == 0 { *next = 1; }
                    id
                };

                if let Some(key) = sync_key {
                    sync_keys.lock().unwrap().insert(key, id);
                }

                let action_pairs: Vec<(String, String)> = actions_raw
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
                    app_name: app_name.clone(),
                    app_icon,
                    summary: summary.clone(),
                    body: body_text.clone(),
                    actions: action_pairs,
                    hints,
                    expire_timeout,
                    timestamp: SystemTime::now(),
                };

                log::info!("Notification #{}: [{}] {} - {}", id, app_name, summary, body_text);
                let _ = tx.send(NotifyEvent::New(notification));

                Ok(vec![Value::U32(id)])
            }),
        );
    }

    // Run the event loop
    dispatcher.run(
        50,
        |d| {
            if shutdown_check.load(Ordering::Relaxed) {
                stop.store(true, Ordering::Relaxed);
            }
            // Poll action_rx and emit ActionInvoked signals
            while let Ok((id, action_key)) = action_rx.try_recv() {
                log::info!("ActionInvoked #{}: {}", id, action_key);
                if let Err(e) = d.emit_signal(
                    "/org/freedesktop/Notifications",
                    "org.freedesktop.Notifications",
                    "ActionInvoked",
                    &[Value::U32(id), Value::String(action_key)],
                ) {
                    log::error!("Failed to emit ActionInvoked: {}", e);
                }
            }
        },
        |_signal| {}, // no signals to handle
    )
}
