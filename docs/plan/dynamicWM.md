# Dynamic Window Management (`i3more-dwm`)

Make i3's tiling behaviour respond to ordinary user gestures (titlebar buttons,
mouse drags) instead of requiring keyboard shortcuts only. Built on the existing
`i3more::ipc` layer — no fork of i3 unless a level strictly requires it.

## Environment

Two i3 installations coexist on the host once Approach 2 is in flight:

| Path                  | Version          | Source                       | Role                       |
|-----------------------|------------------|------------------------------|----------------------------|
| `/usr/bin/i3`         | 4.23 (2023-10-29)| Ubuntu apt (`i3-wm` package) | Stock fallback / current session before swap |
| `/usr/local/bin/i3`   | 4.25-non-git     | Built from `vendor/i3`       | Patched fork — Feature A + Levels 1–5         |

`PATH` ordering has `/usr/local/bin` before `/usr/bin`, so once the fork is
installed, `which i3` resolves to the patched binary and `i3-msg restart`
swaps the running session over in place.

```
# Before install
$ which i3 && i3 --version
/usr/bin/i3
i3 version 4.23 (2023-10-29) © 2009 Michael Stapelberg and contributors

# After install (sudo cp from vendor/i3/build/install-root)
$ which i3 && i3 --version
/usr/local/bin/i3
i3 version 4.25-non-git © 2009 Michael Stapelberg and contributors
```

The 4.23 → 4.25 upstream jump is the main source of risk beyond our own
patches; IPC, config parser, and EWMH atom set all changed across that range.
Watch for config-parse warnings on first start; recovery is `i3-msg exec
/usr/bin/i3` plus removing `/usr/local/bin/i3` to fall back to stock.

## Goals

Two distinct features bundled under one binary:

1. **Titlebar button → i3 action.** Maximize/minimize buttons on an app's own
   titlebar should toggle i3 fullscreen / scratchpad, not be silently ignored.
2. **Mouse-driven layout edits.** A 5-level ladder of mouse interactions, each
   level a self-contained deliverable.

## Feature A — Titlebar buttons

