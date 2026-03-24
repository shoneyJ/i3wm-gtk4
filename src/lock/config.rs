//! Lock screen configuration loaded from ~/.config/i3more/lock.json.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Default)]
pub struct LockConfig {
    #[serde(default = "default_clock_format")]
    pub clock_format: String,
    pub avatar_path: Option<String>,
}

fn default_clock_format() -> String {
    "%H:%M".to_string()
}

pub fn load() -> LockConfig {
    let path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("i3more")
        .join("lock.json");
    load_from_str(
        &std::fs::read_to_string(&path).unwrap_or_default(),
    )
}

/// Parse a config from a JSON string, falling back to defaults on any error.
pub fn load_from_str(s: &str) -> LockConfig {
    serde_json::from_str(s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let cfg = LockConfig::default();
        assert_eq!(cfg.clock_format, "%H:%M");
        assert!(cfg.avatar_path.is_none());
    }

    #[test]
    fn parse_valid_full_config() {
        let json = r#"{"clock_format": "%I:%M %p", "avatar_path": "/home/user/avatar.png"}"#;
        let cfg = load_from_str(json);
        assert_eq!(cfg.clock_format, "%I:%M %p");
        assert_eq!(cfg.avatar_path.as_deref(), Some("/home/user/avatar.png"));
    }

    #[test]
    fn parse_partial_config_uses_defaults() {
        let json = r#"{"avatar_path": "/tmp/pic.jpg"}"#;
        let cfg = load_from_str(json);
        assert_eq!(cfg.clock_format, "%H:%M");
        assert_eq!(cfg.avatar_path.as_deref(), Some("/tmp/pic.jpg"));
    }

    #[test]
    fn parse_empty_object() {
        let cfg = load_from_str("{}");
        assert_eq!(cfg.clock_format, "%H:%M");
        assert!(cfg.avatar_path.is_none());
    }

    #[test]
    fn parse_invalid_json_falls_back_to_default() {
        let cfg = load_from_str("not json at all");
        assert_eq!(cfg.clock_format, "%H:%M");
        assert!(cfg.avatar_path.is_none());
    }

    #[test]
    fn parse_empty_string_falls_back_to_default() {
        let cfg = load_from_str("");
        assert_eq!(cfg.clock_format, "%H:%M");
        assert!(cfg.avatar_path.is_none());
    }
}
