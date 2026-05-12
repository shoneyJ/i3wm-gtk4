//! Layout indicator for the i3More bar.
//!
//! Shows the layout of the parent container of the currently focused
//! window (splith / splitv / tabbed / stacked). Lives in the sysinfo
//! area between battery and clock so the user can see at a glance how
//! the next opened window will be inserted — useful after Feature A
//! flips a parent to tabbed via the maximize button.
//!
//! Clicking the indicator opens a popover with four buttons; clicking
//! one sends `layout <name>` over IPC, which changes the focused con's
//! parent layout (matching what the indicator displays).

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
    match focused_parent_layout(tree) {
        Some(layout) => {
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
        ("splith",   fa::TABLE_COLUMNS, "split horizontal"),
        ("splitv",   fa::GRIP_LINES,    "split vertical"),
        ("tabbed",   fa::CLONE,         "tabbed"),
        ("stacking", fa::LAYER_GROUP,   "stacked"),
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
        let cmd = format!("layout {}", cmd_name);
        let lbl_refresh = label_for_refresh.clone();
        let con_refresh = container_for_refresh.clone();
        btn.connect_clicked(move |_| {
            if let Ok(mut conn) = crate::ipc::I3Connection::connect() {
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

/// Walk the tree to find the focused leaf, return its parent's `layout`
/// string. Returns None when nothing is focused or focus is on the root.
fn focused_parent_layout(tree: &Value) -> Option<String> {
    let mut stack: Vec<&Value> = Vec::new();
    find_focused_parent(tree, &mut stack)
        .and_then(|parent| parent["layout"].as_str().map(str::to_string))
}

fn find_focused_parent<'a>(
    node: &'a Value,
    stack: &mut Vec<&'a Value>,
) -> Option<&'a Value> {
    if node["focused"].as_bool() == Some(true) {
        return stack.last().copied();
    }

    stack.push(node);
    if let Some(nodes) = node["nodes"].as_array() {
        for child in nodes {
            if let Some(found) = find_focused_parent(child, stack) {
                stack.pop();
                return Some(found);
            }
        }
    }
    if let Some(floating) = node["floating_nodes"].as_array() {
        for child in floating {
            if let Some(found) = find_focused_parent(child, stack) {
                stack.pop();
                return Some(found);
            }
        }
    }
    stack.pop();
    None
}
