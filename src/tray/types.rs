/// Types for the system tray (StatusNotifierItem protocol).

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrayItemId {
    pub bus_name: String,
    pub object_path: String,
}

#[derive(Debug, Clone)]
pub struct TrayItemProps {
    pub id: TrayItemId,
    pub title: String,
    pub icon_name: String,
    pub icon_pixmap: Option<Vec<TrayPixmap>>,
    pub tooltip: String,
    pub status: String,
    pub menu: Option<String>,
    pub item_is_menu: bool,
}

impl TrayItemProps {
    pub fn new(id: TrayItemId) -> Self {
        Self {
            id,
            title: String::new(),
            icon_name: String::new(),
            icon_pixmap: None,
            tooltip: String::new(),
            status: "Active".to_string(),
            menu: None,
            item_is_menu: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrayPixmap {
    pub width: i32,
    pub height: i32,
    pub argb_data: Vec<u8>,
}

#[derive(Debug)]
pub enum TrayEvent {
    ItemRegistered(TrayItemId),
    ItemUnregistered(TrayItemId),
    ItemPropsLoaded(TrayItemProps),
    ItemUpdated(TrayItemId),
}
