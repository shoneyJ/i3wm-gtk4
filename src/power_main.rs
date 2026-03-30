//! i3more-power — power menu with confirmation for destructive actions.
//!
//! Shows Lock / Logout / Reboot / Shutdown / Suspend / Hibernate.
//! Destructive actions prompt for confirmation and show a notification.

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

const WINDOW_WIDTH: i32 = 250;
const WINDOW_HEIGHT: i32 = 280;
const CONFIRM_WIDTH: i32 = 300;
const CONFIRM_HEIGHT: i32 = 150;

struct PowerAction {
    label: &'static str,
    icon: char,
    needs_confirm: bool,
    notify_msg: &'static str,
}

const ACTIONS: &[PowerAction] = &[
    PowerAction { label: "Lock",      icon: '\u{f023}', needs_confirm: false, notify_msg: "" },
    PowerAction { label: "Logout",    icon: '\u{f2f5}', needs_confirm: true,  notify_msg: "Logging out of session..." },
    PowerAction { label: "Suspend",   icon: '\u{f186}', needs_confirm: false, notify_msg: "" },
    PowerAction { label: "Hibernate", icon: '\u{f7e4}', needs_confirm: true,  notify_msg: "System is hibernating..." },
    PowerAction { label: "Reboot",    icon: '\u{f01e}', needs_confirm: true,  notify_msg: "System is rebooting..." },
    PowerAction { label: "Shutdown",  icon: '\u{f011}', needs_confirm: true,  notify_msg: "System is powering off..." },
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
    i3more::css::load_css("power.css", include_str!("../assets/power.css"));

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

    let app_ref = app.clone();
    listbox.connect_row_activated(move |_, row| {
        execute_action(&app_ref, row);
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
    let app_ref = app.clone();
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        match key {
            gdk::Key::Escape => std::process::exit(0),
            gdk::Key::Return | gdk::Key::KP_Enter => {
                if let Some(row) = listbox_ref.selected_row() {
                    execute_action(&app_ref, &row);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(key_ctrl);

    let (target_x, target_y) = compute_position_east();
    window.connect_realize(move |win| {
        set_x11_position(win, target_x, target_y);
    });

    window.present();
    listbox.grab_focus();
}

fn execute_action(app: &gtk4::Application, row: &gtk4::ListBoxRow) {
    let idx: usize = row.widget_name().parse().unwrap_or(0);
    let action = match ACTIONS.get(idx) {
        Some(a) => a,
        None => return,
    };
    log::info!("Power action: {}", action.label);

    if action.needs_confirm {
        show_confirm_dialog(app, action);
    } else {
        run_action(action.label);
    }
}

fn show_confirm_dialog(app: &gtk4::Application, action: &'static PowerAction) {
    // Hide the main menu window
    if let Some(win) = app.active_window() {
        win.set_visible(false);
    }

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.add_css_class("confirm-box");
    vbox.set_valign(gtk4::Align::Center);
    vbox.set_halign(gtk4::Align::Center);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);

    // Icon
    let icon_label = gtk4::Label::new(None);
    icon_label.set_markup(&i3more::fa::fa_icon(action.icon, "#fb4934", 24));
    vbox.append(&icon_label);

    // Question
    let question = gtk4::Label::new(Some(&format!("{}?", action.label)));
    question.add_css_class("confirm-title");
    vbox.append(&question);

    // Message
    let msg = gtk4::Label::new(Some(action.notify_msg));
    msg.add_css_class("confirm-message");
    vbox.append(&msg);

    // Buttons
    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    btn_box.set_halign(gtk4::Align::Center);

    let yes_btn = gtk4::Button::with_label("Yes");
    yes_btn.add_css_class("confirm-yes");
    yes_btn.set_size_request(80, -1);

    let no_btn = gtk4::Button::with_label("No");
    no_btn.add_css_class("confirm-no");
    no_btn.set_size_request(80, -1);

    btn_box.append(&yes_btn);
    btn_box.append(&no_btn);
    vbox.append(&btn_box);

    let confirm_win = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Confirm")
        .resizable(false)
        .decorated(false)
        .default_width(CONFIRM_WIDTH)
        .default_height(CONFIRM_HEIGHT)
        .child(&vbox)
        .build();

    // Yes → notify + execute
    let cw = confirm_win.clone();
    yes_btn.connect_clicked(move |_| {
        cw.close();
        // Send notification before executing
        let _ = std::process::Command::new("notify-send")
            .args(["-u", "critical", "-t", "10000", action.label, action.notify_msg])
            .spawn();
        // Brief delay so the notification renders
        glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
            run_action(action.label);
            std::process::exit(0);
        });
    });

    // No → close and exit
    let cw = confirm_win.clone();
    no_btn.connect_clicked(move |_| {
        cw.close();
        std::process::exit(0);
    });

    // Escape → cancel
    let key_ctrl = gtk4::EventControllerKey::new();
    let cw = confirm_win.clone();
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            cw.close();
            std::process::exit(0);
        }
        if key == gdk::Key::Return || key == gdk::Key::KP_Enter {
            // Enter = Yes (focused by default)
            return glib::Propagation::Proceed;
        }
        glib::Propagation::Proceed
    });
    confirm_win.add_controller(key_ctrl);

    let (target_x, target_y) = compute_position_center();
    confirm_win.connect_realize(move |win| {
        set_x11_position(win, target_x, target_y);
    });

    confirm_win.present();
    yes_btn.grab_focus();
}

