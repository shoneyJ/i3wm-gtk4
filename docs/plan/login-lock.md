# Login Manager DM gdm, Lock session

On ubuntu server, need to have an i3WM compatible display manager which boots during startup.

- It can have option to choose the desktop session, and user to login.
- Authenticate with PAM.
- On locking the system, similar UI design lock user session app should display.
  -- Lock session can only have optioin to enter password.
  -- It should be configured in i3 congfig's lock keybinding mod + L.

## Phase 1 : Lock session - Analysis

### 1. Existing Lock Screen Options

**i3lock** (`/usr/bin/i3lock`, v2.14.1 installed)

- Functional but extremely limited UI — flat colour or a single image, no text input feedback, no theming.
- Cannot render GTK4 widgets, so it will never match the i3More control-panel aesthetic.
- PAM config at `/etc/pam.d/i3lock` simply includes `login` — proves PAM-based auth works at user level.

**GDM lock (`gnome-screensaver` / `gdm3`)**

- GDM3 is installed (`46.2-1ubuntu1`), but its lock path is tightly coupled to the GNOME session and `gnome-shell`.
- Invoking `gdm` lock from an i3 session is unreliable — it expects a full GNOME environment (DBus services, `gnome-shell`, `gnome-session`).
- Not feasible without pulling in the entire GNOME stack.

**Conclusion:** Build a custom `i3more-lock` binary that uses PAM directly, with a GTK4 UI consistent with the existing control panel.

### 2. PAM Authentication Flow

- Create a dedicated PAM service file: `/etc/pam.d/i3more-lock`

  ```
  # /etc/pam.d/i3more-lock
  auth    include   login
  account include   login
  ```

  This mirrors what i3lock does — delegates to the system `login` stack so password, LDAP, fingerprint, etc. all work automatically.

- Use the **`pam-client`** crate (pure-Rust PAM bindings).
  - Open a PAM conversation with service name `"i3more-lock"`.
  - Supply the username from `$USER` / `libc::getlogin`.
  - On `authenticate()` success → release the grab and destroy the lock window.
  - On failure → clear the password buffer, shake/flash the UI, allow retry.

- **No root required** — PAM authenticate works for the calling user as long as the service file exists and the binary has no `setuid` requirement. The existing `/etc/pam.d/i3lock` proves this pattern works on this system.

### 3. X11 Lock Screen Architecture

**Input isolation (security-critical)**

- `XGrabKeyboard` + `XGrabPointer` on the lock window to capture all input.
- If either grab fails (another client holds it), retry in a loop with backoff; if it fails persistently, fall back to `i3lock` as a safety net rather than leaving the session unlocked.

**Window setup**

- Create a full-screen **override-redirect** window via `x11rb` on every connected output.
- Override-redirect bypasses the window manager — i3 cannot move, resize, or close it.
- Set `_NET_WM_WINDOW_TYPE_SPLASH` and raise with `XRaiseWindow` to stay on top.

**Rendering**

- Embed a GTK4 drawing surface inside the X11 window using `gdk4-x11` (already a dependency).
- Render a password entry widget, clock, and user avatar — consistent with the i3More design language.
- GTK4 handles HiDPI scaling, accessibility, and keyboard layout display.

**Session integration**

- Register with `systemd-logind` via DBus (`zbus`, already a dependency) for:
  - `Lock` / `Unlock` signals — lock on `loginctl lock-session`.
  - `PrepareForSleep` — lock before suspend.
- Compatible with **`xss-lock`** — i3 users typically bind: `exec --no-startup-id xss-lock -- i3more-lock` in i3 config.

### 4. Security Requirements Checklist

