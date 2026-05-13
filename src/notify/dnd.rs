//! Do Not Disturb state.
//!
//! When DND is enabled, incoming notifications are still pushed to history
//! (and the unread badge updates) but no popup is shown.
//!
//! The state is persisted to `~/.local/share/i3more/dnd.json` so a restart
//! preserves the user's choice.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use serde::{Deserialize, Serialize};

use crate::fa;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
struct PersistedDnd {
    enabled: bool,
}

fn data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("i3more")
}

fn file_path() -> std::path::PathBuf {
    data_dir().join("dnd.json")
}

fn load() -> bool {
    match std::fs::read_to_string(file_path()) {
        Ok(json) => serde_json::from_str::<PersistedDnd>(&json)
            .map(|p| p.enabled)
            .unwrap_or(false),
        Err(_) => false,
    }
}

fn save(enabled: bool) {
    let dir = data_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("Failed to create data dir {:?}: {}", dir, e);
        return;
    }
    let persisted = PersistedDnd { enabled };
    match serde_json::to_string(&persisted) {
        Ok(json) => {
            if let Err(e) = std::fs::write(file_path(), json) {
                log::warn!("Failed to save DND state: {}", e);
            }
        }
        Err(e) => log::warn!("Failed to serialize DND state: {}", e),
    }
}

/// Shared DND flag readable from anywhere on the main thread.
/// `popup.rs` consults this to decide whether to show a popup.
pub type DndFlag = Rc<Cell<bool>>;

/// Controller that owns the DND flag and keeps UI in sync when it toggles.
pub struct DndController {
    flag: DndFlag,
    bell_label: gtk4::Label,
    panel_toggle: std::cell::RefCell<Option<gtk4::Button>>,
}

impl DndController {
    pub fn new(bell_label: gtk4::Label) -> Rc<Self> {
        let initial = load();
        let ctrl = Rc::new(Self {
            flag: Rc::new(Cell::new(initial)),
            bell_label,
            panel_toggle: std::cell::RefCell::new(None),
        });
        ctrl.update_ui();
        if initial {
            log::info!("DND restored from disk: enabled");
        }
        ctrl
    }

    /// Returns the shared flag for handing to PopupManager.
    pub fn flag(&self) -> DndFlag {
        self.flag.clone()
    }

    pub fn is_enabled(&self) -> bool {
        self.flag.get()
    }

    pub fn toggle(&self) {
        self.set_enabled(!self.is_enabled());
    }

    pub fn set_enabled(&self, enabled: bool) {
        if self.flag.get() == enabled {
            return;
        }
        self.flag.set(enabled);
        save(enabled);
        self.update_ui();
        log::info!("DND {}", if enabled { "enabled" } else { "disabled" });
    }

    /// Register the panel's DND toggle button so it stays in sync when the
    /// state is toggled elsewhere (e.g. right-click on the bell).
    pub fn register_panel_toggle(&self, btn: gtk4::Button) {
        *self.panel_toggle.borrow_mut() = Some(btn);
        self.update_ui();
    }

    fn update_ui(&self) {
        let enabled = self.flag.get();
        let glyph = if enabled { fa::BELL_SLASH } else { fa::BELL };
        self.bell_label
            .set_markup(&fa::fa_icon(glyph, "#a89984", 11));

        if let Some(btn) = self.panel_toggle.borrow().as_ref() {
            let glyph = if enabled { fa::BELL_SLASH } else { fa::BELL };
            btn.set_label("");
            // Build markup label inside the button
            let child = gtk4::Label::new(None);
            child.set_use_markup(true);
            let color = if enabled { "#fb4934" } else { "#a89984" };
            child.set_markup(&format!(
                "{} {}",
                fa::fa_icon(glyph, color, 11),
                if enabled { "DND on" } else { "DND off" }
            ));
            btn.set_child(Some(&child));
            if enabled {
                btn.add_css_class("dnd-active");
            } else {
                btn.remove_css_class("dnd-active");
            }
        }
    }
}
