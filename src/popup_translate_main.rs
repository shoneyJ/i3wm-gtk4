//! i3more-popup-translate — ephemeral translation popup near the cursor.
//!
//! Reads X11 primary selection, translates it using the saved language config,
//! and shows a small popup near the mouse cursor that auto-dismisses after 5 seconds.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

const POPUP_WIDTH: i32 = 300;
const DISMISS_MS: u64 = 5000;
const TICK_MS: u64 = 100;

fn main() {
    i3more::init_logging("i3more-popup-translate");

    // Read primary selection (highlighted text) — exit if empty.
    let selected_text = match read_primary_selection() {
        Some(t) => t,
        None => return,
    };

    // Get mouse position for popup placement.
    let (mouse_x, mouse_y) = get_mouse_position().unwrap_or((100, 100));

    // Load saved language preferences.
    let config = i3more::translate::load_config();
    let source_lang = config
        .source_language
        .unwrap_or_else(|| "English".to_string());
    let target_lang = config
        .target_language
        .unwrap_or_else(|| "German".to_string());

    // Resolve short codes for the language label (e.g. "English → German").
    let lang_label_text = format!("{} → {}", source_lang, target_lang);

    let app = gtk4::Application::builder().build();

    let text = selected_text.clone();
    let src = source_lang.clone();
    let tgt = target_lang.clone();

    app.connect_activate(move |app| {
        i3more::fa::register_font();
        load_css();

        let win_title = "i3more-popup-translate";

        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title(win_title)
            .decorated(false)
            .resizable(false)
            .default_width(POPUP_WIDTH)
            .build();
        window.add_css_class("popup-translate");

        // Set X11 properties: notification type (auto-float) + no focus steal.
        window.connect_realize(|win| {
            let surface = match win.surface() {
                Some(s) => s,
                None => return,
            };
            let x11_surface = match surface.downcast::<gdk4_x11::X11Surface>() {
                Ok(s) => s,
                Err(_) => return,
            };
            let xid = x11_surface.xid().to_string();

            let _ = std::process::Command::new("xprop")
                .args([
                    "-id", &xid,
                    "-f", "_NET_WM_WINDOW_TYPE", "32a",
                    "-set", "_NET_WM_WINDOW_TYPE", "_NET_WM_WINDOW_TYPE_NOTIFICATION",
                ])
                .output();

            let _ = std::process::Command::new("xprop")
                .args([
                    "-id", &xid,
                    "-f", "_NET_WM_USER_TIME", "32c",
                    "-set", "_NET_WM_USER_TIME", "0",
                ])
                .output();
        });

        // Build layout.
        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 2);

        let lang_label = gtk4::Label::new(Some(&lang_label_text));
        lang_label.add_css_class("popup-lang");
        lang_label.set_halign(gtk4::Align::Start);
        vbox.append(&lang_label);

        let text_label = gtk4::Label::new(Some("Translating..."));
        text_label.add_css_class("popup-loading");
        text_label.set_halign(gtk4::Align::Start);
        text_label.set_wrap(true);
        text_label.set_max_width_chars(50);
        text_label.set_selectable(true);
        vbox.append(&text_label);

        window.set_child(Some(&vbox));

        // Hover-aware dismiss: pause timer while mouse is over the popup.
        let hovered = Rc::new(Cell::new(false));

        let hover_ctrl = gtk4::EventControllerMotion::new();
        let h = hovered.clone();
        hover_ctrl.connect_enter(move |_, _, _| {
            h.set(true);
        });
        let h = hovered.clone();
        hover_ctrl.connect_leave(move |_| {
            h.set(false);
        });
        window.add_controller(hover_ctrl);

        // Capture focused window to restore focus after popup maps.
        let focused_xid = std::process::Command::new("xdotool")
            .args(["getactivewindow"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok());

        // Position popup near cursor after window maps.
        let mx = mouse_x;
        let my = mouse_y;
        let title = win_title.to_string();

        window.connect_map(move |_| {
            let title = title.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                let x = mx;
                let y = my + 15;
                let criteria = format!("[title=\"{}\"]", title);
                let cmd = format!(
                    "{criteria} move position {x} px {y} px"
                );
                let _ = std::process::Command::new("i3-msg")
                    .args([&cmd])
                    .output();

                if let Some(prev_xid) = focused_xid {
                    let _ = std::process::Command::new("i3-msg")
                        .args([&format!("[id={}] focus", prev_xid)])
                        .output();
                }
            });
        });

        window.present();

        // Spawn off-thread translation.
        let text_to_translate = text.clone();
        let src_lang = src.clone();
        let tgt_lang = tgt.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();

        std::thread::spawn(move || {
            let result =
                i3more::translate::translate(&text_to_translate, &src_lang, &tgt_lang);
            let _ = tx.send(result);
        });

        let label = text_label.clone();
        let win = window.clone();

        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            match rx.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(translated) => {
                            label.remove_css_class("popup-loading");
                            label.add_css_class("popup-text");
                            label.set_text(&translated);
                        }
                        Err(e) => {
                            label.set_text(&format!("Error: {}", e));
                        }
                    }

                    // Auto-dismiss timer: ticks down, pauses while hovered.
                    let remaining = Rc::new(Cell::new(DISMISS_MS));
                    let w = win.clone();
                    let h = hovered.clone();
                    glib::timeout_add_local(
                        std::time::Duration::from_millis(TICK_MS),
                        move || {
                            if !h.get() {
                                let left = remaining.get().saturating_sub(TICK_MS);
                                remaining.set(left);
                                if left == 0 {
                                    w.close();
                                    return glib::ControlFlow::Break;
                                }
                            }
                            glib::ControlFlow::Continue
                        },
                    );

                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => {
                    win.close();
                    glib::ControlFlow::Break
                }
            }
        });
    });

    app.run_with_args::<String>(&[]);
}

/// Read X11 primary selection via xclip.
fn read_primary_selection() -> Option<String> {
    std::process::Command::new("xclip")
        .args(["-selection", "primary", "-o"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .filter(|s| !s.trim().is_empty())
}

/// Get mouse cursor position via xdotool.
fn get_mouse_position() -> Option<(i32, i32)> {
    let output = std::process::Command::new("xdotool")
        .arg("getmouselocation")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut x = None;
    let mut y = None;
    for part in text.split_whitespace() {
        if let Some(val) = part.strip_prefix("x:") {
            x = val.parse().ok();
        } else if let Some(val) = part.strip_prefix("y:") {
            y = val.parse().ok();
        }
    }
    Some((x?, y?))
}

fn load_css() {
    i3more::css::load_css(
        "popup-translate.css",
        include_str!("../assets/popup-translate.css"),
    );
}
