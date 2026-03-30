/// Types for the desktop notification daemon.

use std::collections::HashMap;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub hints: HashMap<String, linbus::Value>,
    pub expire_timeout: i32,
    pub timestamp: SystemTime,
}

#[derive(Debug)]
pub enum NotifyEvent {
    New(Notification),
    Close(u32),
    ActionInvoked(u32, String), // (notification_id, action_key)
}
