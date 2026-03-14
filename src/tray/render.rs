/// GTK rendering for system tray icons.

use std::collections::HashMap;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;

use super::dbusmenu;
use super::types::{TrayItemId, TrayItemProps, TrayPixmap};

const TRAY_ICON_SIZE: i32 = 16;

/// Rebuild the tray_box with current tray items.
pub fn render_tray(tray_box: &gtk4::Box, items: &HashMap<TrayItemId, TrayItemProps>) {
    // Remove all existing children
    while let Some(child) = tray_box.first_child() {
        tray_box.remove(&child);
    }

    // Sort by bus_name for stable ordering
    let mut sorted: Vec<&TrayItemProps> = items.values().collect();
    sorted.sort_by(|a, b| a.id.bus_name.cmp(&b.id.bus_name));

    for props in sorted {
        // Skip passive items
        if props.status == "Passive" {
            continue;
        }

        let image = create_tray_icon(props);
        let event_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        event_box.append(&image);
        event_box.set_valign(gtk4::Align::Center);

        // Click handling
        attach_click_handlers(&event_box, &props.id, props.item_is_menu, props.menu.as_deref());

        if !props.tooltip.is_empty() {
            event_box.set_tooltip_text(Some(&props.tooltip));
        } else if !props.title.is_empty() {
            event_box.set_tooltip_text(Some(&props.title));
        }

        tray_box.append(&event_box);
    }
}

fn create_tray_icon(props: &TrayItemProps) -> gtk4::Image {
    // Prefer icon_name from theme, fall back to pixmap
    let image = if !props.icon_name.is_empty() {
        gtk4::Image::from_icon_name(&props.icon_name)
    } else if let Some(ref pixmaps) = props.icon_pixmap {
        if let Some(texture) = best_pixmap_texture(pixmaps, TRAY_ICON_SIZE) {
            gtk4::Image::from_paintable(Some(&texture))
        } else {
            gtk4::Image::from_icon_name("application-x-executable")
        }
    } else {
        gtk4::Image::from_icon_name("application-x-executable")
    };

    image.set_pixel_size(TRAY_ICON_SIZE);
    image.add_css_class("tray-icon");
    image.set_valign(gtk4::Align::Center);
    image.set_vexpand(false);
    image
}

/// Select the pixmap closest to the target size and convert ARGB → RGBA texture.
fn best_pixmap_texture(pixmaps: &[TrayPixmap], target: i32) -> Option<gdk::MemoryTexture> {
    if pixmaps.is_empty() {
        return None;
    }

    // Find closest size to target
    let best = pixmaps
        .iter()
        .min_by_key(|p| (p.width - target).abs())?;

    if best.width <= 0 || best.height <= 0 {
        return None;
    }

    let expected_len = (best.width * best.height * 4) as usize;
    if best.argb_data.len() < expected_len {
        return None;
    }

    // Convert ARGB (big-endian, network byte order per SNI spec) to RGBA
    let mut rgba = Vec::with_capacity(expected_len);
    for pixel in best.argb_data[..expected_len].chunks_exact(4) {
        // ARGB big-endian: [A, R, G, B]
        rgba.push(pixel[1]); // R
        rgba.push(pixel[2]); // G
        rgba.push(pixel[3]); // B
        rgba.push(pixel[0]); // A
    }

    let bytes = glib::Bytes::from(&rgba);
    let stride = best.width * 4;

    Some(gdk::MemoryTexture::new(
        best.width,
        best.height,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        stride as usize,
    ))
}

/// Attach left/right/middle click handlers to invoke D-Bus methods on the tray item.
/// If the item has a Menu path, left and right clicks open the DBusMenu popup instead
/// of calling Activate/ContextMenu (many apps like nm-applet don't expose Activate at all).
fn attach_click_handlers(widget: &gtk4::Box, id: &TrayItemId, item_is_menu: bool, menu_path: Option<&str>) {
    let use_dbusmenu = menu_path.is_some();

    if use_dbusmenu {
        let menu_path = menu_path.unwrap().to_string();
        let id_click = id.clone();
        let widget_ref = widget.clone();
        let menu_path_l = menu_path.clone();

        let left_click = gtk4::GestureClick::builder().button(1).build();
        left_click.connect_released(move |_, _, _, _| {
            dbusmenu::show_menu(&widget_ref, &id_click, &menu_path_l);
        });
        widget.add_controller(left_click);

        let id_click = id.clone();
        let widget_ref = widget.clone();
        let menu_path_r = menu_path.clone();

        let right_click = gtk4::GestureClick::builder().button(3).build();
        right_click.connect_released(move |_, _, _, _| {
            dbusmenu::show_menu(&widget_ref, &id_click, &menu_path_r);
        });
        widget.add_controller(right_click);
    } else {
        let id_left = id.clone();
        let left_click = gtk4::GestureClick::builder().button(1).build();
        left_click.connect_released(move |_, _, x, y| {
            invoke_item_method(&id_left, "Activate", x as i32, y as i32);
        });
        widget.add_controller(left_click);

        let id_right = id.clone();
        let right_click = gtk4::GestureClick::builder().button(3).build();
        right_click.connect_released(move |_, _, x, y| {
            invoke_item_method(&id_right, "ContextMenu", x as i32, y as i32);
        });
        widget.add_controller(right_click);
    }

    // Middle click always uses SecondaryActivate
    let id_middle = id.clone();
    let middle_click = gtk4::GestureClick::builder().button(2).build();
    middle_click.connect_released(move |_, _, x, y| {
        invoke_item_method(&id_middle, "SecondaryActivate", x as i32, y as i32);
    });
    widget.add_controller(middle_click);
}

/// Call Activate/ContextMenu/SecondaryActivate on the item's D-Bus interface.
fn invoke_item_method(id: &TrayItemId, method: &str, x: i32, y: i32) {
    let bus_name = id.bus_name.clone();
    let object_path = id.object_path.clone();
    let method = method.to_string();

    glib::spawn_future_local(async move {
        let _ = std::thread::spawn(move || {
            async_io::block_on(async {
                let conn = match zbus::Connection::session().await {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!("Tray D-Bus connect failed: {}", e);
                        return;
                    }
                };
                let proxy = match zbus::Proxy::new(
                    &conn,
                    bus_name.as_str(),
                    object_path.as_str(),
                    "org.kde.StatusNotifierItem",
                )
                .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!("Tray proxy failed: {}", e);
                        return;
                    }
                };
                let result: Result<(), zbus::Error> =
                    proxy.call(method.as_str(), &(x, y)).await;
                if let Err(e) = result {
                    log::warn!("Tray {} call failed: {}", method, e);
                }
            });
        })
        .join();
    });
}
