# Brightness Utility (`i3more-brightness`)

A pure-CLI Rust binary replacing the brightness commands from `volume_brightness.sh`. Bound to `XF86MonBrightness{Up,Down}` media keys — startup latency is the critical constraint.

## Motivation

Brightness hotkeys are frequently pressed (especially on laptops). The current bash script spawns bash -> brightnessctl -> notify-send per keypress. A native Rust binary eliminates bash startup and calls `brightnessctl` directly with fire-and-forget notification.

Separated from `i3more-audio` because brightness control is a display concern, not an audio concern. Different hardware (backlight controller vs sound card), different dependencies (`brightnessctl` vs `pactl`), and different availability (brightness is laptop-only, audio is universal).

## Architecture

- **Binary**: `i3more-brightness` (defined in `Cargo.toml` as `[[bin]]`)
- **Entry point**: `src/brightness_main.rs` (~80 lines, single file)
- **Dependencies**: `std` only — no GTK, no async, no config file I/O
- **Notification**: fire-and-forget `Command::spawn()` of `notify-send` (don't wait for exit)

## Subcommands

```
i3more-brightness up    # Increase brightness by STEP, cap at 100%
i3more-brightness down  # Decrease brightness by STEP, floor at MIN
```

## Constants

| Name              | Value | Notes                       |
| ----------------- | ----- | --------------------------- |
| `BRIGHTNESS_STEP` | 5     | Matches original script     |
| `MIN_BRIGHTNESS`  | 5     | Prevents screen going black |
| `NOTIFICATION_TIMEOUT` | 1000 | 1 second OSD            |

No config file. Env var overrides (`I3MORE_BRIGHTNESS_STEP`, `I3MORE_MIN_BRIGHTNESS`) can be added later if needed.

## Files

| File                     | Action                         |
| ------------------------ | ------------------------------ |
| `src/brightness_main.rs` | **Create** — entire binary     |
| `Cargo.toml`             | **Edit** — add `[[bin]]` entry |

## Reusable Code

- `src/notify/widgets/backlight.rs` — existing backlight widget reads brightness via sysfs. The CLI binary uses `brightnessctl` instead (simpler, handles permissions).
- Shared `notify()` and `run_cmd()` helper patterns from `i3more-audio` — same fire-and-forget approach. (Duplicated, not shared via lib, since these are tiny helpers and adding a shared module for 10 lines is over-engineering.)

## Internal Structure

### Entry point

```
fn main()
    match std::env::args().nth(1).as_deref()
        "up"   => brightness_up()
        "down" => brightness_down()
        _      => print_usage(); exit(1)
```

### Error type

```rust
enum Error { CommandFailed(String), ParseError(String) }
```

### Helpers

- `run_cmd(cmd, args) -> Result<String, Error>` — runs Command, returns stdout
- `notify(summary, body, sync_key) -> Result<(), Error>` — fire-and-forget spawn of `notify-send` with `-t 1000 -h string:x-canonical-private-synchronous:{sync_key}` using sync key `brightness`

### Brightness functions

- `get_brightness_percent() -> Result<u32>` — runs `brightnessctl get` and `brightnessctl max`, computes `100 * current / max`
- `brightness_up()` — read current %, compute `min(current + STEP, 100)`, run `brightnessctl set {target}%`, notify with percentage
- `brightness_down()` — read current %, compute `max(current - STEP, MIN_BRIGHTNESS)`, run `brightnessctl set {target}%`, notify with percentage

### Brightness icon

Uses icon name `display-brightness-symbolic` or falls back to a text-based notification body like `"Brightness: 65%"`.

## i3 Config Changes

```bash
# Before
bindsym XF86MonBrightnessUp exec --no-startup-id ~/.config/i3/scripts/volume_brightness.sh brightness_up
bindsym XF86MonBrightnessDown exec --no-startup-id ~/.config/i3/scripts/volume_brightness.sh brightness_down

# After
bindsym XF86MonBrightnessUp exec --no-startup-id i3more-brightness up
bindsym XF86MonBrightnessDown exec --no-startup-id i3more-brightness down
```

## Phased Implementation

### Phase 1: Scaffold

1. Create `src/brightness_main.rs` with `main()`, usage printer, error type, `run_cmd` helper
2. Add `[[bin]]` entry to `Cargo.toml`
3. Verify it compiles and prints usage

### Phase 2: Brightness Control

1. Implement `get_brightness_percent()` using `brightnessctl get` and `brightnessctl max`
2. Implement `notify()` fire-and-forget helper
3. Implement `brightness_up` with ceiling clamp at 100%
4. Implement `brightness_down` with floor clamp at `MIN_BRIGHTNESS`
5. Test: `i3more-brightness up` — brightness changes, notification appears

### Phase 3: Integration

1. Build release, copy to `dist/`
2. Update i3 config keybindings
3. Test hotkeys work end-to-end
4. `time i3more-brightness up` — verify <30ms

## Edge Cases

| Scenario                        | Handling                                              |
| ------------------------------- | ----------------------------------------------------- |
| `brightnessctl` not installed   | `run_cmd` returns `CommandFailed`, printed to stderr   |
| No backlight device (desktop)   | `brightnessctl` exits with error, printed to stderr    |
| Brightness already at min/max   | Clamped — no error, notification shows current value   |
| Multiple backlight devices      | `brightnessctl` uses the first by default; acceptable  |

## Verification

```bash
# Build
docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more-brightness dist/"

# Brightness
dist/i3more-brightness up     # notification: "Brightness 55%"
dist/i3more-brightness down   # notification: "Brightness 50%"

# Clamping
# Set brightness to 100%, then run up again — should stay at 100%
# Set brightness to 5%, then run down again — should stay at 5%

# Performance
time dist/i3more-brightness up  # should be <30ms total
```

## Performance — Brightness Widget Event-Driven Updates

### Problem

The existing backlight widget (`src/notify/widgets/backlight.rs`) likely polls sysfs on an interval to read brightness values. This is the same polling pattern that caused CPU waste in the EWW setup.

### EWW brightness script — what it got right

The EWW script `get_brightness_listen.sh` used `inotifywait -m /sys/class/backlight/*/brightness` to watch the sysfs file. It only emits output when brightness actually changes — **zero CPU between changes**. This is the correct event-driven approach.

### Recommended approach for i3more backlight widget

Replace any polling in `src/notify/widgets/backlight.rs` with event-driven sysfs monitoring:

#### Option A: Native inotify (preferred)

Use Rust's `inotify` crate to watch `/sys/class/backlight/*/brightness` directly — no process spawning at all:

```rust
// Watch brightness sysfs file for MODIFY events
// On change: read new value, update widget label
// Zero CPU when idle, instant updates
```

#### Option B: `inotifywait` subprocess

Spawn `inotifywait -m /sys/class/backlight/*/brightness` as a long-lived child process and parse its stdout, similar to how the audio widget uses `pactl subscribe`.

### Impact

| Component | Current | Proposed | Saving |
|-----------|---------|----------|--------|
| Brightness widget | Polls sysfs every Ns | `inotify` watch (native) | Zero idle CPU, instant updates |

### Integration with phased implementation

This is a widget-level improvement to `src/notify/widgets/backlight.rs`, separate from the CLI binary. It can be implemented after the CLI binary is working, as an enhancement to the existing backlight widget in the notification panel.

---

## Backlight Hardware Context

- **Controller**: Managed by kernel backlight subsystem (`/sys/class/backlight/`)
- **Tool**: `brightnessctl` handles permission elevation and device selection
- **Absolute setting**: The utility reads current percentage, computes target with clamping, and sets absolute value — avoids `brightnessctl set +5%` which doesn't respect min/max bounds cleanly
- **Desktop systems**: No backlight device exists; `brightnessctl` will fail gracefully and the keybinding simply does nothing (XF86MonBrightness keys typically don't exist on desktop keyboards anyway)
