/// PulseAudio volume widget with device selection.
///
/// Shows a slider for the default sink volume, a mute toggle button,
/// and dropdown selectors for output/input audio devices.
/// Uses `pactl subscribe` for event-driven updates (no polling).

use gtk4::glib;
use gtk4::prelude::*;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::PathBuf;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

struct DeviceInfo {
    name: String,
    description: String,
}

#[derive(Deserialize, Default)]
struct AudioConfig {
    #[serde(default)]
    preferred_sinks: Vec<String>,
    #[serde(default)]
    preferred_sources: Vec<String>,
    #[serde(default)]
    excluded_sinks: Vec<String>,
    #[serde(default)]
    excluded_sources: Vec<String>,
}

enum PactlEvent {
    VolumeChanged,
    DeviceListChanged,
}

struct DeviceSnapshot {
    sinks: HashMap<String, String>,   // name → description
    sources: HashMap<String, String>, // name → description (excluding .monitor)
}

// ---------------------------------------------------------------------------
// Config & glob matching (duplicated from audio_main.rs — separate binary)
// ---------------------------------------------------------------------------

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("i3more")
        .join("audio.json")
}

fn load_config() -> AudioConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => AudioConfig::default(),
    }
}

/// Simple glob matching: `*` matches any sequence of characters.
fn matches_glob(name: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match name[pos..].find(part) {
            Some(found) => {
                if i == 0 && !pattern.starts_with('*') && found != 0 {
                    return false;
                }
                pos += found + part.len();
            }
            None => return false,
        }
    }
    if !pattern.ends_with('*') {
        if let Some(last) = parts.last() {
            if !last.is_empty() {
                return name.ends_with(last);
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Device enumeration
// ---------------------------------------------------------------------------

/// Strip chipset/adapter prefixes from PulseAudio device descriptions.
/// e.g. "Raptor Lake-P/U/H cAVS Speaker + Headphones" → "Speaker + Headphones"
fn simplify_device_description(desc: &str) -> String {
    // Known chipset prefixes end with "cAVS " or "ALC*" patterns.
    // Generic approach: strip everything before and including " cAVS " if present.
    if let Some(idx) = desc.find(" cAVS ") {
        return desc[idx + 6..].to_string();
    }
    desc.to_string()
}

fn list_devices_json(device_type: &str) -> Vec<DeviceInfo> {
    let output = std::process::Command::new("pactl")
        .args(["--format=json", "list", device_type])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    items
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            let description = simplify_device_description(
                item.get("description")?.as_str()?,
            );
            Some(DeviceInfo { name, description })
        })
        .collect()
}

fn filter_devices(
    devices: Vec<DeviceInfo>,
    preferred: &[String],
    excluded: &[String],
) -> Vec<DeviceInfo> {
    if !preferred.is_empty() {
        let mut result = Vec::new();
        for pattern in preferred {
            for device in &devices {
                if matches_glob(&device.name, pattern)
                    && !result.iter().any(|d: &DeviceInfo| d.name == device.name)
                {
                    result.push(DeviceInfo {
                        name: device.name.clone(),
                        description: device.description.clone(),
                    });
                }
            }
        }
        result
    } else if !excluded.is_empty() {
        devices
            .into_iter()
            .filter(|d| !excluded.iter().any(|p| matches_glob(&d.name, p)))
            .collect()
    } else {
        devices
    }
}

fn list_sinks_filtered(config: &AudioConfig) -> Vec<DeviceInfo> {
    let sinks = list_devices_json("sinks");
    filter_devices(sinks, &config.preferred_sinks, &config.excluded_sinks)
}

fn list_sources_filtered(config: &AudioConfig) -> Vec<DeviceInfo> {
    let sources = list_devices_json("sources")
        .into_iter()
        .filter(|s| !s.name.contains(".monitor"))
        .collect();
    filter_devices(sources, &config.preferred_sources, &config.excluded_sources)
}

fn take_device_snapshot() -> DeviceSnapshot {
    let sinks = list_devices_json("sinks")
        .into_iter()
        .map(|d| (d.name, d.description))
        .collect();
    let sources = list_devices_json("sources")
        .into_iter()
        .filter(|s| !s.name.contains(".monitor"))
        .map(|d| (d.name, d.description))
        .collect();
    DeviceSnapshot { sinks, sources }
}

fn notify_device_change(summary: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args([
            summary,
            body,
            "-i",
            "audio-card",
            "-t",
            "3000",
            "-h",
            "string:x-canonical-private-synchronous:audio-device",
        ])
        .spawn();
}

fn process_device_changes(prev: &DeviceSnapshot, current: &DeviceSnapshot) {
    // Detect added sinks
    for (name, desc) in &current.sinks {
        if !prev.sinks.contains_key(name) {
            notify_device_change("Audio Device Connected", desc);
        }
    }
    // Detect removed sinks
    for (name, desc) in &prev.sinks {
        if !current.sinks.contains_key(name) {
            notify_device_change("Audio Device Disconnected", desc);
        }
    }
    // Detect added sources
    for (name, desc) in &current.sources {
        if !prev.sources.contains_key(name) {
            notify_device_change("Audio Device Connected", desc);
            // Headset mic detection
            if desc.to_lowercase().contains("headset") {
                notify_device_change("Headset Microphone Detected", desc);
            }
        }
    }
    // Detect removed sources
    for (name, desc) in &prev.sources {
        if !current.sources.contains_key(name) {
            notify_device_change("Audio Device Disconnected", desc);
        }
    }
}

fn get_default_device(kind: &str) -> Option<String> {
    let output = std::process::Command::new("pactl")
        .args([&format!("get-default-{}", kind)])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn move_all_sink_inputs(sink_name: &str) {
    let output = std::process::Command::new("pactl")
        .args(["list", "short", "sink-inputs"])
        .output();
    if let Ok(output) = output {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(input_id) = line.split('\t').next() {
                let _ = std::process::Command::new("pactl")
                    .args(["move-sink-input", input_id, sink_name])
                    .output();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Volume / mute helpers
// ---------------------------------------------------------------------------

fn read_volume() -> Option<u32> {
    let output = std::process::Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for part in text.split_whitespace() {
        if let Some(pct_str) = part.strip_suffix('%') {
            if let Ok(val) = pct_str.parse::<u32>() {
                return Some(val);
            }
        }
    }
    None
}

fn is_muted() -> bool {
    let output = std::process::Command::new("pactl")
        .args(["get-sink-mute", "@DEFAULT_SINK@"])
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).contains("yes"),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// pactl subscribe
// ---------------------------------------------------------------------------

fn spawn_pactl_subscribe(sender: std::sync::mpsc::Sender<PactlEvent>) {
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
                if line.contains("'change' on sink") || line.contains("'change' on server") {
                    let _ = sender.send(PactlEvent::VolumeChanged);
                }
                if line.contains("'new' on sink")
                    || line.contains("'remove' on sink")
                    || line.contains("'new' on source")
                    || line.contains("'remove' on source")
                {
                    let _ = sender.send(PactlEvent::DeviceListChanged);
                }
            }
        }

        let _ = child.wait();
        std::thread::sleep(std::time::Duration::from_secs(2));
    });
}

// ---------------------------------------------------------------------------
// Device dropdown refresh
// ---------------------------------------------------------------------------

fn refresh_devices(
    sink_dropdown: &gtk4::DropDown,
    source_dropdown: &gtk4::DropDown,
    sink_devices: &Rc<RefCell<Vec<DeviceInfo>>>,
    source_devices: &Rc<RefCell<Vec<DeviceInfo>>>,
    updating: &Rc<RefCell<bool>>,
) {
    let config = load_config();
    *updating.borrow_mut() = true;

    // Sinks
    let sinks = list_sinks_filtered(&config);
    let sink_descs: Vec<&str> = sinks.iter().map(|d| d.description.as_str()).collect();
    sink_dropdown.set_model(Some(&gtk4::StringList::new(&sink_descs)));
    if let Some(current) = get_default_device("sink") {
        if let Some(idx) = sinks.iter().position(|s| s.name == current) {
            sink_dropdown.set_selected(idx as u32);
        }
    }
    *sink_devices.borrow_mut() = sinks;

    // Sources
    let sources = list_sources_filtered(&config);
    let source_descs: Vec<&str> = sources.iter().map(|d| d.description.as_str()).collect();
    source_dropdown.set_model(Some(&gtk4::StringList::new(&source_descs)));
    if let Some(current) = get_default_device("source") {
        if let Some(idx) = sources.iter().position(|s| s.name == current) {
            source_dropdown.set_selected(idx as u32);
        }
    }
    *source_devices.borrow_mut() = sources;

    *updating.borrow_mut() = false;
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// Build a DropDown for audio device selection with ellipsized labels.
fn build_device_dropdown() -> gtk4::DropDown {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
        let label = gtk4::Label::new(None);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_max_width_chars(30);
        label.set_halign(gtk4::Align::Start);
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
        let string_obj = item.item().and_downcast::<gtk4::StringObject>().unwrap();
        let label = item.child().and_downcast::<gtk4::Label>().unwrap();
        label.set_text(&string_obj.string());
    });

    let dd = gtk4::DropDown::from_strings(&[]);
    dd.set_factory(Some(&factory));
    dd.set_hexpand(true);
    dd.add_css_class("widget-audio-device");
    dd.set_size_request(0, -1);
    dd
}

