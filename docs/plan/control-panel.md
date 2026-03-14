# Plan: Dedicated Android-Style Control Panel

## Context

Control widgets (Volume, Backlight, Background) previously lived inside the notification panel (`src/notify/panel.rs`). This plan extracts them into a dedicated, Android-style control panel that:
- Opens from a new sliders icon in the i3More bar (not the bell)
- Floats over windows (non-tiling overlay)
- Groups widgets with section headers and icons
- Places frequently-used controls at the top

## Phases

### Phase 1: Control Panel Window (Empty Shell)

Created `src/control_panel/mod.rs` and `src/control_panel/panel.rs` with a `ControlPanel` struct mirroring `NotificationPanel`:
- `new(app)`, `toggle()`, `is_visible()`, `hide()`
- 400x520px floating window, title `"i3more-control-panel"`
- Same i3-msg positioning pattern (150ms delay after present)
- Added `mod control_panel;` to `src/main.rs`

**Files**: `src/control_panel/mod.rs`, `src/control_panel/panel.rs`, `src/main.rs`.

### Phase 2: Toggle Icon in Bar

Added a SLIDERS icon (`\u{f1de}`) to the bar between tray icons and bell:
- Added `SLIDERS` constant to `src/fa.rs`
- Added clickable label + overlay in `src/navigator.rs` `right_box` (between `tray_box` and `bell_overlay`)
- Returns `ControlPanelHandles` from `build_navigator()`
- Click handler in `src/main.rs` calls `control_panel.toggle()`
- `.control-panel-icon` CSS in `assets/control-panel.css`

**Files**: `src/fa.rs`, `src/navigator.rs`, `src/main.rs`, `assets/control-panel.css`.

### Phase 3: Move Widgets to Control Panel

Relocated widget files from notification to control panel:
- Moved `src/notify/widgets/{volume,backlight,background}.rs` to `src/control_panel/widgets/`
- Created `src/control_panel/widgets/mod.rs`
- Removed `pub mod widgets;` from `src/notify/mod.rs`
- Removed widget_box + separator from `src/notify/panel.rs`
- Added widget building to `src/control_panel/panel.rs`
- Reduced notification `PANEL_HEIGHT` from 500 to 400
- Added `hide()` method to `NotificationPanel`

**Files**: `src/control_panel/widgets/`, `src/notify/mod.rs`, `src/notify/panel.rs`, `src/control_panel/panel.rs`.

### Phase 4: Layout and Grouping

Organized widgets into Android-style sections:

```
+-----------------------------------+
|  Control Panel              [x]   |
+-----------------------------------+
|  [speaker] Audio                  |
|  [mute] ========o======= 75%     |
+-----------------------------------+
|  [sun] Display                    |
|  [sun] ======o=========== 60%    |
+-----------------------------------+
|  [image] Background               |
|  [preview]  [Folder] Mode:[fill]  |
|  [thumb grid]                     |
+-----------------------------------+
```

Created `build_section(title, icon, content) -> gtk4::Box` helper:
- `.cp-section` container with `.cp-section-header` row (FA icon + bold title)
- Content widget appended below header
- Sections ordered: Audio (top), Display (if available), Background

Close button (X) in panel header.

**Files**: `src/control_panel/panel.rs`.

### Phase 5: CSS Styling

Created `assets/control-panel.css` with:
- Panel: `.control-panel` (bg `#1d2021`, border `#3c3836`)
- Header: `.control-panel-header`, `.control-panel-title`
- Sections: `.cp-section` (bg `#282828`, rounded corners 6px, card-like), `.cp-section-header`
- Moved all `.widget-*` classes from `assets/notification.css` to `assets/control-panel.css`
- Loaded via `i3more::css::load_css("control-panel.css", ...)` in `ControlPanel::new()`

**Files**: `assets/control-panel.css`, `assets/notification.css`, `src/control_panel/panel.rs`.

### Phase 6: Mutual Exclusion and Polish

- Opening control panel hides notification panel (and vice versa)
- Bell click handler: hides CP before toggling notification panel
- CP click handler: hides NP before toggling control panel
- Auto-hide timer: `connect_notify_local(Some("is-active"))` pattern (5s timeout on focus loss)
- Close button in header

**Files**: `src/main.rs`, `src/control_panel/panel.rs`.

## File Summary

| Action | File |
|--------|------|
| **Create** | `src/control_panel/mod.rs` |
| **Create** | `src/control_panel/panel.rs` |
| **Create** | `src/control_panel/widgets/mod.rs` |
| **Create** | `assets/control-panel.css` |
| **Create** | `docs/plan/control-panel.md` |
| **Move** | `src/notify/widgets/volume.rs` -> `src/control_panel/widgets/volume.rs` |
| **Move** | `src/notify/widgets/backlight.rs` -> `src/control_panel/widgets/backlight.rs` |
| **Move** | `src/notify/widgets/background.rs` -> `src/control_panel/widgets/background.rs` |
| **Delete** | `src/notify/widgets/` (entire directory) |
| **Modify** | `src/main.rs`, `src/navigator.rs`, `src/fa.rs` |
| **Modify** | `src/notify/mod.rs`, `src/notify/panel.rs` |
| **Modify** | `assets/notification.css` |
