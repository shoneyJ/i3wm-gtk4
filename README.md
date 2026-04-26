# i3More

An extension layer for i3 window manager focused on improving user experience
with visually appealing widgets — without relying on external bash scripts.

i3More is a self-contained standalone program that communicates with i3 via IPC,
with no shell script dependencies at runtime.

**Target environment:** Ubuntu Server + i3wm. A user installs Ubuntu Server,
installs i3, then installs i3More as a single package/binary.

## AI agent user's personal goal

- Practise software architecture.
- Learn system's programming language such as rust, c or c++.
- Learn concepts of rust used in the project.
- Understand linux systems better.

## Quick Start

Builds are split across two containers because the `i3more-speech-text`
binary needs CUDA + whisper.cpp + clang and we don't want that in the
default dev image.

```bash
# Everything except speech-text — builds quickly, no CUDA / cmake / clang.
killall i3more 2>/dev/null; docker compose run --rm dev bash -c \
  "cargo build --release --bin i3more --bin i3more-translate \
                            --bin i3more-audio --bin i3more-launcher && \
   cp target/release/i3more target/release/i3more-translate \
      target/release/i3more-audio target/release/i3more-launcher dist/"

# i3more-lock — opts in pam-sys via the `lock` feature.
docker compose run --rm dev bash -c \
  "cargo build --release --features lock --bin i3more-lock && \
   cp target/release/i3more-lock dist/"

# i3more-speech-text — needs the CUDA-equipped `whisper-build` container.
docker compose run --rm whisper-build bash -c \
  "cargo build --release --features speech-text --bin i3more-speech-text && \
   cp target/release/i3more-speech-text dist/"
```

See [docs/build.md](docs/build.md) for full build and development setup.

## Binaries

| Binary                | Description                                                                       |
| --------------------- | --------------------------------------------------------------------------------- |
| `i3more`              | Main bar — workspace navigator, system tray, notifications, system info, control panel (incl. speech-text widget) |
| `i3more-translate`    | Standalone translation popup                                                      |
| `i3more-audio`        | Volume control & audio device switching                                           |
| `i3more-launcher`     | App search & launch                                                               |
| `i3more-lock`         | Lock screen (PAM-backed; `--features lock`)                                       |
| `i3more-speech-text`  | Real-time German→English speech-to-text from a Bluetooth headset (whisper.cpp / CUDA via whisper-rs; `--features speech-text`; built in the `whisper-build` container) |

Cargo features (declared in `Cargo.toml`, all default-off):

- `speech-text` — pulls in `whisper-rs` (CUDA) + `pipewire`. Required only for `i3more-speech-text`.
- `lock` — pulls in `pam-client` (and its libclang/bindgen chain). Required only for `i3more-lock`.
- Everything else builds with no features at all.

## Documentation

### Learning

- [Learning Plan](docs/learning-plan.md)

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
- [Speech-to-Text (whisper.cpp / CUDA / streaming)](docs/plan/speech-text.md)
- [Control Panel (incl. speech-text widget)](docs/plan/control-panel.md)

### Reference

- [NVIDIA Container Toolkit](docs/reference/nvidia-container-toolkit.md) — host prerequisite for the GPU-accelerated whisper build.

### Vendored / linked sources

- `vendor/whisper.cpp/` — whisper.cpp git submodule pinned at `v1.7.6`. Built with CUDA into `whisper-stream`, `whisper-cli`, and statically linked into `libwhisper-rs` for `i3more-speech-text`.
- `reference/linux/` → symlink to `~/projects/linux` (Linux kernel source). Read-only reference for kernel-level IPC primitives (`eventfd`, `signalfd`, `timerfd`, `memfd_create`, `io_uring`, `epoll`, Unix-domain `SOCK_SEQPACKET`, etc.) referenced by the speech-text streaming plan and any future low-latency / event-loop work.

## Known Issues

See [BUG.md](BUG.md) for tracked bugs and their fixes.
