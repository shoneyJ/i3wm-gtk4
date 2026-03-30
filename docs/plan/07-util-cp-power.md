# Power Utility (`i3more-power`)

A Rust binary replacing `powermenu`, `blur-lock`, and `power-profiles` bash scripts. Provides power menu, screen lock, and power profile switching. Uses GTK4 for interactive menus and reuses `i3more::ipc` for i3 logout.

## Motivation

Three related bash scripts handle session/power management. They share a dependency on rofi and have overlapping concerns (the powermenu calls blur-lock). Consolidating into one binary eliminates the rofi dependency and bash overhead, enables direct i3 IPC for logout (no `i3-msg` spawn), and provides a single dependency-checked entry point with native GTK4 UI.

## Architecture

- **Binary**: `i3more-power` (defined in `Cargo.toml` as `[[bin]]`)
- **Entry point**: `src/power_main.rs` (~30 lines)
- **Module**: `src/power/` (5 files — mod, gtk helper, lock, menu, profiles)
- **Shared code**: `i3more::ipc` for logout, `dirs` crate for path resolution, GTK4 for UI
- **Dependencies**: `std` + existing crate deps (gtk4 already in project)
- **Menus**: GTK4 popup windows styled with Gruvbox CSS (same as notification popups)

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
| `src/power/ui.rs` | **Create** — GTK4 popup menu and confirmation dialog |
| `src/power/lock.rs` | **Create** — blur-lock implementation |
| `src/power/menu.rs` | **Create** — powermenu implementation |
| `src/power/profiles.rs` | **Create** — power-profiles switcher |
| `assets/power.css` | **Create** — Gruvbox-themed power menu styles |
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

### GTK4 UI helper (`src/power/ui.rs`)

```rust
/// Show a menu popup centered on screen with icon buttons.
/// Returns the selected option label, or None if cancelled (Escape).
pub fn show_menu(
    app: &gtk4::Application,
    title: &str,
    options: &[(&str, char)],  // (label, FA icon)
) -> Option<String>

/// Show a Yes/No confirmation dialog.
/// Returns true if user confirmed.
pub fn confirm(
    app: &gtk4::Application,
    title: &str,
    message: &str,
) -> bool
```

**Menu layout**: Centered fullscreen-overlay window (semi-transparent black background),
grid of icon buttons (FA glyph + label), Escape to cancel. Same Gruvbox theme as the
notification popups.

```
┌──────────────────────────────────────────────────────────────────────────┐
│                        (semi-transparent overlay)                        │
│                                                                          │
│                    ┌─────────────────────────────────┐                   │
│                    │         Power Menu               │                   │
│                    │                                 │                   │
│                    │  🔒 Lock     🚪 Logout          │                   │
│                    │  🔄 Reboot   ⏻ Shutdown         │                   │
│                    │  💤 Suspend  ❄ Hibernate         │                   │
│                    │                                 │                   │
│                    │         [Cancel]                │                   │
│                    └─────────────────────────────────┘                   │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

**Confirmation dialog**: Same overlay style, with message text + Yes/No buttons.

```
┌──────────────────────────────────────────────────────────────────────────┐
│                        (semi-transparent overlay)                        │
│                                                                          │
│                    ┌─────────────────────────────────┐                   │
│                    │       Shutdown?                  │                   │
│                    │                                 │                   │
│                    │  System will power off.          │                   │
│                    │                                 │                   │
│                    │     [Yes]       [No]            │                   │
│                    └─────────────────────────────────┘                   │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

Both windows use `i3-msg` to float and position (same pattern as notification popups).

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

1. `require_command("systemctl")`
2. Show GTK power menu with options and FA icons:
   ```rust
   let options = [
       ("Lock", fa::LOCK),
       ("Logout", fa::SIGN_OUT),
       ("Suspend", fa::MOON),
       ("Reboot", fa::SYNC),
       ("Shutdown", fa::POWER_OFF),
       ("Hibernate", fa::SNOWFLAKE),
   ];
   ```
