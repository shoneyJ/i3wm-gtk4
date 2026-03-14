//! Icon resolution: WM_CLASS -> icon name or file path.
//!
//! Port of resolve-app-icon.py with an in-memory LRU cache layer.
//! Resolution pipeline:
//!   1. In-memory LRU cache
//!   2. Disk cache (~/.cache/i3more/app-icons/)
//!   3. .desktop file search (StartupWMClass, filename match)
//!   4. Icon name from .desktop -> resolve via icon theme dirs
//!   5. Cache result and return

use lru::LruCache;
use std::collections::HashMap;
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

const LRU_CAPACITY: usize = 256;
const ICON_SIZES: &[&str] = &["48x48", "64x64", "scalable", "256x256", "128x128", "96x96", "32x32", "24x24", "16x16"];

/// Cached result: either a resolved icon name/path or empty (no icon found).
#[derive(Debug, Clone)]
pub enum IconResult {
    /// An icon theme name (e.g., "firefox") — use with GTK Image::from_icon_name
    IconName(String),
    /// An absolute file path to an icon file
    FilePath(PathBuf),
    /// No icon found for this class
    NotFound,
}

pub struct IconResolver {
    mem_cache: LruCache<String, IconResult>,
    disk_cache_dir: PathBuf,
    desktop_files: Vec<PathBuf>,
    /// Pre-parsed .desktop file data: (path, startup_wm_class, icon_name)
    desktop_index: Vec<DesktopEntry>,
}

pub struct DesktopEntry {
    pub path: PathBuf,
    pub basename_lower: String,
    pub startup_wm_class_lower: String,
    pub icon: String,
}

impl IconResolver {
    pub fn new() -> Self {
        let disk_cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("i3more")
            .join("app-icons");

        let _ = fs::create_dir_all(&disk_cache_dir);

        let desktop_files = find_desktop_files();
        let desktop_index = build_desktop_index(&desktop_files);

        Self {
            mem_cache: LruCache::new(NonZeroUsize::new(LRU_CAPACITY).unwrap()),
            disk_cache_dir,
            desktop_files,
            desktop_index,
        }
    }

    /// Resolve a WM_CLASS to an icon. Uses caches for fast repeated lookups.
    pub fn resolve(&mut self, wm_class: &str) -> IconResult {
        let key = wm_class.to_lowercase();

        // 1. In-memory LRU cache
        if let Some(cached) = self.mem_cache.get(&key) {
            return cached.clone();
        }

        // 2. Disk cache
        if let Some(result) = self.read_disk_cache(&key) {
            self.mem_cache.put(key, result.clone());
            return result;
        }

        // 3. Full resolution
        let result = self.resolve_full(&key);

        // Write to disk cache
        self.write_disk_cache(&key, &result);

        // Write to memory cache
        self.mem_cache.put(key, result.clone());

        result
    }

    /// Batch-resolve multiple classes. Returns a map of class -> IconResult.
    pub fn resolve_batch(&mut self, classes: &[String]) -> HashMap<String, IconResult> {
        let mut results = HashMap::new();
        for class in classes {
            let result = self.resolve(class);
            results.insert(class.clone(), result);
        }
        results
    }

    /// Refresh the .desktop file index (call after installing new apps).
    pub fn refresh_desktop_index(&mut self) {
        self.desktop_files = find_desktop_files();
        self.desktop_index = build_desktop_index(&self.desktop_files);
    }

    fn resolve_full(&self, wm_class_lower: &str) -> IconResult {
        // Search .desktop files for a matching entry
        let icon_name = self.find_icon_name(wm_class_lower);

        match icon_name {
            Some(name) if name.is_empty() => IconResult::NotFound,
            Some(name) => {
                // If it's already an absolute path, use it directly
                if name.starts_with('/') {
                    let path = PathBuf::from(&name);
                    if path.exists() {
                        return IconResult::FilePath(path);
                    }
                    return IconResult::NotFound;
                }
                // Try to resolve via icon theme directories
                if let Some(path) = resolve_icon_in_theme(&name) {
                    return IconResult::FilePath(path);
                }
                // Return as icon name — GTK can try theme lookup at render time
                IconResult::IconName(name)
            }
            None => IconResult::NotFound,
        }
    }

