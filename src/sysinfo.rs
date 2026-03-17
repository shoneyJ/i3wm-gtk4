//! System information readers and popup for the i3More bar.
//!
//! Provides battery status, clock, and on-demand CPU/temp/RAM stats.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Phase 1: static bar info
// ---------------------------------------------------------------------------

/// Read battery capacity and charging status from sysfs.
///
/// Scans `/sys/class/power_supply/BAT*` for the first available battery.
/// Returns `(capacity_percent, status_string)`.
pub fn read_battery() -> Option<(u8, String)> {
    let power_dir = std::path::Path::new("/sys/class/power_supply");
    let entries = std::fs::read_dir(power_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("BAT") {
            continue;
        }
        let base = entry.path();
        let capacity_str = std::fs::read_to_string(base.join("capacity")).ok()?;
        let status_str = std::fs::read_to_string(base.join("status")).ok()?;
        let capacity: u8 = capacity_str.trim().parse().ok()?;
        return Some((capacity, status_str.trim().to_string()));
    }
    None
}

/// Return a formatted clock string, e.g. `"Thu Mar 12  3:14 PM"`.
pub fn read_clock() -> String {
    let now = glib::DateTime::now_local().expect("Could not get local time");
    // %a = short weekday, %b = short month, %e = day, %l = 12h hour, %M = minute, %p = AM/PM
    now.format("%a %b %e  %l:%M %p")
        .map(|s| s.to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Phase 2: on-demand stats (CPU, temperature, RAM)
// ---------------------------------------------------------------------------

/// Snapshot of `/proc/stat` CPU counters.
#[derive(Clone, Default)]
pub struct CpuSnapshot {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
}

impl CpuSnapshot {
    fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq
    }
    fn busy(&self) -> u64 {
        self.total() - self.idle - self.iowait
    }
}

/// Read the aggregate CPU line from `/proc/stat`.
pub fn read_cpu_stat() -> Option<CpuSnapshot> {
    let data = std::fs::read_to_string("/proc/stat").ok()?;
    let line = data.lines().next()?; // first line: "cpu  ..."
    let mut parts = line.split_whitespace();
    parts.next(); // skip "cpu"
    let vals: Vec<u64> = parts.filter_map(|v| v.parse().ok()).collect();
    if vals.len() < 7 {
        return None;
    }
    Some(CpuSnapshot {
        user: vals[0],
        nice: vals[1],
        system: vals[2],
        idle: vals[3],
        iowait: vals[4],
        irq: vals[5],
        softirq: vals[6],
    })
}

/// Compute CPU usage percentage between two snapshots.
pub fn cpu_usage_percent(prev: &CpuSnapshot, curr: &CpuSnapshot) -> f32 {
    let total_d = curr.total().saturating_sub(prev.total());
    if total_d == 0 {
        return 0.0;
    }
    let busy_d = curr.busy().saturating_sub(prev.busy());
    (busy_d as f32 / total_d as f32) * 100.0
}

/// Read CPU temperature from the best available thermal zone (millidegrees → °C).
///
/// Prefers zones named `x86_pkg_temp`, `TCPU`, or `coretemp` over the default
/// `INT3400 Thermal` (zone 0) which reports a fixed virtual temperature.
pub fn read_temperature() -> Option<f32> {
    let thermal_dir = std::path::Path::new("/sys/class/thermal");
    let entries = std::fs::read_dir(thermal_dir).ok()?;

    let preferred = ["x86_pkg_temp", "TCPU", "TCPU_PCI", "coretemp"];
    let mut best_path: Option<std::path::PathBuf> = None;
    let mut fallback_path: Option<std::path::PathBuf> = None;

    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("thermal_zone") {
            continue;
        }
        let base = entry.path();
        let type_path = base.join("type");
        let temp_path = base.join("temp");
        if !temp_path.exists() {
            continue;
        }
        if let Ok(zone_type) = std::fs::read_to_string(&type_path) {
            let zone_type = zone_type.trim();
            if preferred.iter().any(|p| *p == zone_type) {
                best_path = Some(temp_path);
                break;
            }
            // Use any non-INT3400 zone as fallback
            if zone_type != "INT3400 Thermal" && fallback_path.is_none() {
                fallback_path = Some(temp_path);
            }
        }
    }

    let path = best_path.or(fallback_path).unwrap_or_else(|| {
        thermal_dir.join("thermal_zone0/temp")
    });
    let data = std::fs::read_to_string(path).ok()?;
    let millideg: f32 = data.trim().parse().ok()?;
    Some(millideg / 1000.0)
}

/// Read memory info from `/proc/meminfo`. Returns `(used_mb, total_mb)`.
pub fn read_memory() -> Option<(u64, u64)> {
    let data = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = 0u64;
    let mut available_kb = 0u64;
    for line in data.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kb = rest.trim().trim_end_matches(" kB").trim().parse().ok()?;
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available_kb = rest.trim().trim_end_matches(" kB").trim().parse().ok()?;
        }
    }
    if total_kb == 0 {
        return None;
    }
    let used_mb = (total_kb - available_kb) / 1024;
    let total_mb = total_kb / 1024;
    Some((used_mb, total_mb))
}

