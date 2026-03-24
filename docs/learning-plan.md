# Learning Plan: Achieve Your Personal Goals Through i3More

## Context

Your README states four personal goals:
1. **Practice software architecture**
2. **Learn systems programming languages (Rust, C, C++)**
3. **Learn Rust concepts used in the project**
4. **Understand Linux systems better**

This plan maps each goal to concrete tasks in the i3More codebase, ordered from simple to complex, interleaved so you build multiple skills simultaneously.

---

## Track 1: Software Architecture

### 1A — Module Decomposition (Week 2)
- **Study**: Compare `src/audio_main.rs` (single-file binary) vs `src/lock/` (multi-file module). Why did lock need splitting?
- **Build**: Scaffold `i3more-power` per `docs/plan/07-util-power.md` — design `power_main.rs` + `power/mod.rs` + subcommands

### 1B — Event-Driven Bridge Pattern (Week 5)
- **Study**: Trace the X11 thread → mpsc → GTK poll loop in `src/lock_main.rs:54-100`. Same pattern in `src/main.rs` for i3 events
- **Build**: Fix the background widget bug in `src/control_panel/widgets/background.rs` — wallpaper selection doesn't apply changes

### 1C — Multi-Layer Cache Design (Week 9)
- **Study**: Read `src/icon.rs` (328 lines) — 3-tier cache: LRU memory → disk → .desktop resolution
- **Build**: Add cache invalidation when `refresh_desktop_index()` is called (currently caches are never cleared)

### 1D — D-Bus Service Architecture (Week 10)
- **Study**: Compare `src/notify/daemon.rs` and `src/tray/watcher.rs` side-by-side — both serve D-Bus interfaces with `Arc<Mutex>` + mpsc
- **Build**: Write a design doc for headset jack detection: which Linux subsystem to monitor, event flow, module boundaries

---

## Track 2: Systems Programming (Rust)

### 2A — Process Spawning & Error Handling (Week 1)
- **Study**: Read `src/audio_main.rs` — `run_cmd()`, `notify()`, custom `Error` enum
- **Build**: Read brightness from `/sys/class/backlight/*/brightness` + `max_brightness`, compute percentage, display it

### 2B — Interior Mutability: Rc/RefCell (Week 3)
- **Study**: Read `src/lock_main.rs:40-44` — five separate `Rc<RefCell<T>>` for state
- **Build**: Refactor into a single `LockState` struct wrapped in one `Rc<RefCell<LockState>>`

### 2C — Concurrency: Arc/Mutex + mpsc (Week 6)
- **Study**: Read `src/tray/watcher.rs:112-220` — `Arc<Mutex<HashSet>>` shared between async tasks
- **Build**: Create a test harness for lock screen that mocks the X11 key event channel

### 2D — Unsafe FFI (Week 8)
- **Study**: Read `src/lock/x11.rs` (X11 protocol), `src/lock/security.rs` (`mem::forget` for fd leak), `src/main.rs` (`g_source_remove` unsafe)
- **Build**: Read `reference/slock` (C, ~200 lines). Annotate every syscall. Compare with Rust equivalents. Document safety gaps C has that Rust prevents

### 2E — Binary Protocol (Week 4)
- **Study**: Read `src/ipc.rs` (159 lines) — i3 IPC wire format: magic + LE u32 length + LE u32 type + JSON
- **Build**: Write unit tests for IPC serialization — round-trip encoding/decoding, edge cases (empty payload, invalid magic)

---

## Track 3: Rust Concepts

### 3A — Enums as State Machines (Week 2)
- **Study**: Read `KeyAction`, `I3Event`, `NotifyEvent`, `TrayEvent` — each is a channel message type
- **Build**: Design the event enum for `i3more-power` with appropriate payloads

### 3B — Serde Configuration (Week 3)
- **Study**: Read `src/lock/config.rs` (79 lines) and `BackgroundConfig` in background.rs — `#[derive(Serialize, Deserialize)]` + `Default`
- **Build**: Create unified config module with `#[serde(default)]` for backward compat and validation

### 3C — Traits and Generics (Week 5)
- **Study**: Read custom `Error` display impl in audio_main.rs, `#[interface]` proc macro in daemon.rs
- **Build**: Extract a `SystemReader` trait from `src/sysinfo.rs`, implement `SysfsReader` + `MockReader`, refactor stats popover to use `impl SystemReader`

