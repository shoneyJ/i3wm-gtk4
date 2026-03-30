# Power Utility (`i3more-power`)

A Rust binary replacing `powermenu`, `blur-lock`, and `power-profiles` bash scripts. Provides power menu, screen lock, and power profile switching. Uses rofi for interactive menus and reuses `i3more::ipc` for i3 logout.

## Motivation

Three related bash scripts handle session/power management. They share a dependency on rofi and have overlapping concerns (the powermenu calls blur-lock). Consolidating into one binary eliminates bash overhead, enables direct i3 IPC for logout (no `i3-msg` spawn), and provides a single dependency-checked entry point.

## Architecture

- **Binary**: `i3more-power` (defined in `Cargo.toml` as `[[bin]]`)
- **Entry point**: `src/power_main.rs` (~30 lines)
- **Module**: `src/power/` (5 files — mod, rofi helper, lock, menu, profiles)
- **Shared code**: `i3more::ipc` for logout, `dirs` crate for path resolution
- **Dependencies**: `std` + existing crate deps only — no GTK, no new crates
- **Menus**: rofi (not reimplemented — it's purpose-built for this)

### Why keep rofi?

Rofi is a mature, GPU-accelerated dmenu replacement with theme support. Reimplementing its fuzzy search, keyboard navigation, and rendering in GTK would be hundreds of lines with worse UX. The Rust binary's job is orchestration, not UI.

## Subcommands

```
i3more-power menu       # Show power menu (cancel/lock/logout/reboot/shutdown/suspend/hibernate)
i3more-power lock       # Lock screen with blurred screenshot (blocks until unlock)
i3more-power profiles   # Show power profile switcher (performance/balanced/power-saver)
```

## Files

| File | Action |
|------|--------|
| `src/power_main.rs` | **Create** — entry point with subcommand dispatch |
| `src/power/mod.rs` | **Create** — error types, submodule declarations |
| `src/power/rofi.rs` | **Create** — shared rofi invocation helper |
| `src/power/lock.rs` | **Create** — blur-lock implementation |
| `src/power/menu.rs` | **Create** — powermenu implementation |
| `src/power/profiles.rs` | **Create** — power-profiles switcher |
| `src/lib.rs` | **Edit** — add `pub mod power;` |
| `Cargo.toml` | **Edit** — add `[[bin]]` entry |

## Reusable Code

- `i3more::ipc::I3Connection::run_command("exit")` — for logout, avoids spawning `i3-msg`
- `dirs::config_dir()` — for rofi theme path resolution (already a dependency)

## Internal Structure

### Entry point (`src/power_main.rs`)
```
fn main()
    match std::env::args().nth(1).as_deref()
        "menu"     => power::menu::run()
        "lock"     => power::lock::run()
        "profiles" => power::profiles::run()
        _          => print_usage(); exit(1)
```

### Error type (`src/power/mod.rs`)
```rust
pub enum Error {
    CommandNotFound { name: String, hint: String },
    CommandFailed { name: String, stderr: String },
    Io(std::io::Error),
    Ipc(String),
}
```

### Dependency checking
```rust
pub fn require_command(name: &str) -> Result<(), Error>
```
Uses `which {cmd}` to verify. Returns `CommandNotFound` with install hint on failure. Lookup table maps command names to package names (e.g., `"scrot"` → `"sudo apt install scrot"`).

### Shared rofi helper (`src/power/rofi.rs`)
```rust
pub fn rofi_menu(
    prompt: &str,
    options: &[&str],
    theme: Option<&str>,
    message: Option<&str>,
) -> Result<Option<String>, Error>
```
- Builds args: `-dmenu -p {prompt}`
- If `theme` provided and file exists at `~/.config/rofi/{theme}`: adds `-theme {path}`
- If `message` provided: adds `-mesg {message}`
- Pipes options (joined by `\n`) to rofi stdin
- Returns `Ok(None)` on cancel (exit code 1 / empty), `Ok(Some(selection))` on choice

### Lock (`src/power/lock.rs`)

```rust
pub fn run() -> Result<(), Error>
```

1. `require_command("scrot")`, `require_command("convert")`, `require_command("i3lock")`
2. Create temp paths: `/tmp/i3more-screenshot-{pid}.png`, `/tmp/i3more-blur-{pid}.png`
3. `scrot {screenshot_path}` — capture screen
4. `convert {screenshot_path} -blur 5x4 {blur_path}` — gaussian blur
5. `i3lock -i {blur_path}` — **blocks until unlock** (xss-lock compatible)
6. Cleanup: `std::fs::remove_file` both temps (fallback if shred unavailable)

**Critical**: Step 5 blocks. This is required for `xss-lock -l` integration. The process must remain alive while the screen is locked.

### Menu (`src/power/menu.rs`)

```rust
pub fn run() -> Result<(), Error>
```

1. `require_command("rofi")`, `require_command("systemctl")`
2. Define options: `["Cancel", "Lock", "Logout", "Reboot", "Shutdown", "Suspend", "Hibernate"]`
3. `rofi_menu("Power", &options, Some("powermenu.rasi"), None)`
4. Match selection:
   - `"Cancel"` / `None` → exit 0
   - `"Lock"` → `super::lock::run()` (direct function call, not subprocess)
   - `"Logout"` → `i3more::ipc::I3Connection::connect()?.run_command("exit")`
   - `"Reboot"` → `systemctl reboot`
   - `"Shutdown"` → `systemctl poweroff`
   - `"Suspend"` → `systemctl suspend`
   - `"Hibernate"` → `systemctl hibernate`

### Profiles (`src/power/profiles.rs`)

```rust
pub fn run() -> Result<(), Error>
```

1. `require_command("rofi")`, `require_command("powerprofilesctl")`
2. Get current: `powerprofilesctl get` → trim stdout
3. Define options: `["performance", "balanced", "power-saver"]`
4. `rofi_menu("Power Profile", &options, Some("powermenu.rasi"), Some(&format!("Current: {}", current)))`
5. If selection differs from current: `powerprofilesctl set {selected}`
6. Optional: fire-and-forget `notify-send` on change (skip silently if not available)

## i3 Config Changes

```bash
# Before
bindsym $mod+Shift+e exec --no-startup-id ~/.config/i3/scripts/powermenu
bindsym $mod+l exec --no-startup-id ~/.config/i3/scripts/blur-lock
exec --no-startup-id xss-lock -l ~/.config/i3/scripts/blur-lock
bindsym $mod+Shift+p exec --no-startup-id ~/.config/i3/scripts/power-profiles

# After
bindsym $mod+Shift+e exec --no-startup-id i3more-power menu
bindsym $mod+l exec --no-startup-id i3more-power lock
exec --no-startup-id xss-lock -l -- i3more-power lock
bindsym $mod+Shift+p exec --no-startup-id i3more-power profiles
```

## Phased Implementation

### Phase 1: Scaffold + Rofi Helper
1. Create `src/power/mod.rs` with error types and `require_command()`
2. Create `src/power/rofi.rs` with `rofi_menu()`
3. Create `src/power_main.rs` with subcommand dispatch
4. Add `pub mod power;` to `src/lib.rs`, `[[bin]]` to `Cargo.toml`
5. Verify it compiles and prints usage

### Phase 2: Lock
1. Implement `src/power/lock.rs`
2. Test: `i3more-power lock` — screen blurs and locks, unlocks on password
3. Test: verify temp files cleaned up after unlock
4. Test: `xss-lock -l -- i3more-power lock` works on lid close

### Phase 3: Menu
1. Implement `src/power/menu.rs`
2. Test: `i3more-power menu` — rofi appears with all 7 options
3. Test: Lock option calls lock directly (no subprocess)
4. Test: Logout sends i3 exit via IPC

### Phase 4: Profiles
1. Implement `src/power/profiles.rs`
2. Test: `i3more-power profiles` — shows current profile, allows switching
3. Test: notification on profile change

### Phase 5: Integration
1. Build release, copy to `dist/`
2. Update i3 config keybindings
3. Test all power hotkeys end-to-end

## Verification

```bash
# Build
docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more-power dist/"

# Lock (blocks until unlock)
dist/i3more-power lock

# Menu
dist/i3more-power menu     # rofi shows, select Cancel

# Profiles
dist/i3more-power profiles # rofi shows current profile, select one

# xss-lock integration
xss-lock -l -- dist/i3more-power lock &
# Close laptop lid → screen locks with blur
```

## Graceful Shutdown

### Background: Linux Shutdown Sequence

When a shutdown is initiated (via `systemctl poweroff`, power menu, etc.), the sequence is:

1. **logind receives request** — `org.freedesktop.login1.Manager.PowerOff()` via D-Bus
2. **Inhibitor lock check** — apps can hold delay-type locks to postpone briefly (e.g., during downloads/upgrades)
3. **Session manager notifies apps** — EndSession D-Bus signal, ~10s grace period for apps to save state
4. **systemd activates `poweroff.target`** — walks unit dependency graph in reverse (dependents stop first)
5. **Services get SIGTERM, then SIGKILL** — `TimeoutStopSec=90s` default wait before forced kill
6. **Network teardown** — DHCP release, VPN close, interface down
7. **Filesystem unmount** — page cache flush, journal commit, remount root read-only
8. **Kernel `reboot(RB_POWER_OFF)`** → ACPI S5 state → power off

### Critical Finding: i3 Does NOT Signal Child Processes

i3 calls `exit()` directly on `i3-msg exit`. It double-forks with `setsid()` for every `exec` command, so children are in their own session group and receive **no signal** from i3.

What actually kills i3more processes:

| Launch method | What terminates the process |
|---|---|
| `exec` in i3 config (startx/xinit) | X server dies → GTK gets X11 I/O error → crash |
| `exec` in i3 config (display manager) | PAM session cleanup may send SIGTERM, but X usually dies first |
| systemd user service | SIGTERM with configurable timeout, then SIGKILL |

The i3 IPC `shutdown` event exists but fires during `exit()` cleanup — the socket closes immediately after, so it's unreliable as a notification mechanism.

### Current State: No Shutdown Handling

All i3more binaries exit abruptly. No signal handlers, no thread termination, no cleanup. The `nix` crate with `signal` feature is a dependency but unused.

**Resources at risk:**

| Resource | Component | Risk | Cleanup needed? |
|---|---|---|---|
| D-Bus service names | notify daemon, tray watcher | None — auto-released by bus daemon | No |
| X11 grabs (keyboard/pointer) | lock screen | Auto-released by kernel | No |
| X11 cover windows | lock screen | May remain visible on crash | **Yes** |
| i3 IPC socket read | event listener thread | Thread hangs on blocking read | **Yes** |
| Child processes (`pactl subscribe`) | audio widget | May orphan | **Yes** |
| Notification history | notify daemon | In-memory, lost on exit | Optional |
| Temp files (blur screenshots) | lock screen | Leaked on crash | Minor — `/tmp` clears on reboot |
| Debounce `glib::SourceId` timers | main navigator | Stale IDs | Auto-removed by GTK |

### Implementation Plan

#### Signal Handling: `glib::source::unix_signal_add()`

GLib provides native UNIX signal integration into the GTK main loop. It writes to a pipe from the raw signal handler (async-signal-safe), then dispatches the closure on the main thread where GTK/allocator calls are safe.

Do NOT use raw `nix::signal` handlers — they cannot safely call GTK or Rust allocator functions (only async-signal-safe calls allowed in raw handlers).

```rust
// In main(), after app is built but before app.run()
let app_clone = app.clone();
glib::source::unix_signal_add(libc::SIGTERM, move || {
    log::info!("Received SIGTERM, shutting down");
    app_clone.quit();
    glib::ControlFlow::Break
});

let app_clone = app.clone();
glib::source::unix_signal_add(libc::SIGINT, move || {
    log::info!("Received SIGINT, shutting down");
    app_clone.quit();
    glib::ControlFlow::Break
});
```

Requires: `libc` crate (add to `Cargo.toml` if not present, or use `nix::libc`).

#### Shutdown Hook: `app.connect_shutdown()`

GTK4's `Application::shutdown` signal fires synchronously on the main thread after the main loop exits but before `run()` returns. This is where cleanup happens.

```rust
app.connect_shutdown(move |_app| {
    log::info!("Application shutdown signal received");
    SHUTDOWN.store(true, Ordering::Relaxed);
    // Drop receivers to signal channel-based threads
    // Optionally join threads with timeout
});
```

#### Thread Termination: Shared `AtomicBool` + Channel Close

```rust
use std::sync::atomic::{AtomicBool, Ordering};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);
```

**i3 event listener** — already handles channel close correctly:
```rust
// Existing code in start_event_listener:
// tx.send(event) returns Err when receiver is dropped → breaks loop
// Additionally check SHUTDOWN flag for the blocking socket read case
if SHUTDOWN.load(Ordering::Relaxed) { break; }
```

**Notification daemon** — add check to existing 50ms polling loop:
```rust
// In run_daemon() loop body:
if SHUTDOWN.load(Ordering::Relaxed) { break; }
async_io::Timer::after(Duration::from_millis(50)).await;
```

**Tray watcher** — the loader task polls every 500ms, add check there:
```rust
// In loader_task loop body:
if SHUTDOWN.load(Ordering::Relaxed) { break; }
async_io::Timer::after(Duration::from_millis(500)).await;
```

The stream task (`stream.next().await`) will return `None` when the D-Bus connection closes on process exit — no change needed.

#### Shutdown Sequence

```
SIGTERM/SIGINT arrives (or X11 connection dies)
  → glib::unix_signal_add closure fires on GTK main thread
  → calls app.quit()
  → GTK main loop exits
  → app.connect_shutdown() fires
    → SHUTDOWN.store(true, Relaxed)
    → drop mpsc receivers (signals i3 event thread via SendError)
    → optionally join threads with 2s timeout
  → app.run() returns to main()
  → Rust Drop impls run
    → zbus Connection dropped → D-Bus names auto-released by bus daemon
  → process exits cleanly
```

#### Files to Modify

| File | Change |
|---|---|
| `src/main.rs` | Add `SHUTDOWN` AtomicBool, signal handlers, `connect_shutdown`, thread join |
| `src/notify/daemon.rs` | Check `SHUTDOWN` flag in polling loop |
| `src/tray/watcher.rs` | Check `SHUTDOWN` flag in loader task loop |
| `src/lock/x11.rs` | Check `SHUTDOWN` flag; call `destroy_covers()` on exit |
| `src/lock_main.rs` | Add signal handlers (same pattern as main) |
| `Cargo.toml` | Add `libc` dependency if not already present |

#### D-Bus Cleanup: Not Required

The D-Bus specification guarantees name release when the connection file descriptor closes, regardless of how the process exits (clean shutdown, crash, or SIGKILL). No explicit `release_name()` call is needed.

zbus `Connection` behavior on drop: spawns `graceful_shutdown()` in the background, which waits for outstanding method calls to complete before closing the socket. This is sufficient.

#### Optional: systemd User Service

For users who want ordered shutdown with SIGTERM, restart-on-crash, and journalctl logging. Requires manual setup because i3 does not activate `graphical-session.target`.

**`~/.config/systemd/user/i3-session.target`:**
```ini
[Unit]
Description=i3 session
BindsTo=graphical-session.target
```

**`~/.config/systemd/user/i3more.service`:**
```ini
[Unit]
Description=i3more panel
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart=%h/.local/bin/i3more
Restart=on-failure
TimeoutStopSec=5

[Install]
WantedBy=graphical-session.target
```

**i3 config addition:**
```bash
exec --no-startup-id systemctl --user import-environment DISPLAY XAUTHORITY I3SOCK
exec --no-startup-id systemctl --user start i3-session.target
```

Trade-off: adds 3 files of setup complexity but gives proper lifecycle management. Keep `exec` from i3 config as the default; provide systemd files as an optional alternative.

#### Lock Screen: Additional Considerations

The lock screen (`i3more-lock`) has higher-stakes cleanup requirements than other binaries:

- **Cover windows**: Must call `destroy_covers()` before exit to avoid black rectangles persisting on screen after crash
- **X11 grabs**: Auto-released by kernel, but releasing explicitly is cleaner
- **VT switch inhibitor**: Auto-released when logind D-Bus connection closes
- **Password buffer**: Already uses `Zeroizing<String>` — secure on drop

The lock screen should treat `SIGTERM` during active lock as: release grabs → destroy covers → exit. Do NOT unlock on SIGTERM — that would be a security issue. The fallback `i3lock` spawn on error already handles the crash case.