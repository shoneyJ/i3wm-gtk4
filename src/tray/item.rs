/// Read StatusNotifierItem properties from a tray app over D-Bus.

use zbus::Connection;

use super::types::{TrayItemId, TrayItemProps, TrayPixmap};

/// Read all relevant properties from an SNI item.
pub async fn read_item_props(
    conn: &Connection,
    bus_name: &str,
    object_path: &str,
) -> Result<TrayItemProps, zbus::Error> {
    let proxy = zbus::Proxy::new(
        conn,
        bus_name,
        object_path,
        "org.kde.StatusNotifierItem",
    )
    .await?;

    let title = read_string_prop(&proxy, "Title").await;
    let icon_name = read_string_prop(&proxy, "IconName").await;
    let status = read_string_prop(&proxy, "Status").await;
    let menu = read_optional_string_prop(&proxy, "Menu").await;
    let item_is_menu = read_bool_prop(&proxy, "ItemIsMenu").await;
    let tooltip = read_tooltip(&proxy).await;
    let icon_pixmap = read_icon_pixmap(&proxy).await;

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

async fn read_string_prop(proxy: &zbus::Proxy<'_>, name: &str) -> String {
    proxy
        .get_property::<String>(name)
        .await
        .unwrap_or_default()
}

async fn read_optional_string_prop(proxy: &zbus::Proxy<'_>, name: &str) -> Option<String> {
    // Menu property is an object path
    match proxy.get_property::<zbus::zvariant::OwnedObjectPath>(name).await {
        Ok(path) => {
            let s = path.to_string();
            if s.is_empty() || s == "/" {
                None
            } else {
                Some(s)
            }
        }
        Err(_) => None,
    }
}

async fn read_bool_prop(proxy: &zbus::Proxy<'_>, name: &str) -> bool {
    proxy.get_property::<bool>(name).await.unwrap_or(false)
}

/// Read the ToolTip property.
/// The SNI spec defines ToolTip as (s, a(iiay), s, s) — icon_name, icon_pixmap, title, description.
/// We extract the title (3rd element).
async fn read_tooltip(proxy: &zbus::Proxy<'_>) -> String {
    // Try the structured tooltip first
    let result = proxy
        .get_property::<(
            String,
            Vec<(i32, i32, Vec<u8>)>,
            String,
            String,
        )>("ToolTip")
        .await;

    match result {
        Ok((_icon_name, _pixmaps, title, description)) => {
            if !title.is_empty() {
                title
            } else {
                description
            }
        }
        Err(_) => {
            // Some apps use a plain string tooltip
            read_string_prop(proxy, "ToolTip").await
        }
    }
}

/// Read IconPixmap property: a(iiay) — array of (width, height, ARGB data).
async fn read_icon_pixmap(proxy: &zbus::Proxy<'_>) -> Option<Vec<TrayPixmap>> {
    let result = proxy
        .get_property::<Vec<(i32, i32, Vec<u8>)>>("IconPixmap")
        .await;

    match result {
        Ok(pixmaps) if !pixmaps.is_empty() => {
            Some(
                pixmaps
                    .into_iter()
                    .map(|(w, h, data)| TrayPixmap {
                        width: w,
                        height: h,
                        argb_data: data,
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}
