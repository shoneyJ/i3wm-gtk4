## bash-convert-exec

### Goal

Eliminate all third-party UI dependencies (rofi, dunst, scrot, etc.) from the i3 config. Only i3wm + i3More should be needed on a fresh system.

### Current i3 config scripts & external commands

#### Already replaced by i3More

| Script / Command | i3More binary | Config line |
|---|---|---|
| `auto-renumber-workspaces.sh` | i3more (sequencer) | commented out |
| `blur-lock` / `i3lock` | `i3more-lock` | L82 |
| volume (`pactl`) | `i3more-audio volume-*` | L174-182 |
| audio device switch | `i3more-audio audio-switch` | L193 |
| app launcher (rofi) | `i3more-launcher` | L139, L329 |
| translate | `i3more-translate`, `i3more-popup-translate` | L207-208 |
| notifications (dunst) | i3more (notification daemon) | commented out |
| wallpaper | i3more (background widget) | — |
| workspace navigator (i3bar/i3blocks) | i3more (navigator) | L286 |
| workspace move-next | `i3more-workspace move-next` | L148 |

#### Still needs replacement

| # | Script / Command | Config line | Rofi dep? | What it does |
|---|---|---|---|---|
| 1 | `scripts/powermenu` | L79 | Yes | Lock/logout/reboot/shutdown/suspend/hibernate menu |
| 2 | `scripts/keyhint-2` | L91 | Yes | Searches and displays i3 keybindings |
| 3 | `scripts/volume_brightness.sh` | L94-95 | No | Brightness up/down via `brightnessctl` |
| 4 | `scripts/empty_workspace` | L145 | No | Opens next available empty workspace |
| 5 | `scripts/power-profiles` | L204 | Yes | Switches power profile via `powerprofilesctl` |
| 6 | `rofi -show window` | L332-333 | Yes | Window switcher |
| 7 | `scrot` + `notify-send` | L198 | No | Screenshot with notification |
| 8 | `amixer sset Capture toggle` | L185 | No | Mic mute toggle |
| 9 | `playerctl play-pause/next/prev` | L188-190 | No | Media playback control |
| 10 | `~/.screenlayout/home.sh` | L254 | No | Apply monitor layout (arandr output) |

### Implementation plan

#### Phase 1: Quick wins — fold into existing binaries [DONE]

These are small additions to binaries that already exist.

- [x] **1a. Mic mute → `i3more-audio mic-mute`** — toggles via `pactl set-source-mute`
- [x] **1b. Media control → `i3more-audio play-pause|next|prev`** — wraps `playerctl`
- [x] **1c. Brightness → `i3more-audio brightness-up|brightness-down`** — reads sysfs, sets via `brightnessctl`, 5% floor
- [x] **1d. Empty workspace → `i3more-workspace open-empty`** — finds first unused number (1-20) via i3 IPC

i3 config updated for all four. Remaining runtime dep: `playerctl` (install via `apt install playerctl`).

#### Phase 2: GTK popup menus — replace rofi menus

These require a new shared popup window pattern (similar to i3more-launcher).

**2a. Power menu → `i3more-power`**
- GTK4 floating window with options: Lock, Logout, Reboot, Shutdown, Suspend, Hibernate
- Lock calls `i3more-lock`, others call `systemctl` / `i3-msg exit`
- Replaces: L79 `scripts/powermenu` (rofi)

**2b. Power profiles → `i3more-power profile`** (subcommand of 2a, or standalone)
- Reads available profiles from `powerprofilesctl list` or D-Bus (`net.hadess.PowerProfiles`)
- GTK4 popup to select profile
- Replaces: L204 `scripts/power-profiles` (rofi)

**2c. Keybinding hint → `i3more-keyhint`**
- Parses `~/.config/i3/config` for `bindsym` lines
- GTK4 searchable list (reuse launcher pattern)
- Replaces: L91 `scripts/keyhint-2` (rofi)

**2d. Window switcher → `i3more-window`**
- Queries i3 tree via IPC for all windows
- GTK4 searchable list showing window title + workspace
- `i3-msg [con_id=N] focus` on selection
- Replaces: L332-333 `rofi -show window`

#### Phase 3: Screenshot ()

**3a. Screenshot → `i3more-screenshot`**
- Uses X11 `XGetImage` or invokes `grim`/`maim` as fallback
- Saves to `~/` with timestamp filename
- Sends notification via i3more notification daemon (D-Bus `org.freedesktop.Notifications.Notify`)
- Replaces: L198 `scrot` + `notify-send`- Uses X11 `XGetImage` or invokes `grim`/`maim` as fallback

#### Phase 4: Monitor layout (low priority)

**4a. Monitor layout → `i3more-display`**
- Reads `xrandr` output, presents layout editor or preset selector
- This is complex (arandr is a full GUI); consider just auto-applying saved xrandr scripts
- Replaces: L254 `~/.screenlayout/home.sh` and L201 `arandr`
- Low priority — `xrandr` one-liner is acceptable

### Updated i3 config after completion

After all phases, the config would reference only:
- `i3more` (navigator, tray, notifications, sequencer)
- `i3more-launcher` (app launcher, replaces rofi)
- `i3more-lock` (screen lock)
- `i3more-audio` (volume, mic mute, audio switch, media control, brightness)
- `i3more-workspace` (move-next, open-empty)
- `i3more-power` (power menu, power profiles)
- `i3more-keyhint` (keybinding reference)
- `i3more-window` (window switcher)
- `i3more-screenshot` (screen capture)
- `i3more-translate` / `i3more-popup-translate` (translation)

**External runtime deps eliminated:** rofi, scrot, dunst, amixer, playerctl, brightnessctl, notify-send.

**Remaining system deps (unavoidable):** i3wm, picom (compositor), alacritty (terminal), systemctl, xrandr, polkit agent.

### setup.md update

After each phase, update `setup.md` to:
- Remove the corresponding apt/pacman install instructions for replaced tools
- Add the new i3more binary to the install/build steps
- Update the i3 config snippet with the new bindsym lines
