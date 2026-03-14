use gtk4::gdk;
use std::path::PathBuf;

fn css_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("i3more")
        .join("css")
}

/// Load CSS: try runtime file first, fall back to embedded default.
pub fn load_css(filename: &str, embedded_fallback: &str) {
    let provider = gtk4::CssProvider::new();
    let path = css_dir().join(filename);
    let css = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| embedded_fallback.to_string());
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &gdk::Display::default().expect("Could not get default display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
