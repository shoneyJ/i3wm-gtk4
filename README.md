# i3More

An extension layer for i3 window manager focused on improving user experience
with visually appealing widgets — without relying on external bash scripts.

i3More is a self-contained standalone program that communicates with i3 via IPC,
with no shell script dependencies at runtime.

**Target environment:** Ubuntu Server + i3wm. A user installs Ubuntu Server,
installs i3, then installs i3More as a single package/binary.

## Quick Start

```bash
killall i3more 2>/dev/null; docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more target/release/i3more-translate dist/"
```

See [docs/build.md](docs/build.md) for full build and development setup.

## Binaries

| Binary | Description |
|--------|-------------|
| `i3more` | Main bar — workspace navigator, system tray, notifications, system info |
| `i3more-translate` | Standalone translation popup |
| `i3more-audio` | Volume control & audio device switching |
| `i3more-launcher` | App search & launch |

## Documentation

### Core Documentation

- [Design & Requirements](docs/design.md)
- [Architecture](docs/architecture.md)
- [Existing Infrastructure](docs/existing-infrastructure.md)
- [Build & Development](docs/build.md)
- [Setup Guide — Ubuntu Server + i3wm](docs/setup.md)

### Feature Plans

- [01 - Workspace Navigator](docs/plan/01-workspace.md)
- [02 - System Tray](docs/plan/02-system-tray.md)
- [03 - Common Info (Battery, Clock, Stats)](docs/plan/03-common-info.md)
- [04 - Translation Utility](docs/plan/04-util-translation.md)
- [05 - Desktop Notifications](docs/plan/05-desktop-notification.md)
- [06 - Audio Utility](docs/plan/06-util-audio.md)
- [07 - Power Utility](docs/plan/07-util-power.md)
- [08 - Workspace Utility](docs/plan/08-util-workspace.md)
- [09 - Brightness Utility](docs/plan/09-util-brightness.md)
- [10 - App Search / Launcher](docs/plan/10-util-appsearch.md)
- [11 - Logging](docs/plan/11-logs.md)
- [12 - Background Selector](docs/plan/12-util-change-background.md)

## Known Issues

See [BUG.md](BUG.md) for tracked bugs and their fixes.
