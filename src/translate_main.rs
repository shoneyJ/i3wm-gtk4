//! i3More-translate — standalone translation popup utility.
//!
//! A GTK4 dialog that translates text using `trans` (translate-shell).
//! Uses single-instance via GTK Application D-Bus activation (toggle on re-run).

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use std::rc::Rc;

fn main() {
    i3more::init_logging("i3more-translate");

    let app = gtk4::Application::builder()
        .application_id("com.i3more.translate")
        .build();

    app.connect_activate(on_activate);
    app.run();
}

fn on_activate(app: &gtk4::Application) {
    // Toggle: if window already exists, toggle visibility
    if let Some(window) = app.active_window() {
        window.set_visible(!window.is_visible());
        if window.is_visible() {
            window.present();
        }
        return;
    }

    i3more::fa::register_font();
    load_css();

    // Fetch language list
    let languages = i3more::translate::list_languages();
    let config = i3more::translate::load_config();
    let source_default = config.source_language
        .as_ref()
        .and_then(|saved| languages.iter().position(|l| l == saved))
        .unwrap_or_else(|| languages.iter().position(|l| l == "English").unwrap_or(0));
    let target_default = config.target_language
        .as_ref()
        .and_then(|saved| languages.iter().position(|l| l == saved))
        .unwrap_or_else(|| languages.iter().position(|l| l == "German").unwrap_or(1));

    // Build language StringList
    let lang_list: Vec<&str> = languages.iter().map(|s| s.as_str()).collect();
    let source_model = gtk4::StringList::new(&lang_list);
    let target_model = gtk4::StringList::new(&lang_list);

    // Main vertical layout
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    vbox.add_css_class("translate-main");

    // --- Language selector row ---
    let lang_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    lang_row.add_css_class("lang-row");
    lang_row.set_halign(gtk4::Align::Center);

    let source_dropdown = gtk4::DropDown::new(Some(source_model), gtk4::Expression::NONE);
    source_dropdown.set_enable_search(true);
    source_dropdown.set_selected(source_default as u32);
    source_dropdown.add_css_class("lang-dropdown");
    source_dropdown.set_hexpand(true);

    let swap_button = gtk4::Button::new();
    swap_button.set_label(&format!("{}", i3more::fa::EXCHANGE));
    swap_button.add_css_class("swap-button");

    let target_dropdown = gtk4::DropDown::new(Some(target_model), gtk4::Expression::NONE);
    target_dropdown.set_enable_search(true);
    target_dropdown.set_selected(target_default as u32);
    target_dropdown.add_css_class("lang-dropdown");
    target_dropdown.set_hexpand(true);

    {
        let langs_src = languages.clone();
        source_dropdown.connect_notify(Some("selected"), move |dd, _| {
            let idx = dd.selected() as usize;
            if let Some(lang) = langs_src.get(idx) {
                let mut config = i3more::translate::load_config();
                config.source_language = Some(lang.clone());
                i3more::translate::save_config(&config);
            }
        });

        let langs_tgt = languages.clone();
        target_dropdown.connect_notify(Some("selected"), move |dd, _| {
            let idx = dd.selected() as usize;
            if let Some(lang) = langs_tgt.get(idx) {
                let mut config = i3more::translate::load_config();
                config.target_language = Some(lang.clone());
                i3more::translate::save_config(&config);
            }
        });
    }

    lang_row.append(&source_dropdown);
    lang_row.append(&swap_button);
    lang_row.append(&target_dropdown);
    vbox.append(&lang_row);

    // --- Source text input ---
    let source_scroll = gtk4::ScrolledWindow::new();
    source_scroll.set_min_content_height(120);
    source_scroll.set_vexpand(true);
    source_scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);

    let source_view = gtk4::TextView::new();
    source_view.set_wrap_mode(gtk4::WrapMode::Word);
    source_view.set_editable(true);
    source_view.add_css_class("text-input");
    source_view.set_left_margin(4);
    source_view.set_right_margin(4);
    source_view.set_top_margin(4);
    source_view.set_bottom_margin(4);

    source_scroll.set_child(Some(&source_view));
    vbox.append(&source_scroll);

    // --- Translate button ---
    let translate_btn = gtk4::Button::with_label("Translate");
    translate_btn.add_css_class("translate-button");
    translate_btn.set_halign(gtk4::Align::Center);
    translate_btn.set_margin_top(4);
    translate_btn.set_margin_bottom(4);
    vbox.append(&translate_btn);

    // --- Output text ---
    let output_scroll = gtk4::ScrolledWindow::new();
    output_scroll.set_min_content_height(120);
    output_scroll.set_vexpand(true);
    output_scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);

    let output_view = gtk4::TextView::new();
    output_view.set_wrap_mode(gtk4::WrapMode::Word);
    output_view.set_editable(false);
    output_view.add_css_class("text-output");
    output_view.set_left_margin(4);
    output_view.set_right_margin(4);
    output_view.set_top_margin(4);
    output_view.set_bottom_margin(4);

    output_scroll.set_child(Some(&output_view));
    vbox.append(&output_scroll);

    // --- Action buttons row ---
    let action_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    action_row.add_css_class("action-row");
    action_row.set_halign(gtk4::Align::Center);

    let copy_btn = gtk4::Button::with_label(&format!("{} Copy", i3more::fa::COPY));
    copy_btn.add_css_class("action-button");

    let speak_btn = gtk4::Button::with_label(&format!("{} Speak", i3more::fa::VOLUME_UP));
    speak_btn.add_css_class("action-button");

    let clear_btn = gtk4::Button::with_label(&format!("{} Clear", i3more::fa::ERASER));
    clear_btn.add_css_class("action-button");

    action_row.append(&copy_btn);
    action_row.append(&speak_btn);
    action_row.append(&clear_btn);
    vbox.append(&action_row);

    // --- Create window ---
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("i3More-translate")
        .resizable(true)
        .default_width(400)
        .default_height(420)
        .child(&vbox)
        .build();

    // Position on focused monitor
    position_on_focused_monitor(&window);

    // --- Wire up signals ---

    // Shared state for languages list (needed for speak/swap)
    let languages = Rc::new(languages);

    // Translate button
    let translate_btn_ref = translate_btn.clone();
    let source_buf = source_view.buffer();
    let output_buf = output_view.buffer();
    let src_dd = source_dropdown.clone();
    let tgt_dd = target_dropdown.clone();
    let langs = languages.clone();

    translate_btn.connect_clicked(move |btn| {
        trigger_translate(&source_buf, &output_buf, btn, &langs, &src_dd, &tgt_dd);
    });

    // Swap button: swap languages and text
    let src_dd2 = source_dropdown.clone();
    let tgt_dd2 = target_dropdown.clone();
    let src_view2 = source_view.clone();
    let out_view2 = output_view.clone();

    swap_button.connect_clicked(move |_| {
        // Swap dropdown selections
        let src_idx = src_dd2.selected();
        let tgt_idx = tgt_dd2.selected();
        src_dd2.set_selected(tgt_idx);
        tgt_dd2.set_selected(src_idx);

        // Swap text content
        let src_buf = src_view2.buffer();
        let out_buf = out_view2.buffer();
        let (s1, e1) = src_buf.bounds();
        let (s2, e2) = out_buf.bounds();
        let src_text = src_buf.text(&s1, &e1, false).to_string();
        let out_text = out_buf.text(&s2, &e2, false).to_string();
        src_buf.set_text(&out_text);
        out_buf.set_text(&src_text);
    });

    // Copy button: copy output to clipboard
    let out_view3 = output_view.clone();
    copy_btn.connect_clicked(move |_| {
        let buf = out_view3.buffer();
        let (start, end) = buf.bounds();
        let text = buf.text(&start, &end, false).to_string();
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(&text);
        }
    });

    // Speak button: speak the output text
    let out_view4 = output_view.clone();
    let tgt_dd3 = target_dropdown.clone();
    let langs2 = languages.clone();
    speak_btn.connect_clicked(move |_| {
        let buf = out_view4.buffer();
        let (start, end) = buf.bounds();
        let text = buf.text(&start, &end, false).to_string();
        let lang = langs2[tgt_dd3.selected() as usize].clone();
        i3more::translate::speak(&text, &lang);
    });

    // Clear button
    let src_view3 = source_view.clone();
    let out_view5 = output_view.clone();
    let source_view_focus = source_view.clone();
    clear_btn.connect_clicked(move |_| {
        src_view3.buffer().set_text("");
        out_view5.buffer().set_text("");
        source_view_focus.grab_focus();
    });

    // Ctrl+Enter to translate
    let key_ctrl = gtk4::EventControllerKey::new();
    let translate_btn_key = translate_btn_ref.clone();
    key_ctrl.connect_key_pressed(move |_, key, _, modifier| {
        if key == gdk::Key::Return && modifier.contains(gdk::ModifierType::CONTROL_MASK) {
            translate_btn_key.emit_clicked();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_ctrl);

    {
        let src_buf = source_view.buffer();
        let out_buf = output_view.buffer();
        let btn = translate_btn_ref.clone();
        let langs = languages.clone();
        let s_dd = source_dropdown.clone();
        let t_dd = target_dropdown.clone();

        window.connect_show(move |_| {
            auto_paste_and_translate(&src_buf, &out_buf, &btn, &langs, &s_dd, &t_dd);
        });
    }

    window.present();
}

fn trigger_translate(
    source_buf: &gtk4::TextBuffer,
    output_buf: &gtk4::TextBuffer,
    translate_btn: &gtk4::Button,
    languages: &[String],
    source_dd: &gtk4::DropDown,
    target_dd: &gtk4::DropDown,
) {
    let (start, end) = source_buf.bounds();
    let text = source_buf.text(&start, &end, false).to_string();
    if text.trim().is_empty() {
        return;
    }

    let source_lang = languages[source_dd.selected() as usize].clone();
    let target_lang = languages[target_dd.selected() as usize].clone();

    translate_btn.set_sensitive(false);
    let out_buf = output_buf.clone();
    let btn_clone = translate_btn.clone();

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        let result = i3more::translate::translate(&text, &source_lang, &target_lang);
        let _ = tx.send(result);
    });

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(result) => {
                match result {
                    Ok(translated) => out_buf.set_text(&translated),
                    Err(e) => out_buf.set_text(&format!("Error: {}", e)),
                }
                btn_clone.set_sensitive(true);
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                btn_clone.set_sensitive(true);
                glib::ControlFlow::Break
            }
        }
    });
}

