# Project Relevance: Kernel Terms Used in i3More

| Kernel Term                     | Subsystem   | Project Usage                                            | Files                                                                 |
| ------------------------------- | ----------- | -------------------------------------------------------- | --------------------------------------------------------------------- |
| Unix domain sockets             | IPC         | i3/Sway IPC communication via `$I3SOCK`/`$SWAYSOCK`      | `src/ipc.rs`                                                          |
| procfs `/proc/stat`             | System Info | CPU usage calculation (user, nice, system, idle, iowait) | `src/sysinfo.rs`                                                      |
| procfs `/proc/meminfo`          | System Info | Memory usage (MemTotal, MemAvailable)                    | `src/sysinfo.rs`                                                      |
| sysfs `/sys/class/power_supply` | Device      | Battery capacity and charging status                     | `src/sysinfo.rs`                                                      |
| sysfs `/sys/class/thermal`      | Device      | CPU temperature from thermal zones                       | `src/sysinfo.rs`                                                      |
| sysfs `/sys/class/backlight`    | Device      | Display brightness read/write                            | `src/audio_main.rs`, `src/control_panel/widgets/backlight.rs`         |
| OOM killer (oom_score_adj)      | Memory      | Protect lock screen from being killed (-1000)            | `src/lock/security.rs`                                                |
| mlock / mlockall                | Memory      | Prevent password memory from being swapped to disk       | `reference/xsecurelock/mlock_page.h`                                  |
| PAM                             | Security    | Lock screen password authentication                      | `src/lock/auth.rs`                                                    |
| capabilities (CAP_SYS_RESOURCE) | Security    | Required for OOM score adjustment                        | `src/lock/security.rs`                                                |
| D-Bus (session bus)             | IPC         | Notification daemon, system tray watcher, dbusmenu       | `src/notify/daemon.rs`, `src/tray/watcher.rs`, `src/tray/dbusmenu.rs` |
| D-Bus (system bus)              | IPC         | systemd-logind VT switch inhibitor                       | `src/lock/security.rs`                                                |
| X11 protocol (XGrab)            | Display     | Keyboard/pointer grab for lock screen                    | `src/lock/x11.rs`                                                     |
| X11 RandR                       | Display     | Multi-monitor detection and cover windows                | `src/lock/x11.rs`                                                     |
| EWMH properties                 | Display     | Window type, strut, focus control                        | `src/main.rs`, `src/notify/popup.rs`                                  |
| fork / execv / setsid           | Process     | Process creation and session management                  | `reference/xsecurelock/wait_pgrp.c`                                   |
| waitpid (WNOHANG)               | Process     | Non-blocking child process reaping                       | `reference/xsecurelock/wait_pgrp.c`                                   |
| signals (SIGCHLD, SIGTERM)      | Process     | Signal handling for child process lifecycle              | `reference/xsecurelock/wait_pgrp.c`                                   |
| sigaction / sigprocmask         | Process     | Signal installation and masking                          | `reference/xsecurelock/wait_pgrp.c`                                   |
| eventfd (fd leak pattern)       | IPC         | Maintain logind inhibitor via leaked file descriptor     | `src/lock/security.rs`                                                |
| futex (via Rust Mutex)          | IPC         | `Arc<Mutex>` shared state in tray watcher and daemon     | `src/tray/watcher.rs`, `src/notify/daemon.rs`                         |
| socket timeouts                 | IPC         | Read/write timeouts on i3 IPC socket                     | `src/ipc.rs`                                                          |
| environment variables           | Config      | Socket paths, PAM service, bar height, touch mode        | Multiple files                                                        |

# References

- [zbus](https://github.com/z-galaxy/zbus) — the D-Bus crate that linbus replaces
- [linbus design doc](dbus-rust-phases.md) — migration plan and wire protocol details
