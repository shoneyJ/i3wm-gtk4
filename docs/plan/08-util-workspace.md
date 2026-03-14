# Workspace Utility (`i3more-ws`)

A lightweight Rust binary replacing `empty_workspace` and `keyhint-2` bash scripts. Uses the shared `i3more::ipc` module for direct i3 socket communication — zero process spawning for workspace operations.

## Motivation

The `empty_workspace` script spawns `i3-msg`, `jq`, `seq`, `sort`, and `grep` to find a free workspace number. The Rust replacement makes two IPC calls over a single Unix socket connection, completing in under 1ms. The `keyhint-2` script spawns `sed`, `grep`, `tr`, and `rofi` — the Rust version parses the config file natively and only spawns rofi.

## Architecture

- **Binary**: `i3more-ws` (defined in `Cargo.toml` as `[[bin]]`)
- **Entry point**: `src/ws_main.rs` (~100 lines, single file)
- **Shared code**: `i3more::ipc` (already exported in `src/lib.rs`)
- **Dependencies**: `std` + `serde_json` (for IPC response parsing) + `dirs` (for config path) — all already in Cargo.toml
- **No GTK**: pure CLI tool

## Subcommands

```
i3more-ws empty      # Switch to first empty workspace (1-20)
i3more-ws keyhints   # Show i3 keybindings in rofi
```

## Files

| File | Action |
|------|--------|
| `src/ws_main.rs` | **Create** — entire binary |
| `Cargo.toml` | **Edit** — add `[[bin]]` entry |

No changes to `src/lib.rs` — `i3more::ipc` is already public.

## Reusable Code

- `i3more::ipc::I3Connection` — `connect()`, `get_workspaces()`, `run_command()` for direct IPC
- `dirs::config_dir()` — for i3 config file auto-detection

## Internal Structure

### Entry point
```
fn main()
    match std::env::args().nth(1).as_deref()
        "empty"    => cmd_empty()
        "keyhints" => cmd_keyhints()
        _          => print_usage(); exit(1)
```

### `cmd_empty()`

```rust
fn cmd_empty() -> Result<(), Box<dyn std::error::Error>>
```

1. `I3Connection::connect()` — single socket connection
2. `conn.get_workspaces()` — returns JSON array
3. Collect occupied workspace numbers into `HashSet<i64>`:
   ```rust
   workspaces.as_array().iter()
       .filter_map(|ws| ws["num"].as_i64())
       .collect::<HashSet<_>>()
   ```
4. `(1..=20).find(|n| !occupied.contains(n))` — first gap
5. `conn.run_command(&format!("workspace number {}", n))` — switch to it
6. If all 20 occupied: print message to stderr, exit 0

**Performance**: Two IPC round-trips over one socket. No process spawning. Expected latency: <1ms.

### `cmd_keyhints()`

```rust
fn cmd_keyhints() -> Result<(), Box<dyn std::error::Error>>
```

1. `find_i3_config()` — locate config file
2. `std::fs::read_to_string(path)` — read entire config
3. Filter lines starting with `bindsym` (after trimming whitespace)
4. For each line: `strip_prefix("bindsym ")` → `split_once(' ')` → `(key, action)`
5. Format as `"{action}: {key}"` (action first for readability in rofi)
6. Join entries with `\n`, pipe to rofi:
   ```
   rofi -dmenu -p "i3 keybindings" -i
   ```
7. Rofi blocks until user selects/cancels — exit code doesn't matter

### `find_i3_config()`

```rust
fn find_i3_config() -> Result<PathBuf, Box<dyn std::error::Error>>
```

Search order:
1. `$XDG_CONFIG_HOME/i3/config` (via `dirs::config_dir()`)
2. `~/.i3/config` (legacy path)
3. `/etc/i3/config` (system default)

Returns error with message listing searched paths if none found.

## i3 Config Changes

```bash
# Before
bindsym $mod+Shift+n exec --no-startup-id ~/.config/i3/scripts/empty_workspace
bindsym F1 exec --no-startup-id ~/.config/i3/scripts/keyhint-2

# After
bindsym $mod+Shift+n exec --no-startup-id i3more-ws empty
bindsym F1 exec --no-startup-id i3more-ws keyhints
```

## Phased Implementation

### Phase 1: Scaffold
1. Create `src/ws_main.rs` with `main()` and usage printer
2. Add `[[bin]]` entry to `Cargo.toml`
3. Verify it compiles and prints usage

### Phase 2: Empty Workspace
1. Implement `cmd_empty()` using `i3more::ipc`
2. Test: `i3more-ws empty` — switches to first empty workspace
3. Test: run with all 10 workspaces open — handles gracefully
4. Test: `time i3more-ws empty` — verify <5ms

### Phase 3: Keybinding Hints
1. Implement `find_i3_config()` with fallback paths
2. Implement `cmd_keyhints()` with config parsing and rofi piping
3. Test: `i3more-ws keyhints` — rofi shows formatted keybinding list
4. Test: verify bindsym lines from config are correctly parsed and formatted

### Phase 4: Integration
1. Build release, copy to `dist/`
2. Update i3 config keybindings
3. Test hotkeys work end-to-end

## Edge Cases

| Scenario | Handling |
|----------|----------|
| i3 not running / socket not found | `I3Connection::connect()` returns error, printed to stderr |
| All 20 workspaces occupied | Print "All workspaces 1-20 are occupied" to stderr, exit 0 |
| i3 config file not found | Return error listing searched paths |
| Config has no bindsym lines | Return error "No keybindings found in i3 config" |
| Malformed bindsym line (no space after key) | Silently skipped by `split_once` returning None |
| rofi not installed | Spawn fails, error printed to stderr |

## Verification

```bash
# Build
docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more-ws dist/"

# Empty workspace
dist/i3more-ws empty      # switches to first empty workspace number
# Open workspaces 1-5, run again — should switch to 6

# Keyhints
dist/i3more-ws keyhints   # rofi appears with searchable keybinding list
# Type "volume" — should filter to volume-related bindings

# Performance
time dist/i3more-ws empty # should be <5ms
```
