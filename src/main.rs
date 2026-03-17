//! i3More — an extension layer for i3 window manager.
//!
//! Provides a floating workspace navigator panel with app icons.
//! Communicates with i3 via IPC. No external script dependencies.

mod control_panel;
mod fa;
mod ipc;
mod model;
mod navigator;
mod notify;
mod sysinfo;
mod tray;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::prelude::NativeExt; // for .surface() on ApplicationWindow
use navigator::NavigatorState;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use notify::types::NotifyEvent;
use tray::types::{TrayEvent, TrayItemId, TrayItemProps};

/// Message sent from the i3 event listener thread to the GTK main thread.
enum I3Event {
    WorkspacesChanged,
}

/// Safely remove a GLib source without panicking if it was already removed.
/// Unlike `SourceId::remove()`, this does not panic on stale source IDs.
pub(crate) fn safe_source_remove(source_id: glib::SourceId) {
    unsafe {
        glib::ffi::g_source_remove(source_id.as_raw());
    }
}

fn main() {
    i3more::init_logging("i3more");

    let app = gtk4::Application::builder()
        .application_id("com.i3more.navigator")
        .build();

    app.connect_activate(on_activate);
    app.run();
}

fn on_activate(app: &gtk4::Application) {
    fa::register_font();

    // Initial i3 state query
    let (workspaces_json, tree_json) = match query_initial_state() {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to connect to i3: {}", e);
            eprintln!("Error: Failed to connect to i3 IPC: {}", e);
            eprintln!("Make sure i3 is running and the IPC socket is accessible.");
            std::process::exit(1);
        }
    };

    let workspaces = model::build_workspace_state(&workspaces_json, &tree_json);

    // Collect all unique window classes for batch icon resolution
    let all_classes: Vec<String> = workspaces
        .iter()
        .flat_map(|ws| ws.window_classes.iter().cloned())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut icon_resolver = i3more::icon::IconResolver::new();

    // Pre-resolve all icons at startup
    log::info!("Pre-resolving {} icon classes", all_classes.len());
    icon_resolver.resolve_batch(&all_classes);

    let state = Rc::new(RefCell::new(NavigatorState {
        icon_resolver,
        workspaces,
    }));

    // Build the navigator UI
    let (window, container_ref, tray_box_ref, screen_width, sysinfo_labels, notify_handles, cp_handles) =
        navigator::build_navigator(app, state.clone());

    // Set up i3 event listener in background thread
    let (tx, rx) = mpsc::channel::<I3Event>();
    start_event_listener(tx);

    // Set up tray watcher
    let (tray_tx, tray_rx) = mpsc::channel::<TrayEvent>();
    tray::start_watcher(tray_tx);

    let tray_state: Rc<RefCell<HashMap<TrayItemId, TrayItemProps>>> =
        Rc::new(RefCell::new(HashMap::new()));

    // Set up notification daemon
    let (notify_tx, notify_rx) = mpsc::channel::<NotifyEvent>();
    let notify_close_tx = notify_tx.clone();
    let action_tx = notify::start_notification_daemon(notify_tx);

    i3more::css::load_css("notification.css", include_str!("../assets/notification.css"));
    let popup_manager = Rc::new(notify::popup::PopupManager::new(app, notify_close_tx, action_tx));

    // Notification history and panel
    let notify_history: Rc<RefCell<notify::history::NotificationHistory>> =
        Rc::new(RefCell::new(notify::history::NotificationHistory::new()));
    let notify_panel = Rc::new(notify::panel::NotificationPanel::new(
        app,
        notify_history.clone(),
    ));

    // Control panel
    let control_panel = Rc::new(control_panel::panel::ControlPanel::new(app));

    // Bell click handler — toggle notification panel (hide control panel first)
    let panel_for_bell = notify_panel.clone();
    let badge_for_bell = notify_handles.badge_label.clone();
    let cp_for_bell = control_panel.clone();
    let bell_gesture = gtk4::GestureClick::new();
    bell_gesture.connect_released(move |_, _, _, _| {
        if cp_for_bell.is_visible() {
            cp_for_bell.hide();
        }
        let opened = panel_for_bell.toggle();
        if opened {
            badge_for_bell.set_visible(false);
            badge_for_bell.set_text("");
        }
    });
    notify_handles.bell_overlay.add_controller(bell_gesture);

    // Control panel icon click handler — toggle control panel (hide notification panel first)
    let cp_for_click = control_panel.clone();
    let np_for_click = notify_panel.clone();
    let cp_gesture = gtk4::GestureClick::new();
    cp_gesture.connect_released(move |_, _, _, _| {
        if np_for_click.is_visible() {
            np_for_click.hide();
        }
        cp_for_click.toggle();
    });
    cp_handles.cp_overlay.add_controller(cp_gesture);

    let badge_label = notify_handles.badge_label.clone();

    // Set up debounced update handler
    let debounce_source: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let tray_debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    // Poll the channel from the GTK main loop
    let state_clone = state.clone();
    let container_clone = container_ref.clone();
    let debounce_clone = debounce_source.clone();
    let tray_state_clone = tray_state.clone();
    let tray_box_clone = tray_box_ref.clone();
    let tray_debounce_clone = tray_debounce.clone();
    let popup_mgr = popup_manager.clone();
    let history_for_poll = notify_history.clone();
    let badge_for_poll = badge_label.clone();
    let panel_for_poll = notify_panel.clone();

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        // Drain all pending i3 events
        let mut has_event = false;
        while rx.try_recv().is_ok() {
            has_event = true;
        }

        if has_event {
            log::debug!("Received i3 workspace/window event");
            // Cancel any pending debounce timeout (safe: ignore if already fired)
            if let Some(source_id) = debounce_clone.borrow_mut().take() {
                safe_source_remove(source_id);
            }

            // Schedule debounced update (100ms)
            let state_inner = state_clone.clone();
            let container_inner = container_clone.clone();
            let debounce_clear = debounce_clone.clone();
            let source_id = glib::timeout_add_local_once(
                std::time::Duration::from_millis(100),
                move || {
                    // Clear the stored SourceId before it becomes stale
                    debounce_clear.borrow_mut().take();
                    refresh_state(&state_inner, &container_inner);
                },
            );
            *debounce_clone.borrow_mut() = Some(source_id);
        }

        // Drain tray events
        let mut tray_changed = false;
        while let Ok(event) = tray_rx.try_recv() {
            match event {
                TrayEvent::ItemRegistered(id) => {
                    log::info!("Tray item registered: {}:{}", id.bus_name, id.object_path);
                    tray_state_clone.borrow_mut().entry(id.clone()).or_insert_with(|| {
                        TrayItemProps::new(id)
                    });
                    tray_changed = true;
                }
                TrayEvent::ItemUnregistered(id) => {
                    log::info!("Tray item unregistered: {}:{}", id.bus_name, id.object_path);
                    tray_state_clone.borrow_mut().remove(&id);
                    tray_changed = true;
                }
                TrayEvent::ItemPropsLoaded(props) => {
                    log::info!("Tray item props loaded: {} icon={}", props.id.bus_name, props.icon_name);
                    tray_state_clone.borrow_mut().insert(props.id.clone(), props);
                    tray_changed = true;
                }
                TrayEvent::ItemUpdated(id) => {
                    log::debug!("Tray item updated signal: {}:{}", id.bus_name, id.object_path);
                    // The watcher will send ItemPropsLoaded after re-reading
                    tray_changed = true;
                }
            }
        }

        if tray_changed {
            // Cancel any pending tray debounce (safe: ignore if already fired)
            if let Some(source_id) = tray_debounce_clone.borrow_mut().take() {
                safe_source_remove(source_id);
            }
            let ts = tray_state_clone.clone();
            let tb = tray_box_clone.clone();
            let td_clear = tray_debounce_clone.clone();
            let source_id = glib::timeout_add_local_once(
                std::time::Duration::from_millis(100),
                move || {
                    td_clear.borrow_mut().take();
                    let tray_box = tb.borrow();
                    let items = ts.borrow();
                    tray::render::render_tray(&tray_box, &items);
                },
            );
            *tray_debounce_clone.borrow_mut() = Some(source_id);
        }

        // Drain notification events
        while let Ok(event) = notify_rx.try_recv() {
            match event {
                NotifyEvent::New(notif) => {
                    popup_mgr.show(&notif);

                    // Push to history and update badge
                    let mut hist = history_for_poll.borrow_mut();
                    hist.push(&notif);
                    let count = hist.unread_count;
                    drop(hist);

                    if !panel_for_poll.is_visible() {
                        badge_for_poll.set_text(&count.to_string());
                        badge_for_poll.set_visible(true);
                    }
                }
                NotifyEvent::Close(id) => {
                    popup_mgr.dismiss(id);
                }
                NotifyEvent::ActionInvoked(_, _) => {
                    // Actions flow directly via action_tx, not through this channel
                }
            }
        }

        glib::ControlFlow::Continue
    });

    // 1-second timer to update clock and battery labels
    let clock_label = sysinfo_labels.clock;
    let battery_label = sysinfo_labels.battery;
    glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        clock_label.set_text(&sysinfo::read_clock());
        if let Some((cap, status)) = sysinfo::read_battery() {
            let charging = status == "Charging";
            let glyph = fa::battery_glyph(cap, charging);
            let icon_markup = fa::fa_icon(glyph, "#a89984", 9);
            battery_label.set_markup(&format!("{} {}%", icon_markup, cap));
        }
        glib::ControlFlow::Continue
    });

    // Set X11 dock type BEFORE map: connect_realize fires after the X11 window
    // is created but before it is mapped, so i3 sees DOCK type at classification time.
    let sw = screen_width;
    window.connect_realize(move |win| {
        let surface = match win.surface() {
            Some(s) => s,
            None => return,
        };
        let x11_surface = match surface.downcast::<gdk4_x11::X11Surface>() {
            Ok(s) => s,
            Err(_) => return, // not X11 (e.g. Wayland) — skip
        };
        let xid = x11_surface.xid();

        // Set _NET_WM_WINDOW_TYPE_DOCK — must happen before map so i3 sees it
        let _ = std::process::Command::new("xprop")
            .args([
                "-id", &xid.to_string(),
                "-f", "_NET_WM_WINDOW_TYPE", "32a",
                "-set", "_NET_WM_WINDOW_TYPE", "_NET_WM_WINDOW_TYPE_DOCK",
            ])
            .output();

        // Set _NET_WM_STRUT_PARTIAL to reserve 40px at top of screen
        let strut = format!("0, 0, 40, 0, 0, 0, 0, 0, 0, {}, 0, 0", sw - 1);
        let _ = std::process::Command::new("xprop")
            .args([
                "-id", &xid.to_string(),
                "-f", "_NET_WM_STRUT_PARTIAL", "32c",
                "-set", "_NET_WM_STRUT_PARTIAL", &strut,
            ])
            .output();

        // Force exact size (GTK may still expand beyond BAR_HEIGHT)
        let _ = std::process::Command::new("xdotool")
            .args(["windowsize", &xid.to_string(), &sw.to_string(), "40"])
            .output();
    });

    window.present();
}

