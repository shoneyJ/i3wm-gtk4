/// Screen backlight widget.
///
/// Reads brightness from sysfs and adjusts via `brightnessctl`.
/// Only builds the widget if a backlight device is found.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Build the backlight control widget. Returns None if no backlight is available.
pub fn build_widget() -> Option<gtk4::Box> {
    let (current, max) = read_brightness()?;
    if max == 0 {
        return None;
    }

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    container.add_css_class("widget-backlight");
    container.set_margin_start(4);
    container.set_margin_end(4);
    container.set_margin_top(4);

    // Header
    let header = gtk4::Label::new(None);
    header.set_use_markup(true);
    header.set_markup(&format!(
        "{}  <span foreground=\"#ebdbb2\">Brightness</span>",
        crate::fa::fa_icon(crate::fa::SUN, "#a89984", 10)
    ));
    header.set_halign(gtk4::Align::Start);
    header.add_css_class("widget-section-title");
    container.append(&header);

    // Slider row
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row.set_margin_start(4);
    row.set_margin_end(4);

    let icon_label = gtk4::Label::new(None);
    icon_label.set_use_markup(true);
    icon_label.set_markup(&crate::fa::fa_icon(crate::fa::SUN, "#ebdbb2", 11));
    row.append(&icon_label);

    let pct = ((current as f64 / max as f64) * 100.0) as u32;

    let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 1.0, 100.0, 1.0);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale.set_value(pct as f64);
    scale.add_css_class("widget-backlight-scale");

    let pct_label = gtk4::Label::new(Some(&format!("{}%", pct)));
    pct_label.add_css_class("widget-volume-pct");
    pct_label.set_width_chars(4);

    let updating = Rc::new(RefCell::new(false));

    // Scale change handler
    let pct_lbl = pct_label.clone();
    let updating_for_change = updating.clone();
    scale.connect_value_changed(move |scale| {
        if *updating_for_change.borrow() {
            return;
        }
        let val = scale.value() as u32;
        pct_lbl.set_text(&format!("{}%", val));
        let _ = std::process::Command::new("brightnessctl")
            .args(["set", &format!("{}%", val)])
            .output();
    });

    row.append(&scale);
    row.append(&pct_label);
    container.append(&row);

    // Poll every 2s for external changes
    let scale_for_timer = scale;
    let pct_for_timer = pct_label;
    let updating_for_timer = updating;
    glib::timeout_add_local(std::time::Duration::from_secs(2), move || {
        if !scale_for_timer.is_mapped() {
            return glib::ControlFlow::Break;
        }
        if let Some((cur, mx)) = read_brightness() {
            if mx > 0 {
                let p = ((cur as f64 / mx as f64) * 100.0) as u32;
                *updating_for_timer.borrow_mut() = true;
                scale_for_timer.set_value(p as f64);
                pct_for_timer.set_text(&format!("{}%", p));
                *updating_for_timer.borrow_mut() = false;
            }
        }
        glib::ControlFlow::Continue
    });

    Some(container)
}

/// Find the first backlight device and return (brightness, max_brightness).
fn read_brightness() -> Option<(u64, u64)> {
    let path = find_backlight_path()?;
    let current: u64 = std::fs::read_to_string(path.join("brightness"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let max: u64 = std::fs::read_to_string(path.join("max_brightness"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some((current, max))
}

fn find_backlight_path() -> Option<PathBuf> {
    let base = std::path::Path::new("/sys/class/backlight");
    let entries = std::fs::read_dir(base).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.join("brightness").exists() && path.join("max_brightness").exists() {
            return Some(path);
        }
    }
    None
}
