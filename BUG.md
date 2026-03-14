# Known Bugs & Fixes

> Bugs 1–4 have been resolved. Bug 5 (Source ID removal panic) is still open.

---

## Bug 1: `load_from_string` does not exist in gtk4-rs 0.10

**Status:** FIXED

**File:** `src/navigator.rs:83`

**Root cause:** `CssProvider::load_from_string()` was added in gtk4-rs 0.12
(GTK 4.12+). The Docker image ships GTK 4.10, so `gtk4` crate v0.10.x is in
use, which only exposes `load_from_data()`.

**Fix:** Replaced `provider.load_from_string(CSS)` with
`provider.load_from_data(CSS)`. The signature is identical (`&str` argument);
`load_from_data` is the equivalent method available in gtk4-rs 0.10.

---

## Bug 2: Unused import `Path` in icon.rs

**Status:** FIXED

**File:** `src/icon.rs:15`

**Root cause:** `std::path::Path` is imported but never used — only `PathBuf`
is needed.

**Fix:** Removed `Path` from the import: changed
`use std::path::{Path, PathBuf};` to `use std::path::PathBuf;`.

---

## Enhancement: Move CSS to an external file

**Status:** FIXED

**Previous state:** The CSS was embedded as a `const CSS: &str` in
`src/navigator.rs` (lines 17-68).

**Requirement:** CSS should live in an external file for easier theming and
editing without recompilation.

**Fix:**

1. Created `assets/style.css` with the contents of the former `CSS` constant.
2. Removed the `const CSS` block from `navigator.rs`.
3. Loaded the CSS at compile time with `include_str!` so the binary remains
   self-contained while the CSS lives in its own file for easy editing:

   ```rust
   // navigator.rs
   let css = include_str!("../assets/style.css");
   provider.load_from_data(css);
   ```

## BUG 3: `RefCell already borrowed` panic in navigator.rs

**Status:** FIXED

**File:** `src/navigator.rs:78–84`

**Root cause:** `render_workspaces` held an immutable borrow of
`Rc<RefCell<NavigatorState>>` (via `state.borrow()`) while iterating over
workspaces. Inside the loop, `build_workspace_entry` called `state.borrow_mut()`
(line 116) to resolve icons, causing a double-borrow panic at runtime.

**Fix:** Clone the workspaces `Vec` and let the temporary immutable borrow drop
immediately, so the `RefCell` is no longer borrowed when `build_workspace_entry`
takes its mutable borrow:

```rust
let workspaces = state.borrow().workspaces.clone();
for ws in &workspaces {
    let entry = build_workspace_entry(ws, state);
    container.append(&entry);
}
```

### Stack trace

