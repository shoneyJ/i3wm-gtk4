/// StatusNotifierWatcher D-Bus service.
///
/// Owns `org.kde.StatusNotifierWatcher` on the session bus and tracks
/// registered tray items (StatusNotifierItem clients).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use linbus::{Connection, Dispatcher, Value};

use super::item::read_item_props;
use super::types::{TrayEvent, TrayItemId};

/// Start the StatusNotifierWatcher on a background thread.
/// Returns a join handle for graceful shutdown.
pub fn start_watcher(tx: mpsc::Sender<TrayEvent>, shutdown: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        eprintln!("[tray] watcher thread starting");
        match run_watcher(tx, shutdown) {
            Ok(()) => eprintln!("[tray] watcher thread exited normally"),
            Err(e) => eprintln!("[tray] watcher failed: {}", e),
        }
    })
}

fn run_watcher(tx: mpsc::Sender<TrayEvent>, shutdown: Arc<AtomicBool>) -> Result<(), linbus::LinbusError> {
    let items: Arc<Mutex<HashSet<TrayItemId>>> = Arc::new(Mutex::new(HashSet::new()));
    let mut known_loaded: HashSet<TrayItemId> = HashSet::new();

    let mut conn = Connection::session()?;
    conn.request_name("org.kde.StatusNotifierWatcher")?;
    conn.add_match("type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged'")?;

    eprintln!("[tray] StatusNotifierWatcher active on session bus");
    log::info!("StatusNotifierWatcher service active on session bus");

    let match_rule = "type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged'";

    let mut dispatcher = Dispatcher::new(conn);
    dispatcher.track_name("org.kde.StatusNotifierWatcher");
    dispatcher.track_match_rule(match_rule);

    let stop = dispatcher.stop_handle();
    let shutdown_check = shutdown.clone();

    // Properties
    dispatcher.set_property(
        "org.kde.StatusNotifierWatcher",
        "IsStatusNotifierHostRegistered",
        Value::Bool(true),
    );
    dispatcher.set_property(
        "org.kde.StatusNotifierWatcher",
        "ProtocolVersion",
        Value::I32(0),
    );

    // RegisterStatusNotifierItem
    {
        let items = items.clone();
        let tx = tx.clone();
        dispatcher.add_handler(
            "org.kde.StatusNotifierWatcher",
            "RegisterStatusNotifierItem",
            Box::new(move |msg| {
                let service = msg.body.first().and_then(|v| v.as_str()).unwrap_or("");
                let sender = msg.sender.as_deref().unwrap_or("");

                let (bus_name, object_path) = if service.starts_with('/') {
                    (sender.to_string(), service.to_string())
                } else {
                    (service.to_string(), "/StatusNotifierItem".to_string())
                };

                eprintln!("[tray] SNI register: {} @ {}", bus_name, object_path);
                log::info!("SNI register: {} @ {}", bus_name, object_path);

                let id = TrayItemId {
                    bus_name: bus_name.clone(),
                    object_path: object_path.clone(),
                };

                items.lock().unwrap().insert(id.clone());
                let _ = tx.send(TrayEvent::ItemRegistered(id));

                Ok(vec![])
            }),
        );
    }

    // RegisterStatusNotifierHost
    dispatcher.add_handler(
        "org.kde.StatusNotifierWatcher",
        "RegisterStatusNotifierHost",
        Box::new(|_msg| {
            log::info!("StatusNotifierHost registered");
            Ok(vec![])
        }),
    );

    // Shared connection for property reads (avoids Hello() overhead per item)
    let prop_conn = Arc::new(Mutex::new(Connection::session()?));

    // Run event loop with signal handling and periodic property loading
    let items_signal = items.clone();
    let tx_signal = tx.clone();
    let items_loader = items.clone();
    let tx_loader = tx.clone();

    dispatcher.run(
        500,
        move |_d| {
            if shutdown_check.load(Ordering::Relaxed) {
                stop.store(true, Ordering::Relaxed);
                return;
            }
            // Periodic property loading for newly registered items
            let current_items: Vec<TrayItemId> =
                items_loader.lock().unwrap().iter().cloned().collect();

            // Keep RegisteredStatusNotifierItems property in sync
            let list: Vec<Value> = current_items.iter()
                .map(|id| Value::String(format!("{}{}", id.bus_name, id.object_path)))
                .collect();
            _d.set_property(
                "org.kde.StatusNotifierWatcher",
                "RegisteredStatusNotifierItems",
                Value::Array(list),
            );

            // Remove stale entries from known_loaded
            known_loaded.retain(|id| current_items.contains(id));

            for id in current_items {
                if !known_loaded.contains(&id) {
                    known_loaded.insert(id.clone());
                    // Load props on a background thread with a shared persistent connection.
                    let tx = tx_loader.clone();
                    let id_clone = id.clone();
                    let prop_conn_ref = prop_conn.clone();
                    std::thread::spawn(move || {
                        let mut conn = prop_conn_ref.lock().unwrap();
                        match read_item_props(&mut conn, &id_clone.bus_name, &id_clone.object_path) {
                            Ok(props) => {
                                eprintln!("[tray] props: {} icon={} status={}", id_clone.bus_name, props.icon_name, props.status);
                                let _ = tx.send(TrayEvent::ItemPropsLoaded(props));
                            }
                            Err(e) => {
                                eprintln!("[tray] props failed {}: {}", id_clone.bus_name, e);
                            }
                        }
                    });
                }
            }
        },
        move |signal| {
            // Handle NameOwnerChanged signals
            let iface = signal.interface.as_deref().unwrap_or("");
            let member = signal.member.as_deref().unwrap_or("");

            if iface == "org.freedesktop.DBus" && member == "NameOwnerChanged" {
                let name = signal.body.first().and_then(|v| v.as_str()).unwrap_or("");
                let new_owner = signal.body.get(2).and_then(|v| v.as_str()).unwrap_or("");

                if new_owner.is_empty() && !name.is_empty() {
                    let removed: Vec<TrayItemId> = items_signal
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|id| id.bus_name == name)
                        .cloned()
                        .collect();

                    for id in removed {
                        items_signal.lock().unwrap().remove(&id);
                        log::info!("SNI name vanished: {}", name);
                        let _ = tx_signal.send(TrayEvent::ItemUnregistered(id));
                    }
                }
            }
        },
    )
}