3. `ui::show_menu(app, "Power", &options)`
4. Match selection:
   - `"Cancel"` / `None` → exit 0
   - `"Lock"` → `super::lock::run()` (direct function call, not subprocess)
   - `"Logout"` → confirm_and_exec("Logging Out", "i3more::ipc exit")
   - `"Reboot"` → confirm_and_exec("Rebooting", "systemctl reboot")
   - `"Shutdown"` → confirm_and_exec("Shutting Down", "systemctl poweroff")
   - `"Suspend"` → `systemctl suspend` (no confirm — instant, reversible)
   - `"Hibernate"` → confirm_and_exec("Hibernating", "systemctl hibernate")

### Confirmation + Notification Flow

Destructive actions (Shutdown, Reboot, Logout, Hibernate) use a two-step flow:

```
User selects "Shutdown"
    ↓
GTK confirmation dialog: "Shutdown?" [Yes / No]
    ↓ (Yes)
notify-send -u critical "Shutting Down" "System is powering off..."
    ↓ (fire and forget, 2s delay for notification to render)
systemctl poweroff
```

```rust
fn confirm_and_exec(
    app: &gtk4::Application,
    action_label: &str,
    body: &str,
    command: &[&str],
) -> Result<(), Error> {
    // 1. Confirmation via GTK dialog
    if !ui::confirm(app, &format!("{}?", action_label), body) {
        return Ok(()); // cancelled
    }

    // 2. Send notification (fire-and-forget via notify-send)
    let _ = std::process::Command::new("notify-send")
        .args(["-u", "critical", "-t", "10000", action_label, body])
        .spawn();

    // 3. Brief delay so the notification renders before the session dies
    std::thread::sleep(std::time::Duration::from_secs(2));

    // 4. Execute the action
    std::process::Command::new(command[0])
        .args(&command[1..])
        .status()?;

    Ok(())
}
```

### What Happens During `systemctl poweroff`

```
systemctl poweroff
    ↓
systemd (PID 1) receives PowerOff request via logind D-Bus
    ↓
Phase 1: Session teardown
  - logind sends SIGTERM to session leader (i3)
  - i3 exits → i3more gets SIGTERM → graceful shutdown (Phase 6)
  - D-Bus session bus closes
  - PulseAudio, user dbus-daemon stop
    ↓
Phase 2: System service shutdown (reverse dependency order)
  - NetworkManager stops (network goes down)
  - systemd-resolved stops
  - Logging daemons flush and stop
    ↓
Phase 3: Filesystem cleanup
  - All filesystems unmounted or remounted read-only
  - sync() flushes disk buffers
  - dm-crypt / LUKS volumes closed
    ↓
Phase 4: Process cleanup
  - SIGTERM to all remaining processes (90s timeout)
  - SIGKILL to survivors
    ↓
Phase 5: Kernel shutdown
  - reboot(RB_POWER_OFF) syscall
  - ACPI power-off signal to hardware
  - Machine powers off
```

The "Shutting Down" notification is visible for ~2 seconds before systemd kills the session.
Using `-u critical` ensures the notification stays visible (not auto-dismissed).

### Profiles (`src/power/profiles.rs`)

```rust
pub fn run() -> Result<(), Error>
```

1. `require_command("powerprofilesctl")`
2. Get current: `powerprofilesctl get` → trim stdout
3. Show GTK menu with profile options (highlight current):
   ```rust
   let options = [
       ("Performance", fa::BOLT),
       ("Balanced", fa::BALANCE_SCALE),
       ("Power Saver", fa::LEAF),
   ];
   ```
4. `ui::show_menu(app, &format!("Power Profile ({})", current), &options)`
5. If selection differs from current: `powerprofilesctl set {selected}`
6. Fire-and-forget `notify-send` on change

## i3 Config Changes

```bash
# Before
bindsym $mod+Shift+e exec --no-startup-id ~/.config/i3/scripts/powermenu
bindsym $mod+l exec --no-startup-id ~/.config/i3/scripts/blur-lock
exec --no-startup-id xss-lock -l ~/.config/i3/scripts/blur-lock
bindsym $mod+Shift+p exec --no-startup-id ~/.config/i3/scripts/power-profiles

# After (no rofi dependency — GTK4 native UI)
bindsym $mod+Shift+e exec i3more-power menu
bindsym $mod+l exec i3more-power lock
exec --no-startup-id xss-lock -l -- i3more-power lock
bindsym $mod+Shift+p exec i3more-power profiles
```

## Phased Implementation

