/// Notification history panel.
///
/// A floating window anchored at top-right showing scrollable notification history.
/// Toggled by clicking the bell icon in the navigator bar.
/// Notifications are grouped by app name.

use std::collections::BTreeMap;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use super::history::NotificationHistory;
use super::render;

const PANEL_WIDTH: i32 = 380;
const PANEL_HEIGHT: i32 = 400;

/// Manages the notification history panel window.
pub struct NotificationPanel {
    window: gtk4::ApplicationWindow,
    content_box: gtk4::Box,
    history: Rc<RefCell<NotificationHistory>>,
    screen_width: i32,
    visible: Rc<RefCell<bool>>,
}

impl NotificationPanel {
    pub fn new(
        app: &gtk4::Application,
        history: Rc<RefCell<NotificationHistory>>,
    ) -> Self {
        let display = gdk::Display::default().expect("Could not get display");
        let monitors = display.monitors();
        let screen_width = monitors
            .item(0)
            .and_then(|m| m.downcast::<gdk::Monitor>().ok())
            .map(|m| m.geometry().width())
            .unwrap_or(1920);

        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("i3more-notification-panel")
            .decorated(false)
            .resizable(false)
            .default_width(PANEL_WIDTH)
            .default_height(PANEL_HEIGHT)
            .build();
        window.set_size_request(PANEL_WIDTH, -1);
        window.add_css_class("notification-panel");

        // Header with title and clear button
        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        header.add_css_class("notification-panel-header");
        header.set_margin_top(8);
        header.set_margin_start(12);
        header.set_margin_end(12);
        header.set_margin_bottom(4);

        let title_label = gtk4::Label::new(Some("Notifications"));
        title_label.add_css_class("notification-panel-title");
        title_label.set_halign(gtk4::Align::Start);
        title_label.set_hexpand(true);
        header.append(&title_label);

        let clear_btn = gtk4::Button::with_label("Clear All");
        clear_btn.add_css_class("notification-panel-clear");
        header.append(&clear_btn);

        // Scrollable content area
        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        content_box.set_margin_start(8);
        content_box.set_margin_end(8);
        content_box.set_margin_bottom(8);

        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scrolled.set_vexpand(true);
        scrolled.set_child(Some(&content_box));

        // Outer layout
        let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        outer.append(&header);
        outer.append(&scrolled);
        window.set_child(Some(&outer));

        // Clear button handler
        let history_for_clear = history.clone();
        let content_for_clear = content_box.clone();
        clear_btn.connect_clicked(move |_| {
            history_for_clear.borrow_mut().clear();
            rebuild_content(&content_for_clear, &history_for_clear);
        });

        let visible = Rc::new(RefCell::new(false));

        // Auto-hide after 5 seconds when focus leaves the panel
        let hide_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

        {
            let win = window.clone();
            let vis = visible.clone();
            let timer = hide_timer.clone();
            window.connect_notify_local(Some("is-active"), move |w, _| {
                if w.is_active() {
                    // Focus returned — cancel pending hide
                    if let Some(source) = timer.borrow_mut().take() {
                        source.remove();
                    }
                } else {
                    // Focus lost — start 5s auto-hide timer
                    let win_inner = win.clone();
                    let vis_inner = vis.clone();
                    let timer_clear = timer.clone();
                    let source = glib::timeout_add_local_once(
                        std::time::Duration::from_secs(5),
                        move || {
                            timer_clear.borrow_mut().take();
                            if *vis_inner.borrow() {
                                win_inner.set_visible(false);
                                *vis_inner.borrow_mut() = false;
                            }
                        },
                    );
                    *timer.borrow_mut() = Some(source);
                }
            });
        }

        let panel = Self {
            window,
            content_box,
            history,
            screen_width,
            visible,
        };

        panel
    }

    /// Toggle panel visibility. Returns the new visibility state.
    pub fn toggle(&self) -> bool {
        let mut vis = self.visible.borrow_mut();
        if *vis {
            self.window.set_visible(false);
            *vis = false;
            false
        } else {
            self.rebuild();

            // i3 auto-floats via for_window rule. Position and resize after map.
            let x = self.screen_width - PANEL_WIDTH - 8;
            let y = 22; // just below the bar
            let title = "i3more-notification-panel".to_string();
            glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                let criteria = format!("[title=\"{}\"]", title);
                let cmd = format!(
                    "{criteria} resize set {PANEL_WIDTH} px {PANEL_HEIGHT} px, {criteria} move position {x} px {y} px"
                );
                let _ = std::process::Command::new("i3-msg")
                    .args([&cmd])
                    .output();
            });

            self.window.present();

            self.history.borrow_mut().mark_all_read();
            *vis = true;
            true
        }
    }

    /// Rebuild the panel content from current history, grouped by app name.
    pub fn rebuild(&self) {
        rebuild_content(&self.content_box, &self.history);
    }

    /// Returns whether the panel is currently visible.
    pub fn is_visible(&self) -> bool {
        *self.visible.borrow()
    }

    /// Hide the panel.
    pub fn hide(&self) {
        if *self.visible.borrow() {
            self.window.set_visible(false);
            *self.visible.borrow_mut() = false;
        }
    }
}

