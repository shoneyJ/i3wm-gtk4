//! i3More-speech-text-ui — standalone GTK4 viewer for the speech-to-text
//! feature. Toggles the headless `i3more-speech-text` engine, follows
//! the active session's transcript file with `gio::FileMonitor` and
//! renders each `(German, English)` pair into an auto-scrolling list,
//! and exposes a session-history dropdown that swaps the live view for
//! a past session.
//!
//! Single-instance via GTK4 `Application` D-Bus activation: re-running
//! the binary toggles window visibility.
//!
//! Pure UI — does NOT load whisper. The engine binary
//! (`i3more-speech-text`) does the actual capture + inference; this
//! process only spawns/kills it and tails its on-disk transcript.

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

const WINDOW_WIDTH: i32 = 720;
const WINDOW_HEIGHT: i32 = 640;
const ENGINE_BINARY: &str = "i3more-speech-text";

fn main() {
    i3more::init_logging("i3more-speech-text-ui");

    let app = gtk4::Application::builder()
        .application_id("com.i3more.speechtext")
        .build();

    app.connect_activate(on_activate);
    app.run();
}

fn on_activate(app: &gtk4::Application) {
    // Toggle on re-run.
    if let Some(window) = app.active_window() {
        window.set_visible(!window.is_visible());
        if window.is_visible() {
            window.present();
        }
        return;
    }

    // First launch — if a stale `i3more-speech-text` engine is already
    // running (left over from a prior invocation, or spawned outside
    // this UI), kill it so the user sees a clean Start state with an
    // editable session-name entry. Without this, the periodic poll
    // immediately flips the UI into "Stop" mode which is confusing.
    if engine_running() {
        log::info!("UI launch: terminating stale engine");
        kill_engine();
        // Brief pause so the periodic poll, on its first tick, sees
        // the engine gone and the UI stays in Start state.
        std::thread::sleep(Duration::from_millis(300));
    }

    i3more::fa::register_font();
    i3more::css::load_css(
        "speech-text-ui.css",
        include_str!("../assets/speech-text-ui.css"),
    );

    // ---------- Top bar ---------------------------------------------------

    let top_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    top_bar.add_css_class("stt-top-bar");
    top_bar.set_margin_start(10);
    top_bar.set_margin_end(10);
    top_bar.set_margin_top(8);
    top_bar.set_margin_bottom(4);

    let toggle_btn = gtk4::Button::with_label("Start");
    toggle_btn.add_css_class("stt-toggle");
    top_bar.append(&toggle_btn);

    let session_entry = gtk4::Entry::new();
    session_entry.set_placeholder_text(Some("session name (e.g. daily-standup)"));
    session_entry.set_hexpand(true);
    session_entry.add_css_class("stt-session-entry");
    top_bar.append(&session_entry);

    let history_btn = gtk4::Button::with_label("History ▾");
    history_btn.add_css_class("stt-history-btn");
    top_bar.append(&history_btn);

    // ---------- Status bar ------------------------------------------------

    let status = gtk4::Label::new(Some("idle"));
    status.set_halign(gtk4::Align::Start);
    status.add_css_class("stt-status");
    status.set_margin_start(12);
    status.set_margin_end(12);

    // ---------- Transcript view (split: DE top, EN bottom) ----------------

    let listbox_de = gtk4::ListBox::new();
    listbox_de.add_css_class("stt-list-de");
    listbox_de.set_selection_mode(gtk4::SelectionMode::None);

    let scrolled_de = gtk4::ScrolledWindow::new();
    scrolled_de.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled_de.set_vexpand(true);
    scrolled_de.set_child(Some(&listbox_de));
    scrolled_de.add_css_class("stt-scroll-de");

    let listbox_en = gtk4::ListBox::new();
    listbox_en.add_css_class("stt-list-en");
    listbox_en.set_selection_mode(gtk4::SelectionMode::None);

    let scrolled_en = gtk4::ScrolledWindow::new();
    scrolled_en.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled_en.set_vexpand(true);
    scrolled_en.set_child(Some(&listbox_en));
    scrolled_en.add_css_class("stt-scroll-en");

    let paned = gtk4::Paned::new(gtk4::Orientation::Vertical);
    paned.set_start_child(Some(&scrolled_de));
    paned.set_end_child(Some(&scrolled_en));
    paned.set_resize_start_child(true);
    paned.set_resize_end_child(true);
    // Equal split on first show; user can drag the divider afterwards.
    // Subtract a rough top-bar+status allowance (~80 px) so it lands
    // near vertical-centre of the actual content area.
    paned.set_position((WINDOW_HEIGHT - 80) / 2);
    paned.set_vexpand(true);
    paned.add_css_class("stt-paned");

    // ---------- Outer layout ----------------------------------------------

    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    outer.append(&top_bar);
    outer.append(&status);
    outer.append(&paned);

    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("i3More-speechtext")
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .child(&outer)
        .build();

    // Shared state.
    let active_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let last_byte: Rc<RefCell<u64>> = Rc::new(RefCell::new(0));
    let monitor: Rc<RefCell<Option<gio::FileMonitor>>> = Rc::new(RefCell::new(None));

    // ---------- Toggle handler --------------------------------------------

    {
        let session_entry = session_entry.clone();
        let toggle_btn_clone = toggle_btn.clone();
        let status = status.clone();
        let listbox_de = listbox_de.clone();
        let listbox_en = listbox_en.clone();
        let scrolled_de = scrolled_de.clone();
        let scrolled_en = scrolled_en.clone();
        let active_path = active_path.clone();
        let last_byte = last_byte.clone();
        let monitor = monitor.clone();
        toggle_btn.connect_clicked(move |_| {
            if engine_running() {
                kill_engine();
                status.set_text("stopped");
                toggle_btn_clone.set_label("Start");
                session_entry.set_editable(true);
                session_entry.set_can_focus(true);
            } else {
                let raw = session_entry.text().to_string();
                let resolved = resolve_session_name(&raw);
                session_entry.set_text(&resolved);
                session_entry.set_editable(false);
                session_entry.set_can_focus(false);

                if let Err(e) = spawn_engine(&resolved) {
                    status.set_text(&format!("spawn failed: {}", e));
                    session_entry.set_editable(true);
                    session_entry.set_can_focus(true);
                    return;
                }
                toggle_btn_clone.set_label("Stop");
                status.set_text(&format!("recording — {}", resolved));

                // Open + monitor the transcript file.
                let Some(path) = transcript_path_for(&resolved) else {
                    return;
                };
                clear_listbox(&listbox_de);
                clear_listbox(&listbox_en);
                *active_path.borrow_mut() = Some(path.clone());
                *last_byte.borrow_mut() = 0;
                attach_file_monitor(
                    &path,
                    &listbox_de,
                    &listbox_en,
                    &scrolled_de,
                    &scrolled_en,
                    &last_byte,
                    &monitor,
                );
            }
        });
    }

    // ---------- History dropdown -----------------------------------------

    {
        let listbox_de = listbox_de.clone();
        let listbox_en = listbox_en.clone();
        let scrolled_de = scrolled_de.clone();
        let scrolled_en = scrolled_en.clone();
        let status = status.clone();
        let active_path = active_path.clone();
        let last_byte = last_byte.clone();
        let monitor = monitor.clone();
        let session_entry = session_entry.clone();
        history_btn.connect_clicked(move |btn| {
            // Build the popover lazily so it always reflects the current
            // contents of ~/.local/share/i3more/stt/.
            let popover = gtk4::Popover::new();
            let plist = gtk4::ListBox::new();
            plist.set_selection_mode(gtk4::SelectionMode::None);
            plist.add_css_class("stt-history-list");
            for entry in list_sessions() {
                let row = gtk4::ListBoxRow::new();
                let lbl = gtk4::Label::new(Some(&format!(
                    "{}  ·  {}",
                    entry.date, entry.session
                )));
                lbl.set_halign(gtk4::Align::Start);
                lbl.set_margin_start(8);
                lbl.set_margin_end(8);
                lbl.set_margin_top(4);
                lbl.set_margin_bottom(4);
                row.set_child(Some(&lbl));
                {
                    let path = entry.path.clone();
                    let listbox_de_inner = listbox_de.clone();
                    let listbox_en_inner = listbox_en.clone();
                    let scrolled_de_inner = scrolled_de.clone();
                    let scrolled_en_inner = scrolled_en.clone();
                    let status_inner = status.clone();
                    let active_path_inner = active_path.clone();
                    let last_byte_inner = last_byte.clone();
                    let monitor_inner = monitor.clone();
                    let session_inner = entry.session.clone();
                    let popover_clone = popover.clone();
                    let session_entry_inner = session_entry.clone();
                    let click = gtk4::GestureClick::new();
                    click.connect_released(move |_, _, _, _| {
                        clear_listbox(&listbox_de_inner);
                        clear_listbox(&listbox_en_inner);
                        *last_byte_inner.borrow_mut() = 0;
                        *active_path_inner.borrow_mut() = Some(path.clone());
                        // Detach any prior monitor — we're now showing
                        // a static historical session.
                        *monitor_inner.borrow_mut() = None;
                        // Render whole file once.
                        if let Ok(meta) = std::fs::metadata(&path) {
                            *last_byte_inner.borrow_mut() = meta.len();
                        }
                        let segments = parse_transcript(&path);
                        for seg in &segments {
                            append_de_row(&listbox_de_inner, seg);
                            if seg.en.is_some() {
                                append_en_row(&listbox_en_inner, seg);
                            }
                        }
                        scroll_to_bottom(&scrolled_de_inner);
                        scroll_to_bottom(&scrolled_en_inner);
                        status_inner.set_text(&format!("viewing — {}", session_inner));
                        session_entry_inner.set_text(&session_inner);
                        popover_clone.popdown();
                    });
                    row.add_controller(click);
                }
                plist.append(&row);
            }
            let scrolled_pop = gtk4::ScrolledWindow::new();
            scrolled_pop.set_policy(
                gtk4::PolicyType::Never,
                gtk4::PolicyType::Automatic,
            );
            scrolled_pop.set_min_content_height(280);
            scrolled_pop.set_min_content_width(320);
            scrolled_pop.set_child(Some(&plist));
            popover.set_child(Some(&scrolled_pop));
            popover.set_parent(btn);
            popover.popup();
        });
    }

    // ---------- Periodic engine-state poll --------------------------------

    {
        let toggle_btn = toggle_btn.clone();
        let session_entry = session_entry.clone();
        let status = status.clone();
        glib::timeout_add_local(Duration::from_secs(2), move || {
            let running = engine_running();
            let label_now = toggle_btn
                .label()
                .map(|s| s.to_string())
                .unwrap_or_default();
            if running && label_now != "Stop" {
                toggle_btn.set_label("Stop");
                session_entry.set_editable(false);
                session_entry.set_can_focus(false);
                if !status.text().contains("recording") {
                    status.set_text("recording (started elsewhere)");
                }
            } else if !running && label_now != "Start" {
                toggle_btn.set_label("Start");
                session_entry.set_editable(true);
                session_entry.set_can_focus(true);
                if !status.text().contains("error") && !status.text().contains("viewing") {
                    status.set_text("idle");
                }
            }
            glib::ControlFlow::Continue
        });
    }

    window.present();
}

