/// System tray module — StatusNotifierItem/Watcher protocol implementation.

pub mod dbusmenu;
pub mod item;
pub mod render;
pub mod types;
mod watcher;

pub use watcher::start_watcher;
