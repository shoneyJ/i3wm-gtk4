# Custom Wayland Tiling WM (C++23) + i3More Port

## Goal

Replace the vendored i3wm with a self-hosted tiling Wayland compositor written
in C++23 on top of wlroots, while keeping (and porting) the existing i3More
Rust + GTK4 bar and helper binaries as separate Wayland clients.

XWayland is included so existing X11-only applications continue to work during
and after the migration.

## Direction (decided)

| Decision | Choice | Implication |
| --- | --- | --- |
| Compositor base | **wlroots** (C library, used via C++23) | Battle-tested scene graph, XDG/layer-shell/XWayland already implemented; we focus on tiling logic and policy. |
| IPC protocol | **New native IPC** (not i3-compatible) | Cleaner schema designed around Wayland concepts (outputs, surfaces, layouts). Existing i3more Rust code that calls `i3 IPC` must be rewritten against a new client library. |
| Bar & helper binaries | **Stay Rust + GTK4**, ported to Wayland via `gtk4-layer-shell` | Smallest delta; the C++ WM only has to be a good compositor + IPC server. Avoids a parallel rewrite of every widget. |
| v1 scope | **i3 daily-driver parity** | h/v split, tabbed, stacked containers; workspaces per output; keybindings; fullscreen; floating; multi-monitor. No animations / blur / rounded corners. |

## Non-goals (v1)

- Animations, blur, rounded corners, fractional-scaling polish.
- Screen capture / desktop portal support beyond what wlroots gives for free.
- An i3-compatible IPC compatibility shim.
- Mobile / touch gestures.
- Configuration hot-reload (file watch). Reload-on-SIGUSR1 only.

## High-level architecture

```
┌───────────────────────────────────────────────────────────────┐
│                       i3more-wm  (C++23)                       │
│                                                                │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │  wlroots    │  │  Tiling tree │  │  Native IPC server   │  │
│  │  scene/     │◄─┤  (containers,│◄─┤  (Unix SOCK_SEQPACKET│  │
│  │  output/    │  │   workspaces,│  │   + msgpack frames)  │  │
│  │  seat/      │  │   layouts)   │  │                      │  │
│  │  xdg-shell/ │  └──────┬───────┘  └──────────┬───────────┘  │
│  │  layer-shell│         │                     │              │
│  │  xwayland)  │  ┌──────▼───────┐             │              │
│  └─────────────┘  │  Keybinding/ │             │              │
│                   │  config      │             │              │
│                   └──────────────┘             │              │
└────────────────────────────────────────────────┼──────────────┘
                                                 │
            ┌────────────────────────────────────┼──────────────┐
            │                                    │              │
   ┌────────▼────────┐ ┌──────────────┐ ┌────────▼────────┐
   │ i3more (bar)    │ │ i3more-      │ │ i3more-audio /  │
   │ Rust + GTK4     │ │ translate    │ │ launcher / lock │
   │ + layer-shell   │ │ (popup)      │ │ (popup / OSD)   │
   └─────────────────┘ └──────────────┘ └─────────────────┘
            │                  │                  │
            └──────────┬───────┴──────────────────┘
                       │
                Native IPC client crate
                (new Rust crate: i3more-ipc)
```

## Tech stack

### Compositor (`i3more-wm`)

- Language: **C++23** (modules where supported, `std::expected`, ranges, coroutines for async IPC).
- Build: **CMake + Ninja**. Reproducible via the existing Docker dev image.
- Core deps (system packages on Ubuntu Server):
  - `libwlroots-dev` (target the version Ubuntu LTS ships; consider pinning via submodule if features lag).
  - `wayland-protocols`, `libwayland-dev`.
  - `libxkbcommon-dev`, `libinput-dev`, `libpixman-1-dev`, `libdrm-dev`, `libudev-dev`, `libseat-dev`.
  - `libxcb1-dev` + `xwayland` for X11 compatibility.
- Logging: structured logs to stderr; level via env var.
- Test: GoogleTest for unit logic (tiling tree, layout math); a headless wlroots backend (`WLR_BACKENDS=headless`) for integration tests.

### Native IPC

