//! i3more-keyhint — searchable keybinding viewer replacing rofi keyhint.
//!
//! Parses the i3 config for bindsym lines and displays them in a searchable list.

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const WINDOW_WIDTH: i32 = 600;
const WINDOW_HEIGHT: i32 = 500;

struct Keybinding {
    keys: String,
    action: String,
}

fn main() {
    i3more::init_logging("i3more-keyhint");

    let app = gtk4::Application::builder()
        .application_id("com.i3more.keyhint")
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

    let bindings = Rc::new(parse_keybindings());
    log::info!("Loaded {} keybindings", bindings.len());

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    vbox.add_css_class("menu-main");

    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search keybindings..."));
    search_entry.add_css_class("menu-search");
    search_entry.set_hexpand(true);
    vbox.append(&search_entry);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

    let listbox = gtk4::ListBox::new();
    listbox.add_css_class("menu-list");
    listbox.set_selection_mode(gtk4::SelectionMode::Single);
    scrolled.set_child(Some(&listbox));
    vbox.append(&scrolled);

    populate_list(&listbox, &bindings, "");

    // Search with debounce
    let debounce_gen: Rc<RefCell<u64>> = Rc::new(RefCell::new(0));
    {
        let bindings_ref = bindings.clone();
        let listbox_ref = listbox.clone();
        let gen_ref = debounce_gen.clone();
        search_entry.connect_search_changed(move |entry| {
            let gen = {
                let mut g = gen_ref.borrow_mut();
                *g += 1;
                *g
            };
            let query = entry.text().to_string();
            let bindings_inner = bindings_ref.clone();
            let listbox_inner = listbox_ref.clone();
            let gen_check = gen_ref.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                if *gen_check.borrow() == gen {
                    populate_list(&listbox_inner, &bindings_inner, &query);
                }
            });
        });
    }

    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("i3More-keyhint")
        .resizable(false)
        .decorated(false)
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .child(&vbox)
        .build();

    let key_ctrl = gtk4::EventControllerKey::new();
    let search_ref = search_entry.clone();
    let listbox_ref = listbox.clone();
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        match key {
            gdk::Key::Escape => std::process::exit(0),
            gdk::Key::Return | gdk::Key::KP_Enter => std::process::exit(0),
            gdk::Key::Down => {
                if search_ref.has_focus() {
                    if let Some(first) = listbox_ref.row_at_index(0) {
                        first.grab_focus();
                    }
                }
                glib::Propagation::Proceed
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(key_ctrl);

    let (target_x, target_y) = compute_position_center();
    window.connect_realize(move |win| {
        set_x11_position(win, target_x, target_y);
    });

    let search_focus = search_entry.clone();
    window.connect_show(move |_| { search_focus.grab_focus(); });

    window.present();
    search_entry.grab_focus();
}

fn populate_list(listbox: &gtk4::ListBox, bindings: &[Keybinding], query: &str) {
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }

    let query_lower = query.to_lowercase();

    for binding in bindings {
        if !query.is_empty()
            && !binding.keys.to_lowercase().contains(&query_lower)
            && !binding.action.to_lowercase().contains(&query_lower)
        {
            continue;
        }

        let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        hbox.add_css_class("menu-row");

        let key_label = gtk4::Label::new(Some(&binding.keys));
        key_label.add_css_class("menu-key");
        key_label.set_halign(gtk4::Align::Start);
        key_label.set_width_chars(24);
        key_label.set_xalign(0.0);
        hbox.append(&key_label);

        let action_label = gtk4::Label::new(Some(&binding.action));
        action_label.add_css_class("menu-desc");
        action_label.set_halign(gtk4::Align::Start);
        action_label.set_hexpand(true);
        action_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        hbox.append(&action_label);

        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&hbox));
        listbox.append(&row);
    }

    if let Some(first) = listbox.row_at_index(0) {
        listbox.select_row(Some(&first));
    }
}

fn parse_keybindings() -> Vec<Keybinding> {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/i3/config");

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to read i3 config: {}", e);
            return Vec::new();
        }
    };

    // Read $mod variable
    let mod_key = content.lines()
        .find_map(|line| {
            let t = line.trim();
            if t.starts_with("set $mod ") {
                Some(t.strip_prefix("set $mod ")?.to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Mod4".to_string());

    let mut bindings = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("bindsym ") {
            continue;
        }
        let rest = &trimmed["bindsym ".len()..];
        let mut parts = rest.splitn(2, ' ');
        let keys = match parts.next() {
            Some(k) => k.replace("$mod", &mod_key).replace("Mod1", "Alt"),
            None => continue,
        };
        let action = match parts.next() {
            Some(a) => a
                .replace("exec --no-startup-id ", "")
                .replace("exec ", "")
                .trim()
                .to_string(),
            None => continue,
        };

        if action.is_empty() {
            continue;
        }

        bindings.push(Keybinding { keys, action });
    }

    bindings
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

fn compute_position_center() -> (i32, i32) {
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
                    let x = geom.x() + (geom.width() - WINDOW_WIDTH) / 2;
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
