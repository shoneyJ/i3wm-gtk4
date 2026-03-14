# i3More bar

## Workspace navigation

- Keeping the layout of screen as it is, Keeping the bar on **position bottom**.
- On clicking the workspace the i3wm should focus the clicked workspace.

## Update i3 config

- what updates should the i3 config be made

```bash
ls ~/dotfiles/i3/.config/i3/*
```

## User Interface

### Layout

- The workspace container is horizontally centered (`halign: Center`) — a `CenterBox` holds sysinfo (left), workspaces (center), tray + notification bell (right).
- The window spans full screen width and sits at the top (positioned by i3 config at `0 0`).
- Each workspace entry is a **vertical box** with the workspace number on top and app icons in a horizontal row below.

### Achieving 40px bar height

- GTK4 enforces minimum sizes on widgets, making exact sizing difficult with CSS alone.
- **Key findings:**
  - `set_size_request(width, 40)` + `set_default_size(width, 40)` on the window sets the _requested_ height, but GTK may still expand it.
  - Setting `min-height: 0` on all widgets (`.navigator`, `.workspace-entry`, `.workspace-num`, `.workspace-icon`) prevents GTK's default minimums from inflating the bar.
  - `set_overflow(gtk4::Overflow::Hidden)` on the container clips any content that would push beyond the allocated height.
  - `set_vexpand(false)` + `set_valign(Center)` on child widgets prevents vertical expansion.
  - **`connect_realize` + `gdk4-x11`** (in `main.rs`): before `window.present()`, a `connect_realize` handler runs after the X11 window is created but before it is mapped. It uses `gdk4_x11::X11Surface::xid()` to get the window ID, then calls `xprop` to set `_NET_WM_WINDOW_TYPE_DOCK` and `_NET_WM_STRUT_PARTIAL`, and `xdotool windowsize` to force exact height. This ensures i3 sees the dock type at classification time.
- Icon size is 16px (`ICON_SIZE`), font-size is 12px — stacked vertically within the 40px bar.

### Dock window behavior ✅

- **Problem:** Setting `_NET_WM_WINDOW_TYPE_DOCK` via `xprop` 300ms after `window.present()` did NOT work — i3 only evaluates window type at map time. The bar could still be killed with `mod+q`, moved, and focused.
- **Fix:** Use `gdk4-x11` crate (added to `Cargo.toml`) + `window.connect_realize()` to set the dock type **before** the window is mapped:
  1. `connect_realize` fires after the X11 window is created but before `MapRequest`
  2. `gdk4_x11::X11Surface::xid()` gets the X11 window ID from the GDK surface
  3. `xprop` sets `_NET_WM_WINDOW_TYPE_DOCK` — i3 classifies it as a dock at map time
  4. `xprop` sets `_NET_WM_STRUT_PARTIAL` (`0, 0, 40, 0, ...`) — i3 auto-reserves 40px at top
  5. `xdotool windowsize` forces exact height
  6. `window.present()` then maps the window — i3 sees dock type immediately
- **How i3 native bar does it:** i3bar uses the same `_NET_WM_WINDOW_TYPE_DOCK` hint. Dock windows cannot receive focus, cannot be moved between workspaces/monitors, and cannot be closed via WM keybindings.
- **i3 config:** `gaps top` removed — `_NET_WM_STRUT_PARTIAL` handles space reservation (gap disappears if bar crashes). `for_window` rule removed — i3 auto-handles dock positioning, floating, sticky, borderless. `no_focus` kept as defense-in-depth fallback.
- **Graceful degradation:** On Wayland (no X11Surface), the `downcast` returns `Err` and the handler is a no-op.

### CSS architecture (`assets/style.css`)

- `.navigator` — dark background (`#1d2021`), zero padding/margin/min-height.
- `.workspace-entry` — vertical padding (`2px 4px`), zero min-height, hover/focused/urgent/visible state colors.
- `.workspace-num` — gruvbox light text (`#ebdbb2`), 12px font, bold, zero padding, centered.
- `.workspace-icon` — 0.85 opacity, zero padding/margin/min-height/min-width.
- Focused state uses i3 blue (`#4c7899`), urgent uses red (`#cc241d`), visible uses dark gray (`#32302f`).

## UI improvement ✅

- place the workspace number on top of application icon.
- make the fonts bigger.
- adjust the i3 config accordingly
- prevent the bar from being killed, moved, or focused (dock window type hint).