- Transport: Unix domain socket, `SOCK_SEQPACKET` (preserves message framing without length-prefix bookkeeping).
- Wire format: **msgpack** (compact, schema-friendly, broad language support).
- Discovery: `$I3MORE_WM_SOCK` env var, falls back to `$XDG_RUNTIME_DIR/i3more-wm.sock`.
- Schema versioned at the protocol level (`version` field in handshake). Backwards-incompatible changes bump it.
- Schema lives in `proto/i3more-wm.proto.md` (human-readable spec) and is the source of truth for both the C++ server and the Rust client crate.

### Bar / helpers (Rust)

- Continue using **GTK4** via `gtk4-rs`.
- Switch from X11 docking / strut hints to **`gtk4-layer-shell`** to anchor the bar to the bottom edge of each output.
- Replace all `i3_ipc` calls with a new **`i3more-ipc`** Rust crate (client for the native protocol).
- Per-binary impact:
  - `i3more` (main bar): workspace/window state from new IPC; layer-shell anchor.
  - `i3more-launcher`, `i3more-translate`, `i3more-audio`, `i3more-lock`: independent popups — layer-shell only, minimal IPC use.
  - `i3more-speech-text`: unaffected (PipeWire + whisper, no WM coupling).

## IPC: minimum message set for v1

