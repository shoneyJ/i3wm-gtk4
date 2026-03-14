/// Notification history storage and persistence.

use super::types::Notification;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

const MAX_HISTORY: usize = 500;

/// A serializable snapshot of a notification for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub timestamp_secs: u64,
}

impl HistoryEntry {
    fn from_notification(notif: &Notification) -> Self {
        let timestamp_secs = notif
            .timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            id: notif.id,
            app_name: notif.app_name.clone(),
            app_icon: notif.app_icon.clone(),
            summary: notif.summary.clone(),
            body: notif.body.clone(),
            timestamp_secs,
        }
    }

    /// Format a relative time string like "2m ago", "1h ago", "3d ago".
    pub fn relative_time(&self) -> String {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let diff = now.saturating_sub(self.timestamp_secs);
        if diff < 60 {
            "just now".to_string()
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < 86400 {
            format!("{}h ago", diff / 3600)
        } else {
            format!("{}d ago", diff / 86400)
        }
    }
}

/// Manages notification history with an unread counter.
pub struct NotificationHistory {
    pub entries: Vec<HistoryEntry>,
    pub unread_count: u32,
}

impl NotificationHistory {
    pub fn new() -> Self {
        let mut hist = Self {
            entries: Vec::new(),
            unread_count: 0,
        };
        hist.load();
        hist
    }

    /// Push a new notification into history.
    pub fn push(&mut self, notif: &Notification) {
        // If replacing, remove the old entry
        self.entries.retain(|e| e.id != notif.id);

        self.entries.insert(0, HistoryEntry::from_notification(notif));
        self.unread_count += 1;

        // Cap history size
        if self.entries.len() > MAX_HISTORY {
            self.entries.truncate(MAX_HISTORY);
        }

        self.save();
    }

    /// Remove a single notification from history.
    pub fn remove(&mut self, id: u32) {
        self.entries.retain(|e| e.id != id);
        self.save();
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.unread_count = 0;
        self.save();
    }

    /// Mark all notifications as read (reset badge).
    pub fn mark_all_read(&mut self) {
        self.unread_count = 0;
    }

    fn data_path() -> std::path::PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("i3more")
    }

    fn file_path() -> std::path::PathBuf {
        Self::data_path().join("notifications.json")
    }

    fn save(&self) {
        let dir = Self::data_path();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("Failed to create data dir {:?}: {}", dir, e);
            return;
        }
        match serde_json::to_string(&self.entries) {
            Ok(json) => {
                if let Err(e) = std::fs::write(Self::file_path(), json) {
                    log::warn!("Failed to save notification history: {}", e);
                }
            }
            Err(e) => log::warn!("Failed to serialize notification history: {}", e),
        }
    }

    fn load(&mut self) {
        let path = Self::file_path();
        if let Ok(json) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<Vec<HistoryEntry>>(&json) {
                Ok(entries) => {
                    self.entries = entries;
                    log::info!("Loaded {} notification history entries", self.entries.len());
                }
                Err(e) => {
                    log::warn!("Failed to parse notification history: {}", e);
                }
            }
        }
    }
}
