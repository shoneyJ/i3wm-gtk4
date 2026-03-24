//! i3more-lock — X11 screen locker with GTK4 rendering and PAM authentication.
//!
//! Architecture: x11rb handles input capture (XGrabKeyboard/XGrabPointer) and
//! override-redirect cover windows. GTK4 is used purely for rendering (clock,
//! password feedback, status). Key events are bridged from the X11 event thread
//! to the GTK main loop via channels + glib::timeout_add_local.

mod lock;

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use zeroize::Zeroizing;

fn main() {
    i3more::init_logging("i3more-lock");

    // Security: crash handler falls back to i3lock
    lock::security::set_crash_handler();

    // Security: OOM protection — prevent kernel from killing the lock screen
    lock::security::set_oom_score();

    let app = gtk4::Application::builder()
        .application_id("com.i3more.lock")
        .build();

    app.connect_activate(on_activate);
    app.run();
}

fn on_activate(app: &gtk4::Application) {
    i3more::css::load_css("lock-screen.css", include_str!("../assets/lock-screen.css"));

    let config = lock::config::load();
    let (lock_ui, window) = lock::ui::build(app, &config);

    let password: Rc<RefCell<Zeroizing<String>>> =
        Rc::new(RefCell::new(Zeroizing::new(String::with_capacity(64))));
    let failed_attempts: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
    let locked_until: Rc<RefCell<Option<std::time::Instant>>> = Rc::new(RefCell::new(None));
    let authenticating: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    // Start X11 input capture after the GTK window is realized (XID available)
    let pw = password.clone();
    let ui = lock_ui.clone();
    let fails = failed_attempts.clone();
    let locked = locked_until.clone();
    let authing = authenticating.clone();
    let app_ref = app.clone();

    window.connect_realize(move |win| {
        let surface = match win.surface() {
            Some(s) => s,
            None => return,
        };
        let x11_surface = match surface.downcast::<gdk4_x11::X11Surface>() {
            Ok(s) => s,
            Err(_) => return,
        };
        let xid = x11_surface.xid() as u32;

        let (key_tx, key_rx) = mpsc::channel();

        // X11 input thread: grabs keyboard/pointer, reads KeyPress events
        std::thread::spawn(move || {
            if let Err(e) = lock::x11::run(xid, key_tx) {
                log::error!("X11 input loop failed: {}", e);
                // Emergency fallback — never leave session unlocked
                let _ = std::process::Command::new("/usr/bin/i3lock")
                    .args(["-c", "000000"])
                    .spawn();
            }
        });

        // Inhibit VT switching via logind
        std::thread::spawn(|| {
            match lock::security::inhibit_vt_switch() {
                Ok(_) => log::info!("VT switch inhibitor active"),
                Err(e) => log::warn!("Failed to inhibit VT switch: {}", e),
            }
        });

        // Poll key events from X11 thread (60 Hz)
        let pw = pw.clone();
        let ui = ui.clone();
        let fails = fails.clone();
        let locked = locked.clone();
        let authing = authing.clone();
        let app_ref = app_ref.clone();

        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            while let Ok(action) = key_rx.try_recv() {
                handle_key(&action, &pw, &ui, &fails, &locked, &authing, &app_ref);
            }
            glib::ControlFlow::Continue
        });
    });

    // Update clock every second
    let clock = lock_ui.clock_label.clone();
    let date = lock_ui.date_label.clone();
    let clock_format = config.clock_format.clone();
    lock::ui::update_clock(&clock, &date, &clock_format);
    glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        lock::ui::update_clock(&clock, &date, &clock_format);
        glib::ControlFlow::Continue
    });

    window.present();
}

/// Calculate brute-force backoff duration in seconds given a failure count.
/// Returns `None` if count < 3 (no backoff), otherwise exponential backoff
/// capped at 32 seconds: 1s, 2s, 4s, 8s, 16s, 32s.
fn backoff_secs(failed_count: u32) -> Option<u64> {
    if failed_count < 3 {
        return None;
    }
    Some(1u64 << (failed_count - 3).min(5))
}

