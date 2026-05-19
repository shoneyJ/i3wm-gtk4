# Production-grade build improvements

Triage of changes that would make the i3More binaries leaner, faster, and
quieter for daily use. Roughly ordered by impact-per-effort; the top of
the list is mechanical, the bottom is real refactoring.

## 1. Cargo profile + log feature gate — highest impact, lowest effort

`Cargo.toml`:

```toml
[profile.release]
lto = "fat"            # whole-program optimization; the biggest single
                       # perf win for a GTK app
codegen-units = 1      # better inlining at the cost of build time
strip = "symbols"      # ~40% smaller binaries, no runtime effect
panic = "abort"        # smaller code, faster unwind-free paths;
                       # we don't catch panics anywhere anyway

[dependencies]
log = { version = "0.4", features = ["release_max_level_info"] }
# or "release_max_level_warn" to strip info! too. This is what actually
# removes logging cost — the macros become no-ops at compile time, so
# the format!() args inside them are never evaluated. Runtime filtering
# (env_logger) still allocates the args even when filtered.
```

Expected on this codebase: `i3more` binary drops from ~8 MB to ~3–4 MB,
hot-path runtime improves ~5–15%. Single file change.

## 2. Cleanup of compiler warnings — hygiene

19 warnings at last `cargo build --release`. Most are dead code that
accumulated as features stabilised:

| Site | What |
|---|---|
| `src/ipc.rs::EVENT_WINDOW` | constant never read |
| `src/navigator.rs::NotifyHandles::bell_label` | field never read |
| `src/notify/types.rs::NotifyEvent::ActionInvoked` | variant never constructed |
| `src/tray/dbusmenu.rs::MenuItem::toggle_type`, `toggle_state` | fields never read |
| `src/tray/types.rs::TrayEvent::ItemUpdated` | variant never constructed |
| `src/main.rs` + others | 2 fixable-by-cargo-fix suggestions |

Delete or `#[allow(dead_code)]` with a reason. ~30 min, low risk.

## 3. Single-pass tree walk — measurable perf

`refresh_state` in `src/main.rs` triggers three independent `get_tree`
consumers:

- `layout_indicator::update_from_tree` → walks tree finding focused leaf
- `auto_unmax::revert_command` → walks tree looking for marked maxwrap
- `model::build_workspace_state` → walks tree collecting per-workspace
  classes + con IDs + focused class

Three full traversals per workspace/window/binding event. Fold into one
pass that emits a struct carrying all three results. The traversal is
linear in tree size (~10–50 nodes typically), so the absolute saving is
small — but a `binding` event fires on EVERY keypress in i3, so this
adds up under heavy use.

Files:

| File | Change |
|---|---|
| `src/model.rs` | add a `TreeSnapshot` that holds workspace_state + focused_class + focused_parent_layout + maxwrap_revert_cmd |
| `src/main.rs::refresh_state` | call a single `analyze_tree(&tree)` and dispatch its outputs |
| `src/layout_indicator.rs`, `src/auto_unmax.rs` | consume the snapshot instead of doing their own walks |

## 4. Workspace re-render diffing — UX-visible

`navigator::render_workspaces` drops every child of the workspace
container and rebuilds Label/Image/GestureClick widgets on every event.
For the typical 5–10 workspace count it's invisible. For users with
20+ workspaces or rapid event bursts (e.g., autostart spawning 30
windows), the rebuild is a visible jank source.

Approach: keep a `HashMap<i64, gtk4::Box>` of workspace_num →
entry_widget. On render, compute the diff and patch existing widgets in
place (set_text, set_visible, add/remove css class). Only construct/
destroy widgets for added/removed workspaces.

Largest change in this list — would touch most of `navigator.rs`. Defer
until measured jank justifies the rewrite.

## 5. Persistent IPC connection — minor

`crate::ipc::I3Connection::connect()` opens a fresh Unix socket on every
outbound command (popover button, focus-cycle, auto-unmax, layout CLI).
Cost is a few syscalls per call — negligible for human-triggered
events, sums up if any code path runs commands in a loop.

Approach: keep a single long-lived "command" connection in main.rs's
state, separate from the "event subscription" connection that
`listen_events` owns. Pass it to handlers that need to send commands.

