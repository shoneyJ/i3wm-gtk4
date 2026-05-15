//! `i3more-layout` — apply a layout to the focused workspace AND every
//! non-leaf container inside it. Bound from i3 config so keyboard
//! shortcuts produce the same workspace-wide rearrange as clicking the
//! layout glyph on the i3More bar.
//!
//! Usage:
//!   i3more-layout splith
//!   i3more-layout splitv
//!   i3more-layout tabbed
//!   i3more-layout stacking
//!   i3more-layout toggle           # cycles splith ↔ splitv based on the
//!                                  # focused workspace's current layout

use i3more::ipc::I3Connection;
use i3more::layout_cmd::{build_cascade_command, focused_workspace};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        usage(&args[0]);
        std::process::exit(2);
    }
    let arg = args[1].as_str();
    if !matches!(arg, "splith" | "splitv" | "tabbed" | "stacking" | "toggle") {
        eprintln!("invalid layout: {}", arg);
        usage(&args[0]);
        std::process::exit(2);
    }

    let mut conn = match I3Connection::connect() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("connect to i3 failed: {}", e);
            std::process::exit(1);
        }
    };

    let tree = match conn.get_tree() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("get_tree failed: {}", e);
            std::process::exit(1);
        }
    };

    let resolved = match arg {
        "toggle" => match focused_workspace(&tree).map(|(_, l)| l).as_deref() {
            Some("splitv") => "splith",
            // splith, tabbed, stacked, or unknown all toggle TO splitv
            _ => "splitv",
        },
        other => other,
    };

    let cmd = build_cascade_command(Some(&tree), resolved);
    if let Err(e) = conn.run_command(&cmd) {
        eprintln!("run_command failed: {}", e);
        std::process::exit(1);
    }
}

fn usage(argv0: &str) {
    eprintln!(
        "usage: {} <splith|splitv|tabbed|stacking|toggle>",
        argv0
    );
}