    fn find_icon_name(&self, wm_class_lower: &str) -> Option<String> {
        // Pass 1: Match by StartupWMClass (case-insensitive)
        for entry in &self.desktop_index {
            if !entry.startup_wm_class_lower.is_empty()
                && entry.startup_wm_class_lower == wm_class_lower
            {
                return Some(entry.icon.clone());
            }
        }

        // Pass 2: Match by .desktop filename
        for entry in &self.desktop_index {
            // Handle snap naming: "code_code.desktop" -> split by "_"
            let parts: Vec<&str> = entry.basename_lower.split('_').collect();
            if parts.contains(&wm_class_lower) || entry.basename_lower == wm_class_lower {
                return Some(entry.icon.clone());
            }
        }

        // Pass 3: Loose substring match on filename
        for entry in &self.desktop_index {
            if entry.basename_lower.contains(wm_class_lower) {
                return Some(entry.icon.clone());
            }
        }

        None
    }

    fn disk_cache_path(&self, key: &str) -> PathBuf {
        // Use a simple hash for the filename (same as Python version uses md5)
        let hash = simple_hash(key);
        self.disk_cache_dir.join(hash)
    }

    fn read_disk_cache(&self, key: &str) -> Option<IconResult> {
        let path = self.disk_cache_path(key);
        let content = fs::read_to_string(&path).ok()?;
        let content = content.trim();

        if content.is_empty() {
            return Some(IconResult::NotFound);
        }

        // Check if the cached path/name still exists
        if content.starts_with('/') {
            let icon_path = PathBuf::from(content);
            if icon_path.exists() {
                return Some(IconResult::FilePath(icon_path));
            }
            // Cached file no longer exists — invalidate
            let _ = fs::remove_file(&path);
            return None;
        }

        Some(IconResult::IconName(content.to_string()))
    }

    fn write_disk_cache(&self, key: &str, result: &IconResult) {
        let path = self.disk_cache_path(key);
        let content = match result {
            IconResult::IconName(name) => name.as_str().to_string(),
            IconResult::FilePath(p) => p.to_string_lossy().to_string(),
            IconResult::NotFound => String::new(),
        };
        let _ = fs::write(path, content);
    }
}

/// Find all .desktop files from standard locations.
pub fn find_desktop_files() -> Vec<PathBuf> {
    let dirs = [
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("applications"),
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/var/lib/snapd/desktop/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];

    let mut files = Vec::new();
    for dir in &dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "desktop") {
                    files.push(path);
                }
            }
        }
    }
    files
}

/// Pre-parse .desktop files into an index for fast searching.
pub fn build_desktop_index(files: &[PathBuf]) -> Vec<DesktopEntry> {
    files
        .iter()
        .filter_map(|path| {
            let content = fs::read_to_string(path).ok()?;
            let (icon, startup_wm_class) = parse_desktop_entry(&content);

            let basename = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();

            Some(DesktopEntry {
                path: path.clone(),
                basename_lower: basename,
                startup_wm_class_lower: startup_wm_class.to_lowercase(),
                icon,
            })
        })
        .collect()
}

/// Extract Icon= and StartupWMClass= from a .desktop file content.
pub fn parse_desktop_entry(content: &str) -> (String, String) {
    let mut icon = String::new();
    let mut startup_wm_class = String::new();
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        }
        if trimmed.starts_with('[') && trimmed != "[Desktop Entry]" {
            if in_desktop_entry {
                break; // We've left the [Desktop Entry] section
            }
            continue;
        }

        if in_desktop_entry {
            if let Some(value) = trimmed.strip_prefix("Icon=") {
                icon = value.trim().to_string();
            } else if let Some(value) = trimmed.strip_prefix("StartupWMClass=") {
                startup_wm_class = value.trim().to_string();
            }
        }
    }

    (icon, startup_wm_class)
}

/// Search common icon theme directories for an icon by name.
pub fn resolve_icon_in_theme(icon_name: &str) -> Option<PathBuf> {
    let extensions = ["svg", "png", "xpm"];
    let theme_dirs = [
        "/usr/share/icons/hicolor",
        "/usr/share/icons/Adwaita",
    ];

    // Search theme directories with preferred sizes
    for theme_dir in &theme_dirs {
        for size in ICON_SIZES {
            for ext in &extensions {
                let path = PathBuf::from(theme_dir)
                    .join(size)
                    .join("apps")
                    .join(format!("{}.{}", icon_name, ext));
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }

    // Fallback: pixmaps
    for ext in &extensions {
        let path = PathBuf::from(format!("/usr/share/pixmaps/{}.{}", icon_name, ext));
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Simple string hash for disk cache filenames.
fn simple_hash(s: &str) -> String {
    // Use a basic FNV-1a style hash, output as hex
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}