fn run_action(label: &str) {
    match label {
        "Lock" => { let _ = std::process::Command::new("i3more-lock").spawn(); }
        "Logout" => { let _ = std::process::Command::new("i3-msg").arg("exit").spawn(); }
        "Reboot" => { let _ = std::process::Command::new("systemctl").arg("reboot").spawn(); }
        "Shutdown" => { let _ = std::process::Command::new("systemctl").arg("poweroff").spawn(); }
        "Suspend" => { let _ = std::process::Command::new("systemctl").arg("suspend").spawn(); }
        "Hibernate" => { let _ = std::process::Command::new("systemctl").arg("hibernate").spawn(); }
        _ => {}
    }
}

/// Set the X11 window position before mapping.
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

    let _ = std::process::Command::new("xdotool")
        .args([
            "windowmove", "--sync",
            &xid.to_string(),
            &x.to_string(), &y.to_string(),
        ])
        .output();
}

/// Compute position on the right edge of the focused monitor (for main menu).
fn compute_position_east() -> (i32, i32) {
    let (geom, _) = get_focused_monitor_geom();
    (geom.0 + geom.2 - WINDOW_WIDTH - 10, geom.1 + (geom.3 - WINDOW_HEIGHT) / 2)
}

/// Compute centered position on the focused monitor (for confirmation).
fn compute_position_center() -> (i32, i32) {
    let (geom, _) = get_focused_monitor_geom();
    (geom.0 + (geom.2 - CONFIRM_WIDTH) / 2, geom.1 + (geom.3 - CONFIRM_HEIGHT) / 2)
}

/// Returns ((x, y, width, height), output_name) for the focused monitor.
fn get_focused_monitor_geom() -> ((i32, i32, i32, i32), String) {
    let output_name = get_focused_output().unwrap_or_default();
    if output_name.is_empty() {
        return ((0, 0, 1920, 1080), String::new());
    }

    let display = match gdk::Display::default() {
        Some(d) => d,
        None => return ((0, 0, 1920, 1080), output_name),
    };
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        if let Some(obj) = monitors.item(i) {
            if let Ok(monitor) = obj.downcast::<gdk::Monitor>() {
                if monitor.connector().map(|s| s.to_string()).as_deref() == Some(&output_name) {
                    let g = monitor.geometry();
                    return ((g.x(), g.y(), g.width(), g.height()), output_name);
                }
            }
        }
    }
    ((0, 0, 1920, 1080), output_name)
}

fn get_focused_output() -> Option<String> {
    let mut conn = i3more::ipc::I3Connection::connect().ok()?;
    let workspaces = conn.get_workspaces().ok()?;
    workspaces.as_array()?.iter()
        .find(|ws| ws["focused"].as_bool() == Some(true))
        .and_then(|ws| ws["output"].as_str().map(String::from))
}
