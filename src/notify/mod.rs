/// Desktop notification daemon module — org.freedesktop.Notifications implementation.

pub mod types;
pub mod popup;
pub mod history;
pub mod panel;
pub mod render;
mod daemon;

pub use daemon::start_notification_daemon;