Watch for: GTK main thread can't share a `Mutex<I3Connection>` cleanly
across thread spawns; the focus-cycle code currently spawns a thread
specifically because it needs IPC. Pick one ownership model
(`Rc<RefCell>` on main thread, or worker thread + mpsc).

## 6. Thread-per-click in focus cycle — cleanup

`navigator::focus_next_of_class` spawns a fresh OS thread for one
`get_tree` + `run_command`. Each thread runs ~1 ms then exits. Cheap
individually; piles up if user spams icons.

Options:
- Run synchronously on the GTK thread (i3-msg round-trip locally is
  sub-ms; the visible lag is below the click frame).
- Reuse a single mpsc worker — send `Vec<i64>` over a channel and have
  one background thread serialise the requests.

Pairs naturally with #5.

## 7. Logging volume — quieter daemon

After the `release_max_level_info`/`warn` feature gate in #1, the
release binary is already much quieter. Beyond that, audit current
`log::info!` calls and demote anything that fires per-event to
`log::debug!`:

- `src/main.rs`: "Received i3 event …", "Switching to workspace …"
- `src/auto_unmax.rs`: "auto-unmax: …"
- `src/sequencer.rs`: per-rename log
- `src/tray/watcher.rs`: per-SNI-register log

Keep `info!` for: startup, daemon lifecycle, error recovery, the
i3-fork install/restart. Combined with the compile-time gate, a typical
release session produces only a handful of log lines per day.

## 8. Startup path `unwrap` / `expect` audit — robustness

A few panic points hide in startup code:

| File | Site |
|---|---|
| `src/navigator.rs` | `expect("Could not get display")` on no GDK display |
| `src/layout_cmd.rs::collect_proxy_children` | `nodes.unwrap()` after has_children check (provably safe, but reviewer-hostile) |
| `src/main.rs::query_initial_state` | `?` chain that exits with `std::process::exit(1)` on first failure |

The `expect` is the only one that can fire in a real failure mode (no
X server). Replace with a graceful exit + clear stderr message. The
`unwrap()` after a length check is fine but `if let Some(kids) = nodes
{ for child in kids ... }` reads cleaner.

## 9. Icon resolver cache persistence — startup latency

`IconResolver` builds an in-memory cache at startup by scanning desktop
files and icon themes. Cold start is the slow path (hundreds of ms on
spinning disks, ~50 ms on SSD). Persist the cache to
`~/.cache/i3more/icon-cache.bincode` and load it at startup; rebuild
asynchronously when desktop files change.

Touchpoints: `src/icon.rs`, `src/main.rs` (startup ordering — render
the bar with cached icons, hot-swap any updates). Most complex item on
this list; skip unless cold-start latency is a felt pain point.

## Order of execution

1. **Cargo.toml + log feature** (1 file, ~10 min). Immediate measurable
   win. Verify with `ls -la /opt/i3more/bin/i3more` before/after.
2. **Cleanup warnings** (~30 min, low risk). Bring the build clean so
   future regressions stand out.
3. **Single-pass tree walk** (~1–2 hours, real perf win on busy
   sessions, easy to verify with `RUST_LOG=debug` comparing tree
   walks per event before vs. after).
4. **Logging volume audit** (~30 min). Pair with the feature gate from
   step 1 for a quiet release binary + a verbose dev binary toggled
   via `RUST_LOG`.
5. **Persistent IPC connection + thread-per-click cleanup** (paired,
   ~2 hours). Only worth it after measuring contention.
6. **Workspace re-render diffing** (longest, last). Defer until felt.
7. **Icon cache persistence** — optional, only if cold-start ever
   becomes a pain point.

## Validation

Before/after metrics worth capturing for the top three items:

```bash
# Binary size
ls -la /opt/i3more/bin/i3more

# Startup time (release binary, cold cache)
time /opt/i3more/bin/i3more &
sleep 1; killall i3more

# Per-event tree walks — temporary debug log in refresh_state:
#   log::debug!("refresh_state: tree size={} walks={}", n_nodes, n_walks);
# then RUST_LOG=debug and exercise the bar.

# Memory baseline (long-running)
ps -o pid,rss,vsz,cmd -p "$(pgrep -f /opt/i3more/bin/i3more)"
```