### Phase 1: Scaffold + GTK UI Helper
1. Create `src/power/mod.rs` with error types and `require_command()`
2. Create `src/power/ui.rs` with `show_menu()` and `confirm()`
3. Create `assets/power.css` with Gruvbox-themed styles
4. Create `src/power_main.rs` with subcommand dispatch
5. Add `pub mod power;` to `src/lib.rs`, `[[bin]]` to `Cargo.toml`
6. Verify it compiles and prints usage

### Phase 2: Lock
1. Implement `src/power/lock.rs`
2. Test: `i3more-power lock` — screen blurs and locks, unlocks on password
3. Test: verify temp files cleaned up after unlock
4. Test: `xss-lock -l -- i3more-power lock` works on lid close

### Phase 3: Menu
1. Implement `src/power/menu.rs`
2. Test: `i3more-power menu` — GTK overlay appears with 6 icon buttons
3. Test: Escape closes menu (cancel)
4. Test: Shutdown shows confirmation dialog → "Shutting Down" notification → poweroff
5. Test: Lock option calls lock directly (no subprocess)
6. Test: Logout sends i3 exit via IPC

### Phase 4: Profiles
1. Implement `src/power/profiles.rs`
2. Test: `i3more-power profiles` — GTK menu shows current profile highlighted
3. Test: switching profile runs `powerprofilesctl set`
4. Test: notification on profile change

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
dist/i3more-power menu     # GTK overlay shows, select Cancel or press Escape

# Profiles
dist/i3more-power profiles # GTK menu shows current profile, select one

# xss-lock integration
xss-lock -l -- dist/i3more-power lock &
# Close laptop lid → screen locks with blur
```

---

## Graceful Shutdown

### Problem

i3More has no graceful shutdown path. When the process exits:

| Resource | Current Behavior | Impact |
|---|---|---|
| D-Bus names (`org.freedesktop.Notifications`, `org.kde.StatusNotifierWatcher`) | Never released | Quick restart can fail; stale names on bus |
| D-Bus match rules | Never removed | Daemon routes signals to dead connections |
| Background threads (notify daemon, tray watcher, i3 IPC) | Orphaned, never joined | No cleanup, abrupt termination |
| i3 IPC socket | Dropped by OS | No explicit unsubscribe |
| X11 grabs (lock screen) | Never released | Potential input stuck if abnormal exit |

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  GTK Application                                                │
│                                                                 │
│  app.connect_shutdown() ─────┐                                  │
│                              │                                  │
│                              ▼                                  │
│                    shutdown_flag.store(true)                     │
│                    (Arc<AtomicBool> shared with all threads)     │
│                              │                                  │
│              ┌───────────────┼───────────────┐                  │
│              ▼               ▼               ▼                  │
│   ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐       │
│   │ notify daemon│ │ tray watcher │ │ i3 IPC listener  │       │
│   │              │ │              │ │                   │       │
│   │ dispatcher   │ │ dispatcher   │ │ loop { recv() }  │       │
│   │  .stop()     │ │  .stop()     │ │  check flag      │       │
│   │  breaks loop │ │  breaks loop │ │  break            │       │
│   │              │ │              │ │                   │       │
│   │ ReleaseName  │ │ ReleaseName  │ │ close socket      │       │
│   │ RemoveMatch  │ │ RemoveMatch  │ │                   │       │
│   └──────┬───────┘ └──────┬───────┘ └────────┬─────────┘       │
│          │                │                   │                 │
│          └────────────────┼───────────────────┘                 │
│                           ▼                                     │
│                   thread.join() (with timeout)                  │
│                           │                                     │
│                           ▼                                     │
│                    process exit                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Phase 6: Graceful Shutdown

#### Step 1: linbus — `Dispatcher::stop()` and `release_name()`

**File**: `linbus/src/dispatch.rs`

Add an `AtomicBool` stop flag to `Dispatcher`:

```rust
pub struct Dispatcher {
    pub conn: Connection,
    handlers: HashMap<(String, String), MethodHandler>,
    properties: HashMap<(String, String), Value>,
    stop_flag: Arc<AtomicBool>,  // NEW
}

impl Dispatcher {
    pub fn stop_handle(&self) -> Arc<AtomicBool> {
        self.stop_flag.clone()
    }

