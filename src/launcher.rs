//! App launcher: .desktop file parsing, search/filtering, and app launching.

use crate::icon::{self, IconResult};
use std::fs;
use std::path::PathBuf;

/// A launchable application entry parsed from a .desktop file.
pub struct LauncherEntry {
    pub name: String,
    pub generic_name: String,
    pub exec: String,
    pub icon: IconResult,
    pub terminal: bool,
    /// Lowercased concatenation of name + generic_name + keywords + categories for search.
    search_haystack: String,
}

/// Raw fields extracted from a .desktop file before icon resolution.
struct RawEntry {
    name: String,
    generic_name: String,
    exec: String,
    icon_name: String,
    keywords: String,
    categories: String,
    terminal: bool,
}

/// Load all launchable .desktop entries, sorted by name.
pub fn load_entries() -> Vec<LauncherEntry> {
    let files = icon::find_desktop_files();
    let mut entries: Vec<LauncherEntry> = files
        .iter()
        .filter_map(|path| {
            let content = fs::read_to_string(path).ok()?;
            let raw = parse_launcher_entry(&content)?;

            let icon = resolve_icon(&raw.icon_name);

            let haystack = format!(
                "{} {} {} {}",
                raw.name, raw.generic_name, raw.keywords, raw.categories
            )
            .to_lowercase();

            Some(LauncherEntry {
                name: raw.name,
                generic_name: raw.generic_name,
                exec: raw.exec,
                icon,
                terminal: raw.terminal,
                search_haystack: haystack,
            })
        })
        .collect();

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries.dedup_by(|a, b| a.name == b.name && a.exec == b.exec);
    entries
}

/// Filter entries by query, returning references ranked by relevance. Capped at 50.
pub fn filter_entries<'a>(entries: &'a [LauncherEntry], query: &str) -> Vec<&'a LauncherEntry> {
    if query.is_empty() {
        return entries.iter().take(50).collect();
    }

    let q = query.to_lowercase();

    let mut scored: Vec<(usize, &LauncherEntry)> = entries
        .iter()
        .filter(|e| e.search_haystack.contains(&q))
        .map(|e| {
            let name_lower = e.name.to_lowercase();
            let score = if name_lower.starts_with(&q) {
                0 // best: name starts with query
            } else if name_lower.contains(&q) {
                1 // good: name contains query
            } else {
                2 // ok: matched in keywords/categories
            };
            (score, e)
        })
        .collect();

    scored.sort_by_key(|(score, e)| (*score, e.name.to_lowercase()));
    scored.into_iter().map(|(_, e)| e).take(50).collect()
}

/// Launch an application from its entry.
pub fn launch(entry: &LauncherEntry) {
    let cmd = strip_field_codes(&entry.exec);
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return;
    }

    if entry.terminal {
        let terminal = std::env::var("TERMINAL")
            .unwrap_or_else(|_| "x-terminal-emulator".to_string());
        let full_cmd = format!("{} -e {}", terminal, cmd);
        log::info!("Launching (terminal): {}", full_cmd);
        let _ = std::process::Command::new("sh")
            .args(["-c", &full_cmd])
            .spawn();
    } else {
        log::info!("Launching: {}", cmd);
        let _ = std::process::Command::new("sh")
            .args(["-c", cmd])
            .spawn();
    }
}

/// Strip freedesktop Exec field codes (%U, %F, %u, %f, %i, %c, %k, %%).
fn strip_field_codes(exec: &str) -> String {
    let mut result = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.peek() {
                Some('U' | 'F' | 'u' | 'f' | 'i' | 'c' | 'k' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => {
                    chars.next(); // skip the code
                }
                Some('%') => {
                    chars.next();
                    result.push('%');
                }
                _ => result.push(ch),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Parse a .desktop file, returning None for non-launchable entries.
fn parse_launcher_entry(content: &str) -> Option<RawEntry> {
    let mut name = String::new();
    let mut generic_name = String::new();
    let mut exec = String::new();
    let mut icon_name = String::new();
    let mut keywords = String::new();
    let mut categories = String::new();
    let mut terminal = false;
    let mut no_display = false;
    let mut entry_type = String::new();
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        }
        if trimmed.starts_with('[') && trimmed != "[Desktop Entry]" {
            if in_desktop_entry {
                break;
            }
            continue;
        }

        if in_desktop_entry {
            if let Some(val) = trimmed.strip_prefix("Name=") {
                if name.is_empty() {
                    name = val.trim().to_string();
                }
            } else if let Some(val) = trimmed.strip_prefix("GenericName=") {
                generic_name = val.trim().to_string();
            } else if let Some(val) = trimmed.strip_prefix("Exec=") {
                exec = val.trim().to_string();
            } else if let Some(val) = trimmed.strip_prefix("Icon=") {
                icon_name = val.trim().to_string();
            } else if let Some(val) = trimmed.strip_prefix("Keywords=") {
                keywords = val.trim().to_string();
            } else if let Some(val) = trimmed.strip_prefix("Categories=") {
                categories = val.trim().to_string();
            } else if let Some(val) = trimmed.strip_prefix("Terminal=") {
                terminal = val.trim().eq_ignore_ascii_case("true");
            } else if let Some(val) = trimmed.strip_prefix("NoDisplay=") {
                no_display = val.trim().eq_ignore_ascii_case("true");
            } else if let Some(val) = trimmed.strip_prefix("Type=") {
                entry_type = val.trim().to_string();
            }
        }
    }

    // Skip non-application or hidden entries
    if no_display || (!entry_type.is_empty() && entry_type != "Application") {
        return None;
    }
    if name.is_empty() || exec.is_empty() {
        return None;
    }

    Some(RawEntry {
        name,
        generic_name,
        exec,
        icon_name,
        keywords,
        categories,
        terminal,
    })
}

/// Resolve an icon name to an IconResult.
fn resolve_icon(icon_name: &str) -> IconResult {
    if icon_name.is_empty() {
        return IconResult::NotFound;
    }
    if icon_name.starts_with('/') {
        let path = PathBuf::from(icon_name);
        if path.exists() {
            return IconResult::FilePath(path);
        }
        return IconResult::NotFound;
    }
    if let Some(path) = icon::resolve_icon_in_theme(icon_name) {
        return IconResult::FilePath(path);
    }
    IconResult::IconName(icon_name.to_string())
}
