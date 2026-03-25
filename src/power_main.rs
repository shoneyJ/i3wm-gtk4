//! i3more-power — power menu replacing rofi powermenu.
//!
//! Shows Lock / Logout / Reboot / Shutdown / Suspend / Hibernate.

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;

const WINDOW_WIDTH: i32 = 250;
const WINDOW_HEIGHT: i32 = 280;

struct PowerAction {
    label: &'static str,
    icon: char,
}

const ACTIONS: &[PowerAction] = &[
    PowerAction { label: "Lock", icon: '\u{f023}' },
    PowerAction { label: "Logout", icon: '\u{f2f5}' },
    PowerAction { label: "Suspend", icon: '\u{f186}' },
    PowerAction { label: "Hibernate", icon: '\u{f7e4}' },
    PowerAction { label: "Reboot", icon: '\u{f01e}' },
    PowerAction { label: "Shutdown", icon: '\u{f011}' },
];

fn main() {
    i3more::init_logging("i3more-power");

    let app = gtk4::Application::builder()
        .application_id("com.i3more.power")
        .build();

    app.connect_activate(on_activate);
    app.run();
}

fn on_activate(app: &gtk4::Application) {
    if let Some(window) = app.active_window() {
        if window.is_visible() {
            std::process::exit(0);
        }
        window.present();
        return;
    }

    i3more::fa::register_font();
    i3more::css::load_css("menu.css", include_str!("../assets/menu.css"));

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    vbox.add_css_class("menu-main");

    let listbox = gtk4::ListBox::new();
    listbox.add_css_class("menu-list");
    listbox.set_selection_mode(gtk4::SelectionMode::Single);

    for (idx, action) in ACTIONS.iter().enumerate() {
        let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        hbox.add_css_class("menu-row");

        let icon_label = gtk4::Label::new(None);
        icon_label.set_markup(&i3more::fa::fa_icon(action.icon, "#ebdbb2", 14));
        icon_label.set_width_chars(3);
        hbox.append(&icon_label);

        let label = gtk4::Label::new(Some(action.label));
        label.add_css_class("menu-label");
        label.set_halign(gtk4::Align::Start);
        hbox.append(&label);

        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&hbox));
        row.set_widget_name(&idx.to_string());
        listbox.append(&row);
    }

    if let Some(first) = listbox.row_at_index(0) {
        listbox.select_row(Some(&first));
    }

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_child(Some(&listbox));
    vbox.append(&scrolled);

    listbox.connect_row_activated(|_, row| {
        execute_action(row);
    });

    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("i3More-power")
        .resizable(false)
        .decorated(false)
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .child(&vbox)
        .build();

    let key_ctrl = gtk4::EventControllerKey::new();
    let listbox_ref = listbox.clone();
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        match key {
            gdk::Key::Escape => std::process::exit(0),
            gdk::Key::Return | gdk::Key::KP_Enter => {
                if let Some(row) = listbox_ref.selected_row() {
                    execute_action(&row);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(key_ctrl);

    // Set X11 position in connect_realize — fires BEFORE the window is mapped,
    // so i3 sees the correct position from the start (no flicker).
    let (target_x, target_y) = compute_position_east();
    window.connect_realize(move |win| {
        set_x11_position(win, target_x, target_y);
    });

    window.present();
    listbox.grab_focus();
}

fn execute_action(row: &gtk4::ListBoxRow) {
    let idx: usize = row.widget_name().parse().unwrap_or(0);
    let label = ACTIONS.get(idx).map(|a| a.label).unwrap_or("");
    log::info!("Power action: {}", label);

    match label {
        "Lock" => { let _ = std::process::Command::new("i3more-lock").spawn(); }
        "Logout" => { let _ = std::process::Command::new("i3-msg").arg("exit").spawn(); }
        "Reboot" => { let _ = std::process::Command::new("systemctl").arg("reboot").spawn(); }
        "Shutdown" => { let _ = std::process::Command::new("systemctl").arg("poweroff").spawn(); }
        "Suspend" => { let _ = std::process::Command::new("systemctl").arg("suspend").spawn(); }
        "Hibernate" => { let _ = std::process::Command::new("systemctl").arg("hibernate").spawn(); }
        _ => {}
    }
    std::process::exit(0);
}

/// Set the X11 window position before mapping using x11rb ConfigureWindow.
/// Called from connect_realize (after X11 window exists, before map).
fn set_x11_position(win: &gtk4::ApplicationWindow, x: i32, y: i32) {
    let surface = match win.surface() {
        Some(s) => s,
        None => return,
    };
    let x11_surface = match surface.downcast::<gdk4_x11::X11Surface>() {
        Ok(s) => s,
        Err(_) => return,
    };
    let xid = x11_surface.xid() as u32;

    // Use xdotool with the exact XID to move before map
    let _ = std::process::Command::new("xdotool")
        .args([
            "windowmove", "--sync",
            &xid.to_string(),
            &x.to_string(), &y.to_string(),
        ])
        .output();
}

/// Compute position on the right edge of the focused monitor.
fn compute_position_east() -> (i32, i32) {
    let output_name = match get_focused_output() {
        Some(name) => name,
        None => return (0, 0),
    };
    let display = match gdk::Display::default() {
        Some(d) => d,
        None => return (0, 0),
    };
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        if let Some(obj) = monitors.item(i) {
            if let Ok(monitor) = obj.downcast::<gdk::Monitor>() {
                if monitor.connector().map(|s| s.to_string()).as_deref() == Some(&output_name) {
                    let geom = monitor.geometry();
                    let x = geom.x() + geom.width() - WINDOW_WIDTH - 10;
                    let y = geom.y() + (geom.height() - WINDOW_HEIGHT) / 2;
                    return (x, y);
                }
            }
        }
    }
    (0, 0)
}

fn get_focused_output() -> Option<String> {
    let mut conn = i3more::ipc::I3Connection::connect().ok()?;
    let workspaces = conn.get_workspaces().ok()?;
    workspaces.as_array()?.iter()
        .find(|ws| ws["focused"].as_bool() == Some(true))
        .and_then(|ws| ws["output"].as_str().map(String::from))
}
