/// Notification popup window rendering.
///
/// Displays notification popups at the top-right of the screen.
/// Each popup auto-dismisses after its timeout expires.

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use super::render;
use super::types::{Notification, NotifyEvent};

const DEFAULT_TIMEOUT_MS: u32 = 5000;
const POPUP_WIDTH: i32 = 350;
const POPUP_GAP: i32 = 4;
const POPUP_TOP_OFFSET: i32 = 4;
const POPUP_HEIGHT_ESTIMATE: i32 = 90;

struct PopupEntry {
    id: u32,
    window: gtk4::Window,
    timeout_source: Option<glib::SourceId>,
}

/// Manages the stack of visible notification popups.
pub struct PopupManager {
    popups: Rc<RefCell<Vec<PopupEntry>>>,
    close_tx: mpsc::Sender<NotifyEvent>,
    action_tx: mpsc::Sender<(u32, String)>,
    screen_width: i32,
    app: gtk4::Application,
}

impl PopupManager {
    pub fn new(
        app: &gtk4::Application,
        close_tx: mpsc::Sender<NotifyEvent>,
        action_tx: mpsc::Sender<(u32, String)>,
    ) -> Self {
        let display = gdk::Display::default().expect("Could not get display");
        let monitors = display.monitors();
        let screen_width = if let Some(monitor) =
            monitors.item(0).and_then(|m| m.downcast::<gdk::Monitor>().ok())
        {
            monitor.geometry().width()
        } else {
            1920
        };

        Self {
            popups: Rc::new(RefCell::new(Vec::new())),
            close_tx,
            action_tx,
            screen_width,
            app: app.clone(),
        }
    }

    /// Show a notification popup. If a popup with the same ID exists, update it
    /// in-place (no window destroy/recreate). Otherwise create a new popup.
    pub fn show(&self, notif: &Notification) {
        let existing_idx = {
            let popups = self.popups.borrow();
            popups.iter().position(|p| p.id == notif.id)
        };

        if let Some(idx) = existing_idx {
            self.update_existing(idx, notif);
        } else {
            self.create_new(notif);
        }
    }

    /// Update an existing popup's content and reset its timeout timer.
    fn update_existing(&self, idx: usize, notif: &Notification) {
        let mut popups = self.popups.borrow_mut();
        let entry = &mut popups[idx];

        // Cancel the old timeout
        if let Some(source) = entry.timeout_source.take() {
            crate::safe_source_remove(source);
        }

        // Build new content widget
        let widget = render::build_notification_widget(notif, Some(self.action_tx.clone()));

        // Close button
        let close_btn = gtk4::Button::with_label("\u{00d7}"); // ×
        close_btn.add_css_class("notification-close");
        close_btn.set_valign(gtk4::Align::Start);
        let popups_ref = self.popups.clone();
        let notif_id = notif.id;
        let close_tx = self.close_tx.clone();
        close_btn.connect_clicked(move |_| {
            let _ = close_tx.send(NotifyEvent::Close(notif_id));
            dismiss_popup(&popups_ref, notif_id);
        });
        widget.append(&close_btn);

        // Replace the window's child content (no window destroy/recreate)
        entry.window.set_child(Some(&widget));

        // Start fresh timeout
        let timeout_ms = if notif.expire_timeout <= 0 {
            DEFAULT_TIMEOUT_MS
        } else {
            notif.expire_timeout as u32
        };

        let popups_ref = self.popups.clone();
        let id = notif.id;
        entry.timeout_source = Some(glib::timeout_add_local_once(
            std::time::Duration::from_millis(timeout_ms as u64),
            move || {
                dismiss_popup(&popups_ref, id);
            },
        ));
    }

