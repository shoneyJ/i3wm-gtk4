/// DBusMenu client — reads menu trees from `com.canonical.dbusmenu` and
/// displays them as GTK popover menus.

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use linbus::Value;

use std::collections::HashMap;

use super::types::TrayItemId;

/// A parsed menu item from the DBusMenu tree.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon_name: Option<String>,
    pub toggle_type: Option<String>,
    pub toggle_state: i32,
    pub item_type: Option<String>,
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    fn from_layout(
        id: i32,
        props: &HashMap<String, Value>,
        children_raw: &[Value],
    ) -> Self {
        let label = prop_string(props, "label").unwrap_or_default();
        let enabled = prop_bool(props, "enabled").unwrap_or(true);
        let visible = prop_bool(props, "visible").unwrap_or(true);
        let icon_name = prop_string(props, "icon-name");
        let toggle_type = prop_string(props, "toggle-type");
        let toggle_state = prop_i32(props, "toggle-state").unwrap_or(-1);
        let item_type = prop_string(props, "type");

        let children = parse_children(children_raw);

        MenuItem {
            id,
            label,
            enabled,
            visible,
            icon_name,
            toggle_type,
            toggle_state,
            item_type,
            children,
        }
    }
}

fn prop_string(props: &HashMap<String, Value>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn prop_bool(props: &HashMap<String, Value>, key: &str) -> Option<bool> {
    props.get(key).and_then(|v| v.as_bool())
}

fn prop_i32(props: &HashMap<String, Value>, key: &str) -> Option<i32> {
    props.get(key).and_then(|v| v.as_i32())
}

/// Parse an array of children from the GetLayout response.
/// Each child is a struct `(i, a{sv}, av)` encoded as a Value.
fn parse_children(children_raw: &[Value]) -> Vec<MenuItem> {
    let mut items = Vec::new();
    for child_val in children_raw {
        // Each child may be wrapped in a variant
        let inner = match child_val {
            Value::Variant(v) => v.as_ref(),
            other => other,
        };

        let fields = match inner.as_struct_fields() {
            Some(f) => f,
            None => {
                log::debug!("DBusMenu: child is not a Struct");
                continue;
            }
        };

        if fields.len() < 3 {
            log::debug!("DBusMenu: struct has {} fields, expected 3", fields.len());
            continue;
        }

        let id = match fields[0].as_i32() {
            Some(v) => v,
            None => continue,
        };

        // Parse properties dict a{sv}
        let props: HashMap<String, Value> = fields[1]
            .to_string_dict()
            .unwrap_or_default();

        // Parse children array av
        let sub_children: Vec<Value> = fields[2]
            .as_array()
            .map(|arr| arr.to_vec())
            .unwrap_or_default();

        items.push(MenuItem::from_layout(id, &props, &sub_children));
    }
    items
}

/// Fetch the full menu layout from a DBusMenu service.
fn get_layout(
    conn: &mut linbus::Connection,
    bus_name: &str,
    menu_path: &str,
) -> Result<MenuItem, linbus::LinbusError> {
    let mut proxy = linbus::Proxy::new(conn, bus_name, menu_path, "com.canonical.dbusmenu");

    // GetLayout(parentId: i, recursionDepth: i, propertyNames: as) -> (u, (ia{sv}av))
    let result = proxy.call("GetLayout", vec![
        Value::I32(0),
        Value::I32(-1),
        Value::TypedArray("s".into(), vec![]),
    ])?;

    // Response is a struct: (u32 revision, struct(i32 id, a{sv} props, av children))
    // But it comes as individual body values: [u32, struct(...)]
    let root_struct = result.get(1)
        .or_else(|| result.first())
        .ok_or_else(|| linbus::LinbusError::ProtocolError("empty GetLayout reply".into()))?;

    let inner = match root_struct {
        Value::Variant(v) => v.as_ref(),
        other => other,
    };

    let fields = inner.as_struct_fields()
        .ok_or_else(|| linbus::LinbusError::ProtocolError("GetLayout root not a struct".into()))?;

    let root_id = fields.first().and_then(|v| v.as_i32()).unwrap_or(0);
    let root_props = fields.get(1)
        .and_then(|v| v.to_string_dict())
        .unwrap_or_default();
    let root_children: Vec<Value> = fields.get(2)
        .and_then(|v| v.as_array())
        .map(|arr| arr.to_vec())
        .unwrap_or_default();

    Ok(MenuItem::from_layout(root_id, &root_props, &root_children))
}

/// Send an event to a DBusMenu item (e.g. "clicked").
fn send_event(
    conn: &mut linbus::Connection,
    bus_name: &str,
    menu_path: &str,
    item_id: i32,
    event_id: &str,
) -> Result<(), linbus::LinbusError> {
    let mut proxy = linbus::Proxy::new(conn, bus_name, menu_path, "com.canonical.dbusmenu");

    // Event(id: i, eventId: s, data: v, timestamp: u)
    let _: Vec<Value> = proxy.call("Event", vec![
        Value::I32(item_id),
        Value::String(event_id.into()),
        Value::Variant(Box::new(Value::I32(0))),
        Value::U32(0),
    ])?;
    Ok(())
}

/// Build a `gio::Menu` model from the parsed menu tree.
fn build_gio_menu(
    items: &[MenuItem],
    action_group: &gio::SimpleActionGroup,
    bus_name: &str,
    menu_path: &str,
) -> gio::Menu {
    let menu = gio::Menu::new();

    for item in items {
        if !item.visible {
            continue;
        }

        if item.item_type.as_deref() == Some("separator") {
            let section = gio::Menu::new();
            menu.append_section(None, &section);
            continue;
        }

        if !item.children.is_empty() {
            let submenu =
                build_gio_menu(&item.children, action_group, bus_name, menu_path);
            let label = clean_label(&item.label);
            menu.append_submenu(Some(&label), &submenu);
        } else {
            let label = clean_label(&item.label);
            let action_name = format!("dbusmenu.item-{}", item.id);
            let menu_item = gio::MenuItem::new(Some(&label), Some(&action_name));

            if let Some(ref icon) = item.icon_name {
                if !icon.is_empty() {
                    let icon_obj = gio::ThemedIcon::new(icon);
                    menu_item.set_icon(&icon_obj);
                }
            }

            let short_name = format!("item-{}", item.id);
            let action = gio::SimpleAction::new(&short_name, None);
            action.set_enabled(item.enabled);

            let bus = bus_name.to_string();
            let path = menu_path.to_string();
            let id = item.id;
            action.connect_activate(move |_, _| {
                let bus = bus.clone();
                let path = path.clone();
                glib::spawn_future_local(async move {
                    let _ = std::thread::spawn(move || {
                        let mut conn = match linbus::Connection::session() {
                            Ok(c) => c,
                            Err(e) => {
                                log::warn!("DBusMenu event connect failed: {}", e);
                                return;
                            }
                        };
                        if let Err(e) = send_event(&mut conn, &bus, &path, id, "clicked") {
                            log::warn!("DBusMenu Event call failed: {}", e);
                        }
                    })
                    .join();
                });
            });

            action_group.add_action(&action);
            menu.append_item(&menu_item);
        }
    }

    menu
}

/// Strip mnemonic underscores from labels (e.g. "_Connect" → "Connect").
fn clean_label(label: &str) -> String {
    label.replace('_', "")
}

/// Show a DBusMenu popup for the given tray item.
/// Called from click handlers when `item_is_menu` is true.
pub fn show_menu(widget: &gtk4::Box, id: &TrayItemId, menu_path: &str) {
    let bus_name = id.bus_name.clone();
    let menu_object_path = menu_path.to_string();
    let widget = widget.clone();

    glib::spawn_future_local(async move {
        let bus = bus_name.clone();
        let path = menu_object_path.clone();

        let result = std::thread::spawn(move || {
            let mut conn = linbus::Connection::session()?;
            get_layout(&mut conn, &bus, &path)
        })
        .join();

        let root = match result {
            Ok(Ok(root)) => root,
            Ok(Err(e)) => {
                eprintln!("[dbusmenu] GetLayout failed: {}", e);
                log::warn!("DBusMenu GetLayout failed: {}", e);
                return;
            }
            Err(_) => {
                log::warn!("DBusMenu thread panicked");
                return;
            }
        };

        // Build gio menu model and action group
        let action_group = gio::SimpleActionGroup::new();
        let gio_menu =
            build_gio_menu(&root.children, &action_group, &bus_name, &menu_object_path);

        // Create popover menu
        let popover = gtk4::PopoverMenu::from_model(Some(&gio_menu));
        popover.set_parent(&widget);
        popover.set_has_arrow(false);

        // Insert action group so actions are found
        widget.insert_action_group("dbusmenu", Some(&action_group));

        popover.popup();
    });
}