```bash
./dist/i3more

thread 'main' (188990) panicked at src/navigator.rs:116:35:
RefCell already borrowed
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'main' (188990) panicked at /rustc/4a4ef493e3a1488c6e321570238084b38948f6db/library/core/src/panicking.rs:225:5:
panic in a function that cannot unwind
stack backtrace:
   0:     0x5e7d4f03c3b2 - <std::sys::backtrace::BacktraceLock::print::DisplayBacktrace as core::fmt::Display>::fmt::h93773fc827e3113d
   1:     0x5e7d4f04d3aa - core::fmt::write::hed7b5c73d82ecb7c
   2:     0x5e7d4f00dfa6 - std::io::Write::write_fmt::h6f0185aecf0ed75f
   3:     0x5e7d4f01b9a9 - std::panicking::default_hook::{{closure}}::h2be84df4f189ae36
   4:     0x5e7d4f01b809 - std::panicking::default_hook::hf0ea8939246f43a9
   5:     0x5e7d4f01bbeb - std::panicking::panic_with_hook::hb4bd9ac1123582a0
   6:     0x5e7d4f01ba9a - std::panicking::panic_handler::{{closure}}::hde00dd15f5637fe2
   7:     0x5e7d4f018b39 - std::sys::backtrace::__rust_end_short_backtrace::hb72197fa777c1785
   8:     0x5e7d4f001afd - __rustc[4425a7e20b4c8619]::rust_begin_unwind
   9:     0x5e7d4f05170d - core::panicking::panic_nounwind_fmt::h0fb754c2e2cbb5f3
  10:     0x5e7d4f05168b - core::panicking::panic_nounwind::h8e1b8f50cfcb7944
  11:     0x5e7d4f051817 - core::panicking::panic_cannot_unwind::h8e14f223f3c2508e
  12:     0x5e7d4ef1fc35 - gio::auto::application::ApplicationExt::connect_activate::activate_trampoline::ha00292415df94db4
  13:     0x75c2daca12fa - g_closure_invoke
  14:     0x75c2dacd090c - <unknown>
  15:     0x75c2dacc1591 - <unknown>
  16:     0x75c2dacc17c1 - g_signal_emit_valist
  17:     0x75c2dacc1883 - g_signal_emit
  18:     0x75c2d9f176a0 - <unknown>
  19:     0x75c2d9f17833 - g_application_run
  20:     0x5e7d4ef0ecb8 - gio::application::ApplicationExtManual::run::h69897303b838d798
  21:     0x5e7d4ef17f02 - i3more::main::h72cdbbfc99ad0e29
  22:     0x5e7d4ef18f73 - std::sys::backtrace::__rust_begin_short_backtrace::h6d64d7b0868adff2
  23:     0x5e7d4ef24149 - std::rt::lang_start::{{closure}}::h152844d4a4d4eb54
  24:     0x5e7d4f00fa26 - std::rt::lang_start_internal::h9f282d832ae47dd5
  25:     0x5e7d4ef188a5 - main
  26:     0x75c2d9a2a1ca - __libc_start_call_main
                               at ./csu/../sysdeps/nptl/libc_start_call_main.h:58:16
  27:     0x75c2d9a2a28b - __libc_start_main_impl
                               at ./csu/../csu/libc-start.c:360:3
  28:     0x5e7d4ef06e65 - _start
  29:                0x0 - <unknown>
thread caused non-unwinding panic. aborting.
Aborted (core dumped)
```

## Bug 4: System tray click does nothing on nm-applet (and similar DBusMenu apps)

**Status:** FIXED

**Files:** `src/tray/render.rs`, `src/tray/dbusmenu.rs` (new), `src/tray/mod.rs`, `~/.config/i3/config`

**Root cause:** Two issues combined:

1. **nm-applet uses DBusMenu, not Activate/ContextMenu.** Introspection of nm-applet's
   SNI interface shows it does **not** expose `Activate` or `ContextMenu` methods at all.
   It only provides `Scroll`, `SecondaryActivate`, and a `Menu` object path pointing to
   `/org/ayatana/NotificationItem/nm_applet/Menu` which implements `com.canonical.dbusmenu`.
   The original click handlers called `Activate`/`ContextMenu` unconditionally, which
   silently failed on nm-applet.