// ---------------------------------------------------------------------------
// Popover builder
// ---------------------------------------------------------------------------

/// Build a stats popover parented to the given icon widget.
///
/// The popover polls CPU/temp/RAM every second while visible and stops when hidden.
pub fn build_stats_popover(icon: &impl gtk4::prelude::IsA<gtk4::Widget>) -> gtk4::Popover {
    let popover = gtk4::Popover::new();
    popover.set_parent(icon);
    popover.set_autohide(true);

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    vbox.set_margin_top(6);
    vbox.set_margin_bottom(6);
    vbox.set_margin_start(8);
    vbox.set_margin_end(8);

    let cpu_label = gtk4::Label::new(None);
    cpu_label.set_use_markup(true);
    cpu_label.set_halign(gtk4::Align::Start);
    cpu_label.add_css_class("sysinfo-popup-label");

    let temp_label = gtk4::Label::new(None);
    temp_label.set_use_markup(true);
    temp_label.set_halign(gtk4::Align::Start);
    temp_label.add_css_class("sysinfo-popup-label");

    let ram_label = gtk4::Label::new(None);
    ram_label.set_use_markup(true);
    ram_label.set_halign(gtk4::Align::Start);
    ram_label.add_css_class("sysinfo-popup-label");

    vbox.append(&cpu_label);
    vbox.append(&temp_label);
    vbox.append(&ram_label);
    popover.set_child(Some(&vbox));

    // Shared state for the polling timer
    let timer_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let prev_cpu: Rc<RefCell<Option<CpuSnapshot>>> = Rc::new(RefCell::new(None));

    let timer_id_clone = timer_id.clone();
    let prev_cpu_clone = prev_cpu.clone();
    let cpu_lbl = cpu_label.clone();
    let temp_lbl = temp_label.clone();
    let ram_lbl = ram_label.clone();

    popover.connect_notify_local(Some("visible"), move |popover, _| {
        if popover.is_visible() {
            // Take initial CPU snapshot so the first tick has a delta
            *prev_cpu_clone.borrow_mut() = read_cpu_stat();

            let cpu_l = cpu_lbl.clone();
            let temp_l = temp_lbl.clone();
            let ram_l = ram_lbl.clone();
            let prev = prev_cpu_clone.clone();
            let tid = timer_id_clone.clone();

            // Immediate first update
            update_stats_labels(&cpu_l, &temp_l, &ram_l, &prev);

            let sid = glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
                // Check the label is still mapped (popover may have been destroyed)
                if !cpu_l.is_mapped() {
                    tid.borrow_mut().take();
                    return glib::ControlFlow::Break;
                }
                update_stats_labels(&cpu_l, &temp_l, &ram_l, &prev);
                glib::ControlFlow::Continue
            });
            *timer_id_clone.borrow_mut() = Some(sid);
        } else {
            // Stop polling
            if let Some(sid) = timer_id_clone.borrow_mut().take() {
                crate::safe_source_remove(sid);
            }
        }
    });

    popover
}

/// Pango markup key color (muted gruvbox fg)
const KEY_COLOR: &str = "#a89984";
/// Pango markup value color (bright gruvbox fg)
const VAL_COLOR: &str = "#ebdbb2";

/// Format a key-value pair with an FA icon and Pango markup colors.
fn markup_kv(icon: char, value: &str) -> String {
    format!(
        "{}  <span foreground=\"{VAL_COLOR}\">{value}</span>",
        crate::fa::fa_icon(icon, KEY_COLOR, 10),
    )
}

/// Helper: read current stats and update the three labels.
fn update_stats_labels(
    cpu_label: &gtk4::Label,
    temp_label: &gtk4::Label,
    ram_label: &gtk4::Label,
    prev_cpu: &Rc<RefCell<Option<CpuSnapshot>>>,
) {
    // CPU
    let curr = read_cpu_stat();
    if let (Some(prev), Some(ref cur)) = (prev_cpu.borrow().as_ref(), &curr) {
        let pct = cpu_usage_percent(prev, cur);
        cpu_label.set_markup(&markup_kv(crate::fa::MICROCHIP, &format!("{:.0}%", pct)));
    }
    *prev_cpu.borrow_mut() = curr;

    // Temperature
    if let Some(temp) = read_temperature() {
        temp_label.set_markup(&markup_kv(crate::fa::TEMPERATURE, &format!("{:.0} °C", temp)));
    } else {
        temp_label.set_markup(&markup_kv(crate::fa::TEMPERATURE, "N/A"));
    }

    // RAM
    if let Some((used, total)) = read_memory() {
        ram_label.set_markup(&markup_kv(crate::fa::MEMORY, &format!("{} / {} MB", used, total)));
    } else {
        ram_label.set_markup(&markup_kv(crate::fa::MEMORY, "N/A"));
    }
}