    // In run() loop:
    // if self.stop_flag.load(Ordering::Relaxed) { break; }
}
```

**File**: `linbus/src/bus.rs`

```rust
impl Connection {
    pub fn release_name(&mut self, name: &str) -> Result<(), LinbusError> {
        let msg = Message::method_call(
            "org.freedesktop.DBus", "/org/freedesktop/DBus",
            "org.freedesktop.DBus", "ReleaseName",
        ).with_body(vec![Value::String(name.into())]);
        self.call(&msg, 2000)?;
        Ok(())
    }

    pub fn remove_match(&mut self, rule: &str) -> Result<(), LinbusError> {
        let msg = Message::method_call(
            "org.freedesktop.DBus", "/org/freedesktop/DBus",
            "org.freedesktop.DBus", "RemoveMatch",
        ).with_body(vec![Value::String(rule.into())]);
        self.call(&msg, 2000)?;
        Ok(())
    }
}
```

**File**: `linbus/src/dispatch.rs` — `Drop` impl

```rust
impl Drop for Dispatcher {
    fn drop(&mut self) {
        // Best-effort cleanup — errors are logged but ignored
        for (iface, _) in &self.properties {
            let _ = self.conn.release_name(iface);
        }
    }
}
```

#### Step 2: Background threads accept shutdown signal

**File**: `src/notify/daemon.rs`

```rust
pub fn start_notification_daemon(
    tx: mpsc::Sender<NotifyEvent>,
    shutdown: Arc<AtomicBool>,   // NEW parameter
) -> (mpsc::Sender<(u32, String)>, JoinHandle<()>) {
    let (action_tx, action_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        // ... setup dispatcher ...
        let stop = dispatcher.stop_handle();
        // Link external shutdown flag to dispatcher
        // Check in idle_fn: if shutdown.load() { stop.store(true); }
        dispatcher.run(50, |d| { ... }, |_| {});
    });
    (action_tx, handle)  // Return JoinHandle
}
```

Same pattern for `src/tray/watcher.rs` — return `JoinHandle`, accept `Arc<AtomicBool>`.

#### Step 3: GTK shutdown hook

**File**: `src/main.rs`

```rust
fn main() {
    let shutdown_flag = Arc::new(AtomicBool::new(false));

    // ... in on_activate:
    let notify_handle = notify::start_notification_daemon(tx, shutdown_flag.clone());
    let tray_handle = tray::start_watcher(tray_tx, shutdown_flag.clone());

    app.connect_shutdown(move |_| {
        shutdown_flag.store(true, Ordering::Relaxed);

        // Join threads with timeout
        let _ = notify_handle.join();
        let _ = tray_handle.join();
    });
}
```

#### Step 4: i3 IPC cleanup

**File**: `src/ipc.rs`

The i3 IPC listener thread checks the shutdown flag each iteration:

```rust
loop {
    if shutdown_flag.load(Ordering::Relaxed) { break; }
    // ... existing event read with timeout ...
}
// Socket dropped automatically on thread exit
```

### Files Modified

| File | Change |
|---|---|
| `linbus/src/dispatch.rs` | Add `stop_flag`, `stop_handle()`, check in `run()` loop, `Drop` impl |
| `linbus/src/bus.rs` | Add `release_name()`, `remove_match()` |
| `src/main.rs` | Add `Arc<AtomicBool>` shutdown flag, `connect_shutdown()`, thread joins |
| `src/notify/daemon.rs` | Accept shutdown flag, return `JoinHandle` |
| `src/tray/watcher.rs` | Accept shutdown flag, return `JoinHandle` |
| `src/ipc.rs` | Check shutdown flag in event loop |

### Verification

```bash
# Build
docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more dist/"

# Start i3more, verify threads are running
dist/i3more &
PID=$!

# Check D-Bus name ownership
dbus-send --session --dest=org.freedesktop.DBus --type=method_call --print-reply \
  /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:"org.freedesktop.Notifications"
# Expected: boolean true

# Kill gracefully (SIGTERM)
kill $PID
wait $PID

# Verify name released
dbus-send --session --dest=org.freedesktop.DBus --type=method_call --print-reply \
  /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:"org.freedesktop.Notifications"
# Expected: boolean false

# Quick restart should work without REPLACE_EXISTING
dist/i3more &
# Should start cleanly, no name conflict errors
```
