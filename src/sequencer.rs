//! Workspace number sequencer.
//!
//! Ensures workspace numbers are always sequential (1, 2, 3, ...) across monitors,
//! with left monitors receiving lower numbers than right monitors.
//! Triggered by workspace "empty" and "init" events — not by polling.

use crate::ipc;
use serde_json::Value;
use std::collections::HashSet;

/// Get spatially ordered output (monitor) names, sorted left-to-right by x coordinate.
pub fn get_output_spatial_order(outputs_json: &Value) -> Vec<String> {
    let mut outputs: Vec<(i64, String)> = outputs_json
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter(|o| o["active"].as_bool() == Some(true))
        .filter_map(|o| {
            let name = o["name"].as_str()?.to_string();
            let x = o["rect"]["x"].as_i64().unwrap_or(0);
            Some((x, name))
        })
        .collect();

    outputs.sort_by_key(|(x, _)| *x);
    outputs.into_iter().map(|(_, name)| name).collect()
}

/// Renumber workspaces to be sequential across monitors (left monitor gets lower numbers).
///
/// Returns `Ok(true)` if any renames were performed, `Ok(false)` if already sequential.
pub fn renumber_workspaces() -> Result<bool, Box<dyn std::error::Error>> {
    let mut conn = ipc::I3Connection::connect()?;
    let outputs_json = conn.get_outputs()?;
    let workspaces_json = conn.get_workspaces()?;

    let output_order = get_output_spatial_order(&outputs_json);

    let ws_array = match workspaces_json.as_array() {
        Some(arr) => arr,
        None => return Ok(false),
    };

    // Parse workspaces, only consider numbered ones (num > 0)
    let mut workspaces: Vec<(i64, String, bool)> = ws_array
        .iter()
        .filter_map(|ws| {
            let num = ws["num"].as_i64()?;
            if num <= 0 {
                return None;
            }
            let output = ws["output"].as_str().unwrap_or("").to_string();
            let focused = ws["focused"].as_bool().unwrap_or(false);
            Some((num, output, focused))
        })
        .collect();

    // Sort by spatial output order, then by number within each output
    workspaces.sort_by(|a, b| {
        let a_pos = output_order.iter().position(|o| o == &a.1).unwrap_or(usize::MAX);
        let b_pos = output_order.iter().position(|o| o == &b.1).unwrap_or(usize::MAX);
        a_pos.cmp(&b_pos).then(a.0.cmp(&b.0))
    });

    // Compute target numbering: sequential starting from 1
    let mut renames: Vec<(String, String, bool)> = Vec::new(); // (old_name, new_name, is_focused)
    let mut focused_target: Option<i64> = None;

    for (idx, (current_num, _output, focused)) in workspaces.iter().enumerate() {
        let target = (idx + 1) as i64;
        if *current_num != target {
            renames.push((current_num.to_string(), target.to_string(), *focused));
        }
        if *focused {
            focused_target = Some(target);
        }
    }

    if renames.is_empty() {
        return Ok(false);
    }

    log::info!(
        "Sequencer: renumbering {} workspace(s): {}",
        renames.len(),
        renames
            .iter()
            .map(|(old, new, _)| format!("{}->{}", old, new))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Check for collisions: does any target name already exist as a workspace
    // that isn't being renamed away in this batch?
    let current_names: HashSet<&str> = ws_array
        .iter()
        .filter_map(|ws| ws["name"].as_str())
        .collect();
    let sources: HashSet<&str> = renames.iter().map(|(old, _, _)| old.as_str()).collect();

    let has_collision = renames.iter().any(|(_, new, _)| {
        // Target name exists AND isn't a source being renamed away
        current_names.contains(new.as_str()) && !sources.contains(new.as_str())
    });

    if has_collision {
        // Two-pass rename to avoid collisions
        log::debug!("Sequencer: using two-pass rename (collision detected)");
        for (old, new, _) in &renames {
            let cmd = format!("rename workspace \"{}\" to \"_tmp_{}\"", old, new);
            conn.run_command(&cmd)?;
        }
        for (_, new, _) in &renames {
            let cmd = format!("rename workspace \"_tmp_{}\" to \"{}\"", new, new);
            conn.run_command(&cmd)?;
        }
    } else {
        // Single-pass rename in ascending target order (already sorted)
        for (old, new, _) in &renames {
            let cmd = format!("rename workspace \"{}\" to \"{}\"", old, new);
            conn.run_command(&cmd)?;
        }
    }

    // Restore focus to the workspace the user was on
    if let Some(target) = focused_target {
        conn.run_command(&format!("workspace number {}", target))?;
    }

    Ok(true)
}