2. **nm-applet does not expose `ItemIsMenu`.** The initial plan assumed `ItemIsMenu=true`
   would signal DBusMenu usage. In reality, nm-applet doesn't have this property at all
   (it's not in the introspection data), so it defaults to `false`. The correct heuristic
   is: **use DBusMenu whenever a `Menu` object path is present**, regardless of `ItemIsMenu`.

3. **i3bar was potentially claiming the system tray.** The i3 config had no `tray_output none`
   in the `bar {}` block, so i3bar's default behavior could block i3More from registering
   `org.kde.StatusNotifierWatcher`.

**Key findings from testing:**

- `gdbus introspect` on nm-applet's SNI object shows only `Scroll` and `SecondaryActivate`
  methods — no `Activate`, no `ContextMenu`, no `ItemIsMenu` property
- `Menu` property returns `/org/ayatana/NotificationItem/nm_applet/Menu`
- `com.canonical.dbusmenu.GetLayout(0, -1, [])` on that path returns a full menu tree
  with items like "Wi-Fi Network", network names, "Disconnect", "VPN Connections", etc.

**Fix:**

1. Added `tray_output none` to `~/.config/i3/config` bar block to stop i3bar from
   claiming the tray.
2. Created `src/tray/dbusmenu.rs` — a `com.canonical.dbusmenu` client that:
   - Calls `GetLayout` to fetch the full menu tree
   - Parses nested `(i, a{sv}, av)` zvariant structures into a `MenuItem` model
   - Builds a `gtk4::PopoverMenu` from `gio::Menu`/`gio::SimpleAction`
   - Sends `Event("clicked", id, ...)` back over D-Bus when items are activated
3. Modified `src/tray/render.rs` — `attach_click_handlers` now checks if a `Menu` path
   exists. If so, left/right click calls `dbusmenu::show_menu()` instead of
   `invoke_item_method()`. Middle click always uses `SecondaryActivate`.

---

## Bug 5: Source ID removal panic (`GLib-CRITICAL`)

**Status:** OPEN

**File:** `glib::source::SourceId::remove` (triggered at runtime)

**Root cause:** A GLib source is being removed after it has already been invalidated. The `SourceId::remove()` call panics because the source ID no longer exists in the GLib main context.

### Stack trace

```bash
./dist/i3more

(i3more:14037): GLib-CRITICAL **: 06:17:38.231: Source ID 19 was not found when attempting to remove it

thread 'main' (14037) panicked at /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/glib-0.21.5/src/source.rs:41:14:
called `Result::unwrap()` on an `Err` value: BoolError { message: "Failed to remove source", filename: "/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/glib-0.21.5/src/source.rs", function: "glib::source::SourceId::remove", line: 37 }
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'main' (14037) panicked at /rustc/4a4ef493e3a1488c6e321570238084b38948f6db/library/core/src/panicking.rs:225:5:
panic in a function that cannot unwind
stack backtrace:
   0:     0x57b06a5ae702 - <std::sys::backtrace::BacktraceLock::print::DisplayBacktrace as core::fmt::Display>::fmt::h93773fc827e3113d
   1:     0x57b06a5bf6fa - core::fmt::write::hed7b5c73d82ecb7c
   2:     0x57b06a5802f6 - std::io::Write::write_fmt::h6f0185aecf0ed75f
   3:     0x57b06a58dcf9 - std::panicking::default_hook::{{closure}}::h2be84df4f189ae36
   4:     0x57b06a58db59 - std::panicking::default_hook::hf0ea8939246f43a9
   5:     0x57b06a58df3b - std::panicking::panic_with_hook::hb4bd9ac1123582a0
   6:     0x57b06a58ddea - std::panicking::panic_handler::{{closure}}::hde00dd15f5637fe2
   7:     0x57b06a58ae89 - std::sys::backtrace::__rust_end_short_backtrace::hb72197fa777c1785
   8:     0x57b06a573e4d - __rustc[4425a7e20b4c8619]::rust_begin_unwind
   9:     0x57b06a5c3a5d - core::panicking::panic_nounwind_fmt::h0fb754c2e2cbb5f3
  10:     0x57b06a5c39db - core::panicking::panic_nounwind::h8e1b8f50cfcb7944
  11:     0x57b06a5c3b67 - core::panicking::panic_cannot_unwind::h8e14f223f3c2508e
  12:     0x57b06a495a76 - glib::source::trampoline_local::h3fed00dbe07b64db
  13:     0x74b5deb454f2 - <unknown>
  14:     0x74b5deb4445e - <unknown>
  15:     0x74b5deba3977 - <unknown>
  16:     0x74b5deb43a23 - g_main_context_iteration
  17:     0x74b5ded1789d - g_application_run
  18:     0x57b06a482f48 - gio::application::ApplicationExtManual::run::h69897303b838d798
  19:     0x57b06a48a162 - i3more::main::h72cdbbfc99ad0e29
  20:     0x57b06a48b2c3 - std::sys::backtrace::__rust_begin_short_backtrace::h6d64d7b0868adff2
  21:     0x57b06a496499 - std::rt::lang_start::{{closure}}::h152844d4a4d4eb54
  22:     0x57b06a581d76 - std::rt::lang_start_internal::h9f282d832ae47dd5
  23:     0x57b06a48abf5 - main
  24:     0x74b5de82a1ca - __libc_start_call_main
                               at ./csu/../sysdeps/nptl/libc_start_call_main.h:58:16
  25:     0x74b5de82a28b - __libc_start_main_impl
                               at ./csu/../csu/libc-start.c:360:3
  26:     0x57b06a478eb5 - _start
  27:                0x0 - <unknown>
thread caused non-unwinding panic. aborting.
Aborted (core dumped)

```
