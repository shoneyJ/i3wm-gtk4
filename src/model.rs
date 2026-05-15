//! Workspace data model and tree parsing.
//!
//! `build_tree_snapshot` is the single entry point: one walk over
//! `get_tree` yields the workspace state (icons / focus underline /
//! class→con_ids), the focused leaf's parent layout (for the bar's
//! layout indicator), and any auto-unmax revert command (Bug 1 fix in
//! docs/plan/dynamicWM.md). This replaces three separate walks that
//! used to run on every i3 event.

use serde_json::Value;
use std::collections::HashMap;

/// Mark prefix written by the i3 maximize-button handler in
/// `vendor/i3/src/handlers.c::max_set_mark`. Bar auto-reverts the
/// marked container's layout when a new window joins it.
const MAX_MARK_PREFIX: &str = "_i3more_maxed_";

/// Information about a single workspace, including its windows.
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub num: i64,
    pub focused: bool,
    pub urgent: bool,
    pub visible: bool,
    pub output: String,
    /// Distinct window classes present on this workspace, in tree-walk
    /// order. Drives one-icon-per-class rendering on the bar.
    pub window_classes: Vec<String>,
    /// Class of the focused leaf on this workspace, if any. Drives the
    /// per-class focus underline under the workspace icons.
    pub focused_class: Option<String>,
    /// Per-class con IDs in tree-walk order. Used by the click handler
    /// to cycle focus through multiple windows of the same class.
    pub class_con_ids: HashMap<String, Vec<i64>>,
}

/// Everything `refresh_state` needs from a single i3 tree query. Built
/// in one tree walk to avoid re-traversing for each consumer.
pub struct TreeSnapshot {
    pub workspaces: Vec<WorkspaceInfo>,
    /// Layout of the focused leaf's immediate parent container — the
    /// container that determines where the next-opened window lands.
    /// `None` when nothing is focused.
    pub focused_parent_layout: Option<String>,
    /// `;`-separated revert command for every container carrying a
    /// `_i3more_maxed_*` mark that is now tabbed/stacked with >1
    /// children. `None` when no auto-unmax action is needed.
    pub auto_unmax_cmd: Option<String>,
}

/// Per-workspace accumulator filled during the tree walk.
#[derive(Default)]
struct WorkspaceTreeData {
    classes: Vec<String>,                       // tree-walk order, deduped later
    class_con_ids: HashMap<String, Vec<i64>>,   // class → con ids in tree-walk order
    focused_class: Option<String>,              // class of focused leaf, if any
}

/// Build the full snapshot from a `get_tree` payload and the
/// `get_workspaces` metadata in one pass.
pub fn build_tree_snapshot(
    workspaces_json: &Value,
    tree_json: &Value,
    output_order: &[String],
) -> TreeSnapshot {
    let mut tree_data: HashMap<i64, WorkspaceTreeData> = HashMap::new();
    let mut walker = TreeWalker {
        focused_parent_layout: None,
        auto_unmax_parts: Vec::new(),
    };
    walk_root(tree_json, &mut walker, &mut tree_data);

    let workspaces = assemble_workspaces(workspaces_json, output_order, tree_data);

    let auto_unmax_cmd = if walker.auto_unmax_parts.is_empty() {
        None
    } else {
        Some(walker.auto_unmax_parts.join("; "))
    };

    TreeSnapshot {
        workspaces,
        focused_parent_layout: walker.focused_parent_layout,
        auto_unmax_cmd,
    }
}

struct TreeWalker {
    focused_parent_layout: Option<String>,
    auto_unmax_parts: Vec<String>,
}

fn walk_root(node: &Value, walker: &mut TreeWalker, map: &mut HashMap<i64, WorkspaceTreeData>) {
    let node_type = node["type"].as_str().unwrap_or("");
    if node_type == "workspace" {
        if let Some(num) = node["num"].as_i64() {
            if num > 0 {
                let mut data = WorkspaceTreeData::default();
                walk_inside_workspace(node, None, walker, &mut data);
                map.insert(num, data);
                return;
            }
        }
    }
    if let Some(nodes) = node["nodes"].as_array() {
        for child in nodes {
            walk_root(child, walker, map);
        }
    }
    if let Some(floating) = node["floating_nodes"].as_array() {
        for child in floating {
            walk_root(child, walker, map);
        }
    }
}

