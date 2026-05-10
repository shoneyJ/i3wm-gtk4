//! whisper-stt state indicator for the navigator bar.
//!
//! Subscribes to whisperd's IPC over a Unix socket and renders a small pill
//! reflecting the daemon's state: `idle | listening | transcribing | error`.
//!
//! - Hidden when whisperd isn't running (mirrors mic_indicator's "no source
//!   → hidden" pattern).
//! - Reconnects automatically every 2 s if the daemon goes away or restarts.
//! - Wire shape (NDJSON, one line per event) is documented at
//!   `whisper-stt/doc/plan/02-protocol.md`. To avoid coupling the two repos
//!   at the manifest level, the protocol is reproduced inline as a minimal
//!   serde-only deserialiser. If you ever want to share the actual types,
//!   add a path dep on `whisper-stt-proto`.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::Rc;

const SOCKET_PATH_ENV: &str = "WHISPER_STT_SOCK";
const DEFAULT_SOCKET_REL_PATH: &str = ".cache/whisper-stt/whisperd.sock";
const SUBSCRIBE_LINE: &[u8] = b"{\"cmd\":\"subscribe\"}\n";
const RECONNECT_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
const TICK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

const STATE_CLASSES: [&str; 4] = [
    "whisper-stt-state-idle",
    "whisper-stt-state-listening",
    "whisper-stt-state-transcribing",
    "whisper-stt-state-error",
];

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum DaemonState {
    Idle,
    Listening,
    Transcribing,
    Error,
}

/// Minimal mirror of `whisper-stt-proto::CtlEvent`. Extra fields on the wire
/// (`worker_loaded`, `idle_seconds`, …) are ignored.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum CtlEvent {
    StateChanged { state: DaemonState },
}

#[derive(Debug, Clone, Copy)]
enum WhisperEvent {
    State(DaemonState),
    Disconnected,
}

pub struct WhisperSttIndicatorHandles {
    pub container: gtk4::Box,
}

fn socket_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(SOCKET_PATH_ENV) {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(DEFAULT_SOCKET_REL_PATH))
}

fn spawn_subscriber(sender: std::sync::mpsc::Sender<WhisperEvent>) {
    std::thread::spawn(move || loop {
        let path = match socket_path() {
            Some(p) => p,
            None => {
                std::thread::sleep(RECONNECT_DELAY);
                continue;
            }
        };

        if let Ok(mut stream) = UnixStream::connect(&path) {
            if stream.write_all(SUBSCRIBE_LINE).is_ok() {
                let reader = BufReader::new(stream);
                for line in reader.lines().map_while(Result::ok) {
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(CtlEvent::StateChanged { state }) =
                        serde_json::from_str::<CtlEvent>(&line)
                    {
                        if sender.send(WhisperEvent::State(state)).is_err() {
                            return; // GTK side gone — stop the thread
                        }
                    }
                }
            }
            // Either the write failed (rare) or the read loop ended on EOF.
            // In both cases the daemon is gone or restarted; tell the UI and
            // back off before reconnecting.
            let _ = sender.send(WhisperEvent::Disconnected);
        }
        // No daemon yet; keep trying quietly.
        std::thread::sleep(RECONNECT_DELAY);
    });
}

fn state_color(state: DaemonState) -> &'static str {
    match state {
        DaemonState::Idle => "#a89984",         // gruvbox light4 (subdued)
        DaemonState::Listening => "#fb4934",    // gruvbox red (recording)
        DaemonState::Transcribing => "#fabd2f", // gruvbox yellow (busy)
        DaemonState::Error => "#cc241d",        // gruvbox dark red
    }
}

fn state_tooltip(state: DaemonState) -> &'static str {
    match state {
        DaemonState::Idle => "whisper-stt: idle",
        DaemonState::Listening => "whisper-stt: listening",
        DaemonState::Transcribing => "whisper-stt: transcribing",
        DaemonState::Error => "whisper-stt: error",
    }
}

fn state_class(state: DaemonState) -> &'static str {
    match state {
        DaemonState::Idle => "whisper-stt-state-idle",
        DaemonState::Listening => "whisper-stt-state-listening",
        DaemonState::Transcribing => "whisper-stt-state-transcribing",
        DaemonState::Error => "whisper-stt-state-error",
    }
}

pub fn build_whisper_stt_indicator() -> WhisperSttIndicatorHandles {
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    container.add_css_class("whisper-stt-indicator");
    container.set_valign(gtk4::Align::Center);

    let icon_label = gtk4::Label::new(None);
    icon_label.set_use_markup(true);
    icon_label.set_valign(gtk4::Align::Center);
    container.append(&icon_label);

    // Hidden until the first state event arrives. Disconnected daemon → invisible.
    container.set_visible(false);

    let (tx, rx) = std::sync::mpsc::channel::<WhisperEvent>();
    spawn_subscriber(tx);

    let icon_for_tick = icon_label.clone();
    let container_for_tick = container.clone();
    let last_state = Rc::new(RefCell::new(None::<DaemonState>));

    glib::timeout_add_local(TICK_INTERVAL, move || {
        // Drain the channel; coalesce successive State events into the last one.
        let mut new_state: Option<DaemonState> = None;
        let mut went_offline = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                WhisperEvent::State(state) => new_state = Some(state),
                WhisperEvent::Disconnected => {
                    went_offline = true;
                    new_state = None;
                }
            }
        }

        if went_offline && new_state.is_none() {
            container_for_tick.set_visible(false);
            *last_state.borrow_mut() = None;
        }

        if let Some(state) = new_state {
            for class in STATE_CLASSES {
                icon_for_tick.remove_css_class(class);
            }
            icon_for_tick.add_css_class(state_class(state));
            icon_for_tick.set_markup(&crate::fa::fa_icon(
                crate::fa::KEYBOARD,
                state_color(state),
                11,
            ));
            container_for_tick.set_tooltip_text(Some(state_tooltip(state)));
            container_for_tick.set_visible(true);
            *last_state.borrow_mut() = Some(state);
        }

        glib::ControlFlow::Continue
    });

    WhisperSttIndicatorHandles { container }
}