| Direction | Message | Purpose |
| --- | --- | --- |
| C→S | `subscribe(topics[])` | Subscribe to events: `workspace`, `window`, `output`, `binding`. |
| C→S | `get_outputs` | List physical outputs with geometry & active workspace. |
| C→S | `get_workspaces` | All workspaces with focus / urgency / output. |
| C→S | `get_tree` | Full container tree (for the bar's window list). |
| C→S | `run_command(str)` | i3-style command parser (`workspace 3`, `kill`, `split h`, …). |
| S→C | `event:workspace` | Focus / init / empty / urgent transitions. |
| S→C | `event:window` | New / close / focus / title / fullscreen. |
| S→C | `event:output` | Hotplug / mode change. |
| S→C | `event:binding` | Keybinding fired (for OSD hooks). |

The schema is intentionally shaped like i3's but in msgpack and with cleaner
field names — porting the bar is a renaming exercise, not a redesign.

## Tiling model (v1)

Reuse i3's mental model directly so muscle memory survives:

- **Container tree**: every node is either a leaf (window) or an inner node with a layout (`splith`, `splitv`, `tabbed`, `stacked`).
- **Workspaces**: per-output, numbered, named. Move workspace between outputs.
- **Floating layer**: per-workspace floating list, rendered above tiled layer.
- **Fullscreen**: per-workspace and global.
- **Focus stack**: most-recently-focused per container.

Keybindings & commands mirror i3 syntax where reasonable so the existing
`~/.config/i3/config` style can be reused with minimal translation (a small
`i3-config-to-i3more-wm` converter is a stretch goal, not v1).

## Phases / milestones

Each phase is a checkpoint where the work is dogfoodable to some degree.

### Phase 0 — Skeleton & dev loop (1–2 weeks)

- CMake project, Docker dev container, headless wlroots backend smoke test.
- Empty compositor renders a solid background, accepts a single XDG client.
- Logging, config-file stub, signal handling.
- **Exit criteria**: `weston-terminal` opens inside `i3more-wm` running on a nested DRM/X11 backend.

### Phase 1 — Single-output tiling (2–3 weeks)

- Tiling tree data structure + h/v split.
- Keyboard input via `libinput` + `xkbcommon`, hard-coded bindings.
- Workspaces (numbered 1–10) on a single output.
- Focus model + window borders.
- **Exit criteria**: can open 3 terminals, split, navigate, switch workspaces. No bar yet.

### Phase 2 — Native IPC + bar port (2–3 weeks)

- IPC server with the v1 message set above.
- New `i3more-ipc` Rust crate.
- Port `i3more` (main bar) off `src/ipc.rs` onto `i3more-ipc`; switch to `gtk4-layer-shell`.
- **Exit criteria**: bar shows live workspace / window state under `i3more-wm`.

### Phase 3 — Daily-driver parity (3–4 weeks)

- Tabbed + stacked layouts.
- Multi-output: per-output workspaces, hotplug, move-workspace-to-output.
- Floating windows + drag/resize.
- Global + per-workspace fullscreen.
- XWayland integration.
- Config file (i3-like syntax, parsed at startup) for keybindings and basic rules.
- **Exit criteria**: user can switch their daily session from i3wm to `i3more-wm` for a full workday.

### Phase 4 — Helper-binary port (1–2 weeks)

- `i3more-launcher`, `i3more-translate`, `i3more-audio`, `i3more-lock` ported to layer-shell.
- `i3more-speech-text` re-verified (should be a no-op aside from build).
- Remove `src/ipc.rs` and any remaining i3-IPC dependencies.

### Phase 5 — Polish & hardening (open-ended)

- Crash recovery (state snapshot every N seconds; replay on restart).
- Config hot-reload on SIGUSR1.
- Per-app rules (`for_window` equivalent).
- IPC schema v2 once real usage exposes friction.

## Repo layout (proposed)

```
i3More/
├── wm/                          # NEW: C++23 compositor
│   ├── CMakeLists.txt
│   ├── src/
│   │   ├── main.cpp
│   │   ├── server.{cpp,hpp}
│   │   ├── tiling/              # tree, layouts, workspaces
│   │   ├── input/               # keyboard, pointer, bindings
│   │   ├── output/              # output mgr, hotplug
│   │   ├── ipc/                 # IPC server, msgpack codec
│   │   └── xwayland/
│   └── tests/
├── proto/
│   └── i3more-wm.proto.md       # IPC schema spec (source of truth)
├── crates/
│   └── i3more-ipc/              # NEW: Rust client crate
│       ├── Cargo.toml
│       └── src/lib.rs
├── src/                         # existing Rust bar + helpers
└── docs/plan/xwayland.md        # this doc
```

## Risks & mitigations

| Risk | Mitigation |
| --- | --- |
| wlroots is C with a fast-moving API; C++23 wrapping is non-trivial. | Keep a thin RAII wrapper layer (`wlr_*` → `Wlr*`) rather than a deep abstraction. Pin wlroots version; bump deliberately. |
| Two-way porting (compositor + bar) before either works is a long stretch with no dogfood signal. | Phase ordering ensures Phase 1 is dogfoodable for terminals before the bar is touched, and Phase 2 ships a working bar before tabbed/stacked layouts. |
| Native IPC schema churn breaks the bar repeatedly during Phase 2. | Lock the v1 schema in `proto/` before writing the Rust client; treat changes as protocol-version bumps from day one. |
| Daily-driver replacement risks losing work if the WM crashes mid-session. | Phase 5 snapshot/restore; until then, keep i3wm installed as a fallback session entry. |
| C++23 toolchain availability on Ubuntu LTS. | Build inside the existing Docker dev container with a known clang/libc++; deploy as a static-ish binary. |

## Open questions (to resolve before Phase 0)

1. **wlroots versioning**: track Ubuntu's `libwlroots-dev`, or vendor as a submodule like `vendor/whisper.cpp`? Submodule gives reproducibility but adds a build step.
2. **Config file format**: i3-like plain text (familiar) vs TOML (typed, easier parser). Default leaning: i3-like for v1, TOML later.
3. **Session integration**: ship a `i3more-wm.desktop` session file so GDM/lightdm can launch it directly?
4. **XWayland scope**: launch on demand (`lazy = true`) or always-on? Lazy saves memory but adds a first-launch hiccup.

## Reference material

- wlroots: <https://gitlab.freedesktop.org/wlroots/wlroots>
- Wayland core: <https://gitlab.freedesktop.org/wayland/wayland>
- Wayland protocols: <https://gitlab.freedesktop.org/wayland/wayland-protocols>
- TinyWL (wlroots' own minimal reference compositor): in `wlroots/tinywl/` — the obvious starting point for Phase 0.
- gtk4-layer-shell (for the bar): <https://github.com/wmww/gtk4-layer-shell>
- Sway (i3-compatible Wayland WM in C, built on wlroots): <https://github.com/swaywm/sway> — reference for tiling tree, IPC ergonomics, XWayland handling. Read, don't copy.
