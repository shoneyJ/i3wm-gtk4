/// Read StatusNotifierItem properties from a tray app over D-Bus.

use std::collections::HashMap;
use linbus::{Connection, Message, Value};

use super::types::{TrayItemId, TrayItemProps, TrayPixmap};

/// Read all relevant properties from an SNI item using Properties.GetAll.
pub fn read_item_props(
    conn: &mut Connection,
    bus_name: &str,
    object_path: &str,
) -> Result<TrayItemProps, linbus::LinbusError> {
    let msg = Message::method_call(
        bus_name,
        object_path,
        "org.freedesktop.DBus.Properties",
        "GetAll",
    ).with_body(vec![Value::String("org.kde.StatusNotifierItem".into())]);

    let reply = conn.call(&msg, 3000)?;

    let props_dict: HashMap<String, Value> = reply.body.first()
        .and_then(|v| v.to_string_dict())
        .unwrap_or_default();

    let title = prop_str(&props_dict, "Title");
    let icon_name = prop_str(&props_dict, "IconName");
    let status = prop_str(&props_dict, "Status");
    let menu = prop_path(&props_dict, "Menu");
    let item_is_menu = prop_bool(&props_dict, "ItemIsMenu");
    let tooltip = extract_tooltip(&props_dict);
    let icon_pixmap = extract_icon_pixmap(&props_dict);

    Ok(TrayItemProps {
        id: TrayItemId {
            bus_name: bus_name.to_string(),
            object_path: object_path.to_string(),
        },
        title,
        icon_name,
        icon_pixmap,
        tooltip,
        status,
        menu,
        item_is_menu,
    })
}

fn unwrap_variant(v: &Value) -> &Value {
    match v {
        Value::Variant(inner) => unwrap_variant(inner),
        other => other,
    }
}

fn prop_str(props: &HashMap<String, Value>, key: &str) -> String {
    props.get(key)
        .map(unwrap_variant)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn prop_path(props: &HashMap<String, Value>, key: &str) -> Option<String> {
    let s = props.get(key)
        .map(unwrap_variant)
        .and_then(|v| v.as_str())?;
    if s.is_empty() || s == "/" { None } else { Some(s.to_string()) }
}

fn prop_bool(props: &HashMap<String, Value>, key: &str) -> bool {
    props.get(key)
        .map(unwrap_variant)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn extract_tooltip(props: &HashMap<String, Value>) -> String {
    let val = match props.get("ToolTip") {
        Some(v) => unwrap_variant(v),
        None => return String::new(),
    };

    if let Some(fields) = val.as_struct_fields() {
        if fields.len() >= 4 {
            let title = fields[2].as_str().unwrap_or("");
            if !title.is_empty() { return title.to_string(); }
            return fields[3].as_str().unwrap_or("").to_string();
        }
    }
    val.as_str().unwrap_or("").to_string()
}

fn extract_icon_pixmap(props: &HashMap<String, Value>) -> Option<Vec<TrayPixmap>> {
    let val = props.get("IconPixmap").map(unwrap_variant)?;
    let arr = val.as_array()?;
    if arr.is_empty() { return None; }

    let pixmaps: Vec<TrayPixmap> = arr.iter().filter_map(|entry| {
        let inner = unwrap_variant(entry);
        let fields = inner.as_struct_fields()?;
        if fields.len() < 3 { return None; }
        let w = fields[0].as_i32()?;
        let h = fields[1].as_i32()?;
        let data: Vec<u8> = fields[2].as_array()?
            .iter().filter_map(|v| v.as_u8()).collect();
        Some(TrayPixmap { width: w, height: h, argb_data: data })
    }).collect();

    if pixmaps.is_empty() { None } else { Some(pixmaps) }
}
