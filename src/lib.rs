//! Shared modules for i3More binaries.

pub mod css;
pub mod fa;
pub mod icon;
pub mod ipc;
pub mod launcher;
pub mod translate;

use std::path::PathBuf;

/// Initialize file-based logging for an i3More binary.
///
/// Writes to `~/.cache/i3more/<name>.log` in append mode.
/// Respects the `RUST_LOG` env var for filtering; defaults to `info`.
pub fn init_logging(name: &str) {
    let log_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("i3more");
    let _ = std::fs::create_dir_all(&log_dir);

    let log_path = log_dir.join(format!("{}.log", name));

    // Truncate if over 1 MB to prevent unbounded growth
    if let Ok(meta) = std::fs::metadata(&log_path) {
        if meta.len() > 1_000_000 {
            let _ = std::fs::write(&log_path, b"");
        }
    }

    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(_) => {
            // Fall back to stderr-only logging
            env_logger::init();
            return;
        }
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(file)))
        .init();
}
