//! Workspace data model and tree parsing.

use serde_json::Value;
use std::collections::HashMap;

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

/// Per-workspace data collected from a single tree walk.
#[derive(Default)]
struct WorkspaceTreeData {
    classes: Vec<String>,                       // tree-walk order, deduped
    class_con_ids: HashMap<String, Vec<i64>>,   // class → con ids in tree-walk order
    focused_class: Option<String>,              // class of focused leaf, if any
}

/// Build a complete workspace state by combining `get_workspaces` and `get_tree` data.
/// `output_order` provides spatially-sorted output names (left-to-right) for correct grouping.
pub fn build_workspace_state(
    workspaces_json: &Value,
    tree_json: &Value,
    output_order: &[String],
) -> Vec<WorkspaceInfo> {
    let mut tree_data: HashMap<i64, WorkspaceTreeData> = HashMap::new();
    collect_workspaces(tree_json, &mut tree_data);

    // Build WorkspaceInfo list from the workspace metadata
    let ws_array = match workspaces_json.as_array() {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    let mut result: Vec<WorkspaceInfo> = ws_array
        .iter()
        .filter_map(|ws| {
            let num = ws["num"].as_i64()?;
            let mut data = tree_data.remove(&num).unwrap_or_default();
            // Keep classes in tree-walk order but de-duplicated (preserving
            // first-seen order so the bar's icon order stays stable).
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

    // Sort by spatial output position (left-to-right), then by workspace number.
    // This groups workspaces by physical monitor position for the separator logic.
    result.sort_by(|a, b| {
        let a_pos = output_order.iter().position(|o| o == &a.output).unwrap_or(usize::MAX);
        let b_pos = output_order.iter().position(|o| o == &b.output).unwrap_or(usize::MAX);
        a_pos.cmp(&b_pos).then(a.num.cmp(&b.num))
    });
    result
}

fn collect_workspaces(node: &Value, map: &mut HashMap<i64, WorkspaceTreeData>) {
    let node_type = node["type"].as_str().unwrap_or("");

    if node_type == "workspace" {
        if let Some(num) = node["num"].as_i64() {
            if num > 0 {
                let mut data = WorkspaceTreeData::default();
                collect_leaves(node, &mut data);
                map.insert(num, data);
                return; // Don't recurse further for workspaces
            }
        }
    }

    if let Some(nodes) = node["nodes"].as_array() {
        for child in nodes {
            collect_workspaces(child, map);
        }
    }
    if let Some(floating) = node["floating_nodes"].as_array() {
        for child in floating {
            collect_workspaces(child, map);
        }
    }
}

fn collect_leaves(node: &Value, data: &mut WorkspaceTreeData) {
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
            }
        }
    }

    if let Some(nodes) = node["nodes"].as_array() {
        for child in nodes {
            collect_leaves(child, data);
        }
    }
    if let Some(floating) = node["floating_nodes"].as_array() {
        for child in floating {
            collect_leaves(child, data);
        }
    }
}
