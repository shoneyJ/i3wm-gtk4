# Production-grade i3 fork (vendor/i3)

Companion to `production-build.md`. Looks at the patched i3 source at
`vendor/i3` and identifies what's worth changing given how this user
runs i3 — `i3More` handles the bar, notifications, launcher, lock,
keyhint, and most decoration concerns, so several i3 sub-binaries and
code paths are dead weight on this host.

Reference data (current production install):

```
$ ls -la /usr/local/bin/i3* | awk '{print $5, $9}'
681280  /usr/local/bin/i3
147424  /usr/local/bin/i3bar
124608  /usr/local/bin/i3-config-wizard
 31840  /usr/local/bin/i3-dump-log
 63112  /usr/local/bin/i3-input
 32504  /usr/local/bin/i3-msg
 69280  /usr/local/bin/i3-nagbar
  7812  /usr/local/bin/i3-save-tree

# i3 main binary section sizes (release, no LTO):
$ size /usr/local/bin/i3
text=559925  data=28720  bss=7816  total=596461 (~582 KB)
```

i3 sources: ~28.7k LOC in `vendor/i3/src/`. Meson default is `release`
buildtype with no LTO.

## Patches already applied (recap)

| File | Patch | Status |
|---|---|---|
| `src/handlers.c` | Feature A — `_NET_WM_STATE_MAXIMIZED_*` and `WM_CHANGE_STATE → ICONIC` mapped to parent-layout flip + `_i3more_maxed_<ws>` mark | shipped |
| `src/ipc.c` | Fixed upstream bug — `last_split_layout` was serialising `con->layout` instead of `con->last_split_layout` (caused auto-unmax to always revert to splith) | shipped — worth upstreaming |
| `src/render.c` | `p->deco_height = 0` in `render_con_stacked` and `render_con_tabbed` so the in-tree title strip vanishes for grouped layouts | shipped |

## Optimisation candidates

### 1. Enable LTO on the i3 build — quick win

`vendor/i3` uses meson; LTO is one flag. Edit `justfile`:

```just
# in i3-build recipe, add -Db_lto=true:
meson setup {{i3_build}} {{i3_src}} \
    --buildtype=release -Ddocs=false -Dmans=false -Db_lto=true
```

Expected: i3 binary 681 KB → 500–550 KB, similar wins on the other
binaries. Comes essentially free other than a slower build (+10–20 s).

### 2. Strip symbols on install

meson can strip at install time:

```just
sudo meson install -C {{i3_build}} --strip
```

Or pass `--Dstrip=true` at setup. Both i3 and the secondary binaries
shed another ~20–30% size.

### 3. Skip building binaries we don't use

| Binary | Size | We use it? |
|---|---|---|
| `i3bar` | 147 KB | No — i3More is the bar |
| `i3-config-wizard` | 124 KB | No |
| `i3-nagbar` | 69 KB | **Yes, indirectly** — i3's main process spawns it on config errors (see `src/util.c::start_nagbar`, `src/config_parser.c:847`). Removing the binary makes config errors silently swallowed |
| `i3-input` | 63 KB | No — i3More launcher / translate cover the same surface |
| `i3-dump-log` | 32 KB | Sometimes — useful for debugging the Feature A patch |
| `i3-msg` | 33 KB | **Yes, daily** — we shell out to it from scripts and the layout CLI fallback |
| `i3-save-tree` | 8 KB | No |

i3's meson.build doesn't gate executables behind options today — each
target is unconditionally `install: true`. To skip them we'd patch
`meson.build` to wrap them in `if get_option('build_bar')` etc., add
matching entries in `meson_options.txt`, and pass `-Dbar=false` from
the justfile.

The patch is small (~30 lines of meson). Worth it for `i3bar`,
`i3-config-wizard`, `i3-input` (260 KB saved, three less things on
`PATH`). Skip `i3-nagbar` removal unless we also redirect its callers
in `src/util.c` to use `notify-send` or an `i3more-notify` helper —
otherwise we lose all config-error feedback.

### 4. Replace `i3-nagbar` with `notify-send` — UX upgrade

`src/util.c::start_nagbar` currently fork+execs `i3-nagbar` with
arguments describing the error. Replace with a `notify-send` call (or
better, `/opt/i3more/bin/i3more-notify` once we have it) so config
errors appear in i3More's notification panel instead of a stand-alone
red bar that obscures the user's bar.

Files: `src/util.c` (the `start_nagbar` function, ~60 lines). Risk:
medium — the function is called from several sites (config errors,
key-binding conflicts) and the callers expect a PID back to track. Need
to either keep the fork-exec dance but with notify-send as the target,
or refactor the callers.

### 5. Compile-time strip of unused subsystems

i3 unconditionally compiles a few subsystems we don't use:

| Subsystem | LOC | Used by this setup? |
|---|---|---|
| `restore_layout.c` | ~750 | No (we don't use `i3-save-tree`/`append_layout`) |
| `load_layout.c` | ~350 | No (same as above) |
| `i3-config-wizard/` | ~1500 | No — own binary, skip via #3 |
| `i3bar/` | ~5000 | No — own binary, skip via #3 |
| Drag-tile (`tiling_drag.c`, ~600 lines) | — | **Partial** — user has `tiling_drag modifier titlebar` configured, but our render.c patch hides titlebars in grouped layouts. Only splith/splitv leaves have draggable area when border style is `normal`; user has `pixel 1` so this is effectively dead too |

`restore_layout.c`/`load_layout.c` could be `#ifdef I3MORE_NO_LAYOUT_RESTORE`-gated
behind a meson option. Saves a few KB and removes the codepaths that
read JSON layout snapshots. Low priority — keep them in case the user
ever wants to use saved layouts.

### 6. Quieter logging

i3's `DLOG()` macro writes to a shared-memory buffer (no disk by
default) unless `--shmlog-size` is configured. So baseline log noise
costs little. **However**, every event still computes the format
string for `DLOG`. The `log.h` macros could be gated by
`#ifdef NDEBUG` to evaluate to nothing in release builds. Real but
modest perf win — i3 isn't CPU-bound.

If this user ever runs with a populated shmlog, the patch in
`handlers.c` adds two `DLOG` lines per maximize/unmax. They're not in
a hot loop, so impact is negligible.

### 7. Patch hygiene — `handlers.c` mark buffer size

Our `max_set_mark` uses a 128-byte stack buffer:

```c
char mark_name[128];
snprintf(mark_name, sizeof(mark_name), "_i3more_maxed_%s", ws->name);
```

Workspace names are user-controlled (config / `rename workspace`). 128
is comfortable for any sane workspace name but truncation would silently
break the mark namespacing if exceeded. Two cheap improvements:

- Use a fmt that errors / fallbacks on truncation (`snprintf` returns
  written length; check it).
- Or move to `sasprintf` (i3's own heap-printf helper used elsewhere)
  to avoid the limit entirely.

Cost: 4 lines. Worth it.

### 8. Patch hygiene — `render.c` `p->deco_height = 0`

The patch mutates the shared `render_params *p` which is passed by
pointer across sibling rendering. The mutation is **idempotent**
(first call sets to 0, every subsequent call sees 0), but it's the
kind of cross-sibling state coupling that breaks if someone later adds
another consumer of `p->deco_height` between the function entry and
the per-child block.

Safer pattern: keep `p->deco_height` untouched and instead use a local
`const int deco_height = 0;` plus replace references inside this
function. Three replacements per function. Cost: trivial.

### 9. Upstream the `ipc.c` `last_split_layout` fix

The bug we fixed is in upstream i3 (`con->layout` should be
`con->last_split_layout` at line 482-490 of `src/ipc.c`). One-line fix.
Worth a PR to upstream so the next vendor bump doesn't regress.

Cost: 1 hour for PR + issue write-up. Optional.

### 10. meson `buildtype=release` is fine; consider `=releaseminsize`

`meson_options.txt` lets us override buildtype. `release` optimises for
speed; `releaseminsize` would shave a bit more size at the cost of some
inline expansion. Probably not worth it — i3 isn't size-critical and
LTO + strip already provides most of the win.

## Recommended order

1. **LTO + strip-on-install** (10 minutes total — `justfile` edits to
   the `i3-build` / `i3-install` recipes). Two binary-size wins for the
   price of zero risk.
2. **Tighten the two patch hygiene items (#7, #8)** (~15 minutes,
   `handlers.c` + `render.c`). No behaviour change, just defensive.
3. **Upstream the `ipc.c` fix** (#9) — when next motivated to interact
   with the upstream i3 community.
4. **Skip `i3bar` / `i3-config-wizard` / `i3-input` at build** (#3,
   without the nagbar removal) — ~1 hour patching `meson.build` +
   `meson_options.txt`. Saves ~260 KB and three unused binaries.
5. **Replace `i3-nagbar` with notify-send** (#4) — only if config-error
   UX in the existing red bar is actively painful.

The rest (compile-time DLOG strip, load_layout removal, releaseminsize)
are diminishing returns — leave them on the list but don't prioritise.

## Validation hooks

```bash
# Binary size + segment breakdown
size /usr/local/bin/i3
ls -la /usr/local/bin/i3*

# Verify LTO actually triggered
nm -D /usr/local/bin/i3 | wc -l    # fewer exported symbols after LTO
file /usr/local/bin/i3              # "stripped" after meson install --strip

# Confirm patches survived rebuild
i3-msg -t get_version | python3 -c \
    'import sys,json; print(json.load(sys.stdin)["human_readable"])'
md5sum /usr/local/bin/i3 vendor/i3/build/i3      # should match
```
