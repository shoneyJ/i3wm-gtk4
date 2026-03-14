/// StatusNotifierWatcher D-Bus service.
///
/// Owns `org.kde.StatusNotifierWatcher` on the session bus and tracks
/// registered tray items (StatusNotifierItem clients).

use std::collections::HashSet;
use std::sync::{mpsc, Arc, Mutex};

use futures_util::StreamExt;
use zbus::object_server::SignalEmitter;
use zbus::{connection, interface, Connection, MatchRule, MessageStream};

use super::item::read_item_props;
use super::types::{TrayEvent, TrayItemId};

/// The watcher D-Bus object, served at /StatusNotifierWatcher.
pub struct StatusNotifierWatcher {
    items: Arc<Mutex<HashSet<TrayItemId>>>,
    host_registered: bool,
    tx: mpsc::Sender<TrayEvent>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl StatusNotifierWatcher {
    /// Called by tray apps to register themselves.
    async fn register_status_notifier_item(
        &mut self,
        service: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();

        // `service` may be a bus name like ":1.42" or an object path like "/StatusNotifierItem".
        let (bus_name, object_path) = if service.starts_with('/') {
            (sender.clone(), service.to_string())
        } else {
            (service.to_string(), "/StatusNotifierItem".to_string())
        };

        let id = TrayItemId {
            bus_name: bus_name.clone(),
            object_path: object_path.clone(),
        };

        log::info!("SNI register: {} @ {}", bus_name, object_path);

        self.items.lock().unwrap().insert(id.clone());
        let _ = self.tx.send(TrayEvent::ItemRegistered(id));

        let service_str = format!("{}{}", bus_name, object_path);
        let _ = Self::status_notifier_item_registered(&emitter, &service_str).await;
    }

    /// Called by hosts (like us) to register.
    fn register_status_notifier_host(&mut self, _service: &str) {
        self.host_registered = true;
        log::info!("StatusNotifierHost registered");
    }

    /// Property: list of registered items as "busname/objectpath" strings.
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items
            .lock()
            .unwrap()
            .iter()
            .map(|id| format!("{}{}", id.bus_name, id.object_path))
            .collect()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        self.host_registered
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    /// Signals
    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(
        emitter: &SignalEmitter<'_>,
    ) -> zbus::Result<()>;
}

/// Start the StatusNotifierWatcher on a background thread.
pub fn start_watcher(tx: mpsc::Sender<TrayEvent>) {
    std::thread::spawn(move || {
        async_io::block_on(async {
            if let Err(e) = run_watcher(tx).await {
                log::error!("Tray watcher failed: {}", e);
            }
        });
    });
}

async fn run_watcher(tx: mpsc::Sender<TrayEvent>) -> zbus::Result<()> {
    let items: Arc<Mutex<HashSet<TrayItemId>>> = Arc::new(Mutex::new(HashSet::new()));

    let watcher = StatusNotifierWatcher {
        items: items.clone(),
        host_registered: true,
        tx: tx.clone(),
    };

    let conn = connection::Builder::session()?
        .name("org.kde.StatusNotifierWatcher")?
        .serve_at("/StatusNotifierWatcher", watcher)?
        .build()
        .await?;

    log::info!("StatusNotifierWatcher service active on session bus");

    // Monitor NameOwnerChanged to detect when registered items disappear
    monitor_name_changes(conn.clone(), items, tx).await
}

/// Watch for D-Bus NameOwnerChanged signals and load props for new items.
///
/// Runs two concurrent tasks:
/// 1. Stream-reader: processes NameOwnerChanged to detect vanished bus names
/// 2. Prop-loader: periodically checks for newly registered items and reads their properties
async fn monitor_name_changes(
    conn: Connection,
    items: Arc<Mutex<HashSet<TrayItemId>>>,
    tx: mpsc::Sender<TrayEvent>,
) -> zbus::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&conn).await?;
    proxy.add_match_rule(rule).await?;

    let mut stream: MessageStream = MessageStream::from(&conn);

    // Task 1: read NameOwnerChanged from stream (runs in background)
    let items_stream = items.clone();
    let tx_stream = tx.clone();
    let stream_task = async move {
        while let Some(Ok(msg)) = stream.next().await {
            if let Ok(body) = msg.body().deserialize::<(String, String, String)>() {
                let (name, _old_owner, new_owner) = body;
                if new_owner.is_empty() {
                    let removed: Vec<TrayItemId> = items_stream
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|id| id.bus_name == name)
                        .cloned()
                        .collect();

                    for id in removed {
                        items_stream.lock().unwrap().remove(&id);
                        log::info!("SNI name vanished: {}", name);
                        let _ = tx_stream.send(TrayEvent::ItemUnregistered(id));
                    }
                }
            }
        }
    };

    // Task 2: periodically load properties for newly registered items
    let items_loader = items.clone();
    let tx_loader = tx.clone();
    let conn_loader = conn.clone();
    let loader_task = async move {
        let mut known_loaded: HashSet<TrayItemId> = HashSet::new();
        loop {
            async_io::Timer::after(std::time::Duration::from_millis(500)).await;

            let current_items: Vec<TrayItemId> =
                items_loader.lock().unwrap().iter().cloned().collect();

            // Remove stale entries from known_loaded
            known_loaded.retain(|id| current_items.contains(id));

            for id in current_items {
                if !known_loaded.contains(&id) {
                    known_loaded.insert(id.clone());
                    match read_item_props(&conn_loader, &id.bus_name, &id.object_path).await
                    {
                        Ok(props) => {
                            let _ = tx_loader.send(TrayEvent::ItemPropsLoaded(props));
                        }
                        Err(e) => {
                            log::warn!(
                                "Failed to read props for {}: {}",
                                id.bus_name,
                                e
                            );
                        }
                    }
                }
            }
        }
    };

    // Run both tasks concurrently — neither should return under normal operation
    futures_util::future::join(stream_task, loader_task).await;
    Ok(())
}
