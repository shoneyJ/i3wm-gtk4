# Wayland support

i3More is currently X11-only. Targeting Wayland means swapping every
"talk to the WM via X11" path for the equivalent Wayland protocol, and
deciding what to do about `vendor/i3` (X11-only by design). The natural
target compositor is **Sway** — wlroots-based, speaks the i3 IPC
protocol over a Unix socket, has the same workspace/tree model — so a
lot of the Rust code carries over once we replace the X11 surface
plumbing.

## Compatibility audit — what works as-is on Sway

Some of i3More is already protocol-agnostic and runs against Sway with
zero source changes:

| Component | Status on Sway |
|---|---|
| `src/ipc.rs` (i3 IPC over Unix socket) | **Works** — Sway implements i3-ipc; we already fall back to `$SWAYSOCK` at `ipc.rs:142` |
| `src/sequencer.rs` (workspace renumber) | **Works** — uses IPC commands only |
| `src/model.rs` (tree parsing) | **Works** — tree JSON format is shared |
| `src/auto_unmax` / `layout_cmd` | **Works** — pure IPC |
| `src/icon.rs`, `src/launcher.rs`, `src/translate.rs` | **Works** — no X11 calls |
| `src/sysinfo.rs`, `src/sequencer.rs`, control panel widgets | **Works** — read sysfs / D-Bus / pactl |
| Notifications daemon (`src/notify/*`) | **D-Bus part works**, popup window placement does not |
| Tray (`src/tray/*`) | **Works** — StatusNotifier is D-Bus, but icons render in a GTK window that needs Wayland layer-shell to dock |

What's broken or needs replacement:

| Site | What it does on X11 | What it needs on Wayland |
|---|---|---|
| `src/main.rs:349-383` | Sets `_NET_WM_WINDOW_TYPE_DOCK` + `_NET_WM_STRUT_PARTIAL` via xprop, forces size with xdotool | `wlr-layer-shell-unstable-v1` — anchor TOP, exclusive_zone 40, layer TOP |
| `src/notify/popup.rs:139-160` | Sets `_NET_WM_WINDOW_TYPE_NOTIFICATION` via xprop | Layer-shell overlay layer, or xdg-popup if anchored to bar |
| `src/popup_translate_main.rs:75-…` | Same xprop dance for translate popup | Layer-shell or xdg-popup |
| `src/keyhint_main.rs:121-260` | `xdotool windowmove` to place the keyhint window | Layer-shell with anchor/margin, or have Sway place via `for_window` |
| `src/lock_main.rs` + `src/lock/x11.rs` | `XGrabKeyboard` / `XGrabPointer` via x11rb to capture input during lock | `ext-session-lock-v1` protocol — proper compositor-blessed lock |
| `vendor/i3` fork | Our entire Feature A patch in `handlers.c` (maximize buttons → tabbed) and the `ipc.c`/`render.c` patches | **None of this applies to Sway.** Sway has its own source tree at `wlroots/sway` — the patches would need re-implementing there |
| `gdk4-x11` dependency | Used in `Cargo.toml` for `gdk4_x11::X11Surface::xid()` downcasts | Drop in favour of `gdk4_wayland` or, better, the GDK4 surface API without X11/Wayland-specific downcasts |
| `xdotool` / `xprop` shellouts | Set window properties post-realize | None — layer-shell handles positioning at surface creation |

## Compositor target

**Sway** — the only viable compositor target for i3More's behavioural
model (i3-ipc, i3-like tree, marks, fullscreen semantics, scratchpad).
We do NOT want to target a different Wayland compositor (Hyprland,
KWin, Mutter) because:

- They don't speak i3-ipc → `src/ipc.rs` would need a per-compositor
  driver, multiplying maintenance.