pub fn build_widget() -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    container.add_css_class("widget-volume");
    container.set_margin_start(4);
    container.set_margin_end(4);
    container.set_margin_top(4);
    container.set_overflow(gtk4::Overflow::Hidden);

    // Header
    let header = gtk4::Label::new(None);
    header.set_use_markup(true);
    header.set_markup(&format!(
        "{}  <span foreground=\"#ebdbb2\">Volume</span>",
        crate::fa::fa_icon(crate::fa::VOLUME_HIGH, "#a89984", 10)
    ));
    header.set_halign(gtk4::Align::Start);
    header.add_css_class("widget-section-title");
    container.append(&header);

    // Slider row: mute button + scale + percentage label
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row.set_margin_start(4);
    row.set_margin_end(4);

    // Mute toggle
    let mute_btn = gtk4::Button::new();
    let mute_label = gtk4::Label::new(None);
    mute_label.set_use_markup(true);
    mute_label.set_markup(&crate::fa::fa_icon(crate::fa::VOLUME_HIGH, "#ebdbb2", 11));
    mute_btn.set_child(Some(&mute_label));
    mute_btn.add_css_class("widget-volume-mute");

    let mute_label_ref = mute_label.clone();
    mute_btn.connect_clicked(move |_| {
        let _ = std::process::Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .output();
        let muted = is_muted();
        let icon = if muted {
            crate::fa::VOLUME_OFF
        } else {
            crate::fa::VOLUME_HIGH
        };
        mute_label_ref.set_markup(&crate::fa::fa_icon(icon, "#ebdbb2", 11));
    });
    row.append(&mute_btn);

    // Volume scale
    let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale.add_css_class("widget-volume-scale");

    // Percentage label
    let pct_label = gtk4::Label::new(Some("0%"));
    pct_label.add_css_class("widget-volume-pct");
    pct_label.set_width_chars(4);

    // Read initial volume
    let current_vol = read_volume().unwrap_or(0);
    scale.set_value(current_vol as f64);
    pct_label.set_text(&format!("{}%", current_vol));

    // Set mute icon state
    if is_muted() {
        mute_label.set_markup(&crate::fa::fa_icon(crate::fa::VOLUME_OFF, "#ebdbb2", 11));
    }

    // Track programmatic updates to avoid feedback loops
    let updating = Rc::new(RefCell::new(false));

    // Scale change handler — set volume via pactl
    let pct_lbl = pct_label.clone();
    let updating_for_change = updating.clone();
    scale.connect_value_changed(move |scale| {
        if *updating_for_change.borrow() {
            return;
        }
        let vol = scale.value() as u32;
        pct_lbl.set_text(&format!("{}%", vol));
        let _ = std::process::Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", vol)])
            .output();
    });

    row.append(&scale);
    row.append(&pct_label);
    container.append(&row);

    // --- Device dropdowns ---

    let sink_devices: Rc<RefCell<Vec<DeviceInfo>>> = Rc::new(RefCell::new(Vec::new()));
    let source_devices: Rc<RefCell<Vec<DeviceInfo>>> = Rc::new(RefCell::new(Vec::new()));

    // Output row
    let output_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    output_row.set_margin_start(4);
    output_row.set_margin_end(4);
    output_row.set_margin_top(2);
    let output_label = gtk4::Label::new(Some("Output"));
    output_label.add_css_class("widget-audio-device-label");
    output_label.set_width_chars(6);
    output_label.set_halign(gtk4::Align::Start);
    let sink_dropdown = build_device_dropdown();
    output_row.append(&output_label);
    output_row.append(&sink_dropdown);
    container.append(&output_row);

    // Input row
    let input_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    input_row.set_margin_start(4);
    input_row.set_margin_end(4);
    let input_label = gtk4::Label::new(Some("Input"));
    input_label.add_css_class("widget-audio-device-label");
    input_label.set_width_chars(6);
    input_label.set_halign(gtk4::Align::Start);
    let source_dropdown = build_device_dropdown();
    input_row.append(&input_label);
    input_row.append(&source_dropdown);
    container.append(&input_row);

    // Sink selection handler
    {
        let devs = sink_devices.clone();
        let guard = updating.clone();
        sink_dropdown.connect_selected_notify(move |dd| {
            if *guard.borrow() {
                return;
            }
            let sel = dd.selected();
            if sel == u32::MAX {
                return;
            }
            let devices = devs.borrow();
            if let Some(device) = devices.get(sel as usize) {
                let _ = std::process::Command::new("pactl")
                    .args(["set-default-sink", &device.name])
                    .output();
                move_all_sink_inputs(&device.name);
            }
        });
    }

    // Source selection handler
    {
        let devs = source_devices.clone();
        let guard = updating.clone();
        source_dropdown.connect_selected_notify(move |dd| {
            if *guard.borrow() {
                return;
            }
            let sel = dd.selected();
            if sel == u32::MAX {
                return;
            }
            let devices = devs.borrow();
            if let Some(device) = devices.get(sel as usize) {
                let _ = std::process::Command::new("pactl")
                    .args(["set-default-source", &device.name])
                    .output();
            }
        });
    }

    // Initial device list population
    refresh_devices(
        &sink_dropdown,
        &source_dropdown,
        &sink_devices,
        &source_devices,
        &updating,
    );

    // Snapshot for device change detection (no notifications on startup)
    let device_snapshot = Rc::new(RefCell::new(take_device_snapshot()));

    // Event-driven updates via pactl subscribe
    let (tx, rx) = std::sync::mpsc::channel::<PactlEvent>();
    spawn_pactl_subscribe(tx);

    let scale_ev = scale;
    let pct_ev = pct_label;
    let mute_ev = mute_label;
    let upd_ev = updating;
    let sink_dd_ev = sink_dropdown;
    let source_dd_ev = source_dropdown;
    let sink_dev_ev = sink_devices;
    let source_dev_ev = source_devices;
    let snapshot_ev = device_snapshot;

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        while let Ok(event) = rx.try_recv() {
            match event {
                PactlEvent::VolumeChanged => {
                    if let Some(vol) = read_volume() {
                        *upd_ev.borrow_mut() = true;
                        scale_ev.set_value(vol as f64);
                        pct_ev.set_text(&format!("{}%", vol));
                        *upd_ev.borrow_mut() = false;
                    }
                    let muted = is_muted();
                    let icon = if muted {
                        crate::fa::VOLUME_OFF
                    } else {
                        crate::fa::VOLUME_HIGH
                    };
                    mute_ev.set_markup(&crate::fa::fa_icon(icon, "#ebdbb2", 11));
                }
                PactlEvent::DeviceListChanged => {
                    let current = take_device_snapshot();
                    process_device_changes(&snapshot_ev.borrow(), &current);
                    *snapshot_ev.borrow_mut() = current;
                    refresh_devices(
                        &sink_dd_ev,
                        &source_dd_ev,
                        &sink_dev_ev,
                        &source_dev_ev,
                        &upd_ev,
                    );
                }
            }
        }
        glib::ControlFlow::Continue
    });

    container
}
