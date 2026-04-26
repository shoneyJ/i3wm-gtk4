//! Speech-to-text control-panel widget.
//!
//! Surfaces a Start/Stop toggle for `dist/i3more-speech-text`, a session
//! name `GtkEntry`, a status row, and a "Summarise with Claude" button
//! that runs against the most-recent stopped session.
//!
//! The widget does NOT load whisper itself — it spawns the standalone
//! `i3more-speech-text` binary as a detached process so the GTK process
//! stays light. Toggle-off SIGTERMs the running instance via `pkill -fx`.
//! Status is polled every 2 s via `pgrep`.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

const BINARY_PATH: &str = "i3more-speech-text"; // assumed on PATH or via the dist/ symlink
const SUMMARISE_PROMPT: &str = "Read the German + English transcript at $TRANSCRIPT and produce a structured markdown summary with: meeting title, date, key decisions, action items (with owners if mentioned), open questions. Save the result to ${TRANSCRIPT%.md}-summary.md. Reply with the file path.";

pub fn build_widget() -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    container.add_css_class("widget-speech-text");
    container.set_margin_start(4);
    container.set_margin_end(4);
    container.set_margin_top(4);

    // Header — emoji-free; matches existing widgets.
    let header = gtk4::Label::new(None);
    header.set_use_markup(true);
    header.set_markup(&format!(
        "{}  <span foreground=\"#ebdbb2\">Speech-to-Text (DE→EN)</span>",
        crate::fa::fa_icon(crate::fa::MICROPHONE, "#a89984", 10)
    ));
    header.set_halign(gtk4::Align::Start);
    header.add_css_class("widget-section-title");
    container.append(&header);

    // Session-name entry + Start/Stop button row.
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row.set_margin_start(4);
    row.set_margin_end(4);

    let session_entry = gtk4::Entry::new();
    session_entry.set_placeholder_text(Some("session name (e.g. daily standup)"));
    session_entry.set_hexpand(true);
    session_entry.add_css_class("widget-stt-entry");
    row.append(&session_entry);

    let toggle_btn = gtk4::Button::new();
    toggle_btn.set_label("Start");
    toggle_btn.add_css_class("widget-stt-toggle");
    row.append(&toggle_btn);

    container.append(&row);

    // Status row.
    let status = gtk4::Label::new(Some("idle"));
    status.set_halign(gtk4::Align::Start);
    status.add_css_class("widget-stt-status");
    status.set_margin_start(4);
    container.append(&status);

    // Summarise-with-Claude button (greyed while running).
    let summary_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    summary_row.set_margin_start(4);
    summary_row.set_margin_end(4);

    let summary_btn = gtk4::Button::with_label("Summarise with Claude");
    summary_btn.add_css_class("widget-stt-summary");
    summary_btn.set_hexpand(true);
    summary_row.append(&summary_btn);

    container.append(&summary_row);

    let summary_status = gtk4::Label::new(None);
    summary_status.set_halign(gtk4::Align::Start);
    summary_status.add_css_class("widget-stt-summary-status");
    summary_status.set_margin_start(4);
    container.append(&summary_status);

    // Shared state.
    let last_session_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    // Toggle handler.
    {
        let session_entry = session_entry.clone();
        let toggle_btn_clone = toggle_btn.clone();
        let status = status.clone();
        let summary_status_clone = summary_status.clone();
        let last_session_path = last_session_path.clone();
        toggle_btn.connect_clicked(move |_| {
            if is_running() {
                kill_running();
                status.set_text("stopped");
                toggle_btn_clone.set_label("Start");
                session_entry.set_editable(true);
                session_entry.set_can_focus(true);
            } else {
                // Requirement A: resolve the session name ONCE here so
                // the widget and the spawned binary agree on the file
                // path even if a minute boundary falls between them.
                let raw = session_entry.text().to_string();
                let resolved = resolve_session_name(&raw);

                // Visual feedback — show the user exactly which session
                // is now being recorded (especially the auto-generated
                // `untitled-<date>-<time>` form).
                session_entry.set_text(&resolved);
                // Lock the entry while running — Requirement A says
                // session names are immutable mid-session in v1.
                session_entry.set_editable(false);
                session_entry.set_can_focus(false);

                // Clear any stale summary message from a previous run.
                summary_status_clone.set_text("");

                if let Err(e) = spawn_speech_text(&resolved) {
                    status.set_text(&format!("spawn failed: {}", e));
                    session_entry.set_editable(true);
                    session_entry.set_can_focus(true);
                    return;
                }
                status.set_text(&format!("running — {}", resolved));
                toggle_btn_clone.set_label("Stop");
                // Compute the transcript path from the SAME resolved
                // name; no race with the spawned binary's own time
                // because we don't ask the binary to derive the name.
                if let Some(p) = transcript_path_for(&resolved) {
                    *last_session_path.borrow_mut() = Some(p);
                }
            }
        });
    }

    // Summarise handler.
    {
        let last_session_path = last_session_path.clone();
        let summary_status = summary_status.clone();
        summary_btn.connect_clicked(move |btn| {
            if is_running() {
                summary_status.set_text("stop the session first");
                return;
            }
            let Some(path) = last_session_path.borrow().clone() else {
                summary_status.set_text("no session yet");
                return;
            };
            if !path.exists() {
                summary_status.set_text(&format!("transcript missing: {}", path.display()));
                return;
            }
            btn.set_sensitive(false);
            summary_status.set_text("running claude…");
            let path_clone = path.clone();
            let summary_status_main = summary_status.clone();
            let btn_main = btn.clone();

            // Cross thread boundaries via mpsc — GTK widgets are !Send,
            // so the worker thread only carries the (Send-able) path
            // and result; the timer on the main thread owns the
            // widgets and the receiver.
            let (done_tx, done_rx) =
                std::sync::mpsc::channel::<Result<String, String>>();

            std::thread::spawn(move || {
                let mut claude_cmd = Command::new("claude");
                claude_cmd
                    .arg("-p")
                    .arg(SUMMARISE_PROMPT)
                    .arg("--allowedTools")
                    .arg("Read,Write")
                    .env("TRANSCRIPT", &path_clone);
                // Same XDG hygiene as `spawn_speech_text` — keep claude
                // out of any snap-private home regardless of how the
                // parent i3more was launched.
                if let Some(home) = dirs::home_dir() {
                    claude_cmd.env("XDG_DATA_HOME", home.join(".local/share"));
                    claude_cmd.env("XDG_CONFIG_HOME", home.join(".config"));
                    claude_cmd.env("XDG_CACHE_HOME", home.join(".cache"));
                }
                let output = claude_cmd.output();
                let result = match output {
                    Ok(o) if o.status.success() => {
                        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
                    }
                    Ok(o) => Err(format!(
                        "claude exit {}: {}",
                        o.status,
                        String::from_utf8_lossy(&o.stderr)
                    )),
                    Err(e) => Err(format!("claude spawn: {}", e)),
                };
                let _ = done_tx.send(result);
            });

            glib::timeout_add_local(Duration::from_millis(200), move || {
                match done_rx.try_recv() {
                    Ok(Ok(reply)) => {
                        btn_main.set_sensitive(true);
                        summary_status_main
                            .set_text(&format!("done: {}", short(&reply, 80)));
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        btn_main.set_sensitive(true);
                        summary_status_main.set_text(&format!("error: {}", short(&e, 80)));
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        btn_main.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                }
            });
        });
    }

    // Periodic running-state poll — keeps the widget honest if the user
    // kills the binary from another shell or it crashes.
    {
        let toggle_btn = toggle_btn.clone();
        let status = status.clone();
        let session_entry = session_entry.clone();
        glib::timeout_add_local(Duration::from_secs(2), move || {
            let running = is_running();
            let label_now = toggle_btn.label().map(|s| s.to_string()).unwrap_or_default();
            if running && label_now != "Stop" {
                toggle_btn.set_label("Stop");
                status.set_text("running");
                session_entry.set_editable(false);
                session_entry.set_can_focus(false);
            } else if !running && label_now != "Start" {
                toggle_btn.set_label("Start");
                if !status.text().contains("error") && !status.text().contains("failed") {
                    status.set_text("idle");
                }
                session_entry.set_editable(true);
                session_entry.set_can_focus(true);
            }
            glib::ControlFlow::Continue
        });
    }

    container
}

