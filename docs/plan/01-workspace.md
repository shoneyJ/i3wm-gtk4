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

- The workspace container is horizontally centered (`halign: Center`) — no CenterBox needed, a single `gtk4::Box` with `halign` handles centering.
- The window spans full screen width and sits at the bottom (positioned by i3 config).
- Each workspace entry is a horizontal box containing a number label followed by inline app icons.

### Achieving 18px bar height

- GTK4 enforces minimum sizes on widgets, making a thin bar difficult with CSS alone.
- **Key findings:**
  - `set_size_request(width, 18)` + `set_default_size(width, 18)` on the window sets the _requested_ height, but GTK may still expand it.
  - Setting `min-height: 0` on all widgets (`.navigator`, `.workspace-entry`, `.workspace-num`, `.workspace-icon`) prevents GTK's default minimums from inflating the bar.
  - `set_overflow(gtk4::Overflow::Hidden)` on the container clips any content that would push beyond the allocated height.
  - `set_vexpand(false)` + `set_valign(Center)` on child widgets prevents vertical expansion.
  - **xdotool fallback** (in `main.rs`): after window is presented, a 300ms delayed `xdotool search --name i3More-navigator windowsize <width> 18` forces X11 to honor the exact size, overriding any GTK minimum. This is the reliable guarantee.
- Icon size is 16px (`ICON_SIZE`), font-size is 8px — both fit within the 18px bar with zero padding.

### CSS architecture (`assets/style.css`)

- `.navigator` — dark background (`#1d2021`), zero padding/margin/min-height.
- `.workspace-entry` — minimal horizontal padding (3px), zero min-height, hover/focused/urgent/visible state colors.
- `.workspace-num` — gruvbox light text (`#ebdbb2`), 8px font, bold, 1px right padding.
- `.workspace-icon` — 0.85 opacity, zero padding/margin/min-height/min-width.
- Focused state uses i3 blue (`#4c7899`), urgent uses red (`#cc241d`), visible uses dark gray (`#32302f`).

### increase size of text

- the central workspace navigator is perfect. with number above and icon below.
- The left side info widget's text needs to be adjusted accordimgly.
