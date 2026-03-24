/// Dedicated control panel — Android-style quick settings.
///
/// A floating window anchored at top-right showing volume, backlight, and
/// background controls. Toggled by clicking the sliders icon in the navigator bar.

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const PANEL_WIDTH: i32 = 400;
const PANEL_HEIGHT: i32 = 520;

/// Manages the control panel window.
pub struct ControlPanel {
    window: gtk4::ApplicationWindow,
    screen_width: i32,
    screen_height: i32,
    visible: Rc<RefCell<bool>>,
}

impl ControlPanel {
    pub fn new(app: &gtk4::Application) -> Self {
        i3more::css::load_css("control-panel.css", include_str!("../../assets/control-panel.css"));

        let display = gdk::Display::default().expect("Could not get display");
        let monitors = display.monitors();
        let monitor = monitors
            .item(0)
            .and_then(|m| m.downcast::<gdk::Monitor>().ok());
        let screen_width = monitor.as_ref().map(|m| m.geometry().width()).unwrap_or(1920);
        let screen_height = monitor.as_ref().map(|m| m.geometry().height()).unwrap_or(1080);

        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("i3more-control-panel")
            .decorated(false)
            .resizable(false)
            .default_width(PANEL_WIDTH)
            .default_height(PANEL_HEIGHT)
            .build();
        window.set_size_request(PANEL_WIDTH, -1);
        window.add_css_class("control-panel");

        // Set X11 properties so i3 auto-floats and doesn't steal focus.
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

        // Header with title and close button
        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        header.add_css_class("control-panel-header");
        header.set_margin_top(8);
        header.set_margin_start(12);
        header.set_margin_end(12);
        header.set_margin_bottom(4);

        let title_label = gtk4::Label::new(Some("Control Panel"));
        title_label.add_css_class("control-panel-title");
        title_label.set_halign(gtk4::Align::Start);
        title_label.set_hexpand(true);
        header.append(&title_label);

        let close_btn = gtk4::Button::with_label("\u{00d7}");
        close_btn.add_css_class("notification-close");
        header.append(&close_btn);

        // Build sections
        let sections = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        sections.set_margin_start(8);
        sections.set_margin_end(8);
        sections.set_margin_bottom(8);

        // Audio section
        let audio_section = build_section(
            "Audio",
            crate::fa::VOLUME_HIGH,
            super::widgets::volume::build_widget(),
        );
        sections.append(&audio_section);

        // Display section (conditional)
        if let Some(bl) = super::widgets::backlight::build_widget() {
            let display_section = build_section(
                "Display",
                crate::fa::SUN,
                bl,
            );
            sections.append(&display_section);
        }

        // Background section
        let bg_section = build_section(
            "Background",
            crate::fa::IMAGE,
            super::widgets::background::build_widget(),
        );
        sections.append(&bg_section);

        // Scrollable content
        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scrolled.set_vexpand(true);
        scrolled.set_child(Some(&sections));

        // Outer layout
        let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        outer.append(&header);
        outer.append(&scrolled);
        window.set_child(Some(&outer));

        let visible = Rc::new(RefCell::new(false));

        // Close button handler
        {
            let win = window.clone();
            let vis = visible.clone();
            close_btn.connect_clicked(move |_| {
                win.set_visible(false);
                *vis.borrow_mut() = false;
            });
        }

        // Auto-hide after 5 seconds when focus leaves the panel
        let hide_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

        {
            let win = window.clone();
            let vis = visible.clone();
            let timer = hide_timer.clone();
            window.connect_notify_local(Some("is-active"), move |w, _| {
                if w.is_active() {
                    if let Some(source) = timer.borrow_mut().take() {
                        crate::safe_source_remove(source);
                    }
                } else {
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

        Self {
            window,
            screen_width,
            screen_height,
            visible,
        }
    }

    /// Toggle panel visibility. Returns the new visibility state.
    pub fn toggle(&self) -> bool {
        let mut vis = self.visible.borrow_mut();
        if *vis {
            self.window.set_visible(false);
            *vis = false;
            false
        } else {
            let x = (self.screen_width - PANEL_WIDTH) / 2;
            let y = (self.screen_height - PANEL_HEIGHT) / 2;
            let title = "i3more-control-panel".to_string();

            // Capture currently focused window so we can restore focus after present()
            let focused_xid = std::process::Command::new("xdotool")
                .args(["getactivewindow"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.trim().parse::<u64>().ok());

            glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                let criteria = format!("[title=\"{}\"]", title);
                let cmd = format!(
                    "{criteria} floating enable, {criteria} resize set {PANEL_WIDTH} px {PANEL_HEIGHT} px, {criteria} move position {x} px {y} px"
                );
                let _ = std::process::Command::new("i3-msg")
                    .args([&cmd])
                    .output();

                // Restore focus to the window that was active before the panel opened
                if let Some(prev_xid) = focused_xid {
                    let _ = std::process::Command::new("i3-msg")
                        .args([&format!("[id={}] focus", prev_xid)])
                        .output();
                }
            });

            self.window.present();
            *vis = true;
            true
        }
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

/// Build a card-style section with a header (icon + title) and content widget.
fn build_section(title: &str, icon: char, content: gtk4::Box) -> gtk4::Box {
    let section = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    section.add_css_class("cp-section");

    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    header.add_css_class("cp-section-header");
    header.set_margin_start(8);
    header.set_margin_end(8);
    header.set_margin_top(6);
    header.set_margin_bottom(2);

    let icon_label = gtk4::Label::new(None);
    icon_label.set_use_markup(true);
    icon_label.set_markup(&crate::fa::fa_icon(icon, "#a89984", 11));

    let title_label = gtk4::Label::new(Some(title));
    title_label.add_css_class("cp-section-title");
    title_label.set_halign(gtk4::Align::Start);

    header.append(&icon_label);
    header.append(&title_label);

    section.append(&header);

    // Remove the widget's own header since the section provides one
    // The widget content is appended as-is
    section.append(&content);

    section
}
