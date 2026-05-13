//! i3more-back — focus the previously-focused window.
//!
//! Reads the MRU list maintained by the main `i3more` binary and asks i3 to
//! focus `entries[1]` (entries[0] is the currently-focused window). Press
//! the bound key twice and you naturally return to where you started.

use i3more::ipc::I3Connection;
use i3more::mru;

fn main() {
    i3more::init_logging("i3more-back");

    let entries = mru::load();
    if entries.len() < 2 {
        log::info!("MRU has fewer than 2 entries; nothing to focus");
        return;
    }

    let target = &entries[1];
    log::info!(
        "Focusing previous window: con_id={} class={:?} title={:?}",
        target.con_id,
        target.class,
        target.title
    );

    let cmd = format!("[con_id=\"{}\"] focus", target.con_id);
    match I3Connection::connect() {
        Ok(mut conn) => {
            if let Err(e) = conn.run_command(&cmd) {
                log::error!("Failed to send focus command: {}", e);
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => {
            log::error!("Failed to connect to i3: {}", e);
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
