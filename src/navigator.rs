//! GTK4 workspace navigator panel.
//!
//! A horizontal floating bar on the bottom screen edge showing workspace numbers
//! and application icons. Clicking a workspace switches to it.

use i3more::icon::{IconResolver, IconResult};
use crate::model::WorkspaceInfo;
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const ICON_SIZE: i32 = 16;
const BAR_HEIGHT: i32 = 40; // match i3 gaps top

/// State shared between the navigator UI and the update mechanism.
pub struct NavigatorState {
    pub icon_resolver: IconResolver,
    pub workspaces: Vec<WorkspaceInfo>,
}

/// Labels returned for periodic updates from main.rs.
pub struct SysinfoLabels {
    pub clock: gtk4::Label,
    pub battery: gtk4::Label,
}

/// Handles for the notification bell icon in the bar.
pub struct NotifyHandles {
    pub bell_label: gtk4::Label,
    pub badge_label: gtk4::Label,
    pub bell_overlay: gtk4::Overlay,
}

/// Handles for the control panel icon in the bar.
pub struct ControlPanelHandles {
    pub cp_overlay: gtk4::Overlay,
}

/// Build the navigator panel and return the window + handles to update its contents.
///
/// Returns (window, workspace_container, tray_box, screen_width, sysinfo_labels, notify_handles, cp_handles).
pub fn build_navigator(
    app: &gtk4::Application,
    state: Rc<RefCell<NavigatorState>>,
) -> (gtk4::ApplicationWindow, Rc<RefCell<gtk4::Box>>, Rc<RefCell<gtk4::Box>>, i32, SysinfoLabels, NotifyHandles, ControlPanelHandles) {
    i3more::css::load_css("style.css", include_str!("../assets/style.css"));

    // Create workspace container (center content)
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    container.add_css_class("workspace-container");
    container.set_overflow(gtk4::Overflow::Hidden);

    let container_ref = Rc::new(RefCell::new(container.clone()));

    // Create tray area (right side) — wraps tray icons + bell icon
    let tray_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
    tray_box.add_css_class("tray-area");
    tray_box.set_halign(gtk4::Align::End);
    tray_box.set_valign(gtk4::Align::Center);

    let tray_box_ref = Rc::new(RefCell::new(tray_box.clone()));

    // Notification bell icon (right side, after tray icons)
    let bell_label = gtk4::Label::new(None);
    bell_label.set_use_markup(true);
    bell_label.set_markup(&crate::fa::fa_icon(crate::fa::BELL, "#a89984", 10));
    bell_label.add_css_class("notification-bell");
    bell_label.set_valign(gtk4::Align::Center);

    let badge_label = gtk4::Label::new(None);
    badge_label.add_css_class("notification-badge");
    badge_label.set_valign(gtk4::Align::Start);
    badge_label.set_halign(gtk4::Align::End);
    badge_label.set_visible(false); // hidden until there are unread notifications

    // Bell + badge in an overlay
    let bell_overlay = gtk4::Overlay::new();
    bell_overlay.set_child(Some(&bell_label));
    bell_overlay.add_overlay(&badge_label);
    bell_overlay.add_css_class("notification-bell-area");

    // Build sysinfo area (left side)
    let sysinfo_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    sysinfo_box.add_css_class("sysinfo-area");
    sysinfo_box.set_valign(gtk4::Align::Center);

    // Battery label
    let battery_label = gtk4::Label::new(None);
    battery_label.set_use_markup(true);
    battery_label.add_css_class("sysinfo-label");
    battery_label.set_valign(gtk4::Align::Center);
    if let Some((cap, status)) = crate::sysinfo::read_battery() {
        let charging = status == "Charging";
        let glyph = crate::fa::battery_glyph(cap, charging);
        let icon_markup = crate::fa::fa_icon(glyph, "#a89984", 9);
        battery_label.set_markup(&format!("{} {}%", icon_markup, cap));
    }
    // Only add battery label if a battery exists
    if crate::sysinfo::read_battery().is_some() {
        sysinfo_box.append(&battery_label);
    }

    // Clock label
    let clock_label = gtk4::Label::new(Some(&crate::sysinfo::read_clock()));
    clock_label.add_css_class("sysinfo-label");
    clock_label.set_valign(gtk4::Align::Center);
    sysinfo_box.append(&clock_label);

    // System stats icon with hover popover (FA gauge glyph)
    let stats_icon = gtk4::Label::new(None);
    stats_icon.set_use_markup(true);
    stats_icon.set_markup(&crate::fa::fa_icon(crate::fa::GAUGE, "#a89984", 10));
    stats_icon.add_css_class("sysinfo-icon");
    stats_icon.set_valign(gtk4::Align::Center);
    sysinfo_box.append(&stats_icon);

    let popover = crate::sysinfo::build_stats_popover(&stats_icon);
    let popover_for_hover = popover.clone();
    let hover_ctrl = gtk4::EventControllerMotion::new();
    hover_ctrl.connect_enter(move |_, _, _| {
        popover_for_hover.popup();
    });
    stats_icon.add_controller(hover_ctrl);

    let sysinfo_labels = SysinfoLabels {
        clock: clock_label,
        battery: battery_label,
    };

    // Control panel (sliders) icon
    let cp_label = gtk4::Label::new(None);
    cp_label.set_use_markup(true);
    cp_label.set_markup(&crate::fa::fa_icon(crate::fa::SLIDERS, "#a89984", 10));
    cp_label.add_css_class("control-panel-icon");
    cp_label.set_valign(gtk4::Align::Center);

    let cp_overlay = gtk4::Overlay::new();
    cp_overlay.set_child(Some(&cp_label));
    cp_overlay.add_css_class("control-panel-icon");

    // Right side: tray icons + sliders icon + bell
    let right_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    right_box.set_halign(gtk4::Align::End);
    right_box.set_valign(gtk4::Align::Center);
    right_box.append(&tray_box);
    right_box.append(&cp_overlay);
    right_box.append(&bell_overlay);

    // CenterBox layout: sysinfo left, workspaces centered, right side on the right
    let center_box = gtk4::CenterBox::new();
    center_box.add_css_class("navigator");
    center_box.set_start_widget(Some(&sysinfo_box));
    center_box.set_center_widget(Some(&container));
    center_box.set_end_widget(Some(&right_box));

    // Build initial workspace entries
    render_workspaces(&container, &state);

    // Create the window
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("i3More-navigator")
        .resizable(false)
        .decorated(false)
        .child(&center_box)
        .build();

    // Force exact height and full screen width
    let display = gdk::Display::default().expect("Could not get display");
    let monitors = display.monitors();
    let screen_width = if let Some(monitor) = monitors.item(0).and_then(|m| m.downcast::<gdk::Monitor>().ok()) {
        monitor.geometry().width()
    } else {
        1920
    };
    window.set_size_request(screen_width, BAR_HEIGHT);
    window.set_default_size(screen_width, BAR_HEIGHT);

    let notify_handles = NotifyHandles {
        bell_label,
        badge_label,
        bell_overlay,
    };

    let cp_handles = ControlPanelHandles {
        cp_overlay,
    };

    (window, container_ref, tray_box_ref, screen_width, sysinfo_labels, notify_handles, cp_handles)
}