// ============================================================================
// Engine control — pgrep / spawn / SIGTERM
// ============================================================================

/// Find PIDs whose argv[0]'s basename is exactly `i3more-speech-text`.
/// Robust against:
/// - The UI's own argv[0] basename `i3more-speech-text-ui` (substring
///   match would catch this).
/// - Any shell whose command line contains the engine path as an arg.
/// - The 15-char truncation of `/proc/<pid>/comm` (both engine and UI
///   truncate to the same `i3more-speech-t`).
fn engine_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(proc_dir) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in proc_dir.flatten() {
        let Some(name_os) = entry.file_name().into_string().ok() else {
            continue;
        };
        let Ok(pid) = name_os.parse::<u32>() else {
            continue;
        };
        let cmdline_bytes = match std::fs::read(format!("/proc/{}/cmdline", pid)) {
            Ok(b) => b,
            Err(_) => continue,
        };
        // argv[0] is up to the first NUL byte. Empty cmdline = kernel thread.
        if cmdline_bytes.is_empty() {
            continue;
        }
        let argv0_end = cmdline_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(cmdline_bytes.len());
        let Ok(argv0) = std::str::from_utf8(&cmdline_bytes[..argv0_end]) else {
            continue;
        };
        let basename = argv0.rsplit('/').next().unwrap_or(argv0);
        if basename == ENGINE_BINARY {
            pids.push(pid);
        }
    }
    pids
}

