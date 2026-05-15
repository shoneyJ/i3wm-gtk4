//! Layout indicator for the i3More bar.
//!
//! Shows the layout of the immediate parent of the currently-focused
//! leaf — i.e. the layout that determines where the next-opened window
//! will land. We deliberately do NOT show workspace.layout: i3 does not
//! update a workspace's own `layout` field on a `layout` command (it
//! wraps the workspace's children in a new split container instead), so
//! workspace.layout often disagrees with what the user actually sees.
//! The cascade command (see layout_cmd.rs) makes the focused parent's
//! layout consistent across the whole workspace, so this single
//! reading is enough.
//!
//! Clicking the indicator opens a popover with three buttons; each
//! sends the cascade command — splith / splitv / tabbed are applied to
//! every container in the workspace.
//!
//! Stacked is intentionally absent from the popover: now that we hide
//! the in-tree title strip for L_STACKED and L_TABBED (see render.c
//! patch), the two are visually indistinguishable, so the popover keeps
//! only the three layouts that look different (splith / splitv / tabbed).
//! Keyboard shortcuts for `layout stacking` still work via i3 directly.

use gtk4::prelude::*;
use serde_json::Value;

use crate::fa;
use i3more::layout_cmd::{build_cascade_command, focused_parent_layout};

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
        apply_layout(&self.label, &self.container, focused_parent_layout(tree));
    }

    /// Fast path used by `refresh_state` — the caller already walked
    /// the tree and extracted the focused parent's layout, so we skip
    /// the redundant walk.
    pub fn apply_layout(&self, layout: Option<String>) {
        apply_layout(&self.label, &self.container, layout);
    }
}

fn apply_layout(label: &gtk4::Label, container: &gtk4::Box, layout: Option<String>) {
    match layout {
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
                    apply_layout(&lbl_refresh, &con_refresh, focused_parent_layout(&tree));
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