fn handle_key(
    action: &lock::x11::KeyAction,
    password: &Rc<RefCell<Zeroizing<String>>>,
    ui: &lock::ui::LockUi,
    failed_attempts: &Rc<RefCell<u32>>,
    locked_until: &Rc<RefCell<Option<std::time::Instant>>>,
    authenticating: &Rc<RefCell<bool>>,
    app: &gtk4::Application,
) {
    // Ignore input while PAM authentication is in progress
    if *authenticating.borrow() {
        return;
    }

    // Enforce brute-force backoff
    if let Some(until) = *locked_until.borrow() {
        if std::time::Instant::now() < until {
            return;
        }
        *locked_until.borrow_mut() = None;
        ui.status_label.set_text("");
    }

    match action {
        lock::x11::KeyAction::Character(c) => {
            password.borrow_mut().push(*c);
            ui.update_dots(password.borrow().len());
            ui.status_label.set_text("");
            ui.status_label.remove_css_class("error");
        }
        lock::x11::KeyAction::Backspace => {
            password.borrow_mut().pop();
            ui.update_dots(password.borrow().len());
        }
        lock::x11::KeyAction::Return => {
            let pw_val = password.borrow().clone();
            if pw_val.is_empty() {
                return;
            }

            *authenticating.borrow_mut() = true;
            ui.status_label.set_text("Authenticating\u{2026}");
            ui.status_label.remove_css_class("error");

            let username = lock::auth::get_username();
            let (auth_tx, auth_rx) = mpsc::channel();

            // PAM authentication in a blocking thread
            std::thread::spawn(move || {
                let result = lock::auth::authenticate(&username, &pw_val);
                let _ = auth_tx.send(result);
            });

            // Poll for auth result
            let pw = password.clone();
            let ui = ui.clone();
            let fails = failed_attempts.clone();
            let locked = locked_until.clone();
            let authing = authenticating.clone();
            let app = app.clone();

            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                match auth_rx.try_recv() {
                    Ok(Ok(())) => {
                        log::info!("Authentication successful, unlocking");
                        app.quit();
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        log::warn!("Authentication failed: {}", e);
                        *authing.borrow_mut() = false;

                        // Zeroize and clear password
                        pw.borrow_mut().clear();
                        ui.update_dots(0);

                        let mut count = fails.borrow_mut();
                        *count += 1;

                        if let Some(secs) = backoff_secs(*count) {
                            let until = std::time::Instant::now()
                                + std::time::Duration::from_secs(secs);
                            *locked.borrow_mut() = Some(until);
                            ui.status_label.set_text(&format!("Try again in {}s", secs));
                            ui.status_label.add_css_class("error");

                            // Countdown timer
                            let status = ui.status_label.clone();
                            let locked_ref = locked.clone();
                            glib::timeout_add_local(
                                std::time::Duration::from_secs(1),
                                move || {
                                    if let Some(until) = *locked_ref.borrow() {
                                        let remaining =
                                            until.saturating_duration_since(std::time::Instant::now());
                                        if remaining.as_secs() > 0 {
                                            status.set_text(&format!(
                                                "Try again in {}s",
                                                remaining.as_secs()
                                            ));
                                            return glib::ControlFlow::Continue;
                                        }
                                    }
                                    status.set_text("");
                                    status.remove_css_class("error");
                                    glib::ControlFlow::Break
                                },
                            );
                        } else {
                            ui.status_label.set_text("Authentication failed");
                            ui.status_label.add_css_class("error");
                            let status = ui.status_label.clone();
                            glib::timeout_add_local_once(
                                std::time::Duration::from_secs(2),
                                move || {
                                    status.set_text("");
                                    status.remove_css_class("error");
                                },
                            );
                        }

                        glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        log::error!("Auth thread disconnected unexpectedly");
                        *authing.borrow_mut() = false;
                        ui.status_label.set_text("Internal error");
                        ui.status_label.add_css_class("error");
                        glib::ControlFlow::Break
                    }
                }
            });
        }
        lock::x11::KeyAction::Escape => {
            // Clear password buffer (zeroize happens via Zeroizing::clear)
            password.borrow_mut().clear();
            ui.update_dots(0);
            ui.status_label.set_text("");
            ui.status_label.remove_css_class("error");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_no_delay_under_3() {
        assert_eq!(backoff_secs(0), None);
        assert_eq!(backoff_secs(1), None);
        assert_eq!(backoff_secs(2), None);
    }

    #[test]
    fn backoff_exponential_schedule() {
        assert_eq!(backoff_secs(3), Some(1));
        assert_eq!(backoff_secs(4), Some(2));
        assert_eq!(backoff_secs(5), Some(4));
        assert_eq!(backoff_secs(6), Some(8));
        assert_eq!(backoff_secs(7), Some(16));
        assert_eq!(backoff_secs(8), Some(32));
    }

    #[test]
    fn backoff_caps_at_32s() {
        assert_eq!(backoff_secs(9), Some(32));
        assert_eq!(backoff_secs(100), Some(32));
    }
}
