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

### Multiple Monitor

- The user uses multiple monitors.
- The monitors are managed be ArandR screen settings.
- Workspace from all the monitors are shown in center of the bar.
- User is not able to identify which workspace belongs to which monitor.
- A suggestion from user is to seperate them with a | , suggest a solution for this issue.

### Workspace number sequencing

- The number of workspace should always be sequencial.
- If there are workspaces such as 1 2 3 4 5 and user closes workspace 3. Then the workspace number should shift and remaining should be 1 2 3 4.
- the workspace number on right monitor should always be bigger than left monitors.
- In the current open workspaces after the fix it should be 1 2 3 4 5 | 6.
- polling is not recomended, check whether i3 provides any workspace close subscription.
- refer to vendor/i3 to plan correctly.

### Workspace nagivation: change app to next sequencial workspace

- When user opens an app in workspace 1, then opens another app on same works space. It gets tiled.
- User prefers to have a short cut key to move the app to next sequence workspace.
  refer ~/dotfiles/i3/.config/i3 for context.
- check if vendor provides change workspace IPC to work with refer to vendor/i3 to plan correctly.

### Auto focus on next workspace of current monitor.

- When user closes a container and the workspace is empty.
- Then change the current workspace to next number workspace of current monitor.
- if the work spaces are 1 2 3 4 5 | 6 7 8
- - user closes all container of workspace 3, then current workspace should be 4, 1 2 3 4 | 6 7 8.

- - user closes all container of workspace 5, then current workspace should be 4, 1 2 3 4 | 6 7 8.

- - user closes all container of workspace 6, then current workspace should be 7, 1 2 3 4 5 | 6 7 .

## BUG

- The workspace number should never be less than 1. Workspace number -1 should never be a case
