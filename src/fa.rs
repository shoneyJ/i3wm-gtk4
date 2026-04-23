//! Font Awesome icon constants and font registration.
//!
//! Bundles Font Awesome 6 Free Solid (OTF) and registers it with fontconfig
//! at startup so Pango can render the glyphs in GTK labels.

/// Font family name (must match the .otf metadata).
pub const FA_FONT: &str = "Font Awesome 6 Free Solid";

// Icon codepoints (Font Awesome 6 Free Solid)
pub const MICROCHIP: char = '\u{f2db}';
pub const TEMPERATURE: char = '\u{f2c9}';
pub const MEMORY: char = '\u{f538}';
pub const BATTERY_FULL: char = '\u{f240}';
pub const BATTERY_3Q: char = '\u{f241}';
pub const BATTERY_HALF: char = '\u{f242}';
pub const BATTERY_1Q: char = '\u{f243}';
pub const BATTERY_EMPTY: char = '\u{f244}';
pub const BOLT: char = '\u{f0e7}';
pub const GAUGE: char = '\u{f3fd}';
pub const COPY: char = '\u{f0c5}';
pub const VOLUME_UP: char = '\u{f028}';
pub const ERASER: char = '\u{f12d}';
pub const EXCHANGE: char = '\u{f362}';
pub const BELL: char = '\u{f0f3}';
pub const BELL_SLASH: char = '\u{f1f6}';
pub const PLAY: char = '\u{f04b}';
pub const PAUSE: char = '\u{f04c}';
pub const FORWARD_STEP: char = '\u{f051}';
pub const BACKWARD_STEP: char = '\u{f048}';
pub const MUSIC: char = '\u{f001}';
pub const VOLUME_OFF: char = '\u{f6a9}';
pub const VOLUME_LOW: char = '\u{f027}';
pub const VOLUME_HIGH: char = '\u{f028}';
pub const SUN: char = '\u{f185}';
pub const IMAGE: char = '\u{f03e}';
pub const SLIDERS: char = '\u{f1de}';
pub const MICROPHONE: char = '\u{f130}';
pub const MICROPHONE_SLASH: char = '\u{f131}';

/// Wrap an FA glyph in Pango markup with color and size.
pub fn fa_icon(icon: char, color: &str, size_pt: u32) -> String {
    // Pango `font` attribute accepts "Family Size", size in points.
    format!(
        "<span font=\"{FA_FONT} {size_pt}\" foreground=\"{color}\">{icon}</span>"
    )
}

/// Pick the appropriate battery icon based on capacity and charging state.
pub fn battery_glyph(capacity: u8, charging: bool) -> char {
    if charging {
        return BOLT;
    }
    match capacity {
        75..=100 => BATTERY_FULL,
        50..=74 => BATTERY_3Q,
        25..=49 => BATTERY_HALF,
        10..=24 => BATTERY_1Q,
        _ => BATTERY_EMPTY,
    }
}

/// Register the bundled Font Awesome font with fontconfig so Pango can find it.
///
/// Embeds the .otf via `include_bytes!`, writes it to a temp file, and calls
/// `FcConfigAppFontAddFile` via raw FFI. Fontconfig is already linked
/// transitively through pango → fontconfig.
pub fn register_font() {
    use std::io::Write;

    static FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/fa-solid-900.otf");

    // Write font to a temp file
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let font_path = format!("{}/i3more-fa-solid-900.otf", dir);

    if let Err(e) = std::fs::File::create(&font_path).and_then(|mut f| f.write_all(FONT_BYTES)) {
        log::warn!("Failed to write FA font to {}: {}", font_path, e);
        return;
    }

    let c_path = match std::ffi::CString::new(font_path.as_bytes()) {
        Ok(p) => p,
        Err(_) => return,
    };

    // fontconfig FFI — explicitly link libfontconfig
    #[link(name = "fontconfig")]
    extern "C" {
        fn FcConfigGetCurrent() -> *mut std::ffi::c_void;
        fn FcConfigAppFontAddFile(
            config: *mut std::ffi::c_void,
            file: *const std::ffi::c_char,
        ) -> i32;
    }

    unsafe {
        let config = FcConfigGetCurrent();
        if config.is_null() {
            log::warn!("FcConfigGetCurrent returned null");
            return;
        }
        let ok = FcConfigAppFontAddFile(config, c_path.as_ptr());
        if ok == 0 {
            log::warn!("FcConfigAppFontAddFile failed for {}", font_path);
        } else {
            log::info!("Registered Font Awesome font from {}", font_path);
        }
    }
}