### 3D — Async/Await (Week 7)
- **Study**: Read `run_daemon()` and `run_watcher()` — `async_io::block_on`, `futures_util::future::join`
- **Build**: Replace notification daemon's 50ms polling timer with a proper async channel for immediate wakeup

### 3E — Security Types (Week 9)
- **Study**: Trace `Zeroizing<String>` usage through lock_main.rs handle_key()
- **Build**: Create `SecureBuffer` newtype — max-length check, `push_char() -> Result`, `Debug` prints `[REDACTED]`

---

## Track 4: Linux Systems

### 4A — sysfs and procfs (Week 1)
- **Study**: Read `src/sysinfo.rs` (289 lines) — reads `/sys/class/power_supply`, `/proc/stat`, `/sys/class/thermal`, `/proc/meminfo`
- **Build**: Add disk usage (read `/proc/mounts` + `statvfs`) and network throughput (`/proc/net/dev` delta) to stats popover

### 4B — Unix Sockets (Week 4)
- **Study**: Read `src/ipc.rs` — `UnixStream::connect()`, timeouts, blocking `read_event()`
- **Build**: Create `i3more-ipc-dump` diagnostic tool that subscribes to all i3 events and logs formatted JSON

### 4C — D-Bus (Week 7)
- **Study**: Compare session bus (`notify/daemon.rs`) vs system bus (`lock/security.rs` inhibit_vt_switch)
- **Build**: Add `GetHistory() -> String` method to notification daemon, test with `busctl`

### 4D — X11 Protocol (Week 6)
- **Study**: Read `src/lock/x11.rs` — `RustConnection`, `create_cover_windows()`, `grab_keyboard()`, `resolve_key()`
- **Build**: Add NumLock-aware key resolution for numpad keys (KP_0-KP_9). NumLock state is `KeyButMask::MOD2`. Write tests using existing `make_mapping` helper

### 4E — PAM Authentication (Week 8)
- **Study**: Read `src/lock/auth.rs` (83 lines) + compare with C PAM usage in `reference/slock`
- **Build**: Create PAM integration test using `pam_permit.so` (always succeeds) service file

---

## Recommended Weekly Schedule

| Week | Milestones | What You Build | Key Skills |
|------|-----------|----------------|------------|
| 1 | 2A + 4A | Brightness sysfs reader, disk/network stats | Rust basics, file I/O, procfs |
| 2 | 3A + 1A | Power utility scaffold + event enum | Module design, enum patterns |
| 3 | 3B + 2B | Unified config, lock state refactor | Serde, Rc/RefCell |
| 4 | 4B + 2E | IPC dump tool, protocol tests | Unix sockets, byte manipulation |
| 5 | 1B + 3C | Fix background bug, SystemReader trait | GTK events, traits/generics |
| 6 | 2C + 4D | Lock test harness, NumLock keys | Concurrency, X11 keysyms |
| 7 | 4C + 3D | D-Bus GetHistory, async channels | D-Bus, async/await |
| 8 | 2D + 4E | slock annotation, PAM tests | Unsafe/FFI, PAM |
| 9 | 1C + 3E | Cache invalidation, SecureBuffer | Architecture, security types |
| 10 | 1D | Headset jack detection design | Full architecture design |

---

## Critical Files

| File | Relevant Milestones |
|------|-------------------|
| `src/sysinfo.rs` | 4A, 3C |
| `src/lock_main.rs` | 2B, 2C, 3E |
| `src/lock/x11.rs` | 4D, 2D |
| `src/ipc.rs` | 4B, 2E |
| `src/control_panel/widgets/background.rs` | 1B |
| `src/icon.rs` | 1C |
| `src/notify/daemon.rs` | 4C, 3D, 1D |
| `src/tray/watcher.rs` | 2C, 1D |
| `src/audio_main.rs` | 2A, 3A |
| `src/lock/auth.rs` | 4E |
| `reference/slock` | 2D, 4E |

---

## Verification

For each milestone:
1. **Study tasks**: Summarize what you learned in your own words
2. **Build tasks**: `cargo build --release` must pass, `cargo test` must pass, manual testing where applicable
3. **Architecture tasks**: Review design docs for completeness and trade-off analysis
