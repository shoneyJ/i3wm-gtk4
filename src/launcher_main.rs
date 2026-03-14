//! i3More-launcher — standalone app launcher replacing rofi.
//!
//! A GTK4 search dialog that finds and launches .desktop applications.
//! Uses single-instance via GTK Application D-Bus activation (toggle on re-run).

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const WINDOW_WIDTH: i32 = 500;
const WINDOW_HEIGHT: i32 = 450;
const ICON_SIZE: i32 = 32;

fn main() {
    i3more::init_logging("i3more-launcher");

    let app = gtk4::Application::builder()
        .application_id("com.i3more.launcher")
        .build();

    app.connect_activate(on_activate);
    app.run();
}

fn on_activate(app: &gtk4::Application) {
    // Toggle: if window already exists, toggle visibility
    if let Some(window) = app.active_window() {
        if window.is_visible() {
            window.set_visible(false);
        } else {
            window.set_visible(true);
            window.present();
            // Re-focus search and clear it
            if let Some(child) = window.child() {
                if let Ok(vbox) = child.downcast::<gtk4::Box>() {
                    if let Some(first) = vbox.first_child() {
                        if let Ok(search) = first.downcast::<gtk4::SearchEntry>() {
                            search.set_text("");
                            search.grab_focus();
                        }
                    }
                }
            }
        }
        return;
    }

    i3more::fa::register_font();
    load_css();

    // Load all desktop entries
    let entries = Rc::new(i3more::launcher::load_entries());
    log::info!("Loaded {} launchable entries", entries.len());

    // Main vertical layout
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    vbox.add_css_class("launcher-main");

    // Search entry
    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search applications..."));
    search_entry.add_css_class("launcher-search");
    search_entry.set_hexpand(true);
    vbox.append(&search_entry);

    // Scrollable list
    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

    let listbox = gtk4::ListBox::new();
    listbox.add_css_class("launcher-list");
    listbox.set_selection_mode(gtk4::SelectionMode::Single);
    scrolled.set_child(Some(&listbox));
    vbox.append(&scrolled);

    // Build initial rows (all entries, capped at 50)
    populate_list(&listbox, &entries, "");

    // Search changed handler with debounce (generation counter to avoid SourceId::remove panic)
    let debounce_gen: Rc<RefCell<u64>> = Rc::new(RefCell::new(0));
    {
        let entries_ref = entries.clone();
        let listbox_ref = listbox.clone();
        let gen_ref = debounce_gen.clone();
        search_entry.connect_search_changed(move |entry| {
            // Bump generation — any pending timeout with an older generation will be a no-op
            let gen = {
                let mut g = gen_ref.borrow_mut();
                *g += 1;
                *g
            };

            let query = entry.text().to_string();
            let entries_inner = entries_ref.clone();
            let listbox_inner = listbox_ref.clone();
            let gen_check = gen_ref.clone();

            glib::timeout_add_local_once(
                std::time::Duration::from_millis(50),
                move || {
                    // Only apply if no newer search has been typed
                    if *gen_check.borrow() == gen {
                        populate_list(&listbox_inner, &entries_inner, &query);
                    }
                },
            );
        });
    }

    // Row activated → launch app and exit
    {
        let entries_ref = entries.clone();
        listbox.connect_row_activated(move |_, row| {
            let idx_str = row.widget_name();
            if let Ok(idx) = idx_str.parse::<usize>() {
                if let Some(entry) = entries_ref.get(idx) {
                    i3more::launcher::launch(entry);
                }
            }
            std::process::exit(0);
        });
    }

    // Enter key in search entry → launch selected row and exit
    {
        let entries_for_activate = entries.clone();
        let listbox_for_activate = listbox.clone();
        search_entry.connect_activate(move |_| {
            log::info!("SearchEntry activate signal fired (Enter pressed)");
            if let Some(row) = listbox_for_activate.selected_row() {
                let idx_str = row.widget_name();
                log::info!("Selected row widget_name: {}", idx_str);
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(e) = entries_for_activate.get(idx) {
                        log::info!("Launching: {}", e.name);
                        i3more::launcher::launch(e);
                    }
                }
            } else {
                log::warn!("No row selected in listbox");
            }
            std::process::exit(0);
        });
    }

    // Create window
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("i3More-launcher")
        .resizable(false)
        .decorated(false)
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .child(&vbox)
        .build();

    // Keyboard handling
    let key_ctrl = gtk4::EventControllerKey::new();
    let search_for_key = search_entry.clone();
    let listbox_for_key = listbox.clone();
    let entries_for_key = entries.clone();
    key_ctrl.connect_key_pressed(move |_, key, _, _modifier| {
        match key {
            gdk::Key::Escape => {
                std::process::exit(0);
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                // Launch selected row from anywhere (search entry or list)
                if let Some(row) = listbox_for_key.selected_row() {
                    let idx_str = row.widget_name();
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if let Some(e) = entries_for_key.get(idx) {
                            log::info!("Launching via Enter key: {}", e.name);
                            i3more::launcher::launch(e);
                        }
                    }
                }
                std::process::exit(0);
            }
            gdk::Key::Down => {
                // Move focus from search to list
                if search_for_key.has_focus() {
                    if let Some(first_row) = listbox_for_key.row_at_index(0) {
                        first_row.grab_focus();
                    }
                }
                glib::Propagation::Proceed
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(key_ctrl);

    // Position on focused monitor
    position_on_focused_monitor(&window);

    // Focus the search entry on show
    let search_focus = search_entry.clone();
    window.connect_show(move |_| {
        search_focus.grab_focus();
    });

    window.present();
    search_entry.grab_focus();
}

/// Populate the listbox with filtered entries.
fn populate_list(
    listbox: &gtk4::ListBox,
    entries: &[i3more::launcher::LauncherEntry],
    query: &str,
) {
    // Remove existing rows
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }

    let filtered = i3more::launcher::filter_entries(entries, query);

    for entry_ref in &filtered {
        // Find the index of this entry in the original slice
        let idx = entries
            .iter()
            .position(|e| std::ptr::eq(e, *entry_ref))
            .unwrap_or(0);

        let row = build_row(entry_ref, idx);
        listbox.append(&row);
    }

    // Select first row
    if let Some(first) = listbox.row_at_index(0) {
        listbox.select_row(Some(&first));
    }
}

/// Build a single list row with icon and app name.
fn build_row(entry: &i3more::launcher::LauncherEntry, idx: usize) -> gtk4::ListBoxRow {
    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    hbox.add_css_class("launcher-row");
    hbox.set_margin_start(4);
    hbox.set_margin_end(4);
    hbox.set_margin_top(2);
    hbox.set_margin_bottom(2);

    // Icon
    let image = match &entry.icon {
        i3more::icon::IconResult::FilePath(path) => gtk4::Image::from_file(path),
        i3more::icon::IconResult::IconName(name) => gtk4::Image::from_icon_name(name),
        i3more::icon::IconResult::NotFound => {
            gtk4::Image::from_icon_name("application-x-executable")
        }
    };
    image.set_pixel_size(ICON_SIZE);
    image.set_valign(gtk4::Align::Center);
    hbox.append(&image);

    // Name and generic name
    let text_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    text_box.set_valign(gtk4::Align::Center);

    let name_label = gtk4::Label::new(Some(&entry.name));
    name_label.add_css_class("launcher-name");
    name_label.set_halign(gtk4::Align::Start);
    name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text_box.append(&name_label);

    if !entry.generic_name.is_empty() {
        let desc_label = gtk4::Label::new(Some(&entry.generic_name));
        desc_label.add_css_class("launcher-desc");
        desc_label.set_halign(gtk4::Align::Start);
        desc_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        text_box.append(&desc_label);
    }

    hbox.append(&text_box);

    let row = gtk4::ListBoxRow::new();
    row.set_child(Some(&hbox));
    row.set_widget_name(&idx.to_string());
    row
}

fn load_css() {
    i3more::css::load_css("launcher.css", include_str!("../assets/launcher.css"));
}

/// Position the window centered on the monitor that has the focused i3 workspace.
fn position_on_focused_monitor(_window: &gtk4::ApplicationWindow) {
    let output_name = match get_focused_output() {
        Some(name) => name,
        None => return,
    };

    let display = match gdk::Display::default() {
        Some(d) => d,
        None => return,
    };

    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        if let Some(obj) = monitors.item(i) {
            if let Ok(monitor) = obj.downcast::<gdk::Monitor>() {
                let connector = monitor.connector().map(|s| s.to_string());
                if connector.as_deref() == Some(&output_name) {
                    let geom = monitor.geometry();
                    let x = geom.x() + (geom.width() - WINDOW_WIDTH) / 2;
                    let y = geom.y() + (geom.height() - WINDOW_HEIGHT) / 2;

                    let win_title = "i3More-launcher".to_string();
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(200),
                        move || {
                            let _ = std::process::Command::new("xdotool")
                                .args([
                                    "search", "--name", &win_title,
                                    "windowmove", &x.to_string(), &y.to_string(),
                                ])
                                .output();
                        },
                    );
                    return;
                }
            }
        }
    }
}

/// Get the output name of the currently focused i3 workspace.
fn get_focused_output() -> Option<String> {
    let mut conn = i3more::ipc::I3Connection::connect().ok()?;
    let workspaces = conn.get_workspaces().ok()?;
    let arr = workspaces.as_array()?;
    for ws in arr {
        if ws["focused"].as_bool() == Some(true) {
            return ws["output"].as_str().map(String::from);
        }
    }
    None
}