/// Descend inside a workspace. `parent` is the immediate ancestor of
/// `node` (`None` only when `node` is the workspace itself). We pass it
/// down so the focused leaf can record its parent's layout in one go.
fn walk_inside_workspace<'a>(
    node: &'a Value,
    parent: Option<&'a Value>,
    walker: &mut TreeWalker,
    data: &mut WorkspaceTreeData,
) {
    // Auto-unmax check: containers (workspace or split) with the
    // maxwrap mark that are now tabbed/stacked AND have >1 children.
    if let Some(marks) = node["marks"].as_array() {
        for mark in marks {
            if let Some(name) = mark.as_str() {
                if !name.starts_with(MAX_MARK_PREFIX) {
                    continue;
                }
                let layout = node["layout"].as_str().unwrap_or("");
                let num_kids = node["nodes"].as_array().map(|a| a.len()).unwrap_or(0);
                if (layout == "tabbed" || layout == "stacked") && num_kids > 1 {
                    let target = node["last_split_layout"]
                        .as_str()
                        .filter(|s| *s == "splith" || *s == "splitv")
                        .unwrap_or("splith");
                    walker.auto_unmax_parts.push(format!(
                        "[con_mark=\"{name}\"] layout {target}; \
                         [con_mark=\"{name}\"] unmark {name}"
                    ));
                }
            }
        }
    }

    // Per-class data collection (leaves only).
    if let Some(class) = node["window_properties"]["class"].as_str() {
        if !class.is_empty() {
            data.classes.push(class.to_string());
            if let Some(id) = node["id"].as_i64() {
                data.class_con_ids
                    .entry(class.to_string())
                    .or_default()
                    .push(id);
            }
            if node["focused"].as_bool() == Some(true) {
                data.focused_class = Some(class.to_string());
                // Record focused-leaf's parent.layout for the bar indicator.
                if let Some(parent) = parent {
                    if let Some(layout) = parent["layout"].as_str() {
                        walker.focused_parent_layout = Some(layout.to_string());
                    }
                }
            }
        }
    }

    if let Some(nodes) = node["nodes"].as_array() {
        for child in nodes {
            walk_inside_workspace(child, Some(node), walker, data);
        }
    }
    if let Some(floating) = node["floating_nodes"].as_array() {
        for child in floating {
            walk_inside_workspace(child, Some(node), walker, data);
        }
    }
}

fn assemble_workspaces(
    workspaces_json: &Value,
    output_order: &[String],
    mut tree_data: HashMap<i64, WorkspaceTreeData>,
) -> Vec<WorkspaceInfo> {
    let Some(ws_array) = workspaces_json.as_array() else {
        return Vec::new();
    };
    let mut result: Vec<WorkspaceInfo> = ws_array
        .iter()
        .filter_map(|ws| {
            let num = ws["num"].as_i64()?;
            let mut data = tree_data.remove(&num).unwrap_or_default();
            // De-dupe classes while preserving first-seen order so the
            // bar's icon row stays stable across re-renders.
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            data.classes.retain(|c| seen.insert(c.clone()));
            Some(WorkspaceInfo {
                num,
                focused: ws["focused"].as_bool().unwrap_or(false),
                urgent: ws["urgent"].as_bool().unwrap_or(false),
                visible: ws["visible"].as_bool().unwrap_or(false),
                output: ws["output"].as_str().unwrap_or("").to_string(),
                window_classes: data.classes,
                focused_class: data.focused_class,
                class_con_ids: data.class_con_ids,
            })
        })
        .collect();

    // Sort by spatial output position (left-to-right), then workspace
    // number. Groups workspaces by physical monitor for the separator
    // logic in navigator::render_workspaces.
    result.sort_by(|a, b| {
        let a_pos = output_order.iter().position(|o| o == &a.output).unwrap_or(usize::MAX);
        let b_pos = output_order.iter().position(|o| o == &b.output).unwrap_or(usize::MAX);
        a_pos.cmp(&b_pos).then(a.num.cmp(&b.num))
    });
    result
}