- They model windows differently (Hyprland's "groups" ≠ i3 tabbed,
  Mutter's monocle ≠ i3 fullscreen) → `auto_unmax`, layout cascade, and
  the maximize-button patches don't translate.

Sway gets us 80% of the way for free. The remaining 20% is replacing
X11 window-plumbing with layer-shell / session-lock.

## Phasing

### Phase 0 — protocol abstraction (no behaviour change)

Introduce a `display` module that hides X11/Wayland behind a small
trait. All call sites that currently downcast to `X11Surface` go
through the trait.

```rust
pub trait DockWindow {
    fn anchor_top(&self, height: i32, screen_width: i32);
    fn anchor_floating(&self, x: i32, y: i32, w: i32, h: i32);
    fn set_window_type(&self, ty: WindowType);
}
```

On X11, the impl wraps the existing xprop/xdotool calls. On Wayland,
the impl drives `gtk4-layer-shell` (the GTK4 wrapper around
`wlr-layer-shell-unstable-v1`).

Files touched:
- `Cargo.toml` — add `gtk4-layer-shell = "0.5"`, gate behind a feature.
- `src/display.rs` — new module, two impls behind `cfg(feature)`.
- `src/main.rs`, `src/keyhint_main.rs`, `src/notify/popup.rs`,
  `src/popup_translate_main.rs` — switch from direct xprop/xdotool to
  the trait.

Risk: low. The X11 impl just calls the existing code, so no behaviour
change on the X11 path. Verifying takes one `just bar-deploy` + visual
inspection.

### Phase 1 — bar on layer-shell

Replace the strut/dock dance in `src/main.rs:349-383` with
`gtk4_layer_shell::LayerShell::init_for_window`:

```rust
window.init_layer_shell();
window.set_layer(Layer::Top);
window.set_anchor(Edge::Top, true);
window.set_anchor(Edge::Left, true);
window.set_anchor(Edge::Right, true);
window.set_exclusive_zone(BAR_HEIGHT);
window.set_namespace("i3more-bar");
```

This both removes the xprop/xdotool shellouts AND makes the bar work
identically on X11 (gtk4-layer-shell silently no-ops on X11 once
detected via the runtime backend) **provided** the X11 path keeps its
existing strut code as a fallback. The simplest split is to call
layer-shell when GDK reports Wayland, fall back to the current xprop
path when GDK reports X11.

Verification:
- X11 — same behaviour as today (visible 40px bar, struts honored).
- Wayland (Sway) — bar at top, workspaces visible, no shellouts.

### Phase 2 — other popups (notification, translate, keyhint, popup-translate)

Each of these uses the same xprop pattern. After Phase 0's trait, the
patches reduce to per-binary one-liners.

- **Notification popups** — `notify/popup.rs` sets `WINDOW_TYPE_NOTIFICATION`. On
  Wayland: layer-shell, layer OVERLAY, anchor top-right with margin.
- **Translate popup** — xdg-popup if anchored to a parent surface
  (translate is invoked from a context, has no obvious anchor → use
  layer-shell with anchor top-right).
- **Keyhint** — `xdotool windowmove` to (x,y). On Wayland: layer-shell
  with anchor=CENTER and explicit margins.
- **Popup-translate** — same as translate.

### Phase 3 — lock on ext-session-lock-v1

This one is a real protocol port. `src/lock/x11.rs` is currently 200+
lines of x11rb input grabbing. On Wayland the compositor itself runs
the lock protocol — the client just submits a surface and the
compositor handles input capture.

Crate: `smithay-client-toolkit` or `ext-session-lock-rs` (which-ever is
mature enough). Pattern:

1. Connect to Wayland display.
2. Bind `ext_session_lock_manager_v1`.
3. Call `lock()` — compositor either grants or denies (denies if
   another lock is already active).
4. For each output, create a lock surface from the GTK window.
5. Render password prompt as today. The compositor enforces "only this
   surface receives input."
6. On successful auth, call `unlock_and_destroy()`.

This replaces ~250 lines of `lock/x11.rs` with ~120 lines of Wayland
protocol code. Worth it — the X11 grab approach has well-known race
conditions (xss-lock workaround in current dotfiles is part of this).

Files: new `src/lock/wayland.rs`, gated by `cfg(target_os)` or runtime
detection.

### Phase 4 — `vendor/i3` decisions

Two paths:

#### 4a. Keep `vendor/i3` as the X11 install, point Wayland users at vanilla Sway

The Feature A patch (`_NET_WM_STATE_MAXIMIZED_*` → tabbed) is X11
EWMH-specific. Sway has its own equivalent decisions about how to
handle CSD-driven maximize requests. Most GTK/Qt apps under Wayland
use `xdg_toplevel.set_maximized` — which Sway already handles as
"resize to fill workspace" rather than tabbed-flip.

If the user accepts that behavioural difference, no patch is needed on
Sway. The justfile's `i3-*` recipes stay X11-only; new recipes can
manage a `vendor/sway` submodule if 4b is later pursued.

#### 4b. Port Feature A + IPC fixes to a `vendor/sway` fork

Sway's source structure differs (wlroots-based, written in C, uses
JSON for config, etc.) but the conceptual changes are similar:

| Patch | Sway location |
|---|---|
| `handlers.c::handle_client_message` | `sway/desktop/xdg_shell.c::handle_state_changed` |
| `ipc.c::last_split_layout` serialisation | `sway/ipc-json.c` (Sway already gets this right upstream — verify) |
| `render.c::deco_height = 0` for L_STACKED/L_TABBED | `sway/desktop/render.c::render_container` |

Effort: comparable to the original i3 patches (~2 days). Defer until
the user actually switches to Sway and runs into the missing
maximize-button behaviour.

