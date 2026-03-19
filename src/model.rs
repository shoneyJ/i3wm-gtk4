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
    pub window_classes: Vec<String>,
}

/// Build a complete workspace state by combining `get_workspaces` and `get_tree` data.
pub fn build_workspace_state(
    workspaces_json: &Value,
    tree_json: &Value,
) -> Vec<WorkspaceInfo> {
    // Extract per-workspace window classes from the tree
    let class_map = extract_workspace_classes(tree_json);

    // Build WorkspaceInfo list from the workspace metadata
    let ws_array = match workspaces_json.as_array() {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    let mut result: Vec<WorkspaceInfo> = ws_array
        .iter()
        .filter_map(|ws| {
            let num = ws["num"].as_i64()?;
            Some(WorkspaceInfo {
                num,
                focused: ws["focused"].as_bool().unwrap_or(false),
                urgent: ws["urgent"].as_bool().unwrap_or(false),
                visible: ws["visible"].as_bool().unwrap_or(false),
                output: ws["output"].as_str().unwrap_or("").to_string(),
                window_classes: class_map.get(&num).cloned().unwrap_or_default(),
            })
        })
        .collect();

    // Sort by output (monitor) first, then by workspace number within each output.
    // This groups workspaces by monitor for the separator logic in the navigator.
    result.sort_by(|a, b| a.output.cmp(&b.output).then(a.num.cmp(&b.num)));
    result
}

/// Recursively traverse the i3 tree to extract unique window classes per workspace.
fn extract_workspace_classes(tree: &Value) -> HashMap<i64, Vec<String>> {
    let mut map: HashMap<i64, Vec<String>> = HashMap::new();
    collect_workspaces(tree, &mut map);
    map
}

fn collect_workspaces(node: &Value, map: &mut HashMap<i64, Vec<String>>) {
    let node_type = node["type"].as_str().unwrap_or("");

    if node_type == "workspace" {
        if let Some(num) = node["num"].as_i64() {
            if num > 0 {
                let mut classes = Vec::new();
                collect_window_classes(node, &mut classes);
                classes.sort();
                classes.dedup();
                map.insert(num, classes);
                return; // Don't recurse further for workspaces
            }
        }
    }

    // Recurse into child nodes
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

/// Recursively collect all window_properties.class values from a subtree.
fn collect_window_classes(node: &Value, classes: &mut Vec<String>) {
    if let Some(class) = node["window_properties"]["class"].as_str() {
        if !class.is_empty() {
            classes.push(class.to_string());
        }
    }

    if let Some(nodes) = node["nodes"].as_array() {
        for child in nodes {
            collect_window_classes(child, classes);
        }
    }
    if let Some(floating) = node["floating_nodes"].as_array() {
        for child in floating {
            collect_window_classes(child, classes);
        }
    }
}
