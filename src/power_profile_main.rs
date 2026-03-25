//! i3more-power-profile — power profile switcher replacing rofi power-profiles.
//!
//! Reads available profiles from `powerprofilesctl list`, shows a menu, sets on select.

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;

const WINDOW_WIDTH: i32 = 300;
const WINDOW_HEIGHT: i32 = 200;

struct Profile {
    name: String,
    active: bool,
}

fn main() {
    i3more::init_logging("i3more-power-profile");

    let app = gtk4::Application::builder()
        .application_id("com.i3more.powerprofile")
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

    i3more::css::load_css("menu.css", include_str!("../assets/menu.css"));

    let profiles = read_profiles();
    if profiles.is_empty() {
        eprintln!("No power profiles available");
        std::process::exit(1);
    }

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    vbox.add_css_class("menu-main");

    let listbox = gtk4::ListBox::new();
    listbox.add_css_class("menu-list");
    listbox.set_selection_mode(gtk4::SelectionMode::Single);

    let profile_names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
    let mut active_idx = 0;

    for (idx, profile) in profiles.iter().enumerate() {
        let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        hbox.add_css_class("menu-row");

        let check = if profile.active {
            active_idx = idx;
            "  ●  "
        } else {
            "     "
        };

        let check_label = gtk4::Label::new(Some(check));
        if profile.active {
            check_label.add_css_class("menu-active");
        }
        hbox.append(&check_label);

        let name_label = gtk4::Label::new(Some(&profile.name));
        name_label.add_css_class("menu-label");
        name_label.set_halign(gtk4::Align::Start);
        name_label.set_hexpand(true);
        hbox.append(&name_label);

        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&hbox));
        row.set_widget_name(&idx.to_string());
        listbox.append(&row);
    }

    if let Some(row) = listbox.row_at_index(active_idx as i32) {
        listbox.select_row(Some(&row));
    }

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_child(Some(&listbox));
    vbox.append(&scrolled);

    let names = profile_names.clone();
    listbox.connect_row_activated(move |_, row| {
        set_profile(row, &names);
    });

    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("i3More-power-profile")
        .resizable(false)
        .decorated(false)
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .child(&vbox)
        .build();

    let key_ctrl = gtk4::EventControllerKey::new();
    let listbox_ref = listbox.clone();
    let names_key = profile_names;
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        match key {
            gdk::Key::Escape => std::process::exit(0),
            gdk::Key::Return | gdk::Key::KP_Enter => {
                if let Some(row) = listbox_ref.selected_row() {
                    set_profile(&row, &names_key);
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

fn set_profile(row: &gtk4::ListBoxRow, names: &[String]) {
    let idx: usize = row.widget_name().parse().unwrap_or(0);
    if let Some(name) = names.get(idx) {
        log::info!("Setting power profile: {}", name);
        let _ = std::process::Command::new("powerprofilesctl")
            .args(["set", name])
            .output();
        let _ = std::process::Command::new("notify-send")
            .args(["Power Profile", name, "-i", "battery", "-t", "1000",
                   "-h", "string:x-canonical-private-synchronous:power-profile"])
            .spawn();
    }
    std::process::exit(0);
}

fn read_profiles() -> Vec<Profile> {
    let output = match std::process::Command::new("powerprofilesctl").arg("list").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    let mut profiles = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with(':') {
            let active = trimmed.starts_with('*');
            let name = trimmed.trim_start_matches('*').trim().trim_end_matches(':').to_string();
            if !name.is_empty() {
                profiles.push(Profile { name, active });
            }
        }
    }
    profiles
}

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
        .args(["windowmove", "--sync", &xid.to_string(), &x.to_string(), &y.to_string()])
        .output();
}

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
