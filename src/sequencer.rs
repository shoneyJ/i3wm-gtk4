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

/// Focus the next workspace on the same output after an empty event.
/// Prefers the next-higher workspace; falls back to previous-lower.
/// Returns Ok(true) if focus was changed, Ok(false) if no candidate found.
pub fn focus_next_on_output(empty_output: &str, empty_num: i64) -> Result<bool, Box<dyn std::error::Error>> {
    let mut conn = ipc::I3Connection::connect()?;
    let workspaces_json = conn.get_workspaces()?;

    let ws_array = match workspaces_json.as_array() {
        Some(arr) => arr,
        None => return Ok(false),
    };

    // Collect workspaces on the same output, excluding the empty one
    let mut same_output: Vec<i64> = ws_array
        .iter()
        .filter_map(|ws| {
            let num = ws["num"].as_i64()?;
            let output = ws["output"].as_str()?;
            if output == empty_output && num != empty_num && num > 0 {
                Some(num)
            } else {
                None
            }
        })
        .collect();

    same_output.sort();

    // Prefer next-higher, fall back to previous-lower
    let target = same_output.iter().find(|&&n| n > empty_num)
        .or_else(|| same_output.iter().rev().find(|&&n| n < empty_num));

    if let Some(&target_num) = target {
        log::info!("Auto-focus: ws {} (empty ws {} on {})", target_num, empty_num, empty_output);
        conn.run_command(&format!("workspace number {}", target_num))?;
        Ok(true)
    } else {
        log::debug!("Auto-focus: no other workspace on output {}", empty_output);
        Ok(false)
    }
}

/// Recover orphaned `_tmp_N` workspaces left behind by an interrupted two-pass rename.
/// These workspaces have `num: -1` in i3 because the name is non-numeric.
/// Returns the number of workspaces recovered.
fn recover_tmp_workspaces(conn: &mut ipc::I3Connection, ws_array: &[Value]) -> usize {
    let mut recovered = 0;
    for ws in ws_array {
        let name = match ws["name"].as_str() {
            Some(n) => n,
            None => continue,
        };
        if let Some(target) = name.strip_prefix("_tmp_") {
            // Verify the target is a valid number before renaming
            if target.parse::<i64>().is_ok() {
                let cmd = format!("rename workspace \"{}\" to \"{}\"", name, target);
                match conn.run_command(&cmd) {
                    Ok(_) => {
                        log::info!("Sequencer: recovered orphaned workspace {} -> {}", name, target);
                        recovered += 1;
                    }
                    Err(e) => {
                        log::error!("Sequencer: failed to recover workspace {}: {}", name, e);
                    }
                }
            }
        }
    }
    recovered
}

/// Renumber workspaces to be sequential across monitors (left monitor gets lower numbers).
///
/// Returns `Ok(true)` if any renames were performed, `Ok(false)` if already sequential.
pub fn renumber_workspaces() -> Result<bool, Box<dyn std::error::Error>> {
    let mut conn = ipc::I3Connection::connect()?;

    // First, recover any orphaned _tmp_ workspaces from a previous interrupted rename
    let initial_ws = conn.get_workspaces()?;
    if let Some(arr) = initial_ws.as_array() {
        let recovered = recover_tmp_workspaces(&mut conn, arr);
        if recovered > 0 {
            log::info!("Sequencer: recovered {} orphaned _tmp_ workspace(s), re-querying state", recovered);
        }
    }

    // Re-query state (may have changed after recovery)
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

    // Simulate single-pass execution to detect conflicts.
    // Walk renames in order: remove source name, check if target is occupied, add target.
    // If any target is still occupied at execution time, we need two-pass.
    let mut occupied: HashSet<String> = ws_array
        .iter()
        .filter_map(|ws| ws["name"].as_str().map(String::from))
        .collect();

    let has_collision = renames.iter().any(|(old, new, _)| {
        occupied.remove(old.as_str());
        let conflict = occupied.contains(new.as_str());
        occupied.insert(new.clone());
        conflict
    });

    if has_collision {
        // Two-pass rename to avoid collisions.
        // Pass 1: rename all to _tmp_ prefix.
        // Pass 2: rename _tmp_ back to final names.
        // Both passes continue on error to avoid leaving partial _tmp_ state.
        log::debug!("Sequencer: using two-pass rename (collision detected)");
        let mut pass1_ok = true;
        for (old, new, _) in &renames {
            let cmd = format!("rename workspace \"{}\" to \"_tmp_{}\"", old, new);
            if let Err(e) = conn.run_command(&cmd) {
                log::error!("Sequencer: pass 1 failed for {} -> _tmp_{}: {}", old, new, e);
                pass1_ok = false;
                // Try to reconnect and continue
                if let Ok(new_conn) = ipc::I3Connection::connect() {
                    conn = new_conn;
                }
            }
        }
        // Always attempt pass 2 to clean up _tmp_ workspaces, even if pass 1 had errors
        for (_, new, _) in &renames {
            let cmd = format!("rename workspace \"_tmp_{}\" to \"{}\"", new, new);
            if let Err(e) = conn.run_command(&cmd) {
                log::error!("Sequencer: pass 2 failed for _tmp_{} -> {}: {}", new, new, e);
                // Try to reconnect and continue
                if let Ok(new_conn) = ipc::I3Connection::connect() {
                    conn = new_conn;
                    // Retry this rename with the new connection
                    let retry_cmd = format!("rename workspace \"_tmp_{}\" to \"{}\"", new, new);
                    if let Err(e2) = conn.run_command(&retry_cmd) {
                        log::error!("Sequencer: retry also failed for _tmp_{} -> {}: {}", new, new, e2);
                    }
                }
            }
        }
        if !pass1_ok {
            log::warn!("Sequencer: two-pass rename had errors; recovery will run on next invocation");
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
