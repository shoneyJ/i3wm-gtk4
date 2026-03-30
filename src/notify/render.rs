/// Shared notification rendering: markup parsing, image extraction, widget building.

use std::collections::HashMap;
use std::sync::mpsc;

use gtk4::glib;
use gtk4::prelude::*;
use linbus::Value;

use super::types::Notification;

/// Parse notification body markup, allowing safe Pango tags and stripping the rest.
///
/// Allowed: `<b>`, `<i>`, `<u>`, `<a href="...">` (Pango-compatible).
/// `<img>` tags are replaced with alt text if present, otherwise stripped.
/// All other tags are stripped. Text outside tags is escaped for Pango.
pub fn parse_notification_markup(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch == '<' {
            // Collect the full tag
            let mut tag = String::new();
            tag.push(chars.next().unwrap()); // '<'
            while let Some(&c) = chars.peek() {
                tag.push(chars.next().unwrap());
                if c == '>' {
                    break;
                }
            }

            let tag_lower = tag.to_lowercase();
            if is_allowed_tag(&tag_lower) {
                result.push_str(&tag);
            } else if tag_lower.starts_with("<img") {
                // Extract alt text if present
                if let Some(alt) = extract_attr(&tag, "alt") {
                    let escaped = glib::markup_escape_text(&alt);
                    result.push_str(&escaped);
                }
            }
            // All other tags are stripped (not appended)
        } else if ch == '&' {
            // Check for existing entities — pass through valid ones
            let mut entity = String::new();
            entity.push(chars.next().unwrap()); // '&'
            let mut found_semi = false;
            while let Some(&c) = chars.peek() {
                entity.push(chars.next().unwrap());
                if c == ';' {
                    found_semi = true;
                    break;
                }
                if c == ' ' || entity.len() > 10 {
                    break;
                }
            }
            if found_semi && is_valid_entity(&entity) {
                result.push_str(&entity);
            } else {
                // Escape it
                result.push_str(&glib::markup_escape_text(&entity));
            }
        } else {
            // Regular character — escape only special chars
            chars.next();
            match ch {
                '<' => result.push_str("&lt;"),
                '>' => result.push_str("&gt;"),
                _ => result.push(ch),
            }
        }
    }

    result
}

fn is_allowed_tag(tag_lower: &str) -> bool {
    // Opening tags
    if tag_lower == "<b>" || tag_lower == "</b>"
        || tag_lower == "<i>" || tag_lower == "</i>"
        || tag_lower == "<u>" || tag_lower == "</u>"
        || tag_lower == "</a>"
    {
        return true;
    }
    // <a href="...">
    if tag_lower.starts_with("<a ") && tag_lower.contains("href") {
        return true;
    }
    false
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr_name);
    if let Some(start) = tag.find(&pattern) {
        let value_start = start + pattern.len();
        if let Some(end) = tag[value_start..].find('"') {
            return Some(tag[value_start..value_start + end].to_string());
        }
    }
    // Try single quotes
    let pattern_sq = format!("{}='", attr_name);
    if let Some(start) = tag.find(&pattern_sq) {
        let value_start = start + pattern_sq.len();
        if let Some(end) = tag[value_start..].find('\'') {
            return Some(tag[value_start..value_start + end].to_string());
        }
    }
    None
}

fn is_valid_entity(entity: &str) -> bool {
    matches!(
        entity,
        "&amp;" | "&lt;" | "&gt;" | "&quot;" | "&apos;"
    )
}

/// Extract an image texture from notification hints or app_icon path.
///
/// Priority: `image-data` hint -> `image-path` hint -> `app_icon` file path.
/// Returns `None` for icon-theme names (caller falls back to `Image::from_icon_name`).
pub fn extract_image(
    hints: &HashMap<String, Value>,
    app_icon: &str,
) -> Option<gtk4::gdk::Texture> {
    // Try image-data hint (signature: iiibiiay)
    if let Some(image_data) = hints.get("image-data").or_else(|| hints.get("image_data")) {
        if let Some(texture) = parse_image_data(image_data) {
            return Some(texture);
        }
    }

    // Try image-path hint
    if let Some(image_path) = hints.get("image-path").or_else(|| hints.get("image_path")) {
        if let Some(path_str) = image_path.as_str() {
            if let Ok(texture) = gtk4::gdk::Texture::from_file(&gtk4::gio::File::for_path(path_str)) {
                return Some(texture);
            }
        }
    }

    // Try app_icon as file path
    if app_icon.starts_with('/') {
        if let Ok(texture) = gtk4::gdk::Texture::from_file(&gtk4::gio::File::for_path(app_icon)) {
            return Some(texture);
        }
    }

    // Icon-theme names are handled by the caller
    None
}

