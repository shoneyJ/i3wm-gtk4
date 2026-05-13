//! Layout indicator for the i3More bar.
//!
//! Shows the layout of the currently focused workspace (the workspace
//! container's own `layout` field — splith / splitv / tabbed). Lives in
//! the sysinfo area between battery and clock.
//!
//! Clicking the indicator opens a popover with three buttons; clicking
//! one sends `[con_id=<workspace>] layout <name>` over IPC, so the
//! workspace's top-level layout — and therefore the visual arrangement
//! of all top-level windows on it — changes immediately.
//!
//! Stacked is intentionally absent from the popover: now that we hide
//! the in-tree title strip for L_STACKED and L_TABBED (see render.c
//! patch), the two are visually indistinguishable, so the popover keeps
//! only the three layouts that look different (splith / splitv / tabbed).
//! Keyboard shortcuts for `layout stacking` still work via i3 directly.

use gtk4::prelude::*;
use serde_json::Value;

use crate::fa;

pub struct LayoutIndicator {
    pub container: gtk4::Box,
    label: gtk4::Label,
}

impl LayoutIndicator {
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        container.add_css_class("layout-indicator-area");
        container.set_valign(gtk4::Align::Center);
        container.set_visible(false);

        let label = gtk4::Label::new(None);
        label.set_use_markup(true);
        label.add_css_class("layout-indicator");
        label.set_valign(gtk4::Align::Center);
        container.append(&label);

        let popover = build_switcher_popover(&label, label.clone(), container.clone());
        let popover_for_click = popover.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.connect_released(move |_, _, _, _| {
            popover_for_click.popup();
        });
        label.add_controller(gesture);

        Self { container, label }
    }

    pub fn update_from_tree(&self, tree: &Value) {
        apply_tree_to_label(&self.label, &self.container, tree);
    }
}

fn apply_tree_to_label(label: &gtk4::Label, container: &gtk4::Box, tree: &Value) {
    match focused_workspace(tree) {
        Some((_id, layout)) => {
            let (glyph, tooltip) = layout_glyph_tooltip(&layout);
            label.set_markup(&fa::fa_icon(glyph, "#a89984", 11));
            label.set_tooltip_text(Some(tooltip));
            container.set_visible(true);
        }
        None => {
            container.set_visible(false);
        }
    }
}

fn build_switcher_popover(
    anchor: &gtk4::Label,
    label_for_refresh: gtk4::Label,
    container_for_refresh: gtk4::Box,
) -> gtk4::Popover {
    let popover = gtk4::Popover::new();
    popover.set_parent(anchor);
    popover.set_autohide(true);
    popover.add_css_class("layout-popover");

    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    row.set_margin_top(4);
    row.set_margin_bottom(4);
    row.set_margin_start(4);
    row.set_margin_end(4);

    let entries: &[(&str, char, &str)] = &[
        ("splith", fa::TABLE_COLUMNS, "split horizontal"),
        ("splitv", fa::GRIP_LINES,    "split vertical"),
        ("tabbed", fa::CLONE,         "tabbed"),
    ];

    for (cmd_name, glyph, tooltip) in entries {
        let btn = gtk4::Button::new();
        btn.add_css_class("layout-popover-button");
        btn.set_tooltip_text(Some(*tooltip));

        let lbl = gtk4::Label::new(None);
        lbl.set_use_markup(true);
        lbl.set_markup(&fa::fa_icon(*glyph, "#ebdbb2", 14));
        btn.set_child(Some(&lbl));

        let popover_dismiss = popover.clone();
        let cmd_name = *cmd_name;
        let lbl_refresh = label_for_refresh.clone();
        let con_refresh = container_for_refresh.clone();
        btn.connect_clicked(move |_| {
            if let Ok(mut conn) = crate::ipc::I3Connection::connect() {
                // Cascade the layout change to the workspace AND every
                // non-leaf container inside it. Plain `[con_id=ws] layout`
                // only changes the workspace's own layout — nested splits
                // / tabbed / stacked sub-containers keep their existing
                // layout, which is why a "splitv" workspace can still open
                // new windows as tabs when the focused leaf sits inside a
                // nested tabbed/stacked container. The cascade fixes that:
                // every group container inside the workspace adopts the
                // selected layout, so any new window now attaches as a
                // sibling under that layout regardless of focus depth.
                let tree = conn.get_tree().ok();
                let cmd = build_cascade_command(tree.as_ref(), cmd_name);
                if let Err(e) = conn.run_command(&cmd) {
                    log::warn!("layout switch '{}' failed: {}", cmd, e);
                }
                // i3 emits no window event for a bare layout change, so
                // pull a fresh tree and update the indicator inline.
                if let Ok(tree) = conn.get_tree() {
                    apply_tree_to_label(&lbl_refresh, &con_refresh, &tree);
                }
            }
            popover_dismiss.popdown();
        });

        row.append(&btn);
    }

    popover.set_child(Some(&row));
    popover
}