| # | Trigger                                  | i3 action (current patch)                                         |
|---|------------------------------------------|-------------------------------------------------------------------|
| 1 | Click *maximize* on a tiled window       | `con_set_layout(con, L_TABBED)` — parent flips to tabbed, the focused con dominates the workspace, bar stays visible |
| 2 | Click *maximize* on an already-max'd win | `con_set_layout(con, parent->last_split_layout)` — back to splith/splitv |
| 3 | Click *minimize* on a maximized window   | Same as #2 — restore parent to last_split_layout                  |
| 4 | Click *minimize* on a tiled (split) win  | No-op (parent isn't tabbed/stacked, nothing to restore from)      |

We deliberately do **not** map maximize to `fullscreen enable` — that hides the
bar, which isn't what users mean by "maximize". And we don't map minimize to
`scratchpad_move` — in a tiling WM there's no taskbar to recover from, so
hiding a window loses it.

GTK/Qt/X11 apps signal these via `_NET_WM_STATE` ClientMessage to the root
(`_NET_WM_STATE_MAXIMIZED_VERT/HORZ`) and `WM_CHANGE_STATE → IconicState`.
Stock i3 only handles `_NET_WM_STATE_FULLSCREEN`, `_DEMANDS_ATTENTION`, and
`_STICKY`; the maximize atoms fall through with an `Unknown atom` log line.
The patch sits in `vendor/i3/src/handlers.c` inside `handle_client_message`,
just past the existing per-atom loop and inside the `A_WM_CHANGE_STATE`
branch.

### Testing & diagnostics

**Confirm the patched binary is what's running:**

```bash
md5sum /usr/local/bin/i3 vendor/i3/build/i3                 # must match
i3-msg -t get_version | python3 -c \
    'import sys,json; print(json.load(sys.stdin)["human_readable"])'
# Expect: 4.25-non-git
```

**Trigger the patch without relying on an app's titlebar.** `wmctrl` sends
the EWMH `_NET_WM_STATE` ClientMessage directly. Useful to separate "patch
is wrong" from "app doesn't emit the message":

```bash
# Focus a tiled window first (e.g. click into a terminal that shares a
# workspace with at least one sibling), then:
wmctrl -r ":ACTIVE:" -b toggle,maximized_vert,maximized_horz

# Repeat to toggle back.
```

If wmctrl flips the workspace to tabbed and back, the patch is fine and any
app-specific failure (see below) is the app not emitting the message.

**Tail the i3 log to see the patch fire.** i3 logs to a shared-memory buffer
when `--shmlog-size=NNN` (or `shmlog_size` in config) is set. Dump it with:

```bash
i3-dump-log | tail -50
```

Look for `Received maximize request -> parent layout to tabbed` or
`Minimize -> parent layout from ...`. If the ClientMessage arrived but the
log shows the existing `Unknown atom in ClientMessage` line, the patch is
not in the running binary — recheck the md5sum step above.

**Known app caveats:**

| App     | Titlebar buttons emit EWMH?                                          |
|---------|----------------------------------------------------------------------|
| VSCode  | Only when `"window.titleBarStyle": "native"`. The default `"custom"` (Electron-drawn) titlebar manipulates window state internally and bypasses the WM. |
| Firefox / Zen | Yes, when CSD is enabled (default on most distros).            |
| GNOME GTK apps (gnome-text-editor, nautilus, …) | Yes — standard GTK CSD path. |
| Qt apps (with native decorations) | Usually yes.                                   |

VSCode under `"custom"` is the most common false-negative for this feature.
Either switch its title-bar style or test with a GTK CSD app first.

## Feature B — Dynamic mouse levels

| Lvl | Gesture                                    | Result                                                |
|-----|--------------------------------------------|-------------------------------------------------------|
| 1   | Drag a tiling window's edge                | Resize the split / containing parent                  |
| 2   | Drag window body onto another window       | Swap the two tiling windows                           |
| 3   | Drag window onto workspace label / monitor | Move window to that workspace                         |
| 4   | Drag window onto another window's edge     | New split (drop position = split direction)           |
| 5   | Drag a floating window over a tile zone    | Convert floating ↔ tiling seamlessly                  |

Each level is independently shippable; levels 4 and 5 depend on the drop-zone
infrastructure from level 2.

## What i3 already gives us for free

Verify these before reimplementing — partial overlap exists:

- **Resize borders**: `Mod+RightMouse` drag resizes tiling borders. Level 1 is
  about making this work *without* Mod, on the visible border zone.
- **Swap by drag**: i3 supports dragging a container by its title bar to
  rearrange — limited to its current parent. Level 2 extends this to arbitrary
  drop targets in the tree.
- **Move to workspace**: dragging a title onto an `i3bar` workspace button
  already works in stock i3. Our navigator is a separate GTK4 window, so level
  3 must reimplement this against `i3more`'s own workspace buttons.

The plan covers only what's missing.

## Approach decision

### Approach 1 — IPC + X11 listener (chosen)

A new long-running daemon `i3more-dwm`:

- Subscribes to `window` and `workspace` events via `i3more::ipc`.
- Connects to the X server via `x11rb` (already a dependency) and selects
  `SubstructureNotify` / `ClientMessage` on the root window to catch
  `_NET_WM_STATE` and `WM_CHANGE_STATE`.
- Grabs Button1 on tiling-window edges via passive grabs to detect drag-start
  without stealing clicks from the app.
- Issues i3 commands through `I3Connection::run_command`.

Covers Feature A and Levels 1–3 cleanly. Level 4 needs a transparent overlay
window during drag (drop-zone highlight) — still no fork. Level 5 is the most
likely to stress this approach because it crosses i3's floating/tiling boundary
and may need tree manipulation that has no IPC equivalent.

### Approach 2 — Fork i3

Patch i3 directly. Gives correct hit-testing for free (the WM already owns the
pointer / window tree) and removes the X11 grab brittleness. Cost: maintain a
fork, rebuild on every i3 upstream bump.

The fork already lives at `vendor/i3` (submodule pointed at
`git@github.com:shoneyJ/i3.git`, currently at upstream `4.19.1-non-git`). The
upstream sources we'd touch:

| Level / Feature | i3 source files (in `vendor/i3/src/`)            |
|-----------------|--------------------------------------------------|
| Feature A       | `handlers.c` (X11 ClientMessage), `ewmh.c`       |
| Level 1         | `resize.c`, `click.c`                            |
| Level 2         | `tiling_drag.c`, `move.c`                        |
| Level 3         | `tiling_drag.c`, `workspace.c`                   |
| Level 4         | `tiling_drag.c`, `con.c` (split insertion)       |
| Level 5         | `floating.c`, `tiling_drag.c`                    |

A small `i3more-dwm` helper may still be useful (Feature A is cleaner as an
external listener even with a forked i3), but Levels 1–5 move inside i3.

#### Build (containerised)

Add a new compose service so the i3 build deps don't bloat the existing `dev`
image (`libxcb-util-cursor-dev`, `libev-dev`, `libyajl-dev`, `libstartup-notification0-dev`,
`libpcre2-dev`, `meson`, `ninja-build` — see `vendor/i3/DEPENDS` for the full
list).

| File                          | Action                                        |
|-------------------------------|-----------------------------------------------|
| `Dockerfile.i3`               | create — Ubuntu 24.04 + i3 build deps + meson |
| `docker-compose.yaml`         | edit — add `i3-build` service, bind `.:/src`  |
| `docs/build.md`               | edit — new "Building the forked i3" section   |

Following the same bind-mount pattern as `whisper-build`, no copy to `dist/`:
the build tree lives at `vendor/i3/build/` on the host. From inside the
container:

```bash
meson setup vendor/i3/build vendor/i3 --buildtype=release -Ddocs=false -Dmans=false
ninja -C vendor/i3/build
```

Resulting binaries: `vendor/i3/build/i3`, `vendor/i3/build/i3bar`,
`vendor/i3/build/i3-msg`, etc.

#### Deploy (host system replace)

i3 is a privileged binary at `/usr/bin/i3`. Replacing it needs sudo on the
host, not inside the container. Two options:

1. **Side-by-side install** *(safer during development)*. Install to
   `/opt/i3more/` so the system i3 stays as the recoverable fallback. Then add
   an entry to the X session menu (`/usr/share/xsessions/i3more.desktop`)
   pointing at `/opt/i3more/bin/i3`.

   ```bash
   sudo meson install -C vendor/i3/build --destdir=/opt/i3more
   # vendor/i3/build is part of the bind mount, so this runs on the host.
   ```

2. **Full replace** *(once stable)*. `sudo meson install -C vendor/i3/build`
   into the default prefix (`/usr/local`), with `/usr/local/bin` ahead of
   `/usr/bin` on `PATH` so `i3` resolves to the fork.

Either way, restart the i3 session (`i3-msg restart` works in place once the
new binary is on disk and on `PATH`).

#### Upstream sync workflow

```bash
cd vendor/i3
git remote add upstream https://github.com/i3/i3.git   # one-time
git fetch upstream
git rebase upstream/next                                # or merge
cd ../..
git add vendor/i3
git commit -m "vendor/i3: bump to upstream <sha>"
```

Rebuild the `i3-build` image only when `DEPENDS` changes; otherwise just
`ninja -C vendor/i3/build`.

Revisit Approach 1 if maintenance overhead outweighs benefit per level.

## Architecture

New binary, sibling to `i3more-window` and `i3more-workspace`:

```
src/dwm_main.rs        — entry point, event loop, owns connections
src/dwm/x11.rs         — X11 listener (_NET_WM_STATE, ClientMessage)
src/dwm/drag.rs        — pointer grabs, drag state machine
src/dwm/overlay.rs     — transparent GTK4 overlay for drop zones (level 4+)
src/dwm/tree.rs        — helpers over i3 tree JSON (find_container_at(x,y))
```

`Cargo.toml`: add `[[bin]] name = "i3more-dwm" path = "src/dwm_main.rs"`. No
new dependencies — uses existing `x11rb`, `gtk4`, `serde_json`, `i3more::ipc`.

## Phases

### Phase 0 — Scaffolding

- Create the binary, wire `init_logging`, connect to i3 IPC, subscribe to
  `window` and `workspace` events, log them. No behaviour changes.
- Confirm i3More launches the daemon (decide: spawned from `i3more` main, or
  `exec --no-startup-id i3more-dwm` in i3 config — match existing daemon
  binaries' pattern).

### Phase 1 — Feature A (titlebar buttons)

- Open X display, select `SubstructureNotifyMask` on root.
- Filter `ClientMessage` events for atoms `_NET_WM_STATE`,
  `_NET_WM_STATE_MAXIMIZED_VERT`, `_NET_WM_STATE_MAXIMIZED_HORZ`,
  `WM_CHANGE_STATE`.
- Resolve the source window → `con_id` via i3 tree, then send
  `[con_id=N] fullscreen toggle` / `move scratchpad`.
- Open question: does i3 already swallow these messages before we see them?
  Test with `xev -root` first to confirm they reach the root.

### Phase 2 — Level 1 (drag-to-resize)

- For each tiling leaf, register a passive Button1 grab on a 4 px strip along
  each shared edge (compute from `get_tree` rects).
- On press, enter drag state; on motion, issue `resize set <px>` or
  `resize grow/shrink` commands throttled to ~30 Hz.
- Re-register grabs on `window::new`, `window::close`, `window::move`,
  `workspace::focus`.

### Phase 3 — Level 2 + 3 (drag-to-swap, drag-to-workspace)

- Generalise the grab from edge-strip to whole-titlebar.
- On drag-start, show overlay highlight on the candidate target as the pointer
  moves (target = `find_container_at(x,y)` from tree).
- On release, if target is another container → `swap container with con_id N`;
  if target is a workspace button (collaborate with `i3more` navigator via a
  small Unix socket or by checking pointer coords against the navigator's
  geometry) → `move container to workspace N`.

### Phase 4 — Level 4 (drag-to-split)

- Extend the overlay to render four drop zones (N/E/S/W) on the hovered target.
- On drop in zone Z: `move container to mark __dwm_tmp`, then `split <h|v>`,
  then `move container ...` — exact sequence depends on tree layout; prototype
  in `dwm/tree.rs` with unit tests over canned tree JSON.

### Phase 5 — Level 5 (floating ↔ tiling)

- On drag-start of a floating window, treat as Level 4 with an extra "floating
  → tile" transition: `floating disable` issued at drop time.
- Reverse direction: dragging a tile to a free area of the screen (no target
  container) → `floating enable, move position <x> <y>`.
- Most uncertain phase; budget time to evaluate forking i3 if IPC gaps surface.

## Files

| File                       | Action  |
|----------------------------|---------|
| `Cargo.toml`               | edit — add `[[bin]] i3more-dwm` |
| `src/dwm_main.rs`          | create — entry point + main loop |
| `src/dwm/x11.rs`           | create — X11 listener (phase 1) |
| `src/dwm/drag.rs`          | create — pointer grabs + drag FSM (phase 2+) |
| `src/dwm/tree.rs`          | create — tree helpers (phase 3+) |
| `src/dwm/overlay.rs`       | create — drop-zone overlay (phase 3+) |
| `src/lib.rs`               | edit — `pub mod dwm;` |
| `~/dotfiles/i3/.config/i3` | edit — `exec --no-startup-id i3more-dwm` |

## Open questions

- Does i3 4.23 forward `_NET_WM_STATE_MAXIMIZED_*` ClientMessages to clients,
  or does it consume them silently? Verify with `xev -root` before phase 1.
- Passive Button1 grabs on the edge strips — do they conflict with app-internal
  drag handles near window edges (text selection, scrollbars)?
- For level 5, can `move container to workspace` plus geometry suffice, or
  does the floating-to-tile transition need a fresh container that only a fork
  can construct cleanly?

## BUG

### Bug 1 — Maximize on VSCode → all subsequent windows open as tabs

**Repro**

1. Workspace with one VSCode window in a splith/splitv parent.
2. Click VSCode's native titlebar maximize button.
3. Open any new window (terminal, browser) on the same workspace.

**Observed**: every new window arrives as a tab next to VSCode.

**Root cause**

Feature A's patch in `vendor/i3/src/handlers.c` does
`con_set_layout(parent, L_TABBED)` — it mutates the **existing parent**
in-place. i3 opens new windows as siblings under the focused container's
parent, so once the parent is tabbed, every new sibling becomes a tab.
Working as coded; surprising as UX.

**Fix — wrap, don't mutate**

Replace the `con_set_layout` call with a tree-shape change:

1. Allocate a fresh container `wrap`, set its `layout = L_TABBED`,
   `last_split_layout = parent->layout`.
2. Detach the focused con from `parent`; attach it as the sole child of
   `wrap`.
3. Attach `wrap` in the focused con's old slot under `parent`.
4. Store a sentinel mark (`__i3more_maxwrap`) on `wrap` so unmaximize can
   find it.

Unmaximize (minimize button, or maximize on already-max'd):

1. Find ancestor `wrap` with the sentinel mark on the focused con.
2. Reparent the single child of `wrap` back to `wrap->parent` in `wrap`'s
   slot.
3. Free `wrap`.

Result: siblings keep their original layout, new windows go to the **old**
parent (still splith/splitv), and the maximized con visually dominates
because the tabbed wrapper has only one child.

**Files**

| File                              | Change                                  |
|-----------------------------------|-----------------------------------------|
| `vendor/i3/src/handlers.c`        | replace `con_set_layout` calls in the `_NET_WM_STATE_MAXIMIZED_*` and `WM_CHANGE_STATE` branches with wrap/unwrap helpers |
| `vendor/i3/src/con.c` (or new file `con_max_wrap.c`) | implement `con_max_wrap(Con *)` / `con_max_unwrap(Con *)` |

**Validation**

- `wmctrl -r ":ACTIVE:" -b toggle,maximized_vert,maximized_horz` on a
  tiled window — focused con should fill the workspace, bar stays
  visible.
- With the window still maxed, `i3-msg exec xterm` — the new xterm should
  appear in the **original** layout next to the wrapper, not as a tab
  inside it.
- Repeat the wmctrl toggle — wrapper should be gone from `get_tree`
  output (no node carrying mark `__i3more_maxwrap`).

### Bug 2 — Clicking a tab causes the window to float

**Repro (to confirm)**

1. Workspace with parent `layout: tabbed`, two children A and B, A focused.
2. Click B's tab header.

**Observed**: B becomes floating.

**Hypothesis**

Not caused by the Feature A patch — `handle_client_message` only matches
`_NET_WM_STATE_MAXIMIZED_*` and `WM_CHANGE_STATE`. Likely culprits, in
order of probability:

1. **Drag threshold in `tiling_drag.c`**: a click with sub-pixel motion
   passes the drag threshold and falls into the floating-conversion path
   that Levels 2/5 will rework. Stock i3 4.25 reworked
   `tiling_drag_threshold_reached` — check whether the threshold is now
   measured in time + distance vs. distance alone, and whether the
   "drag to detach" code path defaults to floating when the drop target
   is the source's own parent.
2. **`click.c` button-press handler** misroutes tab-header clicks because
   our patch added an event handler earlier in the chain that no longer
   returns the right "I handled it" sentinel.

**Diagnostic plan**

1. `i3-dump-log` with `--shmlog-size=10M` set in config; reproduce the
   click and look for `tiling_drag`, `floating_enable`, and any
   `Received ClientMessage` lines.
2. `xev -id <tab parent window>` to confirm only Button1 press/release
   reach i3, no motion in between (rules out drag-threshold theory if
   true).
3. `git -C vendor/i3 log --oneline -- src/tiling_drag.c src/click.c`
   between 4.23 and 4.25-non-git to spot upstream regressions across the
   version jump.

Defer fix until cause is identified — the wrong patch here is worse than
no patch.

## feat: per-workspace-layout-View-i3MoreBar

User wants the current workspace's layout (splith / splitv / tabbed /
stacked) shown on the i3More bar alongside the existing
[common info](03-common-info.md) widgets. Doubles as the visible cue that
Bug 1's fix should preserve: even with the wrap-not-mutate fix, the user
benefits from knowing at a glance "this workspace's root layout is X".

### Scope

- **View only** for this iteration. Toggling layout from the bar is
  deferred (was an earlier draft, removed).
- The "layout" shown is the **focused container's parent layout** — i.e.
  what determines how a *new* window would be inserted. This is the
  source of Bug 1's surprise, so it's the property worth surfacing.

### UI

Single label/icon in the `sysinfo-area` (left side of the bar), placed
between battery and clock so the layout state sits with other
always-visible state. Glyph mapping (Font Awesome 6 Free Solid, already
loaded — `src/fa.rs`):

| Layout    | Glyph (FA name)         | Tooltip      |
|-----------|-------------------------|--------------|
| `splith`  | `table-columns`         | "split horizontal" |
| `splitv`  | `grip-lines`            | "split vertical"   |
| `tabbed`  | `folder` (or `clone`)   | "tabbed"     |
| `stacked` | `layer-group`           | "stacked"    |

Colour `#a89984` to match other sysinfo glyphs; size 11px.

### Data flow

- New thin module `src/layout_indicator.rs`.
- On startup, do an initial `get_tree`, find focused workspace → focused
  con → parent → `layout`, set the label.
- Subscribe (via existing `i3more::ipc`) to:
  - `workspace::focus`     — workspace switch
  - `window::focus`        — focus moved between cons
  - `window::move`         — tree shape may have changed parent
  - `window::new` / `window::close` — same reason
- Each event triggers a tree re-query + label update on the GTK main
  thread (`glib::MainContext::default().invoke_local`).

Cost: one `get_tree` IPC round-trip per relevant event. Negligible.

### Files

| File                          | Action                                  |
|-------------------------------|-----------------------------------------|
| `src/layout_indicator.rs`     | create — `build_layout_indicator()` returns the GTK label + an update closure |
| `src/navigator.rs`            | edit — instantiate label, append into `sysinfo_box` after battery, hook update closure into the existing IPC dispatcher |
| `src/main.rs`                 | edit — call update on i3 events (alongside the existing clock/battery tick) |
| `src/lib.rs` / `src/fa.rs`    | edit — add the FA glyph constants if not already exported |
| `assets/style.css`            | edit — `.layout-indicator { … }` matching `.sysinfo-label` |
| `docs/plan/03-common-info.md` | edit — note layout indicator alongside battery/clock |

### Open questions

- When focus is on a workspace with no windows, what does the indicator
  show? Proposal: hide it (no parent layout to display) rather than
  show a fallback glyph.
- Multi-monitor: which workspace's layout do we show — the focused one,
  or one-per-output? Proposal: focused only, since the rest of sysinfo
  is single-valued.