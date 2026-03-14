//! Translation backend using `trans` (translate-shell) CLI.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

/// Translate text from source language to target language using `trans`.
pub fn translate(text: &str, source: &str, target: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }

    let output = Command::new("trans")
        .args(["-brief", "-s", source, "-t", target, "--", text])
        .output()
        .map_err(|e| format!("Failed to run trans: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("trans exited with error: {}", stderr));
    }

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(result)
}

/// Speak text in the given language using `trans -speak`.
pub fn speak(text: &str, lang: &str) {
    if text.trim().is_empty() {
        return;
    }
    let text = text.to_string();
    let lang = lang.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("trans")
            .args(["-speak", "-t", &lang, "--", &text])
            .output();
    });
}

/// Query available languages from `trans -list-languages`.
/// Returns a sorted list of language codes. Falls back to a hardcoded list.
pub fn list_languages() -> Vec<String> {
    if let Ok(output) = Command::new("trans").arg("-list-languages").output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let mut langs: Vec<String> = text
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if !langs.is_empty() {
                langs.sort();
                return langs;
            }
        }
    }

    // Fallback list
    vec![
        "Afrikaans", "Arabic", "Chinese", "Czech", "Danish", "Dutch",
        "English", "Finnish", "French", "German", "Greek", "Hebrew",
        "Hindi", "Hungarian", "Indonesian", "Italian", "Japanese",
        "Korean", "Norwegian", "Polish", "Portuguese", "Romanian",
        "Russian", "Slovak", "Spanish", "Swedish", "Thai", "Turkish",
        "Ukrainian", "Vietnamese",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[derive(Serialize, Deserialize, Default)]
pub struct TranslateConfig {
    pub source_language: Option<String>,
    pub target_language: Option<String>,
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("i3more")
        .join("translate.json")
}

pub fn load_config() -> TranslateConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => TranslateConfig::default(),
    }
}

pub fn save_config(config: &TranslateConfig) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&path, data);
    }
}