    /// Create a new popup window for a notification.
    fn create_new(&self, notif: &Notification) {
        let win_title = format!("i3more-notify-{}", notif.id);

        let window = gtk4::ApplicationWindow::builder()
            .application(&self.app)
            .title(&win_title)
            .decorated(false)
            .resizable(false)
            .default_width(POPUP_WIDTH)
            .build();
        window.add_css_class("notification-popup");

        // Set X11 properties before the window maps so i3 sees them at
        // classification time. _NET_WM_WINDOW_TYPE_NOTIFICATION makes i3
        // auto-float. _NET_WM_USER_TIME=0 prevents i3 from stealing focus.
        window.connect_realize(|win| {
            let surface = match win.surface() {
                Some(s) => s,
                None => return,
            };
            let x11_surface = match surface.downcast::<gdk4_x11::X11Surface>() {
                Ok(s) => s,
                Err(_) => return,
            };
            let xid = x11_surface.xid();
            let xid_str = xid.to_string();

            let _ = std::process::Command::new("xprop")
                .args([
                    "-id", &xid_str,
                    "-f", "_NET_WM_WINDOW_TYPE", "32a",
                    "-set", "_NET_WM_WINDOW_TYPE", "_NET_WM_WINDOW_TYPE_NOTIFICATION",
                ])
                .output();

            let _ = std::process::Command::new("xprop")
                .args([
                    "-id", &xid_str,
                    "-f", "_NET_WM_USER_TIME", "32c",
                    "-set", "_NET_WM_USER_TIME", "0",
                ])
                .output();
        });

        // Build content using shared render module
        let widget = render::build_notification_widget(notif, Some(self.action_tx.clone()));

        // Close button
        let close_btn = gtk4::Button::with_label("\u{00d7}"); // ×
        close_btn.add_css_class("notification-close");
        close_btn.set_valign(gtk4::Align::Start);
        let popups_ref = self.popups.clone();
        let notif_id = notif.id;
        let close_tx = self.close_tx.clone();
        close_btn.connect_clicked(move |_| {
            let _ = close_tx.send(NotifyEvent::Close(notif_id));
            dismiss_popup(&popups_ref, notif_id);
        });
        widget.append(&close_btn);

        window.set_child(Some(&widget));

        // Capture the currently focused window so we can restore focus after
        // i3 maps and focuses the notification popup.
        let focused_xid = std::process::Command::new("xdotool")
            .args(["getactivewindow"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok());

        // After map, position and resize the floating window, then restore focus.
        let y_offset = self.compute_y_offset();
        let x = self.screen_width - POPUP_WIDTH - 8;
        let y = POPUP_TOP_OFFSET + y_offset;
        let title_for_map = win_title.clone();

        window.connect_map(move |_| {
            let title = title_for_map.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                let criteria = format!("[title=\"{}\"]", title);
                let cmd = format!(
                    "{criteria} resize set {POPUP_WIDTH} px {POPUP_HEIGHT_ESTIMATE} px, {criteria} move position {x} px {y} px"
                );
                let _ = std::process::Command::new("i3-msg")
                    .args([&cmd])
                    .output();

                // Restore focus to the window that was active before the popup
                if let Some(prev_xid) = focused_xid {
                    let _ = std::process::Command::new("i3-msg")
                        .args([&format!("[id={}] focus", prev_xid)])
                        .output();
                }
            });
        });

        window.present();

        // Auto-dismiss timer
        let timeout_ms = if notif.expire_timeout <= 0 {
            DEFAULT_TIMEOUT_MS
        } else {
            notif.expire_timeout as u32
        };

        let popups_ref = self.popups.clone();
        let id = notif.id;
        let timeout_source = Some(glib::timeout_add_local_once(
            std::time::Duration::from_millis(timeout_ms as u64),
            move || {
                dismiss_popup(&popups_ref, id);
            },
        ));

        let window_generic: gtk4::Window = window.upcast();
        self.popups.borrow_mut().push(PopupEntry {
            id: notif.id,
            window: window_generic,
            timeout_source,
        });
    }

    /// Dismiss a popup by notification ID.
    pub fn dismiss(&self, id: u32) {
        dismiss_popup(&self.popups, id);
    }

    /// Compute Y offset for the next popup based on existing popup count.
    fn compute_y_offset(&self) -> i32 {
        let popups = self.popups.borrow();
        popups.len() as i32 * (POPUP_HEIGHT_ESTIMATE + POPUP_GAP)
    }
}

fn dismiss_popup(popups: &Rc<RefCell<Vec<PopupEntry>>>, id: u32) {
    let mut popups = popups.borrow_mut();
    if let Some(idx) = popups.iter().position(|p| p.id == id) {
        let entry = popups.remove(idx);
        if let Some(source) = entry.timeout_source {
            crate::safe_source_remove(source);
        }
        entry.window.close();
    }
}
