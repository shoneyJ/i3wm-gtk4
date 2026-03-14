use std::cmp::min;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

const NOTIFICATION_TIMEOUT: u32 = 1000;
const VOLUME_STEP: u32 = 1;
const MAX_VOLUME: u32 = 100;

#[derive(Debug)]
enum Error {
    CommandFailed(String),
    ParseError(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::CommandFailed(msg) => write!(f, "command failed: {}", msg),
            Error::ParseError(msg) => write!(f, "parse error: {}", msg),
        }
    }
}

struct Sink {
    id: u32,
    name: String,
}

#[derive(Deserialize, Default)]
struct AudioConfig {
    #[serde(default)]
    preferred_sinks: Vec<String>,
    #[serde(default)]
    excluded_sinks: Vec<String>,
}

/// Run a command, capture stdout. Returns trimmed stdout or error with stderr.
fn run_cmd(cmd: &str, args: &[&str]) -> Result<String, Error> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| Error::CommandFailed(format!("{}: {}", cmd, e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::CommandFailed(format!(
            "{} exited with {}: {}",
            cmd,
            output.status,
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Fire-and-forget notification. Spawns notify-send and returns immediately.
fn notify(summary: &str, body: &str, icon: &str, sync_key: &str) -> Result<(), Error> {
    Command::new("notify-send")
        .args([
            summary,
            body,
            "-i",
            icon,
            "-t",
            &NOTIFICATION_TIMEOUT.to_string(),
            "-h",
            &format!("string:x-canonical-private-synchronous:{}", sync_key),
        ])
        .spawn()
        .map_err(|e| Error::CommandFailed(format!("notify-send: {}", e)))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Volume helpers
// ---------------------------------------------------------------------------

/// Read the current default sink volume percentage via pactl.
fn get_volume() -> Result<u32, Error> {
    let text = run_cmd("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])?;
    for part in text.split_whitespace() {
        if let Some(pct_str) = part.strip_suffix('%') {
            if let Ok(val) = pct_str.parse::<u32>() {
                return Ok(val);
            }
        }
    }
    Err(Error::ParseError("no percentage found in pactl output".into()))
}

/// Check if the default sink is muted.
fn is_muted() -> Result<bool, Error> {
    let text = run_cmd("pactl", &["get-sink-mute", "@DEFAULT_SINK@"])?;
    Ok(text.contains("yes"))
}

/// Return the appropriate icon name for the given volume/mute state.
fn volume_icon(volume: u32, muted: bool) -> &'static str {
    if muted {
        "audio-volume-muted"
    } else if volume < 34 {
        "audio-volume-low"
    } else if volume < 67 {
        "audio-volume-medium"
    } else {
        "audio-volume-high"
    }
}

fn volume_up() -> Result<(), Error> {
    let current = get_volume()?;
    let target = min(current + VOLUME_STEP, MAX_VOLUME);
    run_cmd(
        "pactl",
        &["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", target)],
    )?;
    let vol = get_volume()?;
    let muted = is_muted()?;
    notify("Volume", &format!("{}%", vol), volume_icon(vol, muted), "volume")
}

fn volume_down() -> Result<(), Error> {
    run_cmd(
        "pactl",
        &[
            "set-sink-volume",
            "@DEFAULT_SINK@",
            &format!("-{}%", VOLUME_STEP),
        ],
    )?;
    let vol = get_volume()?;
    let muted = is_muted()?;
    notify("Volume", &format!("{}%", vol), volume_icon(vol, muted), "volume")
}

fn volume_mute() -> Result<(), Error> {
    run_cmd("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "toggle"])?;
    let muted = is_muted()?;
    let vol = get_volume()?;
    let icon = volume_icon(vol, muted);
    if muted {
        notify("Volume", "Muted", icon, "volume")
    } else {
        notify("Volume", &format!("{}%", vol), icon, "volume")
    }
}

// ---------------------------------------------------------------------------
// Audio switch helpers
// ---------------------------------------------------------------------------

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("i3more")
        .join("audio.json")
}

/// Load audio config from ~/.config/i3more/audio.json.
/// Returns default (empty lists) on missing file or parse error.
fn load_config() -> AudioConfig {
    let path = config_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return AudioConfig::default(),
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Simple glob matching: `*` matches any sequence of characters.
/// Splits pattern on `*` and checks that all literal segments appear in order.
fn matches_glob(name: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match name[pos..].find(part) {
            Some(found) => {
                // First segment must match at start if pattern doesn't start with *
                if i == 0 && !pattern.starts_with('*') && found != 0 {
                    return false;
                }
                pos += found + part.len();
            }
            None => return false,
        }
    }

    // Last segment must match at end if pattern doesn't end with *
    if !pattern.ends_with('*') {
        if let Some(last) = parts.last() {
            if !last.is_empty() {
                return name.ends_with(last);
            }
        }
    }

    true
}

/// Parse `pactl list short sinks` into Sink structs.
fn list_sinks() -> Result<Vec<Sink>, Error> {
    let text = run_cmd("pactl", &["list", "short", "sinks"])?;
    let mut sinks = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 2 {
            if let Ok(id) = cols[0].parse::<u32>() {
                sinks.push(Sink {
                    id,
                    name: cols[1].to_string(),
                });
            }
        }
    }
    Ok(sinks)
}

/// Get the current default sink name.
fn get_default_sink() -> Result<String, Error> {
    run_cmd("pactl", &["get-default-sink"])
}

/// Filter sinks according to config preferences.
/// If preferred_sinks is non-empty: keep only matching, in preferred order.
/// If excluded_sinks is non-empty: remove matching.
/// Otherwise: return all.
fn filter_sinks(sinks: Vec<Sink>, config: &AudioConfig) -> Vec<Sink> {
    if !config.preferred_sinks.is_empty() {
        // Keep sinks matching preferred patterns, ordered by pattern priority
        let mut result = Vec::new();
        for pattern in &config.preferred_sinks {
            for sink in &sinks {
                if matches_glob(&sink.name, pattern)
                    && !result.iter().any(|s: &Sink| s.name == sink.name)
                {
                    result.push(Sink {
                        id: sink.id,
                        name: sink.name.clone(),
                    });
                }
            }
        }
        result
    } else if !config.excluded_sinks.is_empty() {
        sinks
            .into_iter()
            .filter(|s| !config.excluded_sinks.iter().any(|p| matches_glob(&s.name, p)))
            .collect()
    } else {
        sinks
    }
}

/// Get the human-readable description for a sink by name.
/// Tries `pactl --format=json list sinks` first, falls back to raw name.
fn get_sink_description(sink_name: &str) -> String {
    if let Ok(json_text) = run_cmd("pactl", &["--format=json", "list", "sinks"]) {
        if let Ok(sinks) = serde_json::from_str::<Vec<serde_json::Value>>(&json_text) {
            for sink in &sinks {
                if sink.get("name").and_then(|v| v.as_str()) == Some(sink_name) {
                    if let Some(desc) = sink.get("description").and_then(|v| v.as_str()) {
                        return desc.to_string();
                    }
                }
            }
        }
    }
    sink_name.to_string()
}

/// Migrate all active sink-inputs to the given sink.
fn move_sink_inputs(sink_id: u32) -> Result<(), Error> {
    let text = run_cmd("pactl", &["list", "short", "sink-inputs"])?;
    let sink_id_str = sink_id.to_string();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if let Some(input_id) = cols.first() {
            // Best-effort: ignore errors for individual moves
            let _ = run_cmd("pactl", &["move-sink-input", input_id, &sink_id_str]);
        }
    }
    Ok(())
}

fn audio_switch() -> Result<(), Error> {
    let config = load_config();
    let all_sinks = list_sinks()?;
    let filtered = filter_sinks(all_sinks, &config);

    if filtered.is_empty() {
        return Err(Error::ParseError("no audio output devices found".into()));
    }

    let current = get_default_sink()?;
    let current_idx = filtered.iter().position(|s| s.name == current);

    // Cycle to next sink (wrap around). If current not in list, pick first.
    let next_idx = match current_idx {
        Some(i) => (i + 1) % filtered.len(),
        None => 0,
    };
    let next = &filtered[next_idx];

    run_cmd("pactl", &["set-default-sink", &next.name])?;
    move_sink_inputs(next.id)?;

    let description = get_sink_description(&next.name);
    notify(
        "Audio Output",
        &description,
        "audio-card",
        "audio-switch",
    )
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn print_usage() {
    eprintln!("Usage: i3more-audio <command>");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  volume-up      Increase volume");
    eprintln!("  volume-down    Decrease volume");
    eprintln!("  volume-mute    Toggle mute");
    eprintln!("  audio-switch   Cycle audio output device");
}

fn main() {
    i3more::init_logging("i3more-audio");

    let result: Result<(), Error> = match std::env::args().nth(1).as_deref() {
        Some("volume-up") => volume_up(),
        Some("volume-down") => volume_down(),
        Some("volume-mute") => volume_mute(),
        Some("audio-switch") => audio_switch(),
        _ => {
            print_usage();
            std::process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_glob_exact() {
        assert!(matches_glob("foo", "foo"));
        assert!(!matches_glob("foo", "bar"));
    }

    #[test]
    fn test_matches_glob_star_suffix() {
        assert!(matches_glob("alsa_output.usb-Logitech_Zone_Wired-00.analog-stereo", "alsa_output.usb-Logitech_Zone_Wired-*"));
        assert!(!matches_glob("alsa_output.pci-something", "alsa_output.usb-Logitech_Zone_Wired-*"));
    }

    #[test]
    fn test_matches_glob_star_prefix() {
        assert!(matches_glob("alsa_output.pci-0000:00:1f.3-platform-skl_hda_dsp_generic.analog-stereo", "*analog-stereo"));
        assert!(!matches_glob("alsa_output.pci-0000:00:1f.3-platform-skl_hda_dsp_generic.hdmi-stereo", "*analog-stereo"));
    }

    #[test]
    fn test_matches_glob_star_middle() {
        assert!(matches_glob("alsa_output.pci-0000:00:1f.3.hdmi-stereo-extra1", "alsa_output.pci-*hdmi-stereo*"));
        assert!(!matches_glob("alsa_output.usb-Logitech.analog-stereo", "alsa_output.pci-*hdmi-stereo*"));
    }

    #[test]
    fn test_matches_glob_multiple_stars() {
        assert!(matches_glob("alsa_output.pci-0000:00:1f.3-platform-skl_hda_dsp_generic.analog-stereo", "alsa_output.pci-*analog-stereo"));
    }

    #[test]
    fn test_filter_sinks_preferred() {
        let sinks = vec![
            Sink { id: 1, name: "alsa_output.pci-hdmi-stereo".into() },
            Sink { id: 2, name: "alsa_output.usb-Logitech-00.analog".into() },
            Sink { id: 3, name: "alsa_output.pci-analog-stereo".into() },
        ];
        let config = AudioConfig {
            preferred_sinks: vec!["alsa_output.usb-Logitech*".into(), "alsa_output.pci-*analog*".into()],
            excluded_sinks: vec![],
        };
        let filtered = filter_sinks(sinks, &config);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name, "alsa_output.usb-Logitech-00.analog");
        assert_eq!(filtered[1].name, "alsa_output.pci-analog-stereo");
    }

    #[test]
    fn test_filter_sinks_excluded() {
        let sinks = vec![
            Sink { id: 1, name: "alsa_output.pci-hdmi-stereo".into() },
            Sink { id: 2, name: "alsa_output.usb-Logitech-00.analog".into() },
            Sink { id: 3, name: "alsa_output.pci-analog-stereo".into() },
        ];
        let config = AudioConfig {
            preferred_sinks: vec![],
            excluded_sinks: vec!["*hdmi*".into()],
        };
        let filtered = filter_sinks(sinks, &config);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|s| !s.name.contains("hdmi")));
    }

    #[test]
    fn test_filter_sinks_no_config() {
        let sinks = vec![
            Sink { id: 1, name: "a".into() },
            Sink { id: 2, name: "b".into() },
        ];
        let config = AudioConfig::default();
        let filtered = filter_sinks(sinks, &config);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_volume_icon() {
        assert_eq!(volume_icon(50, true), "audio-volume-muted");
        assert_eq!(volume_icon(0, false), "audio-volume-low");
        assert_eq!(volume_icon(33, false), "audio-volume-low");
        assert_eq!(volume_icon(34, false), "audio-volume-medium");
        assert_eq!(volume_icon(66, false), "audio-volume-medium");
        assert_eq!(volume_icon(67, false), "audio-volume-high");
        assert_eq!(volume_icon(100, false), "audio-volume-high");
    }
}