fn auto_paste_and_translate(
    source_buf: &gtk4::TextBuffer,
    output_buf: &gtk4::TextBuffer,
    translate_btn: &gtk4::Button,
    languages: &Rc<Vec<String>>,
    source_dd: &gtk4::DropDown,
    target_dd: &gtk4::DropDown,
) {
    // Try X11 primary selection first (highlighted text) via xclip
    let primary_text = std::process::Command::new("xclip")
        .args(["-selection", "primary", "-o"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .filter(|s| !s.trim().is_empty());

    if let Some(text) = primary_text {
        source_buf.set_text(&text);
        trigger_translate(source_buf, output_buf, translate_btn, languages, source_dd, target_dd);
        return;
    }

    // Fall back to clipboard (copied text)
    let display = match gdk::Display::default() {
        Some(d) => d,
        None => return,
    };
    let clipboard = display.clipboard();

    let src_buf = source_buf.clone();
    let out_buf = output_buf.clone();
    let btn = translate_btn.clone();
    let langs = languages.clone();
    let s_dd = source_dd.clone();
    let t_dd = target_dd.clone();

    clipboard.read_text_async(gtk4::gio::Cancellable::NONE, move |result| {
        if let Ok(Some(text)) = result {
            let text = text.to_string();
            if !text.trim().is_empty() {
                src_buf.set_text(&text);
                trigger_translate(&src_buf, &out_buf, &btn, &langs, &s_dd, &t_dd);
            }
        }
    });
}

fn load_css() {
    i3more::css::load_css("translate.css", include_str!("../assets/translate.css"));
}

/// Position the window centered on the monitor that has the focused i3 workspace.
fn position_on_focused_monitor(_window: &gtk4::ApplicationWindow) {
    // Query i3 for the focused workspace's output
    let output_name = match get_focused_output() {
        Some(name) => name,
        None => return, // Fall back to default placement
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
                    let x = geom.x() + (geom.width() - 400) / 2;
                    let y = geom.y() + (geom.height() - 420) / 2;

                    // Use xdotool as X11 fallback for positioning
                    let win_title = "i3More-translate".to_string();
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