| Requirement               | Approach                                                                                                        |
| ------------------------- | --------------------------------------------------------------------------------------------------------------- |
| **Grab safety**           | Verify `XGrabKeyboard` + `XGrabPointer` succeed before accepting input; abort-to-i3lock on failure              |
| **VT switch prevention**  | Use `logind` `Inhibitor` lock (type `handle-switch`) to block Ctrl+Alt+F\* VT switching while locked            |
| **OOM protection**        | Set `oom_score_adj = -1000` via `/proc/self/oom_score_adj` at startup so the kernel never kills the lock screen |
| **Crash handling**        | Wrap main loop in a panic hook that re-execs `i3lock` as a fallback — never leave session unlocked              |
| **Password hygiene**      | Use `zeroize` crate to scrub password buffers from memory immediately after PAM auth completes or fails         |
| **Timeout / brute-force** | Exponential backoff delay after 3 failed attempts (1s, 2s, 4s …) — visual countdown on the UI                   |

### 5. New Dependencies

| Crate        | Purpose                                                         |
| ------------ | --------------------------------------------------------------- |
| `pam-client` | Rust PAM bindings for user authentication                       |
| `x11rb`      | X11 protocol (grabs, override-redirect windows, input handling) |
| `zeroize`    | Secure zeroing of password buffers in memory                    |
| `nix`        | POSIX helpers — `oom_score_adj`, signal handling, `getlogin`    |

Existing deps reused: `gtk4`, `gdk4-x11` (rendering), `zbus` (logind DBus), `log`/`env_logger`.

### 6. New Binary Target

Follow the existing `Cargo.toml` `[[bin]]` pattern:

```toml
[[bin]]
name = "i3more-lock"
path = "src/lock_main.rs"
```

Entry point `src/lock_main.rs` — follows the convention of `workspace_main.rs`, `audio_main.rs`, etc.

### Critical Architecture Note: GTK4 + X11 Grab Limitation

**GTK4 cannot forward X11 key events to widgets after `XGrabKeyboard`.** The GTK4 event system does not see keypresses once the X11 grab is active. Therefore, the architecture must:

- Use **`x11rb`** for all input handling (reading raw `KeyPress` events from the X11 event loop)
- Use **GTK4 purely for rendering** (clock, password dot feedback, avatar)
- Bridge input to UI via `glib::idle_add` to update GTK label widgets from the X11 event thread

Reference implementations confirming this approach:

- **`xsecurelock`** (Google, C) — gold standard for X11 lock security, uses raw X11 input
- **`screenruster`** (Rust, abandoned) — daemon/saver separation pattern, architecturally instructive
- **`slock`** (suckless, ~200 LOC C) — bare minimum grab + auth pattern

---

## Phase 1 : Lock session - Implementation

### Phase 1.1: Scaffold & X11 Lock Window (no auth) — **done**

- Add `[[bin]] name = "i3more-lock"` to `Cargo.toml`, entry `src/lock_main.rs`
- Add deps: `x11rb = "0.13"`, `zeroize = "1"`, `nix = { version = "0.29", features = ["process", "signal"] }`
- Create `src/lock_main.rs` following existing pattern: `init_logging("i3more-lock")`, `Application::builder("com.i3more.lock")`
- Create `src/lock/` module: `mod.rs`, `x11.rs`, `ui.rs`
- `x11.rs`: connect to X display, create full-screen override-redirect window on each output, `XGrabKeyboard` + `XGrabPointer` with retry loop
- `ui.rs`: GTK4 overlay — clock label, password dot feedback label, user avatar
- Bridge: X11 event loop reads `KeyPress` → updates GTK labels via `glib::idle_add`
- **Verification**: Run `cargo build --bin i3more-lock`, launch it, confirm it covers screen and captures all input, Escape to exit (dev-only exit)

### Phase 1.2: PAM Authentication — **done**

- Add dep: `pam-client = "0.5"` (or `pam = "0.7"` — decide during implementation)
- Create `src/lock/auth.rs`: PAM conversation with `conv_mock`, service `"i3more-lock"`
- Install PAM service file: document that user needs `/etc/pam.d/i3more-lock` with `auth include login`
- Wire Enter key from X11 event loop → collect password buffer → spawn blocking thread for `pam.authenticate()` → on success: release grabs, destroy window, exit; on failure: clear buffer with `zeroize`, shake animation, retry
- Password buffer: `Zeroizing<String>` with pre-allocated capacity (64 bytes), no reallocation
- **Verification**: Build, lock, type correct password → unlocks. Wrong password → clears and allows retry.

