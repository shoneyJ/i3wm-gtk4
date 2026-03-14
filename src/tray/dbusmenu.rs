/// DBusMenu client — reads menu trees from `com.canonical.dbusmenu` and
/// displays them as GTK popover menus.

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use zbus::zvariant::OwnedValue;

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
        props: &HashMap<String, OwnedValue>,
        children_raw: &[OwnedValue],
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

fn prop_string(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props
        .get(key)
        .and_then(|v| v.downcast_ref::<String>().ok())
}

fn prop_bool(props: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    props.get(key).and_then(|v| v.downcast_ref::<bool>().ok())
}

fn prop_i32(props: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
    props.get(key).and_then(|v| v.downcast_ref::<i32>().ok())
}

/// Parse an array of children from the GetLayout response.
/// Each child is a struct `(i, a{sv}, av)` encoded as a Value.
fn parse_children(children_raw: &[OwnedValue]) -> Vec<MenuItem> {
    let mut items = Vec::new();
    for child_val in children_raw {
        // Each child is wrapped in a variant; unwrap the Structure (i, a{sv}, av)
        let structure = match child_val.downcast_ref::<zbus::zvariant::Structure>() {
            Ok(s) => s,
            Err(_) => {
                log::debug!("DBusMenu: child is not a Structure");
                continue;
            }
        };

        let fields = structure.fields();
        if fields.len() < 3 {
            log::debug!(
                "DBusMenu: structure has {} fields, expected 3",
                fields.len()
            );
            continue;
        }

        let id: i32 = match fields[0].downcast_ref::<i32>() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Parse properties dict a{sv} via zvariant Dict
        let props: HashMap<String, OwnedValue> = match fields[1]
            .downcast_ref::<zbus::zvariant::Dict>()
        {
            Ok(dict) => match dict.try_clone() {
                Ok(owned_dict) => {
                    <HashMap<String, OwnedValue>>::try_from(owned_dict)
                        .unwrap_or_default()
                }
                Err(_) => HashMap::new(),
            }
            Err(_) => HashMap::new(),
        };

        // Parse children array av via zvariant Array
        let sub_children: Vec<OwnedValue> = match fields[2]
            .downcast_ref::<zbus::zvariant::Array>()
        {
            Ok(arr) => arr
                .iter()
                .filter_map(|v| v.try_to_owned().ok())
                .collect(),
            Err(_) => Vec::new(),
        };

        items.push(MenuItem::from_layout(id, &props, &sub_children));
    }
    items
}

/// Fetch the full menu layout from a DBusMenu service.
async fn get_layout(
    conn: &zbus::Connection,
    bus_name: &str,
    menu_path: &str,
) -> Result<MenuItem, zbus::Error> {
    let proxy =
        zbus::Proxy::new(conn, bus_name, menu_path, "com.canonical.dbusmenu").await?;

    // GetLayout(parentId: i, recursionDepth: i, propertyNames: as) -> (u, (ia{sv}av))
    let result: (u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>)) = proxy
        .call("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
        .await?;

    let (_revision, (root_id, root_props, root_children)) = result;
    Ok(MenuItem::from_layout(root_id, &root_props, &root_children))
}

/// Send an event to a DBusMenu item (e.g. "clicked").
async fn send_event(
    conn: &zbus::Connection,
    bus_name: &str,
    menu_path: &str,
    item_id: i32,
    event_id: &str,
) -> Result<(), zbus::Error> {
    let proxy =
        zbus::Proxy::new(conn, bus_name, menu_path, "com.canonical.dbusmenu").await?;

    // Event(id: i, eventId: s, data: v, timestamp: u)
    let data = zbus::zvariant::Value::I32(0);
    let timestamp = 0u32;
    let _: () = proxy
        .call("Event", &(item_id, event_id, &data, timestamp))
        .await?;
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
                        async_io::block_on(async {
                            let conn = match zbus::Connection::session().await {
                                Ok(c) => c,
                                Err(e) => {
                                    log::warn!("DBusMenu event connect failed: {}", e);
                                    return;
                                }
                            };
                            if let Err(e) =
                                send_event(&conn, &bus, &path, id, "clicked").await
                            {
                                log::warn!("DBusMenu Event call failed: {}", e);
                            }
                        });
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
            async_io::block_on(async {
                let conn = zbus::Connection::session().await?;
                get_layout(&conn, &bus, &path).await
            })
        })
        .join();

        let root = match result {
            Ok(Ok(root)) => root,
            Ok(Err(e)) => {
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
