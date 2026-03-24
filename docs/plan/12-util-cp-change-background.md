## Change Background

- The control panel to select the widget to change background image.
- gtk4 ui executable to select the image from the folder and apply the changes.

---

### Phase 1: Configuration & Data Model

**Status: done**

Create `src/notify/widgets/background.rs` with config support.

- **Config file**: `~/.config/i3more/background.json`
  ```json
  {
    "folder": "~/Pictures/Wallpapers",
    "current": "/home/user/Pictures/Wallpapers/forest.jpg",
    "mode": "fill"
  }
  ```
- **Rust structs**: `BackgroundConfig` with serde for read/write
- **Defaults**: folder = `~/Pictures/Wallpapers`, mode = `fill`
- **Functions**: `load_config()`, `save_config()` using `dirs::config_dir()`
- **Register module** in `src/notify/widgets/mod.rs`

**Files:**

- `src/notify/widgets/background.rs` (new)
- `src/notify/widgets/mod.rs` (add `pub mod background;`)

---

### Phase 2: Background Widget in Control Panel

**Status: done**

Build the GTK4 widget following the backlight/volume pattern.

- **Header row**: Image icon + "Background" label (matching existing widget style)
- **Current wallpaper**: Small preview thumbnail (200x120) of current wallpaper
- **Folder selector**: Button to open a GTK4 FileDialog to pick a folder
- **Image grid**: FlowBox with image thumbnails from the configured folder
  - Scan folder for `.jpg`, `.jpeg`, `.png`, `.webp` files
  - Show thumbnails (80x80) with filename tooltip
  - Highlight the currently selected image
  - Click to select and apply
- **Mode selector**: Dropdown for `fill`, `scale`, `center`, `tile`, `max`
- **Add widget to panel**: Append in `panel.rs` widget_box after backlight

**Files:**

- `src/notify/widgets/background.rs` (update)
- `src/notify/panel.rs` (add background widget)
- `src/fa.rs` (add IMAGE icon constant)

---

### Phase 3: Apply Background with feh

**Status: done**

Wire up the apply logic using `feh` (standard i3 background setter).

- **Apply function**: `apply_background(path, mode)` runs `feh --bg-{mode} {path}`
- **On image click**: Apply immediately + save to config
- **On mode change**: Re-apply with new mode + save to config
- **Startup restore**: Not needed here (user's i3 config typically calls feh on startup; config file serves as the source of truth)

**Files:**

- `src/notify/widgets/background.rs` (update)

---

### Phase 4: CSS Styling

**Status: done**

Add styling for the background widget to match the Gruvbox dark theme.

- `.widget-background` container styling
- `.widget-background-grid` FlowBox styling
- `.widget-background-thumb` thumbnail styling (border, hover highlight)
- `.widget-background-thumb-active` for currently selected image
- `.widget-background-preview` for the current wallpaper preview
- `.widget-background-mode` dropdown styling

**Files:**

- `assets/notification.css` (update)

## UI improvements

- Currently the interface is in the Notification window. Needs to be moved to a dedicated control panel.

- All the utils mentioned in file dd-utils-cp-\*.md files should be moved to a dedicated control panel.

- The user prefers an andriod like control panel which can be clicked opened from above i3More bar.

- The control pannel can be overlayed on windows and need not be tilling.

- In the control panel window, the frequently used widget can stay above and rest can stay below.

- The widgets must be grouped, Use of good proportion on icons is recomended.

## BUG

### select-wallpaper

- User navigated to select a wallpaper and clicked on select, no change appeared.
-

### control-panel-flicker

- when opening the control panel it flickers the current window.

### persistant-wallpaper (resolved)

- ~~After restart the wallpaper should be the same which has been saved.~~
- ~~the i3config replaces it.~~
- Fix: Removed hardcoded feh from i3 config. i3More now restores saved wallpaper on startup via `on_activate()`.
