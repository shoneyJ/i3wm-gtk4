//! Shared layout-cascade command construction.
//!
//! Builds a `;`-separated i3 command that applies a given layout to the
//! focused workspace AND every non-leaf container inside it. Used by:
//! - the bar's layout popover (instant click → workspace-wide rearrange)
//! - the `i3more-layout` CLI (keyboard shortcuts → same behaviour)
//!
//! Cascading is what makes a "set workspace to splitv" action affect
//! windows that are buried inside a nested tabbed/stacked sub-container
//! — a bare `layout splitv` only changes the focused container's
//! immediate parent, so nested groups would keep their old layout and
//! continue absorbing new windows under their own rules.

use serde_json::Value;

/// Build the cascade command for `layout` against the focused workspace
/// found in `tree`. Falls back to `layout <layout>` (parent-of-focused
/// only) when the workspace can't be resolved.
///
/// Strategy: i3's `layout X` always ascends to the focused con's parent
/// before applying — so to set container C's layout we have to send
/// `[con_id=<any child of C>] layout X`. We walk the workspace and
/// collect a proxy child for each container whose layout we want to
/// change. When the workspace already has a single wrapper child that
/// owns its own children, we skip the workspace itself so i3 doesn't
/// create yet another redundant wrapper.
pub fn build_cascade_command(tree: Option<&Value>, layout: &str) -> String {
    let Some(tree) = tree else {
        return format!("layout {}", layout);
    };
    let Some((ws_id, _)) = focused_workspace(tree) else {
        return format!("layout {}", layout);
    };
    let Some(ws_node) = find_node_by_id(tree, ws_id) else {
        return format!("layout {}", layout);
    };

    let mut proxy_ids: Vec<i64> = Vec::new();
    collect_proxy_children(ws_node, true, &mut proxy_ids);

    if proxy_ids.is_empty() {
        // Empty workspace — i3 allows direct layout set in that case.
        return format!("[con_id={}] layout {}", ws_id, layout);
    }
    proxy_ids
        .iter()
        .map(|id| format!("[con_id={}] layout {}", id, layout))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Layout of the focused leaf's immediate parent container — what
/// determines where the next-opened window lands and the right thing to
/// display on the bar indicator. Returns None when nothing is focused.
pub fn focused_parent_layout(tree: &Value) -> Option<String> {
    let mut stack: Vec<&Value> = Vec::new();
    find_focused_leaf(tree, &mut stack)?;
    stack
        .last()
        .copied()
        .and_then(|parent| parent["layout"].as_str().map(str::to_string))
}

/// Return (con_id, layout) of the workspace that is currently focused.
/// Prefers the workspace ancestor of the focused leaf; falls back to
/// walking each node's `focus` array from root so empty workspaces also
/// resolve correctly.
pub fn focused_workspace(tree: &Value) -> Option<(i64, String)> {
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

/// For every non-leaf container under `node`, push the id of ONE of its
/// children so that `[con_id=<child>] layout X` will ascend in i3 and
/// set THAT container's layout. The workspace itself is optionally
/// skipped (when `node` IS the workspace) if it has a single child that
/// is itself a split container — in that case the single child already
/// is the effective wrapper, and including the workspace would only
/// cause i3 to inject another redundant wrapper.
fn collect_proxy_children(node: &Value, is_workspace: bool, ids: &mut Vec<i64>) {
    let Some(kids) = node["nodes"].as_array() else { return; };
    if kids.is_empty() {
        return;
    }
    let skip_self = is_workspace
        && kids.len() == 1
        && kids[0]["nodes"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
    if !skip_self {
        if let Some(id) = kids[0]["id"].as_i64() {
            ids.push(id);
        }
    }
    for child in kids {
        collect_proxy_children(child, false, ids);
    }
}