/// Re-render all workspace entries in the container.
pub fn render_workspaces(
    container: &gtk4::Box,
    state: &Rc<RefCell<NavigatorState>>,
) {
    // Remove all existing children
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let workspaces = state.borrow().workspaces.clone();

    for ws in &workspaces {
        let entry = build_workspace_entry(ws, state);
        container.append(&entry);
    }
}

/// Build a single workspace entry widget.
fn build_workspace_entry(
    ws: &WorkspaceInfo,
    state: &Rc<RefCell<NavigatorState>>,
) -> gtk4::Box {
    let entry_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    entry_box.add_css_class("workspace-entry");
    entry_box.set_valign(gtk4::Align::Center);
    entry_box.set_vexpand(false);

    if ws.focused {
        entry_box.add_css_class("focused");
    }
    if ws.urgent {
        entry_box.add_css_class("urgent");
    }
    if ws.visible && !ws.focused {
        entry_box.add_css_class("visible");
    }

    // Workspace number on top
    let num_label = gtk4::Label::new(Some(&ws.num.to_string()));
    num_label.add_css_class("workspace-num");
    num_label.set_halign(gtk4::Align::Center);
    num_label.set_vexpand(false);
    entry_box.append(&num_label);

    // App icons in a horizontal row below the number
    if !ws.window_classes.is_empty() {
        let icon_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        icon_row.set_halign(gtk4::Align::Center);
        let mut state_mut = state.borrow_mut();
        for class in &ws.window_classes {
            let icon_result = state_mut.icon_resolver.resolve(class);
            let image = create_icon_widget(&icon_result);
            icon_row.append(&image);
        }
        entry_box.append(&icon_row);
    }

    // Click handler to switch workspace
    let ws_num = ws.num;
    let gesture = gtk4::GestureClick::new();
    gesture.connect_released(move |_, _, _, _| {
        switch_workspace(ws_num);
    });
    entry_box.add_controller(gesture);

    entry_box
}

/// Create a GTK Image widget for an icon result.
fn create_icon_widget(icon_result: &IconResult) -> gtk4::Image {
    let image = match icon_result {
        IconResult::FilePath(path) => {
            gtk4::Image::from_file(path)
        }
        IconResult::IconName(name) => {
            gtk4::Image::from_icon_name(name)
        }
        IconResult::NotFound => {
            gtk4::Image::from_icon_name("application-x-executable")
        }
    };

    image.set_pixel_size(ICON_SIZE);
    image.add_css_class("workspace-icon");
    image.set_valign(gtk4::Align::Center);
    image.set_vexpand(false);
    image
}

/// Send an i3 command to switch to a workspace.
fn switch_workspace(num: i64) {
    // Run in a background thread to avoid blocking GTK
    glib::spawn_future_local(async move {
        let _ = std::thread::spawn(move || {
            if let Ok(mut conn) = crate::ipc::I3Connection::connect() {
                let _ = conn.run_command(&format!("workspace number {}", num));
            }
        })
        .join();
    });
}
