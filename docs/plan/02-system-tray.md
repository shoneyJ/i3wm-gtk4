## System Tray

### Overview

i3More implements a system tray on the right side of the bar using the **StatusNotifierItem (SNI)** D-Bus protocol — the modern Linux system tray standard used by nm-applet, blueman, PulseAudio, etc. The bar acts as both a **StatusNotifierWatcher** (registry) and **StatusNotifierHost** (renderer).

### Architecture

```
                         Session D-Bus
                              │
          ┌───────────────────┼───────────────────┐
          │                   │                   │
   nm-applet            blueman-applet       other apps
   (SNI client)         (SNI client)         (SNI client)
          │                   │                   │
          └─── RegisterStatusNotifierItem() ──────┘
                              │
                              ▼
              ┌───────────────────────────┐
              │  StatusNotifierWatcher    │
              │  (tray/watcher.rs)        │
              │                           │
              │  Bus: org.kde.StatusNoti-  │
              │       fierWatcher          │
              │  Path: /StatusNotifier-    │
              │        Watcher             │
              │                           │
              │  - Tracks registered items │
              │  - Monitors NameOwnerChg   │
              │    for cleanup on exit     │
              │  - Triggers prop loading   │
              └─────────┬─────────────────┘
                        │ TrayEvent (mpsc channel)
                        ▼
              ┌───────────────────────────┐
              │  GTK Main Loop (main.rs)  │
              │                           │
              │  - Polls tray channel     │
              │  - Maintains TrayState    │
              │    HashMap<Id, Props>     │
              │  - Debounced re-render    │
              └─────────┬─────────────────┘
                        │
                        ▼
              ┌───────────────────────────┐
              │  Tray Renderer            │
              │  (tray/render.rs)         │
              │                           │
              │  - Icon from theme name   │
              │    or ARGB pixmap→RGBA    │
              │  - 16px, right-aligned    │
              │  - Click → D-Bus method   │
              │    (Activate, ContextMenu,│
              │     SecondaryActivate)    │
              └───────────────────────────┘
```

### Layout

```
ApplicationWindow
  └─ CenterBox (.navigator)
      ├─ center: Box (.workspace-container)   ← workspace entries
      └─ end:    Box (.tray-area)             ← tray icons
```

The `CenterBox` keeps workspaces visually centered regardless of how many tray icons appear on the right.

### Module Structure

```
src/tray/
  mod.rs        — module root, re-exports start_watcher
  types.rs      — TrayItemId, TrayItemProps, TrayPixmap, TrayEvent
  watcher.rs    — StatusNotifierWatcher D-Bus service (runs on background thread)
  item.rs       — reads SNI item properties over D-Bus (IconName, IconPixmap, ToolTip, Status, etc.)
  render.rs     — builds GTK widgets for tray icons, attaches click handlers
```

### Dependencies

- **zbus 5** — pure Rust async D-Bus with `#[interface]` derive macros. No C library deps at runtime.
- **async-io** — lightweight async runtime for the watcher thread (`block_on`).
- **futures-util** — `StreamExt`, `future::join` for concurrent D-Bus stream processing.

### D-Bus Interface

The watcher owns `org.kde.StatusNotifierWatcher` and implements:

| Method / Property                     | Description                                        |
| ------------------------------------- | -------------------------------------------------- |
| `RegisterStatusNotifierItem(service)` | Called by tray apps; parses bus name + object path |
| `RegisterStatusNotifierHost(service)` | Called by hosts (self-registered on startup)       |
| `RegisteredStatusNotifierItems`       | Property: list of `"busname/objectpath"` strings   |
| `IsStatusNotifierHostRegistered`      | Property: always `true`                            |
| `ProtocolVersion`                     | Property: `0`                                      |
| `StatusNotifierItemRegistered`        | Signal: emitted on new registration                |
| `StatusNotifierItemUnregistered`      | Signal: emitted on removal                         |

### Icon Resolution

1. **Icon name** (preferred): `IconName` property → `gtk4::Image::from_icon_name()` using installed theme.
2. **Pixmap fallback**: `IconPixmap` property → select closest size to 16px, convert ARGB big-endian to RGBA, create `gdk::MemoryTexture`.
3. **Default**: `application-x-executable` icon if neither is available.

### Click Handling

| Button     | D-Bus Method Called       |
| ---------- | ------------------------- |
| Left (1)   | `Activate(x, y)`          |
| Right (3)  | `ContextMenu(x, y)`       |
| Middle (2) | `SecondaryActivate(x, y)` |

Calls are dispatched on a background thread to avoid blocking GTK.

### Item Lifecycle

1. App calls `RegisterStatusNotifierItem` → watcher stores ID, sends `TrayEvent::ItemRegistered`.
2. Loader task reads properties via D-Bus proxy → sends `TrayEvent::ItemPropsLoaded`.
3. GTK main loop updates `TrayState` HashMap → debounced re-render of tray box.
4. App exits → `NameOwnerChanged` signal fires (empty new owner) → watcher removes item, sends `TrayEvent::ItemUnregistered`.

### Status: Implemented (Phases 1–5)

- [x] Phase 1: CenterBox layout restructuring
- [x] Phase 2: StatusNotifierWatcher D-Bus service
- [x] Phase 3: Reading tray item properties
- [x] Phase 4: GTK icon rendering (theme + pixmap)
- [x] Phase 5: Click handling (Activate, ContextMenu, SecondaryActivate)
- [ ] Phase 6: DBusMenu client (`com.canonical.dbusmenu`) for native popup menus — deferred, most apps work via Activate/ContextMenu without it

### Issue

- Currently i can see a network app in the system tray. But I cannot change the network. right click or left click does nothing.
- Comment the section of i3 config the part where system tray gets called.

## UI enhancement

- Tray app icons right click popup menu displays the disabled link as white text and enabled link as gray.
  The color template must be switched.
