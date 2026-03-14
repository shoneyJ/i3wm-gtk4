# Logging

## Overview

All i3More binaries write logs to `~/.cache/i3more/<binary-name>.log` in append mode.
Log level defaults to `info`; override with the `RUST_LOG` env var.

## Implementation ✅

Shared function `i3more::init_logging(name)` in `src/lib.rs`:
- Creates `~/.cache/i3more/` directory if missing
- Opens `<name>.log` in append mode
- Auto-truncates if file exceeds 1 MB
- Falls back to stderr if file open fails
- Uses `env_logger::Builder` with `Target::Pipe` to route logs to file

## Binaries Updated

| Binary             | Entry point            | Log file                                  |
| ------------------ | ---------------------- | ----------------------------------------- |
| i3more             | `src/main.rs`          | `~/.cache/i3more/i3more.log`              |
| i3more-translate   | `src/translate_main.rs`| `~/.cache/i3more/i3more-translate.log`     |
| i3more-launcher    | `src/launcher_main.rs` | `~/.cache/i3more/i3more-launcher.log`      |
| i3more-audio       | `src/audio_main.rs`    | `~/.cache/i3more/i3more-audio.log`         |

## Usage

```bash
# View logs
tail -f ~/.cache/i3more/i3more.log

# Override log level
RUST_LOG=debug ./dist/i3more

# Filter to specific module
RUST_LOG=i3more::tray=debug ./dist/i3more
```

## Log Coverage

30+ `log::` macro calls across the codebase covering:
- i3 IPC connectivity and workspace refresh errors
- Font Awesome registration
- Tray item lifecycle (register/unregister/props)
- Notification daemon events (create/close/action)
- Notification history persistence
- Launcher entry loading and app launch commands
- D-Bus connection warnings