### Phase 1.3: Security Hardening — **done**

- OOM protection: write `-1000` to `/proc/self/oom_score_adj` at startup via `nix`
- VT switch prevention: `logind` `Inhibitor` lock via `zbus` (type `handle-switch`)
- Crash handler: `std::panic::set_hook` that exec's `/usr/bin/i3lock` as fallback
- Grab refresh: periodically re-grab keyboard/pointer (every 1s) to defend against late-arriving override-redirect windows stealing focus (xsecurelock pattern)
- Brute-force delay: exponential backoff after 3 failures (1s, 2s, 4s…)
- **Verification**: Test each: `kill -9` the process (i3lock fallback), attempt Ctrl+Alt+F2 (blocked), verify OOM score in `/proc/$(pidof i3more-lock)/oom_score_adj`

### Phase 1.4: Session Integration & Polish — **done**

- `xss-lock` compatibility: ensure clean exit code 0 on successful unlock ✅
- `logind` signals: handled by `xss-lock` (listens for Lock/PrepareForSleep and invokes i3more-lock) ✅
- CSS theme: `assets/lock-screen.css` matching control-panel aesthetic ✅
- Multi-monitor: enumerate outputs via `x11rb` `randr`, one lock window per output ✅
- i3 config: `bindsym $mod+l exec --no-startup-id i3more-lock` + `exec --no-startup-id xss-lock -l -- i3more-lock` ✅
- Config file: `~/.config/i3more/lock.json` (clock_format wired to UI, avatar_path) ✅
- **Verification**: `loginctl lock-session` triggers lock. Suspend/resume triggers lock. Correct display on multi-monitor.

### Git Submodules for Reference

Suggest adding these as git submodules under `reference/` for code exploration during development:

| Repository                      | Why                                                                                                           |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `github.com/google/xsecurelock` | Gold-standard X11 lock security — grab refresh, override-redirect defense, PAM isolation. C, well-documented. |
| `github.com/meh/screenruster`   | Only Rust X11 lock screen with PAM. Abandoned but architecturally instructive (daemon/saver split, JSON IPC). |
| `github.com/tmhedberg/slock`    | Ultra-minimal X11 lock (~200 LOC C). Best for understanding the bare minimum grab + auth pattern.             |

```bash
git submodule add https://github.com/google/xsecurelock.git reference/xsecurelock
git submodule add https://github.com/meh/screenruster.git reference/screenruster
git submodule add https://github.com/tmhedberg/slock.git reference/slock
```

### Key Files to Create/Modify

| File                     | Action                                        |
| ------------------------ | --------------------------------------------- |
| `Cargo.toml`             | Add `[[bin]]` entry + new deps                |
| `src/lock_main.rs`       | New entry point                               |
| `src/lock/mod.rs`        | Module declarations                           |
| `src/lock/x11.rs`        | X11 window, grabs, event loop                 |
| `src/lock/ui.rs`         | GTK4 rendering (clock, password dots, avatar) |
| `src/lock/auth.rs`       | PAM authentication                            |
| `assets/lock-screen.css` | Lock screen styles                            |

---

### UI enhancement — **done**

- ~~User wishes to add a userprofile.json in which he can map a profile picture.~~ → uses `avatar_path` in existing `lock.json`
- ~~On lock the profile picture should be shown instead of username~~ → avatar replaces username when configured
- ~~Password should be on a visible textbox.~~ → masked dots inside a bordered textbox widget
- The current backgroung image

## Phase 2: Full DM implementation - Analysis.

- eplore Key GDM PAM Files in Ubuntu (/etc/pam.d/),

# Reference

```bash
ls ../../README.md
```