fn engine_running() -> bool {
    !engine_pids().is_empty()
}

fn kill_engine() {
    for pid in engine_pids() {
        // SIGTERM via libc::kill avoids spawning yet another `kill` shell
        // and (importantly) cannot be confused by command-line matching.
        unsafe {
            let _ = libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
}

fn spawn_engine(session_name: &str) -> Result<(), String> {
    let dist_path = dirs::home_dir()
        .map(|h| h.join("projects/github/shoneyj/i3More/dist/i3more-speech-text"))
        .filter(|p| p.exists());
    let mut cmd = match dist_path {
        Some(p) => Command::new(p),
        None => Command::new(ENGINE_BINARY),
    };
    if !session_name.is_empty() {
        cmd.env("I3MORE_STT_SESSION", session_name);
    }
    // Force standard XDG paths regardless of how this UI was launched.
    if let Some(home) = dirs::home_dir() {
        cmd.env("XDG_DATA_HOME", home.join(".local/share"));
        cmd.env("XDG_CONFIG_HOME", home.join(".config"));
        cmd.env("XDG_CACHE_HOME", home.join(".cache"));
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    cmd.spawn().map(|_| ()).map_err(|e| format!("{}", e))
}

// ============================================================================
// Session resolution + transcript path (mirrors src/speech_text.rs)
// ============================================================================

fn resolve_session_name(raw: &str) -> String {
    let s = sanitise(raw);
    if !s.is_empty() {
        return s;
    }
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&secs, &mut tm);
    }
    format!(
        "untitled-{:04}-{:02}-{:02}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

fn sanitise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('-');
        }
    }
    let mut prev_dash = false;
    let collapsed: String = out
        .chars()
        .filter(|&c| {
            let dash = c == '-';
            let keep = !(dash && prev_dash);
            prev_dash = dash;
            keep
        })
        .collect();
    collapsed.trim_matches('-').to_string()
}