/// Query initial workspace and tree state from i3.
fn query_initial_state() -> Result<(serde_json::Value, serde_json::Value), Box<dyn std::error::Error>> {
    let mut conn = ipc::I3Connection::connect()?;
    let workspaces = conn.get_workspaces()?;
    let tree = conn.get_tree()?;
    Ok((workspaces, tree))
}

/// Start a background thread that listens for i3 workspace and window events.
fn start_event_listener(tx: mpsc::Sender<I3Event>) {
    std::thread::spawn(move || {
        loop {
            match listen_events(&tx) {
                Ok(()) => break, // Clean shutdown
                Err(e) => {
                    log::error!("i3 event listener error: {}. Reconnecting in 2s...", e);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    });
}

/// Connect to i3, subscribe to events, and forward them.
fn listen_events(tx: &mpsc::Sender<I3Event>) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = ipc::I3Connection::connect_for_events()?;
    conn.subscribe(&["workspace", "window"])?;
    log::info!("Subscribed to i3 workspace and window events");

    loop {
        let (_event_type, _payload) = conn.read_event()?;
        // We don't need to parse the event details — just signal that something changed
        if tx.send(I3Event::WorkspacesChanged).is_err() {
            log::warn!("Event channel closed, GTK app shutting down");
            break;
        }
    }

    Ok(())
}

/// Re-query i3 state and update the navigator.
fn refresh_state(
    state: &Rc<RefCell<NavigatorState>>,
    container_ref: &Rc<RefCell<gtk4::Box>>,
) {
    // Query fresh state from i3 (in a way that doesn't block too long)
    let fresh = match query_initial_state() {
        Ok((ws, tree)) => model::build_workspace_state(&ws, &tree),
        Err(e) => {
            log::error!("Failed to refresh i3 state: {}", e);
            return;
        }
    };

    // Resolve any new icon classes
    let new_classes: Vec<String> = fresh
        .iter()
        .flat_map(|ws| ws.window_classes.iter().cloned())
        .collect();

    {
        let mut state_mut = state.borrow_mut();
        for class in &new_classes {
            state_mut.icon_resolver.resolve(class);
        }
        state_mut.workspaces = fresh;
    }

    // Re-render
    let container = container_ref.borrow();
    navigator::render_workspaces(&container, state);
}
