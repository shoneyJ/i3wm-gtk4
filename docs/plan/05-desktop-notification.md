# Desktop Notification

## Dunst Replace

Currently the i3 config uses dunst as desktop notification. This module replaces dunst
with a built-in notification daemon that integrates directly into the i3more navigator bar.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Session D-Bus                                      │
│                                                                             │
│  Bus name: org.freedesktop.Notifications                                    │
│  Object:   /org/freedesktop/Notifications                                   │
│                                                                             │
│  ┌──────────────┐    Notify()         ┌──────────────────────────────────┐  │
│  │  Any App      │ ─────────────────► │  NotificationDaemon              │  │
│  │  (notify-send,│ ◄──────────────── │  (src/notify/daemon.rs)          │  │
│  │   Slack,      │    returns id      │                                  │  │
│  │   Firefox...) │                    │  Methods:                        │  │
│  │               │ CloseNotification()│  - GetCapabilities() -> [str]    │  │
│  │               │ ─────────────────►│  - Notify(...) -> u32            │  │
│  └──────────────┘                    │  - CloseNotification(id)         │  │
│                                       │  - GetServerInformation()        │  │
│                                       │                                  │  │
│                                       │  Signals:                        │  │
│                                       │  - NotificationClosed(id,reason) │  │
│                                       │  - ActionInvoked(id,action_key)  │  │
│                                       └──────────┬───────────────────────┘  │
└──────────────────────────────────────────────────┼──────────────────────────┘
                                                   │
                                                   │ mpsc::channel<NotifyEvent>
                                                   │
                    ┌──────────────────────────────┼──────────────────────┐
                    │          Background Thread    │    Main GTK Thread   │
                    │                               │                      │
                    │  ┌─────────────────────┐      │                      │
                    │  │ async_io::block_on   │      │                      │
                    │  │                     │      │                      │
                    │  │ D-Bus event loop    │      │                      │
                    │  │ (zbus connection)   │ ────►│                      │
                    │  │                     │      │                      │
                    │  └─────────────────────┘      │                      │
                    │                               │                      │
                    └───────────────────────────────┼──────────────────────┘
                                                    │
                                                    ▼
                    ┌───────────────────────────────────────────────────────┐
                    │              GTK Main Loop (50ms poll)                 │
                    │              src/main.rs                               │
                    │                                                       │
                    │  ┌─────────────────┐  ┌────────────────┐              │
                    │  │ i3 event drain  │  │ tray event     │              │
                    │  │ (workspace/     │  │ drain          │              │
                    │  │  window changes)│  │ (SNI items)    │              │
                    │  └─────────────────┘  └────────────────┘              │
                    │                                                       │
                    │  ┌────────────────────────────────────────────┐       │
                    │  │ notification event drain                    │       │
                    │  │                                            │       │
                    │  │  NotifyEvent::New(notif)                   │       │
                    │  │    ├─► PopupManager.show()                 │       │
                    │  │    └─► NotificationHistory.push()          │       │
                    │  │                                            │       │
                    │  │  NotifyEvent::Close(id)                    │       │
                    │  │    └─► PopupManager.dismiss()              │       │
                    │  │                                            │       │
                    │  │  NotifyEvent::ActionInvoked(id, key)       │       │
                    │  │    └─► reverse channel → daemon → D-Bus    │       │
                    │  └────────────────────────────────────────────┘       │
                    │                          │                            │
                    └──────────────────────────┼────────────────────────────┘
                                               │
                              ┌────────────────┼────────────────┐
                              │                │                │
                              ▼                ▼                ▼
                    ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
                    │ Popup Windows│  │ Bell Icon    │  │ History Panel│
                    │ (top-right)  │  │ (nav bar)    │  │ (top-right)  │
                    │              │  │              │  │              │
                    │ i3-msg float │  │ FA bell glyph│  │ ScrolledWin  │
                    │ auto-dismiss │  │ unread badge │  │ grouped list │
                    │ close button │  │ click→panel  │  │ clear all    │
                    └──────────────┘  └──────────────┘  └──────────────┘


  Module Structure (final)
  ────────────────────────

  src/notify/
  ├── mod.rs            Module root, re-exports start_notification_daemon
  ├── daemon.rs         D-Bus service (#[interface] macro, background thread)
  ├── types.rs          Notification struct, NotifyEvent enum
  ├── popup.rs          PopupManager, window creation, positioning, auto-dismiss
  ├── history.rs        Notification history storage, persistence
  ├── panel.rs          Notification center / control center panel
  ├── render.rs         Shared widget builder, markup parser, image extraction
  ├── dnd.rs            Do Not Disturb state, inhibit tracking, persistence
  ├── twofa.rs          2FA code detection + clipboard copy
  ├── config.rs         JSON config loader, hot-reload watcher
  ├── gestures.rs       Swipe-to-dismiss gesture handling
  ├── shortcuts.rs      Keyboard shortcut handling for panel
  └── widgets/
      ├── mod.rs        ControlWidget trait, submodule declarations
      ├── volume.rs     PulseAudio volume slider (pactl)
      ├── backlight.rs  Screen brightness slider (sysfs)
      └── button_grid.rs  Configurable shortcut button grid

  assets/
  └── notification.css  Gruvbox-themed popup + panel styles


  Threading Model (same pattern as system tray)
  ──────────────────────────────────────────────

  Background Thread                    Main GTK Thread
  ┌─────────────────┐                  ┌──────────────────────┐
  │ std::thread::    │   mpsc::channel  │ glib::timeout_add_   │
  │ spawn            │ ───────────────►│ local(50ms)          │
  │                  │  NotifyEvent     │                      │
  │ async_io::       │                  │ try_recv() in loop   │
  │ block_on(        │ ◄───────────────│ → PopupManager.show  │
  │   run_daemon()   │  reverse channel │ → History.push       │
  │ )                │  (ActionInvoked) │ → Badge.update       │
  └─────────────────┘                  └──────────────────────┘
```

---

## Implementation Phases

### Phase 1: Core D-Bus Daemon + Basic Popup Display ✅

**Status**: Implemented

**Goal**: Claim `org.freedesktop.Notifications` on session bus, accept `Notify` calls,
display popup windows.

**Files created**:

- `src/notify/mod.rs` — module root, re-exports `start_notification_daemon`
- `src/notify/types.rs` — `Notification` struct, `NotifyEvent` enum
- `src/notify/daemon.rs` — D-Bus service with `#[interface]` macro, background thread
- `src/notify/popup.rs` — `PopupManager` with floating window creation, stacking, auto-dismiss
- `assets/notification.css` — gruvbox popup styles

**Files modified**:

- `src/main.rs` — added `mod notify`, notification channel, daemon startup, event drain in 50ms poll loop

**Key findings**:

- zbus `#[interface]` macro cleanly implements the spec, mirrors tray watcher pattern
- i3 tiling workaround: must use `i3-msg` to float/position popups since GTK4 can't set
  `_NET_WM_WINDOW_TYPE` before window map on X11
- Popups must be `ApplicationWindow` bound to the GTK `Application` (bare `Window` won't render)
- Auto-dismiss via `glib::timeout_add_local_once` with stored `SourceId` for cancellation
- Popups stack vertically with 70px height estimate + 4px gap
- `replaces_id > 0` reuses the ID and replaces the existing popup

**Verification**:

```bash
killall dunst 2>/dev/null
docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more dist/"
dist/i3more &
notify-send "Test" "Hello World"                     # popup appears top-right
notify-send -t 0 "Persistent" "Won't auto-close"    # stays until manually closed
dbus-send --session --dest=org.freedesktop.Notifications --type=method_call \
  /org/freedesktop/Notifications org.freedesktop.Notifications.GetServerInformation
```

---

### Phase 2: Bell Icon in Navigator Bar + Notification History

**Status**: Implemented

**Goal**: Bell icon in the bar showing unread count. Click opens a scrollable notification
history panel.

**New files**:

- `src/notify/history.rs` — `NotificationHistory` struct with `push()`, `remove()`,
  `clear()`, `mark_all_read()`. Capped at 500 entries. Persists to
  `~/.local/share/i3more/notifications.json` via serde_json.
- `src/notify/panel.rs` — Notification center panel. `gtk4::Window` (popup type) anchored
  top-right. Contains `ScrolledWindow` → vertical `Box` of notification entries. Each entry
  shows app icon, summary, body preview, relative timestamp, dismiss button. "Clear All"
  button at top. Toggled by bell icon click.

**Modified files**:

- `src/fa.rs` — add `BELL` (`\u{f0f3}`) and `BELL_SLASH` (`\u{f1f6}`) constants
- `src/navigator.rs` — add bell icon label prepended to `tray_box` on the right side.
  Unread badge as small `gtk4::Label` with `.notification-badge` class. Click handler
  toggles panel. Return bell/badge handles so `main.rs` can update them.
- `src/main.rs` — maintain `Rc<RefCell<NotificationHistory>>`. On `NotifyEvent::New`:
  push to history, increment badge. On panel toggle: show/hide panel, mark as read.
- `assets/style.css` — add `.notification-badge` (red circle, white text, small font)
  and `.notification-bell` styles
- `src/notify/types.rs` — extend `NotifyEvent` with panel interaction variants

**Verification**:

- Bell icon visible in bar with FA glyph
- `notify-send` increments unread badge count
- Click bell → panel opens with notification list
- "Clear All" → panel empties, badge resets to zero
- Restart i3more → history persists from disk

---

### Phase 3: Actions, Grouping, Markup/Images

**Status**: Implemented

**Goal**: Support notification action buttons, group notifications by app in the panel,
render body markup (bold, italic, links) and images (app icons, album art, hint images).

**New files**:

- `src/notify/render.rs` — shared notification widget builder:
  - `parse_notification_markup(body) -> String` — converts notification HTML subset
    (`<b>`, `<i>`, `<u>`, `<a href>`, `<img>`) to Pango markup
  - `extract_image(hints, app_icon) -> Option<gdk::Texture>` — extracts from
    `image-data` hint (type `iiibiiay`: width, height, rowstride, has_alpha, bpp,
    channels, data), `image-path` hint (file path), or icon theme name
  - `build_notification_widget(notif, action_tx) -> gtk4::Box` — reusable widget
    for both popup and panel

**Modified files**:

- `src/notify/popup.rs` — render action buttons below body. "default" action key makes
  the entire notification clickable (not just a button). Other actions render as labeled
  buttons in a horizontal row.
- `src/notify/panel.rs` — group notifications by `app_name` with collapsible section
  headers showing app icon, app name, and notification count
- `src/notify/daemon.rs` — expand capabilities: add `body-hyperlinks`, `body-images`,
  `body-markup`, `icon-static`. Wire `ActionInvoked` signal emission via a reverse
  channel back to the daemon thread.
- `src/notify/types.rs` — add `NotifyEvent::ActionInvoked(u32, String)`

**Verification**:

```bash
notify-send --action="reply=Reply" --action="dismiss=Dismiss" "Chat" "New message"
# → action buttons render, clicking emits ActionInvoked D-Bus signal
notify-send --icon=dialog-information "Info" "<b>Bold</b> and <i>italic</i>"
# → markup renders correctly in popup
# Test with Spotify (album art via image-data hint)
# Test with Firefox download notifications (icon from theme)
# Verify grouping in panel when multiple apps send notifications
```

---

### Phase 4: Do Not Disturb, Inhibit, 2FA Detection

**Status**: Not started

**Goal**: DND mode suppresses popups but still stores to history. Inhibit API for
programmatic suppression. Auto-detect 2FA codes and copy to clipboard.

**New files**:

- `src/notify/dnd.rs` — `DndState { enabled: bool, inhibit_count: u32, inhibitors: HashMap }`.
  `is_suppressed()` returns true if DND enabled or any inhibitor active. Persistence to
  `~/.local/share/i3more/dnd.json` — restores DND state after restart.
- `src/notify/twofa.rs` — `detect_2fa_code(body) -> Option<String>`. Regex matching for
  common 2FA patterns: 4-8 digit codes near keywords (code, OTP, verification, token,
  passcode). On detection: copy to clipboard via `gdk::Display::default().clipboard().set_text()`,
  show copy icon indicator on the notification.

**Modified files**:

- `src/notify/popup.rs` — check `DndState::is_suppressed()` before showing popup.
  When 2FA detected, show clipboard indicator on notification.
- `src/notify/daemon.rs` — add custom `com.i3more.NotificationDaemon` interface with:
  `Inhibit(app_name, reason) -> u32` (returns cookie), `UnInhibit(cookie)`,
  `SetDnd(enabled: bool)`, `GetDnd() -> bool`
- `src/navigator.rs` — bell icon switches between `BELL` and `BELL_SLASH` glyph based on
  DND state. Right-click bell toggles DND.
- `src/notify/panel.rs` — add DND toggle widget at top of panel
- `Cargo.toml` — add `regex = "1"` dependency

**Verification**:

- Right-click bell → DND activates, icon changes to bell-slash, popups suppressed
- Notifications still appear in history panel while DND is active
- Restart i3more → DND state restored from disk
- `dbus-send` to test Inhibit/UnInhibit methods
- Send notification with body "Your verification code is 482916" → code auto-copied to clipboard
- Test 2FA detection patterns: "OTP: 1234", "code 567890", "Token: 12345678"

---

### Phase 5: Control Center Widgets (Volume, Backlight)

**Status**: Partial — Volume and Backlight implemented, Button Grid not started

**Goal**: Extend the notification panel into a full control center with media player
controls, volume slider, backlight slider, and a configurable shortcut button grid.

**New files**:

- `src/notify/widgets/mod.rs` — `ControlWidget` trait:
  `name() -> &str`, `build(tx) -> gtk4::Widget`, `update(widget)`.
  Declares submodules: `volume`, `backlight`, `button_grid`.
- `src/notify/widgets/volume.rs` — PulseAudio integration via `pactl` commands:
  `get-sink-volume @DEFAULT_SINK@`, `set-sink-volume`. `gtk4::Scale` slider widget.
  Mute toggle button.
- `src/notify/widgets/backlight.rs` — reads `/sys/class/backlight/*/brightness` and
  `max_brightness` (same sysfs pattern as battery in `sysinfo.rs`). `gtk4::Scale` slider.
  Writes via `brightnessctl` command.
- `src/notify/widgets/button_grid.rs` — configurable grid of shortcut buttons (WiFi
  toggle, Bluetooth, Screenshot, etc.). Each button: FA icon, label, shell command.
  Layout via `gtk4::FlowBox` or `gtk4::Grid`.

**Modified files**:

- `src/notify/panel.rs` — panel layout becomes: Title → DND toggle → Widget sections
  (MPRIS, Volume, Backlight, Button Grid) → Notification list. Each widget section
  is a `gtk4::Box` added to the panel's vertical layout.

**Verification**:

- MPRIS widget shows current Spotify/Firefox track info
- Play/pause/next/prev buttons control media player
- Volume slider reflects current PulseAudio volume, dragging changes it
- Backlight slider works on laptops with `/sys/class/backlight`
- Button grid renders and executes configured shell commands

---

### Phase 6: Config File, Hot-Reload, Gestures, Keyboard Shortcuts

**Status**: Partial — Monitor selection hardcoded to primary, audio.json config exists for volume widget only

**Goal**: JSON configuration file with hot-reload support, swipe-to-dismiss gestures
on notifications, and keyboard shortcuts for panel navigation.

**New files**:

- `src/notify/config.rs` — config file at `~/.config/i3more/notifications.json`:
  ```json
  {
    "popup_timeout_ms": 5000,
    "max_history": 500,
    "popup_position": "top-right",
    "monitor": null,
    "dnd_on_startup": false,
    "widget_order": ["title", "dnd", "mpris", "volume", "notifications"],
    "button_grid": [
      { "icon": "wifi", "label": "WiFi", "command": "nmcli radio wifi toggle" }
    ],
    "filters": [],
    "popup_width": 350,
    "group_by_app": true
  }
  ```
  `load_config() -> NotifyConfig` with sane defaults. Hot-reload via
  `std::fs::metadata().modified()` polling in a background thread — sends
  `NotifyEvent::ConfigReloaded(NotifyConfig)` on change.
- `src/notify/gestures.rs` — `gtk4::GestureSwipe` attached to each popup notification.
  Horizontal swipe (left or right) dismisses with slide-out animation. Velocity
  threshold to distinguish swipe from accidental drag.
- `src/notify/shortcuts.rs` — `gtk4::EventControllerKey` on the panel window:
  `Escape` closes panel, arrow keys navigate notifications, `Enter` activates
  default action, `Delete` dismisses focused notification.

**Modified files**:

- `src/notify/panel.rs` — widget order driven by `config.widget_order`. Panel position
  driven by `config.popup_position` and `config.monitor`. Re-build panel on config reload.
- `src/notify/popup.rs` — popup width, timeout, and position read from config. Attach
  swipe gesture controller to each popup window.
- `src/notify/daemon.rs` — notification action filtering: config defines rules like
  `{ "app": "Slack", "action": "dismiss", "auto_execute": true }` to suppress or
  auto-handle certain actions.
- `src/notify/types.rs` — add `NotifyEvent::ConfigReloaded(NotifyConfig)`

**Verification**:

- Create config file with custom timeout (e.g., 10000ms), verify popups use it
- Modify config while i3more is running → hot-reload applies without restart
- Swipe notification left/right on trackpad → dismisses with slide animation
- Open panel, press arrow keys → focus moves between notifications
- Press Enter on focused notification → executes default action
- Press Delete → dismisses focused notification
- Press Escape → closes panel

---

## Features Checklist

| Feature                                        | Phase | Status  |
| ---------------------------------------------- | ----- | ------- |
| Basic notification daemon (D-Bus service)      | 1     | ✅      |
| Popup display with auto-dismiss                | 1     | ✅      |
| Close button on popups                         | 1     | ✅      |
| App icon display                               | 1     | ✅      |
| replaces_id support                            | 1     | ✅      |
| Bell icon on navigator bar                     | 2     | ✅      |
| Unread notification badge                      | 2     | ✅      |
| Notification history panel                     | 2     | ✅      |
| History persistence across restart             | 2     | ✅      |
| Notification body markup (bold, italic, links) | 3     | ✅      |
| Image support (album art, hint images)         | 3     | ✅      |
| Notification action buttons                    | 3     | ✅      |
| Click notification to execute default action   | 3     | ✅      |
| Grouped notifications by app                   | 3     | ✅      |
| ActionInvoked D-Bus signal                     | 3     | ✅      |
| Do Not Disturb mode                            | 4     |         |
| Restore DND value after restart                | 4     |         |
| Inhibiting notifications through D-Bus         | 4     |         |
| Copy detected 2FA codes to clipboard           | 4     |         |
| Volume slider (PulseAudio)                     | 5     | ✅      |
| Backlight slider                               | 5     | ✅      |
| Configurable button grid                       | 5     |         |
| JSON config file                               | 6     | partial |
| Hot-reload config                              | 6     |         |
| Swipe/gesture to close notification            | 6     |         |
| Keyboard shortcuts                             | 6     |         |
| Notification action filtering                  | 6     |         |
| Monitor selection                              | 6     | partial |
| Customizable widget order                      | 6     |         |

---

## Key Architecture Decisions

1. **Popup as separate `gtk4::ApplicationWindow`** — not Popover, because notifications
   must position at screen edge independently of the bar. Must use `i3-msg` for floating.
2. **Reverse channel for D-Bus signals** — GTK thread sends `NotifyEvent::ActionInvoked`
   back to daemon thread via a second mpsc channel, which emits the D-Bus signal on the
   stored `Connection`.
3. **Same threading model as tray** — D-Bus daemon on background thread with
   `async_io::block_on`, mpsc channels to GTK main loop, 50ms polling + 100ms debounce.
4. **`notify` module in main binary** — private to the navigator binary (not in `lib.rs`),
   same pattern as the `tray` module.

## Critical Reference Files

- `src/tray/watcher.rs` — D-Bus service pattern to mirror
- `src/tray/types.rs` — event enum pattern to mirror
- `src/main.rs:101-178` — polling loop to extend
- `src/navigator.rs:29-132` — bar layout for bell icon integration
- `src/css.rs` — CSS loading pattern to reuse
- `src/fa.rs` — Font Awesome icon constants to extend
- `src/sysinfo.rs` — sysfs reading pattern (for backlight widget)
- `src/tray/dbusmenu.rs` — D-Bus variant parsiinig patterns

## BUG

- ~~The long audio input and output dropdown spans the notification window across another window.~~ **Fixed** — dropdown labels ellipsized at 30 chars via custom `ListItemFactory`, panel width enforced with `set_size_request`, container overflow hidden.
- ~~The entire notification window should stay in the same monitor window~~ **Fixed** — same fix; panel constrained to `PANEL_WIDTH` (380px).

### focus-me-not

- ~~When a notification window pop up, then the focus is set to the notification window.~~ **Fixed** — `connect_realize` handler in `popup.rs` sets `_NET_WM_WINDOW_TYPE_NOTIFICATION` (i3 auto-floats) and `_NET_WM_USER_TIME=0` (i3 skips focus per `manage.c:640-642`) via `xprop` before window maps. Same pattern as navigator dock window in `main.rs:318-352`.

### system-notify-stay

- ~~When user uses keyboard to change volumne or brightness, popup opens and closes again.~~ **Fixed** — `show()` refactored into update-or-create pattern. When `replaces_id` matches an existing popup, `update_existing()` replaces the window's child widget and resets the timeout timer in-place — no window destroy/recreate cycle. New popups still go through `create_new()` with full X11 property setup.
