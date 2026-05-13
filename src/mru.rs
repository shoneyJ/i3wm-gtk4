//! Window MRU (Most Recently Used) tracking.
//!
//! `i3more` (the main binary) subscribes to i3 `window::focus` and
//! `window::close` events and keeps a `VecDeque` of recently-focused windows
//! by their i3 container id. On every change the list is written atomically
//! to `$XDG_RUNTIME_DIR/i3more/mru.json` so the `i3more-back` and (future)
//! `i3more-alt-tab` CLIs can read it and ask i3 to focus a target via
//! `[con_id=N] focus`.
//!
//! Invariant: `entries[0]` is the currently-focused window, `entries[1]` is
//! the previously-focused one. So "back" is just "focus entries[1]". Pressing
//! back twice naturally returns to the original window because each focus
//! moves the chosen window to the front of the deque.
//!
//! Stale entries (window closed without us seeing it) are tolerated: i3
//! ignores `focus` commands for unknown con_ids, and the next legitimate
//! focus event evicts the dead one.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;

const MAX_ENTRIES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MruEntry {
    pub con_id: i64,
    pub class: String,
    pub title: String,
}

pub struct MruTracker {
    entries: VecDeque<MruEntry>,
}

impl MruTracker {
    /// Construct a tracker. If i3 is reachable, seed the deque with the
    /// currently-focused window so `entries[0]` is meaningful before the
    /// first focus event arrives.
    pub fn new() -> Self {
        let mut tracker = Self {
            entries: VecDeque::with_capacity(MAX_ENTRIES),
        };
        if let Some(entry) = query_focused_from_tree() {
            tracker.focus(entry);
        } else {
            tracker.save();
        }
        tracker
    }

    /// Record a focus event. Moves the window to the front of the MRU.
    pub fn focus(&mut self, entry: MruEntry) {
        self.entries.retain(|e| e.con_id != entry.con_id);
        self.entries.push_front(entry);
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_back();
        }
        self.save();
    }

    /// Record a window close. Removes from MRU.
    pub fn close(&mut self, con_id: i64) {
        let before = self.entries.len();
        self.entries.retain(|e| e.con_id != con_id);
        if self.entries.len() != before {
            self.save();
        }
    }

    fn save(&self) {
        let path = file_path();
        let dir = match path.parent() {
            Some(d) => d,
            None => {
                log::warn!("MRU: file path has no parent: {:?}", path);
                return;
            }
        };
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("MRU: failed to create dir {:?}: {}", dir, e);
            return;
        }
        let snapshot: Vec<&MruEntry> = self.entries.iter().collect();
        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("MRU: failed to serialize: {}", e);
                return;
            }
        };
        let tmp = path.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, json) {
            log::warn!("MRU: failed to write {:?}: {}", tmp, e);
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            log::warn!("MRU: failed to rename {:?} -> {:?}: {}", tmp, path, e);
        }
    }
}

impl Default for MruTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Load the MRU list as written by the main i3more binary.
/// Returns an empty vec if the file doesn't exist or is malformed.
pub fn load() -> Vec<MruEntry> {
    let path = file_path();
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn file_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime).join("i3more").join("mru.json")
}

/// Build an `MruEntry` from an i3 event `container` payload. Returns `None`
/// for non-window nodes (split/stacked containers, workspaces) so we don't
/// pollute the MRU with parent containers.
pub fn entry_from_container(container: &serde_json::Value) -> Option<MruEntry> {
    // Only track actual windows — leaf nodes with an X11 window id.
    container["window"].as_u64()?;
    let id = container["id"].as_i64()?;
    let class = container["window_properties"]["class"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let title = container["name"].as_str().unwrap_or("").to_string();
    Some(MruEntry {
        con_id: id,
        class,
        title,
    })
}

fn query_focused_from_tree() -> Option<MruEntry> {
    let mut conn = crate::ipc::I3Connection::connect().ok()?;
    let tree = conn.get_tree().ok()?;
    find_focused(&tree)
}

fn find_focused(node: &serde_json::Value) -> Option<MruEntry> {
    if node["focused"].as_bool() == Some(true) && node["window"].as_u64().is_some() {
        return entry_from_container(node);
    }
    for child_arr in &["nodes", "floating_nodes"] {
        if let Some(children) = node[child_arr].as_array() {
            for child in children {
                if let Some(found) = find_focused(child) {
                    return Some(found);
                }
            }
        }
    }
    None
}
