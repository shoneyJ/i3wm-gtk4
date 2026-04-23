/// Microphone mute/unmute indicator for the navigator bar.
///
/// Shows a clickable mic icon that:
/// - Is only visible when a real (non-monitor) audio source exists
/// - Toggles mute on click via `pactl set-source-mute @DEFAULT_SOURCE@ toggle`
/// - Updates icon color: green (active) / red (muted)
/// - Uses `pactl subscribe` for event-driven state updates

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::io::BufRead;
use std::rc::Rc;

enum MicEvent {
    SourceChanged,
    DeviceListChanged,
}

/// Handles returned to keep the widget alive in the bar.
pub struct MicIndicatorHandles {
    pub container: gtk4::Box,
}

// ---------------------------------------------------------------------------
// State helpers
// ---------------------------------------------------------------------------

fn has_real_source() -> bool {
    let output = std::process::Command::new("pactl")
        .args(["--format=json", "list", "sources"])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return false,
    };
    items.iter().any(|item| {
        item.get("name")
            .and_then(|n| n.as_str())
            .map(|n| !n.contains(".monitor"))
            .unwrap_or(false)
    })
}

fn is_source_muted() -> bool {
    let output = std::process::Command::new("pactl")
        .args(["get-source-mute", "@DEFAULT_SOURCE@"])
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).contains("yes"),
        _ => false,
    }
}

/// Get the human-readable description of the default source.
fn default_source_description() -> Option<String> {
    let name_output = std::process::Command::new("pactl")
        .args(["get-default-source"])
        .output()
        .ok()?;
    if !name_output.status.success() {
        return None;
    }
    let default_name = String::from_utf8_lossy(&name_output.stdout).trim().to_string();

    let json_output = std::process::Command::new("pactl")
        .args(["--format=json", "list", "sources"])
        .output()
        .ok()?;
    if !json_output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&json_output.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&text).ok()?;
    for item in &items {
        if item.get("name")?.as_str()? == default_name {
            return Some(item.get("description")?.as_str()?.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// pactl subscribe
// ---------------------------------------------------------------------------

fn spawn_mic_subscribe(sender: std::sync::mpsc::Sender<MicEvent>) {
    std::thread::spawn(move || loop {
        let child = std::process::Command::new("pactl")
            .arg("subscribe")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();

        let mut child = match child {
            Ok(c) => c,
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
        };

        if let Some(stdout) = child.stdout.take() {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if line.contains("'change' on source") || line.contains("'change' on server") {
                    let _ = sender.send(MicEvent::SourceChanged);
                }
                if line.contains("'new' on source") || line.contains("'remove' on source") {
                    let _ = sender.send(MicEvent::DeviceListChanged);
                }
            }
        }

        let _ = child.wait();
        std::thread::sleep(std::time::Duration::from_secs(2));
    });
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

fn mic_tooltip(muted: bool) -> String {
    let state = if muted { "Muted" } else { "Active" };
    match default_source_description() {
        Some(desc) => format!("{}\n{}", desc, state),
        None => format!("Microphone: {}", state),
    }
}

fn update_mic_icon(label: &gtk4::Label, muted: bool) {
    let (icon, color) = if muted {
        (crate::fa::MICROPHONE_SLASH, "#fb4934") // gruvbox red
    } else {
        (crate::fa::MICROPHONE, "#b8bb26") // gruvbox green
    };
    label.set_markup(&crate::fa::fa_icon(icon, color, 11));
}

pub fn build_mic_indicator() -> MicIndicatorHandles {
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    container.add_css_class("mic-indicator");
    container.set_valign(gtk4::Align::Center);

    let icon_label = gtk4::Label::new(None);
    icon_label.set_use_markup(true);
    icon_label.set_valign(gtk4::Align::Center);
    container.append(&icon_label);

    // Initial state
    let source_exists = has_real_source();
    container.set_visible(source_exists);
    if source_exists {
        let muted = is_source_muted();
        update_mic_icon(&icon_label, muted);
        container.set_tooltip_text(Some(&mic_tooltip(muted)));
    }

    // Click handler — toggle mute
    let icon_for_click = icon_label.clone();
    let container_for_click = container.clone();
    let gesture = gtk4::GestureClick::new();
    gesture.connect_released(move |_, _, _, _| {
        let _ = std::process::Command::new("pactl")
            .args(["set-source-mute", "@DEFAULT_SOURCE@", "toggle"])
            .output();
        let muted = is_source_muted();
        update_mic_icon(&icon_for_click, muted);
        container_for_click.set_tooltip_text(Some(&mic_tooltip(muted)));
    });
    container.add_controller(gesture);

    // Event-driven monitoring
    let (tx, rx) = std::sync::mpsc::channel::<MicEvent>();
    spawn_mic_subscribe(tx);

    let icon_ev = icon_label;
    let container_ev = container.clone();
    let last_visible = Rc::new(RefCell::new(source_exists));

    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let mut needs_mute_check = false;
        let mut needs_device_check = false;

        while let Ok(event) = rx.try_recv() {
            match event {
                MicEvent::SourceChanged => needs_mute_check = true,
                MicEvent::DeviceListChanged => {
                    needs_device_check = true;
                    needs_mute_check = true;
                }
            }
        }

        if needs_device_check {
            let exists = has_real_source();
            container_ev.set_visible(exists);
            *last_visible.borrow_mut() = exists;
            if !exists {
                return glib::ControlFlow::Continue;
            }
        }

        if needs_mute_check && *last_visible.borrow() {
            let muted = is_source_muted();
            update_mic_icon(&icon_ev, muted);
            container_ev.set_tooltip_text(Some(&mic_tooltip(muted)));
        }

        glib::ControlFlow::Continue
    });

    MicIndicatorHandles { container }
}
