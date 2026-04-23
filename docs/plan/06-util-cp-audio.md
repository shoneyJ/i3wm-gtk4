# Audio Utility (`i3more-audio`)

A pure-CLI Rust binary replacing `volume_brightness.sh` (volume commands only) and `audio-device-switch` bash scripts. Bound to media keys — startup latency is the critical constraint. Includes plans for an audio control panel widget, headset jack detection, and configurable device filtering.

## Motivation

Volume hotkeys are the most frequently pressed keys that invoke scripts. The current bash scripts spawn 3-5 processes per keypress (bash -> pactl -> notify-send). A native Rust binary eliminates bash startup and reduces the process chain to direct `pactl` calls with fire-and-forget notification.

## Architecture

- **Binary**: `i3more-audio` (defined in `Cargo.toml` as `[[bin]]`)
- **Entry point**: `src/audio_main.rs` (~200 lines, single file)
- **Dependencies**: `std` only — no GTK, no async, no config file I/O
- **Notification**: fire-and-forget `Command::spawn()` of `notify-send` (don't wait for exit)

### Why not D-Bus for notifications?

Using `zbus` as a D-Bus client requires async runtime initialization (~10-30ms). Spawning `notify-send` without waiting adds ~1ms fork overhead. The i3more daemon already handles the `x-canonical-private-synchronous` hint for OSD replacement.

### Why pactl only (no pacmd)?

`pactl` works identically under both PulseAudio and PipeWire-pulse. The original script's PipeWire detection logic (`pgrep pipewire`) and `pacmd` fallback are unnecessary.

## Subcommands

```
i3more-audio volume-up      # Increase volume by STEP, cap at MAX
i3more-audio volume-down    # Decrease volume by STEP
i3more-audio volume-mute    # Toggle mute
i3more-audio audio-switch   # Cycle to next preferred audio output device
```

## Constants

| Name                   | Value | Notes                   |
| ---------------------- | ----- | ----------------------- |
| `VOLUME_STEP`          | 1     | Matches original script |
| `MAX_VOLUME`           | 100   | Prevents overdrive      |
| `NOTIFICATION_TIMEOUT` | 1000  | 1 second OSD            |

## Files

| File                | Action                         |
| ------------------- | ------------------------------ |
| `src/audio_main.rs` | **Create** — entire binary     |
| `Cargo.toml`        | **Edit** — add `[[bin]]` entry |

## Reusable Code

- `src/notify/widgets/volume.rs:130-163` — `read_volume()` and `is_muted()` functions use the same `pactl` parsing pattern. Duplicate the logic (don't import — the widget module is private to the main binary and pulls in GTK).

## Internal Structure

### Entry point

```
fn main()
    match std::env::args().nth(1).as_deref()
        "volume-up"    => volume_up()
        "volume-down"  => volume_down()
        "volume-mute"  => volume_mute()
        "audio-switch" => audio_switch()
        _              => print_usage(); exit(1)
```

### Error type

```rust
enum Error { CommandFailed(String), ParseError(String) }
```

### Helpers

- `run_cmd(cmd, args) -> Result<String, Error>` — runs Command, returns stdout
- `notify(summary, body, sync_key) -> Result<(), Error>` — fire-and-forget spawn of `notify-send` with `-t 1000 -h string:x-canonical-private-synchronous:{sync_key}`
- `volume_icon(volume, muted) -> &'static str` — returns `audio-volume-{muted,low,medium,high}`

### Volume functions

- `get_volume() -> Result<u32>` — `pactl get-sink-volume @DEFAULT_SINK@`, parse first `%`
- `is_muted() -> Result<bool>` — `pactl get-sink-mute @DEFAULT_SINK@`, check for "yes"
- `volume_up()` — read current, compute `min(current + STEP, MAX)`, set absolute, notify
- `volume_down()` — `pactl set-sink-volume @DEFAULT_SINK@ -{STEP}%`, notify
- `volume_mute()` — `pactl set-sink-mute @DEFAULT_SINK@ toggle`, notify

### Audio switch

- `list_sinks() -> Result<Vec<Sink>>` — parse `pactl list short sinks`
- `get_default_sink() -> Result<String>` — `pactl get-default-sink`
- `audio_switch()` — list -> filter by config -> find current -> cycle next -> `pactl set-default-sink` -> move all sink-inputs -> notify

### Sink struct

```rust
struct Sink { id: u32, name: String }
```

## i3 Config Changes

```bash
# Before
bindsym XF86AudioRaiseVolume exec --no-startup-id ~/.config/i3/scripts/volume_brightness.sh volume_up
bindsym XF86AudioLowerVolume exec --no-startup-id ~/.config/i3/scripts/volume_brightness.sh volume_down
bindsym XF86AudioMute exec --no-startup-id ~/.config/i3/scripts/volume_brightness.sh volume_mute
bindsym $mod+p exec --no-startup-id ~/.config/i3/scripts/audio-device-switch

# After
bindsym XF86AudioRaiseVolume exec --no-startup-id i3more-audio volume-up
bindsym XF86AudioLowerVolume exec --no-startup-id i3more-audio volume-down
bindsym XF86AudioMute exec --no-startup-id i3more-audio volume-mute
bindsym $mod+p exec --no-startup-id i3more-audio audio-switch
```

---

## Audio Settings Configuration

### Problem

The system has many audio output/input devices — built-in speakers, HDMI/DisplayPort outputs (DP-1, DP-2, DP-3), USB headsets (Logitech Zone Wired, Jabra Evolve 85), and the analog 3.5mm combo jack. When cycling with `audio-switch`, the user is forced to scroll through DisplayPort sinks they never use. Only a few devices are relevant day-to-day.

### Solution: Config file with preferred devices

A JSON config at `~/.config/i3more/audio.json` that controls which devices appear in the `audio-switch` cycle and in the control panel widget.

### Config format

```json
{
  "preferred_sinks": [
    "alsa_output.usb-Logitech_Zone_Wired-*",
    "alsa_output.usb-GN_Audio_Jabra_EVOLVE*",
    "alsa_output.pci-*analog-stereo"
  ],
  "preferred_sources": [
    "alsa_input.usb-Logitech_Zone_Wired-*",
    "alsa_input.usb-GN_Audio_Jabra_EVOLVE*"
  ],
  "excluded_sinks": ["alsa_output.pci-*hdmi-stereo*"],
  "excluded_sources": ["alsa_output.pci-*hdmi-stereo*"]
}
```

### Matching rules

- Patterns use simple glob matching (`*` wildcard) against PulseAudio sink/source names from `pactl list short sinks`
- If `preferred_sinks` is non-empty: only cycle through devices matching at least one pattern, in the order listed
- If `preferred_sinks` is empty but `excluded_sinks` is non-empty: cycle through all devices except those matching exclusion patterns
- If no config file exists: cycle through all devices (current behavior, no regression)

### Config file location

`~/.config/i3more/audio.json` — follows the existing pattern from `src/translate.rs:75-102` which uses `dirs::config_dir().join("i3more").join("translate.json")`.

### Implementation approach

- `audio_main.rs` loads config once at startup (single `read_to_string` + `serde_json::from_str`)
- Config is optional — missing file or parse failure falls back to "all devices" behavior
- `list_sinks()` applies the filter after enumeration
- Config is read-only for the CLI binary; the control panel widget (Phase 5) provides a UI to edit it

### Files

| File                | Action                                           |
| ------------------- | ------------------------------------------------ |
| `src/audio_main.rs` | **Edit** — add config loading and sink filtering |

No new dependencies — `serde` and `serde_json` already in Cargo.toml.

---

## User Interface — Audio Control Panel Widget

### Problem

The current volume widget (`src/notify/widgets/volume.rs`) only controls volume and mute for `@DEFAULT_SINK@`. There is no way to see which device is active, switch devices, or manage input devices without launching `pavucontrol`.

### Solution: Expand the volume widget into an audio control panel

Enhance the existing widget in the notification panel (accessible from the i3More bar bell icon) to include device selection alongside volume control.

### Current state

The volume widget (57 lines of widget code in `volume.rs`) provides:

- Volume slider (0-100%) with mute toggle
- Polls `pactl` every 2 seconds for external changes
- Embedded in the notification panel's `widget_box` between header and notification list

### Planned layout

```
+--------------------------------------------+
| 🔊  Volume                                |  Section header (existing)
+--------------------------------------------+
| [🔇] [===========|-------] 64%            |  Mute + slider + pct (existing)
+--------------------------------------------+
| Output: Logitech Zone Wired          [v]  |  Output device dropdown (new)
+--------------------------------------------+
| Input:  Jabra EVOLVE 85 Mic          [v]  |  Input device dropdown (new)
+--------------------------------------------+
```

### Implementation

- **Output dropdown**: `gtk4::DropDown` populated from `pactl list sinks` (filtered by config)
- **Input dropdown**: `gtk4::DropDown` populated from `pactl list sources` (filtered by config, excluding monitor sources)
- **Selection change**: Calls `pactl set-default-sink {name}` / `pactl set-default-source {name}` and migrates active streams
- **Polling**: Existing 2-second poll also refreshes device list and current selection
- **Config interaction**: Dropdown only shows devices matching `preferred_sinks`/`preferred_sources` or excluding `excluded_sinks`/`excluded_sources` from config. If no config, shows all.

### Display names

`pactl list sinks` returns technical names like `alsa_output.usb-Logitech_Zone_Wired-00.analog-stereo`. The dropdown should show the human-readable `description` field instead. Parse from `pactl --format=json list sinks` (available on PipeWire) or fallback to parsing verbose `pactl list sinks` output for the `Description:` line.

### Files

| File                           | Action                                                    |
| ------------------------------ | --------------------------------------------------------- |
| `src/notify/widgets/volume.rs` | **Edit** — add device dropdowns below slider              |
| `assets/notification.css`      | **Edit** — add styles for `.widget-audio-device` dropdown |

### Future extensibility

This audio section of the control panel is the foundation for adding more system controls later (e.g., Bluetooth device management, display settings). The widget architecture in `src/notify/widgets/` already supports this — each widget is a module returning a `gtk4::Box`, appended to the panel's `widget_box`.

---

## Audio Jack — Headset Detection

### Problem

On GNOME, plugging in a 3.5mm headset triggers a popup asking whether the device is a headset (with mic) or headphones (without mic). This popup doesn't appear in i3 because it's provided by `gnome-shell` / `gnome-control-center`, which aren't running.

### System context

- **Sound card**: sof-hda-dsp (Intel SOF, Raptor Lake)
- **Combo jack**: Single 3.5mm TRRS jack for headphones + mic
- **Known issue**: sof-hda-dsp doesn't auto-detect headset mode on combo jacks — the kernel pin configuration defaults to headphones-only, so the mic input never appears

### Detection mechanisms on Linux

| Mechanism                        | How it works                                             | Suitability                               |
| -------------------------------- | -------------------------------------------------------- | ----------------------------------------- |
| `pactl subscribe`                | Listens for PulseAudio/PipeWire device add/remove events | Best for detecting new devices appearing  |
| WirePlumber D-Bus signals        | Real-time node creation/removal events                   | Most responsive, but requires async D-Bus |
| Polling `pactl list short sinks` | Periodic enumeration, diff against previous snapshot     | Simple, 1-2s latency, reliable            |
| udev rules                       | Kernel-level jack switch events                          | Not reliably exposed on sof-hda-dsp       |
| ACPI events (`acpi_listen`)      | Hardware events for jack insertion                       | Inconsistent across hardware              |

### Why GNOME works

1. ALSA/kernel detects jack insertion
2. PipeWire/WirePlumber creates new device nodes
3. `gnome-shell` has a built-in audio service that listens for these D-Bus events
4. It shows a dialog via `org.gnome.Shell` asking headset vs headphones
5. Based on user selection, it configures the jack pin (headset mic enabled or not)

i3 has no equivalent service.

### Proposed approach: `pactl subscribe` listener in the i3more main binary

**Phase 1 — Device change notifications:**

- The main i3more process (already running as a GTK app with event loop) spawns `pactl subscribe` as a background child process
- Parse its stdout for `Event 'new' on sink` / `Event 'remove' on sink` lines
- On new device: show a notification like "Audio device connected: Jabra Evolve 85"
- On remove: show "Audio device disconnected: Jabra Evolve 85"

**Phase 2 — Headset mode popup:**

- When a new analog input source appears (matching `*analog*` or `*headset*` pattern), show a popup with two buttons:
  - "Headset (with mic)" — runs `pactl set-card-profile {card} output:analog-stereo+input:analog-stereo` (or equivalent WirePlumber config)
  - "Headphones only" — keeps current profile (output only)
- This replaces the GNOME popup functionality

**Phase 3 — Kernel-level fix (optional, permanent):**

- Use `hdajackretask` to override the combo jack pin assignment to "Headset Mic"
- Or add kernel module hint: `options snd_sof_intel_hda_common hda_model=dell-headset-multi`
- After this fix, the mic appears automatically and the popup becomes informational only

### Implementation location

| Component                  | File                                                                        | Notes                                             |
| -------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------- |
| `pactl subscribe` listener | `src/notify/widgets/volume.rs` or new `src/notify/widgets/audio_monitor.rs` | Runs in background, integrated with GTK main loop |
| Device change notification | Uses existing `notify::popup` infrastructure                                | Same notification system as all other popups      |
| Headset mode popup         | New popup type in `src/notify/popup.rs`                                     | Two-button dialog, calls `pactl set-card-profile` |

### Research needed

- Test `pactl subscribe` output format when plugging/unplugging the 3.5mm jack on this specific hardware
- Test `pactl list cards` to find the correct card profile names for headset vs headphones mode
- Verify whether `pactl set-card-profile` is sufficient or if WirePlumber-specific configuration is needed

---

## Phased Implementation

### Phase 1: Scaffold

1. **Create `src/audio_main.rs`**
   - Define `fn main()` with `std::env::args().nth(1)` subcommand dispatch
   - Add `print_usage()` function listing all subcommands
   - Define `enum Error { CommandFailed(String), ParseError(String) }`
   - Implement `run_cmd(cmd: &str, args: &[&str]) -> Result<String, Error>` — spawns `Command`, captures stdout, returns trimmed output or error with stderr
   - Implement `notify(summary: &str, body: &str, sync_key: &str) -> Result<(), Error>` — fire-and-forget `Command::spawn()` of `notify-send` with `-t 1000 -h string:x-canonical-private-synchronous:{sync_key}`, returns immediately without waiting
2. **Add `[[bin]]` entry to `Cargo.toml`**
   - `name = "i3more-audio"`, `path = "src/audio_main.rs"`
3. **Verify**
   - `cargo build --bin i3more-audio` compiles cleanly
   - `./target/debug/i3more-audio` prints usage and exits with code 1
   - `./target/debug/i3more-audio volume-up` prints "not implemented" placeholder

### Phase 2: Volume Control

1. **Implement volume readers**
   - `get_volume() -> Result<u32, Error>` — runs `pactl get-sink-volume @DEFAULT_SINK@`, finds first `N%` with regex or string search, parses to u32
   - `is_muted() -> Result<bool, Error>` — runs `pactl get-sink-mute @DEFAULT_SINK@`, checks if stdout contains `"yes"`
   - `volume_icon(volume: u32, muted: bool) -> &'static str` — returns `"audio-volume-muted"` if muted, `"audio-volume-low"` if <34, `"audio-volume-medium"` if <67, `"audio-volume-high"` otherwise
2. **Implement volume commands**
   - `volume_up()` — read current volume, compute `min(current + VOLUME_STEP, MAX_VOLUME)`, run `pactl set-sink-volume @DEFAULT_SINK@ {target}%` (absolute, not relative), read new volume, notify with icon
   - `volume_down()` — run `pactl set-sink-volume @DEFAULT_SINK@ -{VOLUME_STEP}%`, read new volume, notify with icon
   - `volume_mute()` — run `pactl set-sink-mute @DEFAULT_SINK@ toggle`, read mute state + volume, notify with muted icon and "Muted" or volume percentage
3. **Test volume commands**
   - `i3more-audio volume-up` — volume increases by 1%, notification appears with correct icon
   - `i3more-audio volume-down` — volume decreases by 1%
   - `i3more-audio volume-mute` — toggles mute, notification shows muted icon
   - Set volume to 99%, run `volume-up` — caps at 100%, not 101%
   - Verify notification uses sync key `volume` so OSD replaces previous popup

### Phase 3: Audio Device Switching + Config ✅ Complete (2026-03-13)

1. **Implement device enumeration** ✅
   - `struct Sink { id: u32, name: String }` — parsed from `pactl list short sinks`
   - `list_sinks() -> Result<Vec<Sink>, Error>` — runs `pactl list short sinks`, splits each line by tab, extracts id (col 0) and name (col 1)
   - `get_default_sink() -> Result<String, Error>` — runs `pactl get-default-sink`, returns trimmed name
2. **Implement config loading** ✅
   - `struct AudioConfig { preferred_sinks, excluded_sinks }` — `Vec<String>`, defaulting to empty. Source fields deferred to Phase 5 (control panel widget) where they're actually needed.
   - `load_config() -> AudioConfig` — reads `dirs::config_dir().join("i3more").join("audio.json")`, parses with `serde_json`, returns default on missing file or parse error (no crash)
   - `matches_glob(name: &str, pattern: &str) -> bool` — simple glob matching: split on `*`, check if all parts appear in order in name. Handles prefix/suffix/middle wildcards correctly.
3. **Implement sink filtering** ✅
   - `filter_sinks(sinks: Vec<Sink>, config: &AudioConfig) -> Vec<Sink>` — if preferred non-empty: keep only matching sinks, preserve preferred order; if excluded non-empty: remove matching; if both empty: return all
4. **Implement `audio_switch()`** ✅
   - Load config, list sinks, filter, find current default in filtered list
   - Cycle to next (wrap around): `filtered[(current_index + 1) % filtered.len()]`
   - Run `pactl set-default-sink {next.name}`
   - Migrate active streams: `move_sink_inputs()` runs `pactl list short sink-inputs`, moves each input (best-effort, ignores individual failures)
   - Get human-readable name: `get_sink_description()` runs `pactl --format=json list sinks`, finds matching sink, extracts `description` field; fallback to raw name
   - Notify with device description, icon `audio-card`, sync key `audio-switch`
5. **Test audio switching** — 9 unit tests pass (glob matching, sink filtering, volume_icon). Integration testing requires PulseAudio.

**Key implementation decisions:**

- `AudioConfig` only has `preferred_sinks` and `excluded_sinks` (not sources) — YAGNI until Phase 5 adds source dropdowns
- `move_sink_inputs` is best-effort per stream — a single failing stream shouldn't block the switch
- `get_sink_description` uses `serde_json::Value` (not a typed struct) since we only need the `description` field from the large JSON response
- Binary grew from 512KB to 664KB due to serde_json (already a dependency, just not linked before)

### Phase 4: Integration ✅ Complete (2026-03-13)

1. **Build release binary** ✅
   - `docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more-audio dist/"`
2. **Update i3 config keybindings** ✅ (user-tested)
   - Replace all 4 volume/audio bindsym lines in `~/dotfiles/i3/.config/i3/config`
   - Ensure `exec --no-startup-id` prefix is preserved (no terminal, no startup notification)
3. **End-to-end hotkey testing** ✅ (user-tested)
   - Reload i3 config (`$mod+Shift+r`)
   - Press `XF86AudioRaiseVolume` — volume increases, OSD notification appears
   - Press `XF86AudioLowerVolume` — volume decreases
   - Press `XF86AudioMute` — mute toggles
   - Press `$mod+p` — audio device switches
4. **Performance verification** ✅

   | Command      | i3more-audio (Rust) | Bash script         | Speedup |
   | ------------ | ------------------- | ------------------- | ------- |
   | volume-up    | 10-20ms (avg ~12ms) | 30-40ms (avg ~32ms) | ~2.7x   |
   | volume-down  | 10ms                | 30ms                | ~3x     |
   | volume-mute  | 10ms                | 30ms                | ~3x     |
   | audio-switch | 10-20ms (avg ~14ms) | 60-80ms (avg ~64ms) | ~4.6x   |

   All subcommands well under the 30ms target. Audio-switch sees the largest improvement (4.6x) since the bash script spawned the most subprocesses (pactl list + grep + awk + pactl set + pactl move per stream + notify-send).

### Phase 5: Audio Control Panel Widget ✅ Complete (2026-03-13)

1. **Add device enumeration to volume widget** ✅
   - `list_devices_json(device_type)` — generic parser for `pactl --format=json list {sinks|sources}`, extracts name + description pairs
   - `list_sinks_filtered()` / `list_sources_filtered()` — applies config-based glob filtering (preferred/excluded patterns)
   - Config loading, glob matching, and filter logic duplicated from `audio_main.rs` (separate binary, can't share code)
   - `AudioConfig` extended with `preferred_sources` and `excluded_sources` fields for this phase
2. **Create output device dropdown** ✅
   - `gtk4::DropDown::from_strings` below the volume slider row, with "Output" label
   - Model updated via `set_model(Some(&StringList))` on device list changes
   - Selection guarded by `Rc<RefCell<bool>>` updating flag to prevent feedback loops
   - On user selection: `pactl set-default-sink` + `move_all_sink_inputs()` migrates active streams
3. **Create input device dropdown** ✅
   - Second `gtk4::DropDown` with "Input" label
   - Monitor sources (`.monitor` suffix) filtered out automatically
   - On user selection: `pactl set-default-source`
4. **Replace polling with `pactl subscribe`** ✅
   - Background thread spawns `pactl subscribe` with piped stdout, reads lines via `BufReader`
   - Auto-restarts on process exit (2s backoff) for resilience to PipeWire restarts
   - Events sent to GTK main loop via `std::sync::mpsc::channel` polled at 50ms (zero-cost in-memory check vs old 2s pactl spawn cycle)
   - `VolumeChanged` events (sink change / server change) → update slider, pct label, mute icon
   - `DeviceListChanged` events (new/remove sink/source) → full device dropdown refresh with config reload
   - Replaced the old `glib::timeout_add_local` 2-second poll that spawned 2 pactl processes per cycle
5. **Add CSS styles** ✅
   - `.widget-audio-device-label` — gruvbox gray (#a89984), 11px
   - `.widget-audio-device button` — dark background (#3c3836), light text (#d5c4a1), rounded corners
   - Hover state matches existing widget patterns (#504945)
6. **Test config** ✅
   - Created `~/.config/i3more/audio.json` with patterns matching actual hardware:
     - Preferred sinks: Logitech Zone Wired + built-in speaker (skips 3 HDMI/DP sinks)
     - Preferred sources: Logitech Zone Wired mic + built-in mic
   - `audio-switch` cycles correctly between 2 preferred devices
   - `pactl --format=json list sinks` confirmed working on PipeWire, returns human-readable descriptions

**Key findings:**

- `pactl subscribe` output on PipeWire includes `client` events for every pactl invocation — the event filter correctly ignores these and only acts on sink/source/server events
- PipeWire sink names use `HiFi__hw_sofhdadsp{_N}__sink` format (not `analog-stereo` like PulseAudio), so config glob patterns needed adjustment from the original plan
- `DropDown::from_strings` creates the expression internally; `set_model` with new `StringList` preserves the expression for correct rendering
- `std::sync::mpsc::channel` + 50ms `glib::timeout_add_local` poll is simpler and more portable than `glib::MainContext::channel` while achieving the same zero-CPU-when-idle benefit (checking an in-memory channel is ~0 cost)
- The widget grew from 57 lines to ~350 lines, but eliminated all process-spawning polls — zero CPU when idle vs 2 process spawns every 2 seconds
- Note: the main `i3more` binary needs restart to pick up widget changes (it was running during development, so `cp` to dist/ failed for it — only `i3more-audio` was copied)

### Phase 6: Headset Jack Detection

1. **Integrate with `pactl subscribe` stream (from Phase 5)**
   - The `pactl subscribe` listener from Phase 5 already runs in the main i3more process
   - Add parsing for `Event 'new' on sink` / `Event 'remove' on sink` events (already needed for dropdown refresh)
   - Add parsing for `Event 'new' on source` / `Event 'remove' on source` for input device changes
2. **Implement device change notifications**
   - On new sink/source: resolve device description via `pactl --format=json list sinks`
   - Show notification: "Audio device connected: {description}" using existing popup infrastructure
   - On removed sink/source: show "Audio device disconnected: {description}"
   - Use sync key `audio-device` so rapid connect/disconnect replaces the notification
3. **Detect analog jack insertion**
   - When a new source matching `*analog*` or `*headset*` pattern appears, trigger the headset mode popup
   - Only trigger if the source was not present at startup (avoid popup on i3more restart)
4. **Implement headset mode popup**
   - Create a new popup type in `src/notify/popup.rs` with two action buttons:
     - "Headset (with mic)" — runs `pactl set-card-profile {card} output:analog-stereo+input:analog-stereo`
     - "Headphones only" — no action (keeps current headphones-only profile)
   - Resolve card name: parse `pactl list cards short` to find the card associated with the analog jack
   - Popup auto-dismisses after 10 seconds (default to headphones mode)
5. **Test headset detection**
   - Plug in 3.5mm headset → popup appears asking headset vs headphones
   - Select "Headset (with mic)" → mic input appears in `pavucontrol`
   - Select "Headphones only" → mic input does not appear
   - Plug in USB headset → notification "Audio device connected: {name}", no headset popup (USB devices auto-configure)
   - Unplug any device → notification "Audio device disconnected: {name}"

## Verification

```bash
# Build
docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more-audio dist/"

# Volume
dist/i3more-audio volume-up       # notification: "Volume 65%"
dist/i3more-audio volume-down     # notification: "Volume 64%"
dist/i3more-audio volume-mute     # notification: "Volume Muted"

# Audio switch (with config)
echo '{"preferred_sinks":["alsa_output.usb-Logitech*","alsa_output.pci-*analog*"]}' > ~/.config/i3more/audio.json
dist/i3more-audio audio-switch    # cycles only Zone Wired and analog, skips DisplayPort sinks

# Audio switch (without config)
rm ~/.config/i3more/audio.json
dist/i3more-audio audio-switch    # cycles through all sinks

# Performance
time dist/i3more-audio volume-up  # should be <30ms total

# Control panel (after Phase 5)
# Click bell icon -> notification panel shows volume widget with device dropdowns
# Select different output device -> audio switches immediately

# Headset detection (after Phase 6)
# Plug in 3.5mm headset -> popup: "Headset (with mic)" / "Headphones only"
# Select "Headset" -> mic input appears in pavucontrol
```

---

## Audio Environment Reference

Key findings from system sound research (`~/asellerate/research/sound-fix-linux.md`):

### Sound Stack

- **Sound server**: PulseAudio on PipeWire 1.0.5 — `pactl` is the correct interface for both volume control and device switching
- **Sound card**: sof-hda-dsp (Intel SOF, Raptor Lake) — standard HDA driver
- **Multiple devices present**: USB headsets (Logitech Zone Wired), analog 3.5mm combo jack (Jabra Evolve 85), built-in speakers — `audio-switch` must handle all of these

### Device Enumeration

- `pactl list short sinks` — lists output devices (used by `audio-switch` to cycle)
- `pactl list short sources` — lists input devices (used by control panel widget)
- `pactl get-default-sink` / `pactl set-default-sink` — get/set active output
- `pactl --format=json list sinks` — JSON output with human-readable descriptions (PipeWire)

### PipeWire Considerations

- PipeWire exposes PulseAudio-compatible interface via `pipewire-pulse`, so `pactl` commands work identically
- Restarting PipeWire: `systemctl --user restart pipewire pipewire-pulse wireplumber` (useful for debugging, not needed in the utility)
- WirePlumber handles automatic routing — `audio-switch` overrides this by explicitly setting the default sink and moving active streams

### Edge Cases for audio-switch

- **Suspended sinks**: Some sinks (e.g., HDA analog when nothing is plugged in) may be in SUSPENDED state. The utility should skip SUSPENDED sinks when cycling, or include them with a note in the notification.
- **TRRS combo jack**: The 3.5mm jack may not auto-detect headset mode. This is a kernel/ALSA-level issue addressed by Phase 6 (headset detection).
- **Sink-input migration**: When switching default sink, existing audio streams must be explicitly moved via `pactl move-sink-input {stream_id} {new_sink_id}`. New streams will automatically use the new default.
- **DisplayPort sinks**: DP-1/2/3 audio outputs exist but are rarely used. Config-based filtering (Phase 3) prevents these from cluttering the cycle.

### Useful Debugging Tools

| Tool          | Purpose                                |
| ------------- | -------------------------------------- |
| `pavucontrol` | GUI for input/output device management |
| `helvum`      | PipeWire patchbay (visual routing)     |
| `alsamixer`   | Terminal mixer (check mute states)     |

## Performance — Polling vs Event-Driven

### Problem

The current volume widget (`src/notify/widgets/volume.rs:101-124`) polls `pactl` every 2 seconds via `glib::timeout_add_local`. Each poll spawns two processes (`pactl get-sink-volume`, `pactl get-sink-mute`). This is the same pattern that caused high CPU usage in the previous EWW setup.

### EWW CPU issue — root cause analysis

The EWW scripts (`~/dotfiles/eww/.config/eww/scripts/`) reveal the problem. Multiple scripts ran tight polling loops simultaneously:

| EWW Script                 | Method                                    | Interval     | CPU Impact                  |
| -------------------------- | ----------------------------------------- | ------------ | --------------------------- |
| `get_volume_listen.sh`     | `while true; wpctl get-volume; sleep 0.5` | 0.5s         | High — 2 process spawns/sec |
| `get_mic_volume_listen.sh` | `while true; wpctl get-volume; sleep 0.5` | 0.5s         | High — 2 process spawns/sec |
| `get_audio_devices.py`     | `pactl subscribe` + refresh on events     | Event-driven | **Zero CPU when idle**      |
| `sys_info.sh`              | One-shot (called on interval by EWW)      | EWW poll     | Medium                      |

The EWW CPU drain came from the `sleep 0.5` polling loops — each spawning `wpctl` + `awk` twice per second. With volume + mic + other scripts, that's 8+ process spawns per second continuously, even when nothing changes.

### What EWW got right

The EWW script that got it right:

- **`get_audio_devices.py`** — Uses `pactl subscribe` to listen for PulseAudio events. Only calls `pactl -f json list sinks` when an actual event fires. Zero CPU between events. This is the correct pattern for device enumeration.

### Recommended approach for i3more

Replace polling with event-driven updates in the volume widget and future audio features:

#### Volume/Mute state: `pactl subscribe`

Instead of polling every 2 seconds, spawn `pactl subscribe` as a long-lived child process and parse its stdout:

```
Event 'change' on sink #47
Event 'change' on server
```

When a `change` event on `sink` or `server` arrives, read the new volume/mute state. This gives:

- **Zero CPU when idle** (no polling, blocked on pipe read)
- **Instant updates** (<50ms latency vs 2000ms polling)
- **Single long-lived process** vs spawning 2 processes every 2 seconds

Implementation in `volume.rs`:

```rust
// Spawn pactl subscribe as background process
let child = Command::new("pactl").arg("subscribe")
    .stdout(Stdio::piped()).spawn();

// Read lines in a background thread, post to GTK main loop via glib::MainContext
// On relevant event: run read_volume() + is_muted() once
```

#### Device list changes: same `pactl subscribe`

The same `pactl subscribe` stream also emits `new`/`remove` events for sinks and sources. One subscriber process handles both volume changes and device list updates.

### Impact summary

| Component          | Current                      | Proposed                                 | Saving                   |
| ------------------ | ---------------------------- | ---------------------------------------- | ------------------------ |
| Volume widget poll | 2 process spawns / 2s        | `pactl subscribe` (1 persistent process) | ~1 spawn/sec eliminated  |
| Device list        | None (rebuild on panel open) | Same `pactl subscribe` stream            | Event-driven device list |
| Headset detection  | Not implemented              | Same `pactl subscribe` stream            | Already event-driven     |

### Integration with phased implementation

- **Phase 5 (Control Panel Widget)**: Replace the 2-second `glib::timeout_add_local` poll in `volume.rs` with `pactl subscribe` listener. This is the right time since we're already modifying the widget to add device dropdowns.
- **Phase 6 (Headset Detection)**: Reuses the same `pactl subscribe` stream — no additional process needed.

### plugin-headset

- During the initial login with The GNOME Display Manager (GDM), when an audio jack is plugged in, the "Select Audio Device" popup in GNOME Shell appears to distinguish between a Headset (headphones + microphone) and Headphones (audio output only). This allows GNOME to correctly use the microphone embedded in your headset.

In the current audio setup, this is missing.

- The mic on system tray should show with clickable mute and unmute buttons.
- Bluetooth headset (Jabra Evolve 85) needs both audio output AND microphone input.

#### Mic Indicator on Bar ✅ Implemented (2026-04-06)

A microphone mute/unmute indicator in the navigator bar (`src/mic_indicator.rs`):

- **Position**: Between system tray icons and control panel icon in `right_box`
- **Visibility**: Only shown when a real (non-monitor) audio source exists. Auto-hides when no mic detected.
- **Icons**: Font Awesome `MICROPHONE` (green `#b8bb26`) when active, `MICROPHONE_SLASH` (red `#fb4934`) when muted
- **Click**: Toggles `pactl set-source-mute @DEFAULT_SOURCE@ toggle`
- **Monitoring**: Own `pactl subscribe` thread for real-time updates on source changes, mute state, default source changes
- **Tooltip**: Shows "Microphone: Active" or "Microphone: Muted"
- **Jabra Evolve 85 via audio jack**: When user selects "Headset (with mic)" in the headset popup, the mic source appears and the indicator becomes visible automatically

#### jLink Reference Submodule

Added `reference/jLink` → https://github.com/Watchdog0x/jLink.git — a Go CLI tool for managing Jabra headsets on Linux (battery status, Bluetooth pairing, firmware updates). Uses the proprietary Jabra SDK (`libjabra.so`). Potential future integration: battery indicator in bar, pairing management from control panel.

---

#### Bluetooth Headset Architecture — A2DP vs HSP/HFP (2026-04-07)

##### The Problem

Bluetooth audio on Linux has two mutually exclusive profile families:

| Profile                           | Audio Quality            | Microphone | Use Case              |
| --------------------------------- | ------------------------ | ---------- | --------------------- |
| **A2DP** (SBC, SBC-XQ, AAC, LDAC) | High — stereo 44.1/48kHz | None       | Music, media playback |
| **HSP/HFP** (CVSD, mSBC)          | Low — mono 8-16kHz       | Yes        | Voice calls           |

The Jabra Evolve2 85 (`bluez_card.50_C2_75_48_AA_AF`) supports:

- `a2dp-sink` — High Fidelity Playback (SBC)
- `a2dp-sink-sbc_xq` — High Fidelity Playback (SBC-XQ)
- `headset-head-unit` — HSP/HFP (generic)
- `headset-head-unit-cvsd` — HSP/HFP (CVSD codec, 8kHz mono)
- `headset-head-unit-msbc` — HSP/HFP (mSBC codec, 16kHz mono, best HFP quality)

When on A2DP, only a `.monitor` source exists — no real microphone. Switching to HSP/HFP enables the mic but drops audio to mono 16kHz (mSBC) or 8kHz (CVSD).

##### What Already Works — WirePlumber Auto-Switching

WirePlumber 0.4.17 (installed) has **automatic profile switching** enabled by default:

- Config: `/usr/share/wireplumber/policy.lua.d/10-default-policy.lua`
- Setting: `bluetooth_policy.policy["media-role.use-headset-profile"] = true`
- Script: `/usr/share/wireplumber/scripts/policy-bluetooth.lua`

**How it works**: When an app with `media.role = Communication` (or one listed in `media-role.applications`) opens an audio input stream while a Bluetooth device is the default sink:

1. WirePlumber detects the stream and checks if the current Bluetooth profile has an input route
2. If not (A2DP), it auto-switches to the highest-priority profile with input (HSP/HFP mSBC)
3. When the stream closes, it waits 2 seconds then restores the previous A2DP profile

**Pre-configured apps** (already in WirePlumber config):
Firefox, Chromium input, Google Chrome input, Brave input, Microsoft Edge input, Vivaldi input, ZOOM VoiceEngine, Telegram Desktop, linphone, Mumble, WEBRTC VoiceEngine, Skype

**Custom additions** (user override at `~/.config/wireplumber/policy.lua.d/10-default-policy.lua`):
Slack, slack, Microsoft Teams, teams-for-linux

The user file shadows the system file (same filename = higher priority in WirePlumber 0.4.x). After system WirePlumber package updates, diff to check for upstream changes:

```bash
diff ~/.config/wireplumber/policy.lua.d/10-default-policy.lua \
     /usr/share/wireplumber/policy.lua.d/10-default-policy.lua
```

**This means**: Zoom/Teams/Slack/browser calls should auto-switch the Jabra to HSP/HFP when mic is needed, and back to A2DP when the call ends. No i3more code needed for this flow.

##### What's Missing — i3more Enhancements

**Problem 1: No default sink auto-switch on Bluetooth connect**

When the Jabra connects via Bluetooth, the default sink stays on the built-in speakers. The user must manually switch. GNOME auto-switches to newly connected Bluetooth devices.

**Solution**: Extend `process_device_changes()` in `volume.rs` to auto-set default sink when a Bluetooth sink appears. Or add a popup: "Jabra Evolve2 85 connected. Switch audio?" with action buttons.

**Problem 2: Manual profile toggle needed for non-Communication apps**

Some apps don't set `media.role = Communication` and aren't in WirePlumber's app list. The user needs a way to manually switch the Bluetooth profile.

**Solution**: Add a Bluetooth profile toggle to the control panel audio widget. When the current output is a Bluetooth device:

- Show current profile (A2DP / HSP/HFP) next to the output dropdown
- Provide a toggle button or dropdown to switch profiles
- Use `pactl set-card-profile {bluez_card} {profile_name}`

**Problem 3: Mic indicator doesn't reflect Bluetooth state**

When on A2DP, the mic indicator is hidden (no real source). The user has no visual cue that their headset mic is unavailable.

**Solution**: Enhance `mic_indicator.rs` to detect when the default sink is Bluetooth + A2DP and show a distinct state:

- Bluetooth + A2DP (no mic): Show `MICROPHONE_SLASH` in amber/yellow — "Mic unavailable (A2DP mode)"
- Bluetooth + HSP/HFP (mic active): Show `MICROPHONE` in green — normal active state
- Click when in A2DP: Offer to switch to HSP/HFP (profile toggle)

**Problem 4: No notification when profile auto-switches**

When WirePlumber auto-switches A2DP ↔ HSP/HFP for a call, the user gets no feedback. Audio quality changes abruptly.

**Solution**: Monitor card profile changes via `pactl subscribe` (`'change' on card` events). When a Bluetooth card's profile changes, show a notification:

- "Jabra Evolve2 85: Switched to Headset mode (mic enabled)"
- "Jabra Evolve2 85: Switched to High Fidelity mode"

##### Phased Implementation

**Phase A: Bluetooth auto-connect (immediate value)**

1. Detect new Bluetooth sink in `process_device_changes()`
2. Show popup: "Switch audio to {device}?" with action buttons
3. On accept: `pactl set-default-sink` + migrate streams

**Phase B: Profile toggle in control panel**

1. Detect if current output sink is Bluetooth (name starts with `bluez_output.`)
2. Query card profiles via `pactl --format=json list cards`
3. Show profile indicator/toggle next to output dropdown
4. On toggle: `pactl set-card-profile {card} {profile}`

**Phase C: Enhanced mic indicator for Bluetooth**

1. Detect default sink is Bluetooth + check card profile (A2DP vs HSP/HFP)
2. Show amber mic icon when Bluetooth is on A2DP (mic unavailable)
3. Click offers profile switch
4. Show green mic icon when HSP/HFP active

**Phase D: Profile change notifications**

1. Add `'change' on card` to `pactl subscribe` event parsing
2. Diff card active profiles on change events
3. Show notification on Bluetooth profile transitions

##### Hybrid Mic Approach — Built-in Mic + Bluetooth A2DP

An alternative to HSP/HFP: use the built-in laptop microphone for input while keeping the Jabra on A2DP for high-quality audio output. This avoids the quality degradation of HSP/HFP entirely.

**Implementation**: When user clicks mic indicator while on Bluetooth A2DP, offer three choices:

1. "Switch to Headset mode" — HSP/HFP (mono 16kHz, Jabra mic)
2. "Use built-in mic" — keep A2DP, set default source to built-in analog input
3. "Cancel" — no change

This requires `pactl set-default-source {built-in-source}` without changing the sink profile.

---

#### Analysis — Existing Infrastructure (2026-04-06)

The i3more codebase already has all the building blocks for this feature:

| Component                     | Location                                  | What it provides                                                                     |
| ----------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------ |
| `pactl subscribe` listener    | `control_panel/widgets/volume.rs:301-336` | Real-time device add/remove events, auto-restart on failure                          |
| Device change detection       | `control_panel/widgets/volume.rs:205-234` | Diff-based detection via `DeviceSnapshot`, basic headset pattern match at line 223   |
| Popup with action buttons     | `notify/render.rs:262-289`                | Button creation, click handlers, `(id, action_key)` via D-Bus `ActionInvoked` signal |
| `NotificationClosed` signal   | `notify/daemon.rs:137-142`                | Signal defined but not emitted on popup dismiss — **fixed as part of this phase**    |
| Config-based device filtering | `control_panel/widgets/volume.rs:58-64`   | JSON glob patterns for preferred/excluded devices                                    |
| Device info resolution        | `control_panel/widgets/volume.rs:109-132` | `pactl --format=json` parsing for human-readable descriptions                        |

#### Implementation Approach

**Trigger**: When `pactl subscribe` reports a new source matching `*analog*` or `*headset*` patterns (and it wasn't present at startup — the `DeviceSnapshot` diff handles this).

**Popup**: Uses `notify-send --action` (libnotify 0.8+) which sends a D-Bus notification with action buttons through our notification daemon. The `notify-send` process blocks waiting for `ActionInvoked` or `NotificationClosed` D-Bus signal, then prints the selected action key to stdout.

**Profile switch**: On "Headset" action, runs `pactl set-card-profile {card} {profile}` where the card and profile are resolved dynamically from `pactl --format=json list cards` (looking for profiles with both analog output and input).

**Daemon fix required**: `notify-send --action` (implies `--wait`) needs `NotificationClosed` signal on popup timeout/dismiss. Previously the daemon defined the signal but never emitted it. Fixed by:

1. Adding `close_signal_tx` channel from main loop → daemon thread
2. Popup timeout handlers now send `NotifyEvent::Close(id)` back to main loop
3. Main loop forwards close events to daemon via `close_signal_tx`
4. Daemon emits `NotificationClosed(id, reason)` D-Bus signal

**Concurrency guard**: `AtomicBool` prevents multiple headset popups from overlapping during rapid device change events.

#### Known Limitations

- **sof-hda-dsp combo jack**: On PipeWire with UCM (Use Case Manager), plugging in a 3.5mm headset may NOT create new source/sink nodes — the nodes may exist at all times with the jack detection handled at the WirePlumber level. In this case, `pactl subscribe` won't see `'new' on source` events. The detection still works for USB headsets and systems where the driver creates nodes dynamically.
- **Card profile names**: Profile names vary across PulseAudio (`output:analog-stereo+input:analog-stereo`) and PipeWire/UCM (`HiFi`). The implementation searches for profiles containing both "output"/"input"/"analog" or "headset" keywords. May need adjustment per hardware.
- **Fallback**: If `notify-send --action` is unavailable (libnotify < 0.8), falls back to a plain notification without buttons. If no matching card profile is found, shows an informational notification suggesting pavucontrol.

- bluthooth headset connected. but not visible in the control menu.
- bluetooth detected jabra Evolve2 85
