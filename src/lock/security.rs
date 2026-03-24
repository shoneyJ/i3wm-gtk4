//! Security hardening for the lock screen.

/// Set OOM score to -1000 so the kernel never kills the lock screen.
/// Requires CAP_SYS_RESOURCE or appropriate permissions.
pub fn set_oom_score() {
    match std::fs::write("/proc/self/oom_score_adj", "-1000") {
        Ok(()) => log::info!("OOM score set to -1000"),
        Err(e) => log::warn!("Failed to set OOM score (may need elevated privileges): {}", e),
    }
}

/// Install a panic hook that execs i3lock as a fallback.
/// Ensures the session is never left unlocked due to a crash.
pub fn set_crash_handler() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("PANIC in i3more-lock: {}", info);
        // Emergency: spawn i3lock so the session stays locked
        let _ = std::process::Command::new("/usr/bin/i3lock")
            .args(["-c", "000000"])
            .spawn();
        default_hook(info);
    }));
}

/// Inhibit VT switching via systemd-logind. Keeps an inhibitor fd open
/// for the lifetime of the process so Ctrl+Alt+F* is blocked.
pub fn inhibit_vt_switch() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::os::unix::io::AsRawFd;

    let conn = zbus::blocking::Connection::system()?;
    let reply = conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1",
        Some("org.freedesktop.login1.Manager"),
        "Inhibit",
        &("handle-switch", "i3more-lock", "Lock screen active", "block"),
    )?;

    let fd: zbus::zvariant::OwnedFd = reply.body().deserialize()?;
    let raw = fd.as_raw_fd();
    // Leak the fd so the inhibitor stays active until process exit
    std::mem::forget(fd);
    log::info!("VT switch inhibitor acquired (fd={})", raw);

    Ok(())
}
