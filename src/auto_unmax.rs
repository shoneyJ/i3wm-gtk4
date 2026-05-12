//! Bug 1 fix from docs/plan/dynamicWM.md.
//!
//! The Feature A patch in vendor/i3/src/handlers.c flips a parent
//! container's layout to `tabbed` when the user clicks an app's
//! titlebar maximize button, and marks that parent with
//! `_i3more_maxed_<workspace>` via i3's mark facility. While the parent
//! has exactly one child the user sees their window dominating the
//! workspace as intended. When a new window arrives the parent gains a
//! second child — i3's default placement makes it a tab, which is what
//! we're trying to avoid. This module detects that state from a tree
//! snapshot and produces an i3 command string that reverts the layout
//! and drops the mark, so the new window ends up as a normal splith /
//! splitv sibling instead of a tab.
//!
//! The check is invoked on every tree refresh — running on a workspace
//! that doesn't carry the mark is a cheap pure-Rust tree walk.

use serde_json::Value;

const MARK_PREFIX: &str = "_i3more_maxed_";

pub fn revert_command(tree: &Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    walk(tree, &mut parts);
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

fn walk(node: &Value, parts: &mut Vec<String>) {
    if let Some(name) = maxed_mark(node) {
        let layout = node["layout"].as_str().unwrap_or("");
        let num_kids = node["nodes"].as_array().map(|a| a.len()).unwrap_or(0);
        if (layout == "tabbed" || layout == "stacked") && num_kids > 1 {
            let target = node["last_split_layout"]
                .as_str()
                .filter(|s| *s == "splith" || *s == "splitv")
                .unwrap_or("splith");
            parts.push(format!(
                "[con_mark=\"{name}\"] layout {target}; \
                 [con_mark=\"{name}\"] unmark {name}"
            ));
        }
    }

    if let Some(nodes) = node["nodes"].as_array() {
        for child in nodes {
            walk(child, parts);
        }
    }
    if let Some(floating) = node["floating_nodes"].as_array() {
        for child in floating {
            walk(child, parts);
        }
    }
}

fn maxed_mark(node: &Value) -> Option<String> {
    node["marks"]
        .as_array()?
        .iter()
        .filter_map(|m| m.as_str())
        .find(|name| name.starts_with(MARK_PREFIX))
        .map(str::to_string)
}
