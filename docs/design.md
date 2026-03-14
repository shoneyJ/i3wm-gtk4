# Design

## Feature One: Workspace Navigator

### Requirements

- A bottom-edge floating bar showing all workspaces.
- Each workspace entry displays the workspace number and application icons
  (resolved from the apps running in that workspace).
- Clicking a workspace entry switches to it.

### Does i3 have a built-in floating workspace navigator?

**No.** i3 has no native floating workspace panel. `i3bar` supports
`position top` or `position bottom` but cannot float freely, cannot display
app icon images, and cannot be placed as an overlay on the left edge.

What i3 **does** provide is a rich IPC interface that makes building one
straightforward:

- `i3-msg -t get_workspaces` — list of workspaces with num, name, focused, urgent, output.
- `i3-msg -t get_tree` — full window tree; can extract `window_properties.class` per workspace.
- `i3-msg -t subscribe -m '["workspace","window"]'` — real-time event stream for workspace/window changes.

These are the same APIs already used by `get-workspaces.sh` in this repo.

### App Icon Resolution (solved)

The `resolve-app-icon.py` script already handles icon lookup:

```
WM_CLASS → match .desktop file → read Icon= field → resolve via GTK icon theme → file path
```

Results are cached in `~/.cache/eww/app-icons/`. The script supports both
single-class and `--batch` mode (reads classes from stdin, outputs JSON map).

This logic should be ported into the i3More binary directly (no Python
dependency at runtime).

```bash
# Reference location:
ls ~/dotfiles/eww/.config/eww/scripts/resolve-app-icon.py
```

---

## Known Constraints

1. **EWW causes high CPU usage** — The EWW-based bar is disabled in the i3
   config (line 282-283) due to CPU load. If i3More uses a widget toolkit,
   it must avoid the same polling/rendering overhead that affected EWW.

2. **No external bash scripts** — The project should be self-contained.
   Current workspace/icon logic lives in shell scripts (`get-workspaces.sh`,
   `auto-renumber-workspaces.sh`); i3More should internalize this.

3. **Ubuntu Server base** — Cannot assume a full desktop environment. Only
   i3, X11, and explicitly installed packages are available.

4. **Low memory footprint** — The navigator will run persistently; it must
   use minimal RAM.

---

## Performance Requirements

1. **Icon caching** — Frequently used app icons must be cached in memory
   after first resolution. The existing disk cache (`~/.cache/eww/app-icons/`)
   proves the pattern works; the Rust binary should add an in-memory LRU
   cache layer on top so repeated lookups (e.g., Alacritty, Firefox, Code)
   are instant with zero disk I/O on the hot path.

2. **Minimum turnaround time** — The navigator must update within ~100ms of
   any workspace or window event. This means:
   - Event-driven updates only (subscribe to i3 IPC events, never poll).
   - Debounce rapid event bursts (e.g., moving multiple windows) at ~100ms.
   - In-memory icon cache eliminates disk and GTK icon-theme lookups after
     the first resolve.
   - Pre-resolve icons for all open windows at startup so the initial render
     is immediate.
   - Keep the GTK render path minimal — only redraw the workspace entries
     that actually changed.

---

## Recommended Language: Rust

| Language        | Memory            | Dev Speed | i3 IPC        | Single Binary | Verdict                      |
| --------------- | ----------------- | --------- | ------------- | ------------- | ---------------------------- |
| **Rust + GTK4** | Excellent         | Moderate  | `i3ipc` crate | Yes           | **Recommended**              |
| Go + GTK4       | Good              | Fast      | `go-i3`       | Yes           | Strong alternative           |
| Python + GTK3   | Higher (~30-50MB) | Fastest   | `i3ipc` pip   | No            | Good for prototyping         |
| C + GTK3        | Excellent         | Slow      | Raw socket    | Yes           | Maximum control, slowest dev |

**Why Rust:**

- **Single binary** — no runtime dependencies beyond GTK; easy to install on Ubuntu Server.
- **Low memory** — no GC, no interpreter overhead.
- **i3 IPC** — the `i3ipc` crate provides typed workspace/window queries and event subscriptions.
- **GTK4 bindings** — `gtk4-rs` is mature and well-documented for building floating overlay windows.
- **No bash scripts** — the icon resolution, workspace tracking, and renumbering logic all compile into one binary.

If faster prototyping is needed first, **Python + GTK3** is viable (the repo
already uses Python for `resolve-app-icon.py`), then port to Rust once the
design is validated.

---

## Next Steps

1. Decide: prototype in Python first or build directly in Rust.
2. Set up the Rust project with `i3ipc`, `gtk4-rs`, and `freedesktop-icons` crates.
3. Implement workspace data model (subscribe to i3 events, track window classes).
4. Implement icon resolver (port the Python cache + .desktop + GTK theme logic).
5. Build the GTK4 floating panel UI.
6. Package as a single installable binary for Ubuntu.
