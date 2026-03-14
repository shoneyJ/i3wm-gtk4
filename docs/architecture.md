# Architecture

## Recommended Architecture

### Bottom Positioning

i3bar only supports `position top` or `position bottom` — but i3More uses
a custom GTK4 floating window positioned at the bottom screen edge.
Two approaches make a bottom navigator work:

**Approach 1: `gaps bottom` + floating window (Recommended)**

i3-gaps (already used in this config) supports directional gaps.
Reserve space at the bottom for the navigator, then float it in that space:

```
# Add to i3 config:
gaps bottom 40                  # reserve bottom row for navigator

for_window [title="i3More-navigator"] floating enable, border pixel 0, \
    move position 0 px calc(screen_height - 36) px, sticky enable
```

`sticky enable` keeps the navigator visible across all workspaces.
Tiled windows never overlap the reserved gap.

**Approach 2: `_NET_WM_WINDOW_TYPE_DOCK` with struts**

The GTK window sets its X11 window type to `_NET_WM_WINDOW_TYPE_DOCK` and
declares a bottom strut via `_NET_WM_STRUT_PARTIAL`. i3 respects dock struts
and will automatically reserve the space — the same mechanism i3bar uses.
This is more "correct" but adds X11 complexity.

### Screen Layout

```
┌──────────────────────────────────────┐
│                                      │
│                                      │
│         Tiled windows                │
│         (normal i3 tiling)           │
│                                      │
│                                      │
│                                      │
├──────────────────────────────────────┤
│ 1 [ff] │ 2 [te][co] │ 3            │
└──────────────────────────────────────┘
  ▲
  └── i3More navigator
      (floating, in gaps bottom space)
      [ff]=firefox [te]=terminal [co]=code
```

### Internal Architecture

```
┌─────────────────────────────────────────────┐
│                   i3More                     │
│                                              │
│  ┌──────────────┐    ┌───────────────────┐   │
│  │  i3 IPC      │    │  Icon Resolver    │   │
│  │  - get_tree  │    │  - .desktop parse │   │
│  │  - subscribe │    │  - GTK icon theme │   │
│  │  - command   │    │  - LRU mem cache  │   │
│  └──────┬───────┘    │  - disk cache     │   │
│         │            └────────┬──────────┘   │
│         └──────────┬──────────┘              │
│                    │                         │
│         ┌──────────▼──────────┐              │
│         │   GTK4 Floating     │              │
│         │   Window (bottom)   │              │
│         │ ┌─────────────────┐ │              │
│         │ │1[ff]│2[te][co]│3│ │              │
│         │ └─────────────────┘ │              │
│         └─────────────────────┘              │
└─────────────────────────────────────────────┘
```

### Data Flow

1. On startup, query `get_tree` to build workspace → window-class map.
2. Batch-resolve all window classes to icon file paths (port `resolve-app-icon.py` logic). Populate the in-memory LRU cache.
3. Render a horizontal GTK4 floating window anchored to the bottom screen edge.
4. Subscribe to `workspace` and `window` i3 events; update only the changed workspace entries (debounce at ~100ms).
5. On workspace click, send `i3-msg workspace number N` via IPC.
6. On new window class seen, resolve icon once, cache in memory — all subsequent lookups are free.
