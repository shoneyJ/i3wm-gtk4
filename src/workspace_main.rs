//! i3more-workspace — workspace manipulation commands for i3.
//!
//! Subcommands:
//!   move-next   Move focused container to the next sequential workspace number.

use i3more::ipc::I3Connection;

fn main() {
    i3more::init_logging("i3more-workspace");

    match std::env::args().nth(1).as_deref() {
        Some("move-next") => {
            if let Err(e) = move_next() {
                log::error!("move-next failed: {}", e);
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("Usage: i3more-workspace <command>");
            eprintln!("Commands:");
            eprintln!("  move-next   Move focused container to the next sequential workspace");
            std::process::exit(1);
        }
    }
}

/// Move the focused container to workspace max(num) + 1.
/// The sequencer running in i3more will re-sequence numbers afterward.
fn move_next() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = I3Connection::connect()?;
    let workspaces = conn.get_workspaces()?;

    let max_num = workspaces
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|ws| ws["num"].as_i64())
        .filter(|n| *n > 0)
        .max()
        .unwrap_or(0);

    let target = max_num + 1;
    let cmd = format!("move container to workspace number {}", target);
    log::info!("Moving focused container to workspace {}", target);
    conn.run_command(&cmd)?;

    Ok(())
}
