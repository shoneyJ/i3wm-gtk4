//! X11 input capture and cover window management.
//!
//! GTK4 cannot forward X11 key events after XGrabKeyboard, so this module
//! handles all input via a separate x11rb connection. Key events are sent
//! to the GTK main loop through an mpsc channel.

use std::sync::mpsc;
use x11rb::connection::Connection;
use x11rb::protocol::randr;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

/// Actions derived from X11 key events, sent to the GTK UI thread.
pub enum KeyAction {
    Character(char),
    Backspace,
    Return,
    Escape,
}

struct KeyboardMap {
    min_keycode: u8,
    keysyms_per_keycode: usize,
    keysyms: Vec<u32>,
}

/// Main X11 input loop. Connects to the display, sets override-redirect on
/// the GTK window, creates black cover windows on all monitors, grabs
/// keyboard and pointer, then reads key events until the channel closes.
pub fn run(
    gtk_xid: u32,
    tx: mpsc::Sender<KeyAction>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // Set override-redirect on the GTK window so the WM cannot close/move it
    conn.change_window_attributes(
        gtk_xid,
        &ChangeWindowAttributesAux::new().override_redirect(1),
    )?;
    conn.flush()?;

    // Create black cover windows on every monitor (defense in depth)
    let covers = create_cover_windows(&conn, screen_num)?;

    // Raise the GTK window above the covers
    conn.configure_window(
        gtk_xid,
        &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
    )?;
    conn.flush()?;

    // Grab keyboard and pointer on root (captures ALL input globally)
    grab_keyboard(&conn, root)?;
    grab_pointer(&conn, root)?;

    log::info!("Lock screen active: grabs acquired, covers placed");

    // Load keyboard mapping for keysym resolution
    let mapping = load_keyboard_map(&conn)?;

    let mut last_grab_refresh = std::time::Instant::now();

    loop {
        // Check if the application is shutting down (successful PAM auth)
        if i3more::shutdown_requested() {
            log::info!("X11 input loop: shutdown requested, destroying covers");
            destroy_covers(&conn, &covers);
            return Ok(());
        }

        // Drain all pending X11 events
        while let Some(event) = conn.poll_for_event()? {
            if let x11rb::protocol::Event::KeyPress(ev) = event {
                if let Some(action) = resolve_key(&mapping, ev.detail, ev.state.into()) {
                    if tx.send(action).is_err() {
                        // GTK side closed — clean up and exit
                        destroy_covers(&conn, &covers);
                        return Ok(());
                    }
                }
            }
        }

        // Periodically re-grab to defend against late-arriving override-redirect
        // windows stealing focus (xsecurelock pattern)
        if last_grab_refresh.elapsed() > std::time::Duration::from_secs(1) {
            let _ = grab_keyboard(&conn, root);
            let _ = grab_pointer(&conn, root);
            // Re-raise GTK window and covers
            for &w in &covers {
                let _ = conn.configure_window(
                    w,
                    &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
                );
            }
            let _ = conn.configure_window(
                gtk_xid,
                &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            );
            let _ = conn.flush();
            last_grab_refresh = std::time::Instant::now();
        }

        // 10ms sleep keeps CPU usage negligible while maintaining <10ms input latency
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn grab_keyboard(
    conn: &RustConnection,
    window: u32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for attempt in 0..50 {
        let reply = conn
            .grab_keyboard(false, window, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)?
            .reply()?;
        if reply.status == GrabStatus::SUCCESS {
            if attempt > 0 {
                log::info!("Keyboard grab succeeded on attempt {}", attempt + 1);
            }
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err("Failed to grab keyboard after 50 attempts".into())
}

fn grab_pointer(
    conn: &RustConnection,
    window: u32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for attempt in 0..50 {
        let reply = conn
            .grab_pointer(
                false,
                window,
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                window,
                x11rb::NONE,
                x11rb::CURRENT_TIME,
            )?
            .reply()?;
        if reply.status == GrabStatus::SUCCESS {
            if attempt > 0 {
                log::info!("Pointer grab succeeded on attempt {}", attempt + 1);
            }
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err("Failed to grab pointer after 50 attempts".into())
}

fn load_keyboard_map(
    conn: &RustConnection,
) -> Result<KeyboardMap, Box<dyn std::error::Error + Send + Sync>> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let max = setup.max_keycode;
    let reply = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
    Ok(KeyboardMap {
        min_keycode: min,
        keysyms_per_keycode: reply.keysyms_per_keycode as usize,
        keysyms: reply.keysyms,
    })
}

/// Convert an X11 keycode + modifier state into a KeyAction.
fn resolve_key(mapping: &KeyboardMap, keycode: u8, state: u16) -> Option<KeyAction> {
    let offset = (keycode - mapping.min_keycode) as usize * mapping.keysyms_per_keycode;

    let shifted = state & u16::from(KeyButMask::SHIFT) != 0;
    let caps_lock = state & u16::from(KeyButMask::LOCK) != 0;

    let base_sym = mapping.keysyms.get(offset).copied().unwrap_or(0);
    let shift_sym = if mapping.keysyms_per_keycode > 1 {
        mapping.keysyms.get(offset + 1).copied().unwrap_or(0)
    } else {
        0
    };

    // Determine effective keysym:
    // - Shift → use shifted keysym
    // - Caps Lock → toggle case for letters only
    // - Shift + Caps Lock → cancel each other for letters
    let keysym = if shifted && caps_lock && is_letter(base_sym) {
        base_sym // shift + caps cancel out
    } else if shifted {
        if shift_sym != 0 { shift_sym } else { base_sym }
    } else if caps_lock && is_letter(base_sym) {
        if shift_sym != 0 { shift_sym } else { base_sym }
    } else {
        base_sym
    };

    keysym_to_action(keysym)
}

fn is_letter(keysym: u32) -> bool {
    matches!(keysym, 0x0061..=0x007a | 0x0041..=0x005a)
}

fn keysym_to_action(keysym: u32) -> Option<KeyAction> {
    match keysym {
        0xff08 => Some(KeyAction::Backspace), // XK_BackSpace
        0xff0d => Some(KeyAction::Return),    // XK_Return
        0xff8d => Some(KeyAction::Return),    // XK_KP_Enter
        0xff1b => Some(KeyAction::Escape),    // XK_Escape
        // Basic Latin (ASCII printable characters)
        0x0020..=0x007e => Some(KeyAction::Character(keysym as u8 as char)),
        // Latin-1 Supplement
        0x00a0..=0x00ff => char::from_u32(keysym).map(KeyAction::Character),
        _ => None,
    }
}

/// Create a black override-redirect window on each connected output.
fn create_cover_windows(
    conn: &RustConnection,
    screen_num: usize,
) -> Result<Vec<u32>, Box<dyn std::error::Error + Send + Sync>> {
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let black = screen.black_pixel;

    let mut windows = Vec::new();

    // Use RandR to enumerate per-monitor geometries
    if let Ok(reply) = randr::get_screen_resources(conn, root)?.reply() {
        for &crtc_id in &reply.crtcs {
            if let Ok(crtc) = randr::get_crtc_info(conn, crtc_id, 0)?.reply() {
                if crtc.width == 0 || crtc.height == 0 {
                    continue; // disabled output
                }
                let wid = conn.generate_id()?;
                conn.create_window(
                    0, // CopyFromParent
                    wid,
                    root,
                    crtc.x,
                    crtc.y,
                    crtc.width,
                    crtc.height,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0, // CopyFromParent
                    &CreateWindowAux::new()
                        .override_redirect(1)
                        .background_pixel(black),
                )?;
                conn.map_window(wid)?;
                windows.push(wid);
                log::info!(
                    "Cover window: {}x{}+{}+{}",
                    crtc.width, crtc.height, crtc.x, crtc.y
                );
            }
        }
    }

    // Fallback: single window covering the entire root
    if windows.is_empty() {
        let wid = conn.generate_id()?;
        conn.create_window(
            0,
            wid,
            root,
            0,
            0,
            screen.width_in_pixels,
            screen.height_in_pixels,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .override_redirect(1)
                .background_pixel(black),
        )?;
        conn.map_window(wid)?;
        windows.push(wid);
    }

    conn.flush()?;
    Ok(windows)
}

fn destroy_covers(conn: &RustConnection, covers: &[u32]) {
    for &w in covers {
        let _ = conn.destroy_window(w);
    }
    let _ = conn.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a fake keyboard mapping for tests.
    fn make_mapping(keysyms_per_keycode: usize, entries: &[(u8, &[u32])]) -> KeyboardMap {
        let min_keycode = entries.iter().map(|(k, _)| *k).min().unwrap_or(8);
        let max_keycode = entries.iter().map(|(k, _)| *k).max().unwrap_or(8);
        let total_slots = (max_keycode - min_keycode + 1) as usize * keysyms_per_keycode;
        let mut keysyms = vec![0u32; total_slots];
        for (keycode, syms) in entries {
            let offset = (*keycode - min_keycode) as usize * keysyms_per_keycode;
            for (i, &sym) in syms.iter().enumerate() {
                if i < keysyms_per_keycode {
                    keysyms[offset + i] = sym;
                }
            }
        }
        KeyboardMap {
            min_keycode,
            keysyms_per_keycode,
            keysyms,
        }
    }

    // --- is_letter ---

    #[test]
    fn is_letter_lowercase() {
        assert!(is_letter(0x0061)); // 'a'
        assert!(is_letter(0x007a)); // 'z'
    }

    #[test]
    fn is_letter_uppercase() {
        assert!(is_letter(0x0041)); // 'A'
        assert!(is_letter(0x005a)); // 'Z'
    }

    #[test]
    fn is_letter_non_letter() {
        assert!(!is_letter(0x0031)); // '1'
        assert!(!is_letter(0xff08)); // BackSpace
        assert!(!is_letter(0x0020)); // space
    }

    // --- keysym_to_action ---

    #[test]
    fn keysym_backspace() {
        assert!(matches!(keysym_to_action(0xff08), Some(KeyAction::Backspace)));
    }

    #[test]
    fn keysym_return() {
        assert!(matches!(keysym_to_action(0xff0d), Some(KeyAction::Return)));
    }

    #[test]
    fn keysym_kp_enter() {
        assert!(matches!(keysym_to_action(0xff8d), Some(KeyAction::Return)));
    }

    #[test]
    fn keysym_escape() {
        assert!(matches!(keysym_to_action(0xff1b), Some(KeyAction::Escape)));
    }

    #[test]
    fn keysym_printable_ascii() {
        match keysym_to_action(0x0041) {
            Some(KeyAction::Character('A')) => {}
            other => panic!("Expected Character('A'), got {:?}", option_debug(&other)),
        }
        match keysym_to_action(0x0020) {
            Some(KeyAction::Character(' ')) => {}
            other => panic!("Expected Character(' '), got {:?}", option_debug(&other)),
        }
    }

    #[test]
    fn keysym_latin1_supplement() {
        // 0x00e9 = 'é'
        match keysym_to_action(0x00e9) {
            Some(KeyAction::Character('é')) => {}
            other => panic!("Expected Character('é'), got {:?}", option_debug(&other)),
        }
    }

    #[test]
    fn keysym_unknown_returns_none() {
        assert!(keysym_to_action(0xffff).is_none());
        assert!(keysym_to_action(0x0100).is_none());
    }

    // --- resolve_key ---

    #[test]
    fn resolve_key_unshifted_letter() {
        // keycode 38: base='a' (0x61), shift='A' (0x41)
        let map = make_mapping(2, &[(38, &[0x61, 0x41])]);
        match resolve_key(&map, 38, 0) {
            Some(KeyAction::Character('a')) => {}
            other => panic!("Expected 'a', got {:?}", option_debug(&other)),
        }
    }

    #[test]
    fn resolve_key_shifted_letter() {
        let map = make_mapping(2, &[(38, &[0x61, 0x41])]);
        let shift: u16 = u16::from(KeyButMask::SHIFT);
        match resolve_key(&map, 38, shift) {
            Some(KeyAction::Character('A')) => {}
            other => panic!("Expected 'A', got {:?}", option_debug(&other)),
        }
    }

    #[test]
    fn resolve_key_caps_lock_letter() {
        let map = make_mapping(2, &[(38, &[0x61, 0x41])]);
        let caps: u16 = u16::from(KeyButMask::LOCK);
        match resolve_key(&map, 38, caps) {
            Some(KeyAction::Character('A')) => {}
            other => panic!("Expected 'A' (caps lock), got {:?}", option_debug(&other)),
        }
    }

    #[test]
    fn resolve_key_shift_plus_caps_cancels() {
        let map = make_mapping(2, &[(38, &[0x61, 0x41])]);
        let both: u16 = u16::from(KeyButMask::SHIFT) | u16::from(KeyButMask::LOCK);
        match resolve_key(&map, 38, both) {
            Some(KeyAction::Character('a')) => {}
            other => panic!("Expected 'a' (shift+caps cancel), got {:?}", option_debug(&other)),
        }
    }

    #[test]
    fn resolve_key_caps_on_non_letter_no_effect() {
        // keycode 10: base='1' (0x31), shift='!' (0x21)
        let map = make_mapping(2, &[(10, &[0x31, 0x21])]);
        let caps: u16 = u16::from(KeyButMask::LOCK);
        match resolve_key(&map, 10, caps) {
            Some(KeyAction::Character('1')) => {}
            other => panic!("Expected '1' (caps on non-letter), got {:?}", option_debug(&other)),
        }
    }

    #[test]
    fn resolve_key_special_keys() {
        // keycode 22: BackSpace
        let map = make_mapping(2, &[(22, &[0xff08, 0])]);
        assert!(matches!(resolve_key(&map, 22, 0), Some(KeyAction::Backspace)));
    }

    /// Debug helper since KeyAction doesn't derive Debug.
    fn option_debug(opt: &Option<KeyAction>) -> String {
        match opt {
            None => "None".to_string(),
            Some(KeyAction::Character(c)) => format!("Character('{}')", c),
            Some(KeyAction::Backspace) => "Backspace".to_string(),
            Some(KeyAction::Return) => "Return".to_string(),
            Some(KeyAction::Escape) => "Escape".to_string(),
        }
    }
}