fn layout_glyph_tooltip(layout: &str) -> (char, &'static str) {
    match layout {
        "splith" => (fa::TABLE_COLUMNS, "split horizontal"),
        "splitv" => (fa::GRIP_LINES, "split vertical"),
        "tabbed" => (fa::CLONE, "tabbed"),
        "stacked" => (fa::LAYER_GROUP, "stacked"),
        _ => (fa::TABLE_COLUMNS, "unknown layout"),
    }
}

/// Return (con_id, layout) of the workspace that is currently focused.
/// Prefers the workspace ancestor of the focused leaf (covers the common
/// case where a window has focus). Falls back to walking each node's
/// `focus` array from root so empty workspaces also resolve correctly.
fn focused_workspace(tree: &Value) -> Option<(i64, String)> {
    let mut stack: Vec<&Value> = Vec::new();
    if find_focused_leaf(tree, &mut stack).is_some() {
        for ancestor in stack.iter().rev() {
            if ancestor["type"].as_str() == Some("workspace") {
                let id = ancestor["id"].as_i64()?;
                let layout = ancestor["layout"].as_str()?.to_string();
                return Some((id, layout));
            }
        }
    }
    focused_workspace_via_chain(tree)
}

fn find_focused_leaf<'a>(node: &'a Value, stack: &mut Vec<&'a Value>) -> Option<&'a Value> {
    if node["focused"].as_bool() == Some(true) {
        return Some(node);
    }
    stack.push(node);
    if let Some(nodes) = node["nodes"].as_array() {
        for child in nodes {
            if let Some(leaf) = find_focused_leaf(child, stack) {
                return Some(leaf);
            }
        }
    }
    if let Some(floating) = node["floating_nodes"].as_array() {
        for child in floating {
            if let Some(leaf) = find_focused_leaf(child, stack) {
                return Some(leaf);
            }
        }
    }
    stack.pop();
    None
}

/// Build a `;`-separated i3 command that sets the given layout on the
/// focused workspace AND every non-leaf container inside it. Falls back
/// to a plain `layout <name>` if the workspace can't be resolved.
fn build_cascade_command(tree: Option<&Value>, layout: &str) -> String {
    let Some(tree) = tree else {
        return format!("layout {}", layout);
    };
    let Some((ws_id, _)) = focused_workspace(tree) else {
        return format!("layout {}", layout);
    };
    let Some(ws_node) = find_node_by_id(tree, ws_id) else {
        return format!("layout {}", layout);
    };
    let mut ids: Vec<i64> = Vec::new();
    collect_container_ids(ws_node, &mut ids);
    if ids.is_empty() {
        return format!("[con_id={}] layout {}", ws_id, layout);
    }
    ids.iter()
        .map(|id| format!("[con_id={}] layout {}", id, layout))
        .collect::<Vec<_>>()
        .join("; ")
}

fn find_node_by_id(node: &Value, id: i64) -> Option<&Value> {
    if node["id"].as_i64() == Some(id) {
        return Some(node);
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(arr) = node[key].as_array() {
            for child in arr {
                if let Some(found) = find_node_by_id(child, id) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Collect IDs of every node that owns child windows — the workspace
/// itself plus every internal con. Skips leaves (terminal windows) since
/// `layout` is a no-op on them. Skips floating_nodes since changing
/// floating cons' layout doesn't fit the workspace-rearrange intent.
fn collect_container_ids(node: &Value, ids: &mut Vec<i64>) {
    let nodes = node["nodes"].as_array();
    let has_children = nodes.map(|a| !a.is_empty()).unwrap_or(false);
    if has_children {
        if let Some(id) = node["id"].as_i64() {
            ids.push(id);
        }
        for child in nodes.unwrap() {
            collect_container_ids(child, ids);
        }
    }
}

fn focused_workspace_via_chain(node: &Value) -> Option<(i64, String)> {
    if node["type"].as_str() == Some("workspace") {
        let id = node["id"].as_i64()?;
        let layout = node["layout"].as_str()?.to_string();
        return Some((id, layout));
    }
    let focus = node["focus"].as_array()?;
    let first_focus_id = focus.first()?.as_i64()?;
    let nodes = node["nodes"].as_array()?;
    nodes
        .iter()
        .find(|c| c["id"].as_i64() == Some(first_focus_id))
        .and_then(focused_workspace_via_chain)
}