fn transcript_path_for(resolved_name: &str) -> Option<PathBuf> {
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&secs, &mut tm);
    }
    let date = format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    );
    Some(stt_root()?.join(date).join(format!("{}.md", resolved_name)))
}

fn stt_root() -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join(".local/share/i3more/stt"),
    )
}

// ============================================================================
// Transcript parsing + rendering
// ============================================================================

#[derive(Debug, Clone)]
struct Segment {
    timestamp: String,
    de: String,
    en: Option<String>,
}

/// Parse the markdown transcript file into Segments.
/// German line:  `- **HH:MM:SS** — text`
/// English line: `  - _text_`
fn parse_transcript(path: &PathBuf) -> Vec<Segment> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    parse_transcript_str(&text)
}

fn parse_transcript_str(text: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(seg) = parse_de_line(line) {
            out.push(seg);
        } else if let Some(en) = parse_en_line(line) {
            if let Some(last) = out.last_mut() {
                if last.en.is_none() {
                    last.en = Some(en);
                }
            }
        }
    }
    out
}

fn parse_de_line(line: &str) -> Option<Segment> {
    // `- **HH:MM:SS** — <de text>`  (em-dash is `\u{2014}`)
    let line = line.trim_start();
    let rest = line.strip_prefix("- **")?;
    let close = rest.find("**")?;
    let timestamp = rest[..close].to_string();
    let after = &rest[close + 2..];
    let after = after.trim_start();
    // Skip the dash/em-dash separator.
    let de = after
        .trim_start_matches(|c: char| c == '-' || c == '\u{2014}' || c.is_whitespace())
        .to_string();
    if de.is_empty() {
        return None;
    }
    Some(Segment {
        timestamp,
        de,
        en: None,
    })
}

fn parse_en_line(line: &str) -> Option<String> {
    // `  - _<en text>_`
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("- _")?;
    let stripped = rest.strip_suffix('_').unwrap_or(rest);
    Some(stripped.to_string())
}

/// Append a German-only row to the top pane. No timestamp; the goal is
/// to read the transcript as flowing prose.
fn append_de_row(listbox: &gtk4::ListBox, seg: &Segment) {
    let de = gtk4::Label::new(Some(&seg.de));
    de.add_css_class("stt-de");
    de.set_halign(gtk4::Align::Start);
    de.set_wrap(true);
    de.set_xalign(0.0);
    de.set_hexpand(true);
    de.set_margin_start(12);
    de.set_margin_end(12);
    de.set_margin_top(4);
    de.set_margin_bottom(4);
    let row = gtk4::ListBoxRow::new();
    row.set_child(Some(&de));
    listbox.append(&row);
}

/// Append an English-only row to the bottom pane. No timestamp.
fn append_en_row(listbox: &gtk4::ListBox, seg: &Segment) {
    let en = gtk4::Label::new(seg.en.as_deref());
    en.add_css_class("stt-en");
    en.set_halign(gtk4::Align::Start);
    en.set_wrap(true);
    en.set_xalign(0.0);
    en.set_hexpand(true);
    en.set_margin_start(12);
    en.set_margin_end(12);
    en.set_margin_top(4);
    en.set_margin_bottom(4);
    let row = gtk4::ListBoxRow::new();
    row.set_child(Some(&en));
    listbox.append(&row);
}

fn clear_listbox(listbox: &gtk4::ListBox) {
    while let Some(row) = listbox.first_child() {
        listbox.remove(&row);
    }
}

fn scroll_to_bottom(scrolled: &gtk4::ScrolledWindow) {
    if let Some(adj) = scrolled.vadjustment().into() {
        let _ = adj;
        let adj = scrolled.vadjustment();
        adj.set_value(adj.upper() - adj.page_size());
    }
}

