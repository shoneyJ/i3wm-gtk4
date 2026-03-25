//! i3more-window — window switcher replacing rofi window mode.
//!
//! Queries the i3 tree for all windows and shows a searchable list.
//! Selecting a window focuses it.

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;

const WINDOW_WIDTH: i32 = 500;
const WINDOW_HEIGHT: i32 = 400;

#[derive(Clone)]
struct WindowInfo {
    con_id: i64,
    title: String,
    class: String,
    workspace: String,
}

fn main() {
    i3more::init_logging("i3more-window");

    let app = gtk4::Application::builder()
        .application_id("com.i3more.window")
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

    let windows = Rc::new(collect_windows());
    log::info!("Found {} windows", windows.len());

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    vbox.add_css_class("menu-main");

    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search windows..."));
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

    populate_list(&listbox, &windows, "");

    // Search with debounce
    let debounce_gen: Rc<RefCell<u64>> = Rc::new(RefCell::new(0));
    {
        let windows_ref = windows.clone();
        let listbox_ref = listbox.clone();
        let gen_ref = debounce_gen.clone();
        search_entry.connect_search_changed(move |entry| {
            let gen = {
                let mut g = gen_ref.borrow_mut();
                *g += 1;
                *g
            };
            let query = entry.text().to_string();
            let windows_inner = windows_ref.clone();
            let listbox_inner = listbox_ref.clone();
            let gen_check = gen_ref.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                if *gen_check.borrow() == gen {
                    populate_list(&listbox_inner, &windows_inner, &query);
                }
            });
        });
    }

    // Row activated → focus window
    {
        let windows_ref = windows.clone();
        listbox.connect_row_activated(move |_, row| {
            focus_window(row, &windows_ref);
        });
    }

    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("i3More-window")
        .resizable(false)
        .decorated(false)
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .child(&vbox)
        .build();

    let key_ctrl = gtk4::EventControllerKey::new();
    let search_ref = search_entry.clone();
    let listbox_ref = listbox.clone();
    let windows_key = windows.clone();
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        match key {
            gdk::Key::Escape => std::process::exit(0),
            gdk::Key::Return | gdk::Key::KP_Enter => {
                if let Some(row) = listbox_ref.selected_row() {
                    focus_window(&row, &windows_key);
                }
                glib::Propagation::Stop
            }
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

fn populate_list(listbox: &gtk4::ListBox, windows: &[WindowInfo], query: &str) {
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }

    let query_lower = query.to_lowercase();

    for (idx, win) in windows.iter().enumerate() {
        if !query.is_empty()
            && !win.title.to_lowercase().contains(&query_lower)
            && !win.class.to_lowercase().contains(&query_lower)
            && !win.workspace.to_lowercase().contains(&query_lower)
        {
            continue;
        }

        let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        hbox.add_css_class("menu-row");

        // Workspace tag
        let ws_label = gtk4::Label::new(Some(&win.workspace));
        ws_label.add_css_class("menu-key");
        ws_label.set_width_chars(4);
        ws_label.set_xalign(1.0);
        hbox.append(&ws_label);

        // Window info
        let text_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        text_box.set_hexpand(true);

        let class_label = gtk4::Label::new(Some(&win.class));
        class_label.add_css_class("menu-label");
        class_label.set_halign(gtk4::Align::Start);
        class_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        text_box.append(&class_label);

        if !win.title.is_empty() && win.title != win.class {
            let title_label = gtk4::Label::new(Some(&win.title));
            title_label.add_css_class("menu-desc");
            title_label.set_halign(gtk4::Align::Start);
            title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            text_box.append(&title_label);
        }

        hbox.append(&text_box);

        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&hbox));
        row.set_widget_name(&idx.to_string());
        listbox.append(&row);
    }

    if let Some(first) = listbox.row_at_index(0) {
        listbox.select_row(Some(&first));
    }
}

fn focus_window(row: &gtk4::ListBoxRow, windows: &[WindowInfo]) {
    let idx: usize = row.widget_name().parse().unwrap_or(0);
    if let Some(win) = windows.get(idx) {
        log::info!("Focusing window con_id={} class={}", win.con_id, win.class);
        if let Ok(mut conn) = i3more::ipc::I3Connection::connect() {
            let _ = conn.run_command(&format!("[con_id={}] focus", win.con_id));
        }
    }
    std::process::exit(0);
}

fn collect_windows() -> Vec<WindowInfo> {
    let mut conn = match i3more::ipc::I3Connection::connect() {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to connect to i3: {}", e);
            return Vec::new();
        }
    };
    let tree = match conn.get_tree() {
        Ok(t) => t,
        Err(e) => {
            log::error!("Failed to get i3 tree: {}", e);
            return Vec::new();
        }
    };

    let mut windows = Vec::new();
    walk_tree(&tree, "", &mut windows);
    windows
}

fn walk_tree(node: &Value, workspace: &str, windows: &mut Vec<WindowInfo>) {
    let node_type = node["type"].as_str().unwrap_or("");
    let current_ws = if node_type == "workspace" {
        node["name"].as_str().unwrap_or(workspace)
    } else {
        workspace
    };

    // Leaf window node: has "window" property (X11 window id)
    if node["window"].is_number() {
        let con_id = node["id"].as_i64().unwrap_or(0);
        let title = node["name"].as_str().unwrap_or("").to_string();
        let class = node["window_properties"]["class"].as_str().unwrap_or("").to_string();

        if con_id > 0 && (!title.is_empty() || !class.is_empty()) {
            windows.push(WindowInfo {
                con_id,
                title,
                class,
                workspace: current_ws.to_string(),
            });
        }
    }

    if let Some(nodes) = node["nodes"].as_array() {
        for child in nodes {
            walk_tree(child, current_ws, windows);
        }
    }
    if let Some(floating) = node["floating_nodes"].as_array() {
        for child in floating {
            walk_tree(child, current_ws, windows);
        }
    }
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
