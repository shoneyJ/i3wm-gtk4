//! GTK4 lock screen UI — rendering only (no input handling).

use gtk4::prelude::*;

#[derive(Clone)]
pub struct LockUi {
    pub dots_label: gtk4::Label,
    pub status_label: gtk4::Label,
    pub clock_label: gtk4::Label,
    pub date_label: gtk4::Label,
}

impl LockUi {
    pub fn update_dots(&self, count: usize) {
        let dots: String = std::iter::repeat("●").take(count).collect::<Vec<_>>().join("  ");
        self.dots_label.set_text(&dots);
    }
}

pub fn build(
    app: &gtk4::Application,
    config: &super::config::LockConfig,
) -> (LockUi, gtk4::ApplicationWindow) {
    // Get primary monitor dimensions for window sizing
    let display = gtk4::gdk::Display::default().expect("No display available");
    let monitors = display.monitors();
    let (width, height) = if let Some(mon) = monitors.item(0) {
        let monitor = mon.downcast::<gtk4::gdk::Monitor>().unwrap();
        let geo = monitor.geometry();
        (geo.width(), geo.height())
    } else {
        (1920, 1080)
    };

    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .decorated(false)
        .default_width(width)
        .default_height(height)
        .build();

    window.add_css_class("lock-screen");

    // Center-aligned vertical layout
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_halign(gtk4::Align::Center);
    vbox.set_valign(gtk4::Align::Center);

    // Avatar (replaces username) or username fallback
    let has_avatar = if let Some(ref avatar_path) = config.avatar_path {
        if let Ok(texture) = gtk4::gdk::Texture::from_file(&gtk4::gio::File::for_path(avatar_path)) {
            let avatar = gtk4::Image::from_paintable(Some(&texture));
            avatar.set_pixel_size(96);
            avatar.add_css_class("lock-avatar");
            vbox.append(&avatar);
            true
        } else {
            false
        }
    } else {
        false
    };

    if !has_avatar {
        let username_label = gtk4::Label::new(Some(&super::auth::get_username()));
        username_label.add_css_class("lock-username");
        vbox.append(&username_label);
    }

    // Password textbox
    let password_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    password_box.add_css_class("lock-password-box");
    password_box.set_halign(gtk4::Align::Center);

    let dots_label = gtk4::Label::new(None);
    dots_label.add_css_class("lock-dots");
    dots_label.set_halign(gtk4::Align::Start);
    dots_label.set_hexpand(true);
    password_box.append(&dots_label);

    vbox.append(&password_box);

    // Status messages (errors, backoff countdowns)
    let status_label = gtk4::Label::new(None);
    status_label.add_css_class("lock-status");
    status_label.set_height_request(24);
    vbox.append(&status_label);

    // Spacer between input area and clock
    let spacer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    spacer.set_height_request(48);
    vbox.append(&spacer);

    // Clock
    let clock_label = gtk4::Label::new(None);
    clock_label.add_css_class("lock-clock");
    vbox.append(&clock_label);

    // Date
    let date_label = gtk4::Label::new(None);
    date_label.add_css_class("lock-date");
    vbox.append(&date_label);

    window.set_child(Some(&vbox));

    let ui = LockUi {
        dots_label,
        status_label,
        clock_label,
        date_label,
    };

    (ui, window)
}

/// Update clock and date labels using the system `date` command.
pub fn update_clock(clock: &gtk4::Label, date: &gtk4::Label, clock_format: &str) {
    let fmt = format!("+{}", clock_format);
    if let Ok(output) = std::process::Command::new("date").arg(&fmt).output() {
        if output.status.success() {
            clock.set_text(String::from_utf8_lossy(&output.stdout).trim());
        }
    }
    if let Ok(output) = std::process::Command::new("date").arg("+%A, %B %d").output() {
        if output.status.success() {
            date.set_text(String::from_utf8_lossy(&output.stdout).trim());
        }
    }
}