fn parse_image_data(value: &Value) -> Option<gtk4::gdk::Texture> {
    use gtk4::gdk;

    // image-data is a struct (iiibiiay)
    // Unwrap variant if needed
    let inner = match value {
        Value::Variant(v) => v.as_ref(),
        other => other,
    };

    let fields = inner.as_struct_fields()?;
    if fields.len() < 7 {
        return None;
    }

    let width = fields[0].as_i32()?;
    let height = fields[1].as_i32()?;
    let rowstride = fields[2].as_i32()?;
    let has_alpha = fields[3].as_bool()?;
    let _bpp = fields[4].as_i32()?;
    let _channels = fields[5].as_i32()?;
    let data: Vec<u8> = fields[6].as_array()?
        .iter()
        .filter_map(|v| v.as_u8())
        .collect();

    if width <= 0 || height <= 0 || data.is_empty() {
        return None;
    }

    let format = if has_alpha {
        gdk::MemoryFormat::R8g8b8a8
    } else {
        gdk::MemoryFormat::R8g8b8
    };

    let bytes = glib::Bytes::from(&data);
    Some(gdk::MemoryTexture::new(width, height, format, &bytes, rowstride as usize).upcast())
}

/// Build a notification widget with icon, text, markup body, and action buttons.
pub fn build_notification_widget(
    notif: &Notification,
    action_tx: Option<mpsc::Sender<(u32, String)>>,
) -> gtk4::Box {
    let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(10);
    content.set_margin_end(10);

    // Image: try hints first, then icon name
    let image_widget = if let Some(texture) = extract_image(&notif.hints, &notif.app_icon) {
        let paintable = texture;
        let img = gtk4::Image::from_paintable(Some(&paintable));
        img.set_pixel_size(32);
        Some(img)
    } else if !notif.app_icon.is_empty() {
        let img = gtk4::Image::from_icon_name(&notif.app_icon);
        img.set_pixel_size(32);
        Some(img)
    } else {
        None
    };

    if let Some(ref img) = image_widget {
        img.set_valign(gtk4::Align::Start);
        img.add_css_class("notification-icon");
        content.append(img);
    }

    // Text column
    let text_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    text_box.set_hexpand(true);

    // Summary
    let summary_label = gtk4::Label::new(Some(&notif.summary));
    summary_label.set_halign(gtk4::Align::Start);
    summary_label.set_wrap(true);
    summary_label.set_max_width_chars(40);
    summary_label.add_css_class("notification-summary");
    text_box.append(&summary_label);

    // Body with markup
    if !notif.body.is_empty() {
        let markup = parse_notification_markup(&notif.body);
        let body_label = gtk4::Label::new(None);
        body_label.set_halign(gtk4::Align::Start);
        body_label.set_wrap(true);
        body_label.set_max_width_chars(40);
        body_label.add_css_class("notification-body");
        // Try markup; fall back to plain text on parse failure
        body_label.set_use_markup(true);
        body_label.set_markup(&markup);
        // If markup parsing fails, GTK will show nothing — set plain text as fallback
        if body_label.text().is_empty() && !notif.body.is_empty() {
            body_label.set_use_markup(false);
            body_label.set_text(&notif.body);
        }
        text_box.append(&body_label);
    }

    // Action buttons
    let has_default = notif.actions.iter().any(|(k, _)| k == "default");
    let non_default_actions: Vec<_> = notif
        .actions
        .iter()
        .filter(|(k, _)| k != "default")
        .collect();

    if !non_default_actions.is_empty() {
        if let Some(ref atx) = action_tx {
            let action_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            action_row.add_css_class("notification-action-row");

            for (key, label) in &non_default_actions {
                let btn = gtk4::Button::with_label(label);
                btn.add_css_class("notification-action-btn");
                let atx_clone = atx.clone();
                let notif_id = notif.id;
                let key_clone = key.to_string();
                btn.connect_clicked(move |_| {
                    let _ = atx_clone.send((notif_id, key_clone.clone()));
                });
                action_row.append(&btn);
            }

            text_box.append(&action_row);
        }
    }

    content.append(&text_box);

    // Default action: clicking the whole widget
    if has_default {
        if let Some(ref atx) = action_tx {
            let gesture = gtk4::GestureClick::new();
            let atx_clone = atx.clone();
            let notif_id = notif.id;
            gesture.connect_released(move |_, _, _, _| {
                let _ = atx_clone.send((notif_id, "default".to_string()));
            });
            content.add_controller(gesture);
        }
    }

    content
}
