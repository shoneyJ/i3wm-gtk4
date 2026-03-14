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