/// Rebuild panel content from history, grouped by app name.
/// Used by both the rebuild() method and individual dismiss handlers.
fn rebuild_content(
    content_box: &gtk4::Box,
    history: &Rc<RefCell<NotificationHistory>>,
) {
    remove_all_children(content_box);

    let hist = history.borrow();

    if hist.entries.is_empty() {
        let empty_label = gtk4::Label::new(Some("No notifications"));
        empty_label.add_css_class("notification-panel-empty");
        empty_label.set_margin_top(20);
        content_box.append(&empty_label);
        return;
    }

    // Group entries by app_name
    let mut groups: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for entry in &hist.entries {
        let key = if entry.app_name.is_empty() {
            "Unknown".to_string()
        } else {
            entry.app_name.clone()
        };
        groups.entry(key).or_default().push(entry);
    }

    // Must drop the borrow before building widgets that capture history
    let grouped: Vec<(String, Vec<u32>)> = groups
        .iter()
        .map(|(name, entries)| {
            (name.clone(), entries.iter().map(|e| e.id).collect())
        })
        .collect();

    let entries_snapshot: Vec<_> = hist.entries.clone();
    drop(hist);

    for (app_name, entry_ids) in &grouped {
        let group_entries: Vec<_> = entry_ids
            .iter()
            .filter_map(|id| entries_snapshot.iter().find(|e| e.id == *id))
            .collect();

        if group_entries.is_empty() {
            continue;
        }

        // Group header
        let group_header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        group_header.add_css_class("notification-group-header");

        let first_icon = &group_entries[0].app_icon;
        if !first_icon.is_empty() {
            let icon = if first_icon.starts_with('/') {
                gtk4::Image::from_file(first_icon)
            } else {
                gtk4::Image::from_icon_name(first_icon)
            };
            icon.set_pixel_size(24);
            group_header.append(&icon);
        }

        let name_label = gtk4::Label::new(Some(app_name));
        name_label.add_css_class("notification-group-name");
        name_label.set_hexpand(true);
        name_label.set_halign(gtk4::Align::Start);
        group_header.append(&name_label);

        let count_label = gtk4::Label::new(Some(&format!("({})", group_entries.len())));
        count_label.add_css_class("notification-group-count");
        group_header.append(&count_label);

        let toggle_btn = gtk4::ToggleButton::with_label("\u{25bc}"); // ▼
        toggle_btn.set_active(true);
        toggle_btn.add_css_class("notification-close");
        group_header.append(&toggle_btn);

        content_box.append(&group_header);

        // Group content
        let group_content = gtk4::Box::new(gtk4::Orientation::Vertical, 4);

        for entry in &group_entries {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            row.add_css_class("notification-panel-entry");
            row.set_margin_top(4);
            row.set_margin_bottom(4);
            row.set_margin_start(4);
            row.set_margin_end(4);

            // App icon
            if !entry.app_icon.is_empty() {
                let icon = if entry.app_icon.starts_with('/') {
                    gtk4::Image::from_file(&entry.app_icon)
                } else {
                    gtk4::Image::from_icon_name(&entry.app_icon)
                };
                icon.set_pixel_size(24);
                icon.set_valign(gtk4::Align::Start);
                row.append(&icon);
            }

            // Text column
            let text_col = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
            text_col.set_hexpand(true);

            let top_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            let summary = gtk4::Label::new(Some(&entry.summary));
            summary.set_halign(gtk4::Align::Start);
            summary.set_hexpand(true);
            summary.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            summary.set_max_width_chars(30);
            summary.add_css_class("notification-panel-summary");
            top_row.append(&summary);

            let time_label = gtk4::Label::new(Some(&entry.relative_time()));
            time_label.add_css_class("notification-panel-time");
            time_label.set_halign(gtk4::Align::End);
            top_row.append(&time_label);

            text_col.append(&top_row);

            if !entry.body.is_empty() {
                let markup = render::parse_notification_markup(&entry.body);
                let body = gtk4::Label::new(None);
                body.set_halign(gtk4::Align::Start);
                body.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                body.set_max_width_chars(40);
                body.add_css_class("notification-panel-body");
                body.set_use_markup(true);
                body.set_markup(&markup);
                if body.text().is_empty() && !entry.body.is_empty() {
                    body.set_use_markup(false);
                    body.set_text(&entry.body);
                }
                text_col.append(&body);
            }

            row.append(&text_col);

            // Dismiss button — remove single entry then rebuild
            let dismiss_btn = gtk4::Button::with_label("\u{00d7}");
            dismiss_btn.add_css_class("notification-close");
            dismiss_btn.set_valign(gtk4::Align::Start);
            let entry_id = entry.id;
            let history_ref = history.clone();
            let content_ref = content_box.clone();
            dismiss_btn.connect_clicked(move |_| {
                history_ref.borrow_mut().remove(entry_id);
                rebuild_content(&content_ref, &history_ref);
            });
            row.append(&dismiss_btn);

            group_content.append(&row);
        }

        // Toggle expand/collapse
        let gc = group_content.clone();
        toggle_btn.connect_toggled(move |btn| {
            gc.set_visible(btn.is_active());
        });

        content_box.append(&group_content);
    }
}

fn remove_all_children(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