fn is_running() -> bool {
    Command::new("pgrep")
        .arg("-fx")
        .arg(BINARY_PATH)
        // pgrep matches against the binary's process name (or full args
        // with -f). The dist/ binary may be invoked via absolute path,
        // so allow either.
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        // Also try an absolute-path match — the i3 keybind launches via
        // `~/.../dist/i3more-speech-text`, which won't match `-x` alone.
        || Command::new("pgrep")
            .arg("-f")
            .arg("i3more-speech-text")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

fn kill_running() {
    let _ = Command::new("pkill")
        .arg("-TERM")
        .arg("-f")
        .arg("i3more-speech-text")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn spawn_speech_text(session_name: &str) -> Result<(), String> {
    // Locate the binary. Prefer the dist/ in this repo; fall back to PATH.
    let dist_path = dirs::home_dir()
        .map(|h| h.join("projects/github/shoneyj/i3More/dist/i3more-speech-text"))
        .filter(|p| p.exists());
    let mut cmd = match dist_path {
        Some(p) => Command::new(p),
        None => Command::new(BINARY_PATH),
    };
    if !session_name.is_empty() {
        cmd.env("I3MORE_STT_SESSION", session_name);
    }
    // Force the standard XDG paths so the spawned binary writes to
    // ~/.local/share/i3more/stt/... regardless of where the parent
    // i3more was launched from. (Notable case: launching from a
    // VSCode-snap-packaged terminal sets XDG_DATA_HOME to
    // $HOME/snap/code/<rev>/.local/share, which dirs::data_dir() then
    // honours and we end up with transcripts in a snap-private dir.)
    if let Some(home) = dirs::home_dir() {
        cmd.env("XDG_DATA_HOME", home.join(".local/share"));
        cmd.env("XDG_CONFIG_HOME", home.join(".config"));
        cmd.env("XDG_CACHE_HOME", home.join(".cache"));
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("{}", e))
}

/// Single source of truth for the session name shown in the GtkEntry,
/// passed to the spawned binary, and used to compute the transcript
/// path. The empty-string fallback shape **must** match
/// `SessionMeta::new(None)` in `src/speech_text.rs` byte-for-byte;
/// otherwise the widget and the binary would write to / read from
/// different files.
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

/// Compute the transcript path for an already-resolved session name.
/// Mirrors `SessionMeta::transcript_path()` in `src/speech_text.rs`.
/// Caller is responsible for passing a non-empty resolved name (use
/// `resolve_session_name` upstream).
fn transcript_path_for(resolved_name: &str) -> Option<PathBuf> {
    debug_assert!(!resolved_name.is_empty(), "resolve_session_name first");
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
    Some(
        dirs::data_dir()?
            .join("i3more")
            .join("stt")
            .join(date)
            .join(format!("{}.md", resolved_name)),
    )
}

fn short(s: &str, n: usize) -> String {
    let one_line = s.replace('\n', " ");
    if one_line.chars().count() <= n {
        one_line
    } else {
        format!("{}…", one_line.chars().take(n).collect::<String>())
    }
}