// ============================================================================
// File monitor — pushes new segments into the listbox as they're written
// ============================================================================

fn attach_file_monitor(
    path: &PathBuf,
    listbox_de: &gtk4::ListBox,
    listbox_en: &gtk4::ListBox,
    scrolled_de: &gtk4::ScrolledWindow,
    scrolled_en: &gtk4::ScrolledWindow,
    last_byte: &Rc<RefCell<u64>>,
    monitor_slot: &Rc<RefCell<Option<gio::FileMonitor>>>,
) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let gfile = gio::File::for_path(path);
    let monitor = match gfile.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
        Ok(m) => m,
        Err(e) => {
            log::error!("file monitor: {}", e);
            return;
        }
    };

    let listbox_de_c = listbox_de.clone();
    let listbox_en_c = listbox_en.clone();
    let scrolled_de_c = scrolled_de.clone();
    let scrolled_en_c = scrolled_en.clone();
    let last_byte_c = last_byte.clone();
    let path_c = path.clone();
    monitor.connect_changed(move |_, _, _, evt| {
        if matches!(
            evt,
            gio::FileMonitorEvent::Changed
                | gio::FileMonitorEvent::ChangesDoneHint
                | gio::FileMonitorEvent::Created
        ) {
            tail_into_panes(
                &path_c,
                &listbox_de_c,
                &listbox_en_c,
                &scrolled_de_c,
                &scrolled_en_c,
                &last_byte_c,
            );
        }
    });

    *monitor_slot.borrow_mut() = Some(monitor);

    // Initial pass in case the file already has content.
    tail_into_panes(
        path,
        listbox_de,
        listbox_en,
        scrolled_de,
        scrolled_en,
        last_byte,
    );
}

/// Re-render both panes from the current file. The DE pane has one row
/// per German segment; the EN pane has one row per segment that has a
/// translation. Auto-scrolls both on growth.
fn tail_into_panes(
    path: &PathBuf,
    listbox_de: &gtk4::ListBox,
    listbox_en: &gtk4::ListBox,
    scrolled_de: &gtk4::ScrolledWindow,
    scrolled_en: &gtk4::ScrolledWindow,
    last_byte: &Rc<RefCell<u64>>,
) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let size = meta.len();
    let prev = *last_byte.borrow();
    if size <= prev {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let segments = parse_transcript_str(&text);

    // DE pane — append any new German rows.
    let existing_de = listbox_row_count(listbox_de);
    if segments.len() > existing_de {
        for seg in &segments[existing_de..] {
            append_de_row(listbox_de, seg);
        }
        scroll_to_bottom(scrolled_de);
    }

    // EN pane — count rows that should currently exist (segments with
    // a translation), append any new ones, and re-render the last row
    // if the latest segment just gained its translation.
    let translated: Vec<&Segment> =
        segments.iter().filter(|s| s.en.is_some()).collect();
    let existing_en = listbox_row_count(listbox_en);
    if translated.len() > existing_en {
        for seg in &translated[existing_en..] {
            append_en_row(listbox_en, seg);
        }
        scroll_to_bottom(scrolled_en);
    }

    *last_byte.borrow_mut() = size;
}

fn listbox_row_count(listbox: &gtk4::ListBox) -> usize {
    let mut n = 0;
    let mut cur = listbox.first_child();
    while let Some(c) = cur {
        n += 1;
        cur = c.next_sibling();
    }
    n
}

// ============================================================================
// Session history listing
// ============================================================================

#[derive(Debug, Clone)]
struct HistoryEntry {
    date: String,
    session: String,
    path: PathBuf,
}

fn list_sessions() -> Vec<HistoryEntry> {
    let Some(root) = stt_root() else {
        return vec![];
    };
    let Ok(date_iter) = std::fs::read_dir(&root) else {
        return vec![];
    };
    let mut out: Vec<HistoryEntry> = Vec::new();
    for date_entry in date_iter.flatten() {
        let date_path = date_entry.path();
        if !date_path.is_dir() {
            continue;
        }
        let date = date_entry.file_name().to_string_lossy().into_owned();
        let Ok(file_iter) = std::fs::read_dir(&date_path) else {
            continue;
        };
        for f in file_iter.flatten() {
            let p = f.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // Skip *-summary.md files (those are Claude post-processed).
            if stem.ends_with("-summary") {
                continue;
            }
            out.push(HistoryEntry {
                date: date.clone(),
                session: stem.to_string(),
                path: p,
            });
        }
    }
    // Newest first by date (string sort works because YYYY-MM-DD).
    out.sort_by(|a, b| b.date.cmp(&a.date).then(b.session.cmp(&a.session)));
    out
}