**Recommendation: start with 4a.** Add the Sway fork only when the
behavioural gap becomes a real complaint.

## Cross-cutting concerns

### Crate dependencies

| Need | Crate |
|---|---|
| Layer-shell binding for GTK4 | `gtk4-layer-shell` (active, GTK4-native) |
| Session lock protocol | `smithay-client-toolkit` or a focused `ext-session-lock` crate |
| Generic Wayland connection (if not via GTK) | `wayland-client` |
| Replace `gdk4-x11` downcasts | `gdk4-wayland::WaylandSurface` for runtime backend detection; ideally avoid both and use only GDK4 abstract API |

### Feature gating

Two reasonable approaches:

- **Runtime detection** (preferred). At startup, check
  `gdk::Display::default()` — `is::<gdk4_x11::X11Display>()` vs
  `is::<gdk4_wayland::WaylandDisplay>()`. Choose codepath accordingly.
  Single binary serves both displays. Slightly larger.
- **Compile-time features**. `--features wayland` produces a
  Wayland-only build; `--features x11` produces X11-only. Smaller
  binaries, but two installs to maintain.

Go with runtime detection unless binary size becomes a constraint.

### What about Hyprland / other compositors?

Out of scope. If a user wants i3More on Hyprland they can run it via
XWayland (X11 binary on a Wayland compositor — works today, gives them
the bar but no native integration with their compositor's tree).

## Migration story for existing users

Once Phases 0–3 ship:

1. Sway users install `/opt/i3more/bin/*` exactly as today.
2. Their existing config (`~/dotfiles/i3/.config/i3/config`) mostly
   carries over to `~/.config/sway/config` (Sway intentionally
   compatible with i3 config syntax).
3. The maximize-button-flips-to-tabbed behaviour is missing until 4b
   ships — note in docs.
4. The lock daemon (`xss-lock` invocation in i3 config) becomes
   `swayidle` + `i3more-lock` with ext-session-lock — the existing
   `i3more-lock` binary detects Wayland and switches behaviour.

## Effort estimate

| Phase | Effort | Risk |
|---|---|---|
| 0 — abstraction trait | 1 day | Low |
| 1 — bar layer-shell | 1 day | Low |
| 2 — popups (4 binaries) | 2 days | Low |
| 3 — lock on session-lock | 3 days | Medium (protocol learning curve) |
| 4a — defer i3-fork port | 0 days | — |
| 4b — Sway fork patches | 2 days | Medium (separate code review cycle) |

Total without 4b: **~1 week** of focused work. With 4b: ~9 days.

## Order of execution

1. Phase 0 (abstraction) lands first — pure refactor on X11, no
   visible change. Verifies the seam is right before any Wayland code.
2. Phase 1 (bar) — pick a Sway VM / test session, get the bar running
   there. Smallest possible standalone deliverable.
3. Phase 2 (popups) — each binary in isolation. Notification popups
   first since they exercise the layer overlay layer.
4. Phase 3 (lock) — biggest single change. Save for last.
5. Update `docs/build.md`, `docs/plan/login-lock.md`, and the i3
   config recipes in `docs/plan/` to mention the Sway path.

## Validation matrix

| Scenario | Pass criteria |
|---|---|
| Existing X11 install, no rebuild | All binaries behave identically |
| X11 install with new code (runtime-detect path) | Same — should fall through to existing xprop calls |
| Fresh Sway session, `/opt/i3more/bin/*` installed | Bar at top with workspaces / sysinfo / tray, popup translate works, keyhint anchors correctly, notifications appear, lock engages on `i3more-lock` |
| Sway session, multi-output | Bar replicates across outputs (layer-shell per-output) |
| Sway session, `i3-msg layout splitv` (cascade CLI) | Cascade works — pure IPC |

## Open questions

- Does the existing GTK4 CSS still apply on Wayland? Almost certainly
  yes (GTK4 is display-server-agnostic for styling), but verify.
- Does the tray icon area need a different rendering approach? GTK4
  tray widget should work identically; check that mouse-position
  popups (right-click context menus) anchor correctly.
- Hot-output reconfig: i3More currently re-queries `get_outputs` on
  workspace events. On Sway's per-output layer-shell, we may need to
  also handle `wl_output` add/remove explicitly — confirm
  `gtk4-layer-shell` does this transparently or if we need to wire it.

## References

- Sway: https://github.com/swaywm/sway
- `wlr-layer-shell-unstable-v1`: https://wayland.app/protocols/wlr-layer-shell-unstable-v1
- `ext-session-lock-v1`: https://wayland.app/protocols/ext-session-lock-v1
- `gtk4-layer-shell`: https://github.com/wmww/gtk4-layer-shell
- Sway IPC (compatible with i3): https://github.com/swaywm/sway/blob/master/sway/sway-ipc.7.scd
