# speech to text

## Purpose

- I need to learn German as it is the language I use at my work place.
- I need to often use my headset to listen to my colleagues.
- I need to save/stream the audio input received in German, use a speech-to-text translator, save the German text to reference later.

## Context

The audio-monitor work (commit `3c9b8c6`) landed the pattern for interacting with PulseAudio (`pactl`, `parec`, monitor-source subscriptions). The translator stack (`src/translate.rs`, `src/translate_main.rs`) already wraps the `trans` CLI and persists language preferences in `~/.config/i3more/translate.json`. Speech-to-text is a new binary that composes those two: tap the default sink's monitor, stream into `whisper.cpp`, and render each German segment plus its English translation. A toggle (start/stop) keeps CPU cost off when the feature is idle.

## Status (current implementation)

Phases run twice in this doc. The original Phases 0–8 (chunked tumble-window via `parec` → `whisper-rs`) describe what was built first. The Sx phases (S1–S7, in **Streaming architecture (v2)** further down) describe the streaming refactor that supersedes the audio-capture and inference-dispatch layers.

| Phase  | Title                                | Status        | Notes                                                                                  |
| ------ | ------------------------------------ | ------------- | -------------------------------------------------------------------------------------- |
| 0      | Ground truth — whisper-stream on GPU | **Done**      | CUDA build at `vendor/whisper.cpp/build/`; `ggml-base.bin` downloaded.                 |
| 1      | Shell-level audio pipeline           | **Done**      | Validated via `parecord` → WAV → `whisper-cli`. (whisper-stream + SDL2 path was dead-end on PipeWire.) |
| 2      | Rust process plumbing (headless)     | **Done**      | `i3more-speech-text` CLI; `parec` subprocess + in-process `whisper-rs` (CUDA).         |
| 3      | GTK4 shell                           | Pending       | Replaced by S5/S7 + future control-panel widget.                                       |
| 4      | Live transcript rendering (GTK)      | Pending       | Folded into the control-panel widget; CLI stays the source of truth.                    |
| 5      | Inline English translation           | **Done**      | `maybe_translate` via `trans` CLI; only Final segments translated; transcript format `- **HH:MM:SS** — de\n  - _en_`. |
| 6      | Persistence                          | Pending       | Subsumed by Phase 6.5.                                                                 |
| 6.5    | Session metadata + post-process hook | **Done (v1)** | `--session=<name>` / `I3MORE_STT_SESSION` → `~/.local/share/i3more/stt/<date>/<name>.md`; front-matter; append-with-separator on re-use. |
| 7      | Bluetooth profile resilience         | Superseded    | See **7-prime** below.                                                                 |
| 8      | Polish                               | Pending       | Belongs with the control-panel widget.                                                 |
| **7-prime** | Multi-backend capture + auto-switch | **Done**  | parec backend default; pipewire opt-in. Supervisor in `src/speech_text/capture.rs` runs `pactl subscribe` and respawns the backend on every default-sink change. |
| S1     | Direct PipeWire client capture       | **Done (A2DP only)** | Native client works on A2DP; silent on HSP/HFP due to PipeWire 1.0.5 bug. Therefore opt-in (`capture_backend = "pipewire"`). |
| S2     | Sliding window inference             | **Done**      | length=8000 ms / step=1500 ms; Provisional & Final segment kinds; `strip_common_prefix`; speech-end commit via `vad::is_speech_end`. |
| S3     | VAD silence + hallucination filter   | **Done**      | `vad::is_chunk_silent` gates inference (mean \|s\| < 0.003). FullParams hardened (`no_speech_thold=0.6`, `suppress_blank`, `temperature=0`, `no_context`). `is_hallucination` drops residual `[Musik]`/`[Applaus]`/`...` lines. |
| S4–S7  | Remaining streaming work             | Pending       | eventfd dispatch, UDS broadcast, epoll loop, TUI follower. See *Streaming architecture (v2)*. |

The phases that are now "Pending" but UI-shaped (3, 4, 5, 8) were originally framed around a standalone GTK4 window. Direction has shifted: live UI lands inside the i3More **control panel** widget (`docs/plan/control-panel.md` §"speech-text — control panel integration"), and the headless CLI + Sx streaming layer is the reusable backend.

## Architecture

New binary `i3more-speech-text` (added as a `[[bin]]` in `Cargo.toml`), following the same single-instance D-Bus toggle pattern as `i3more-translate` (`application_id = "com.i3more.speechtext"`). Whisper lives in-tree as a git submodule and is built inside the existing Docker dev image; no new Rust crate dependencies.

### New files

| File                                 | Purpose                                                                                       | Status                              |
| ------------------------------------ | --------------------------------------------------------------------------------------------- | ----------------------------------- |
| `src/speech_text.rs`                 | Runtime: spawn/kill `parec`, in-process `whisper-rs` inference, transcript file persistence    | **Created** (Phase 2 + 6.5)        |
| `src/speech_text_main.rs`            | CLI entrypoint for `i3more-speech-text`. GTK4 UI deferred to the control-panel widget          | **Created** (Phase 2)               |
| `Dockerfile.whisper-cuda`            | CUDA devel image (Rust toolchain + clang + GTK4 dev libs + libsdl2-dev) — builds whisper-cli, whisper-stream, and `i3more-speech-text` with `whisper-rs` (cuda feature) | **Created** (Phase 0 + 2)          |
| `vendor/whisper.cpp/`                | Submodule pinned to `v1.7.6`                                                                  | **Created** (Phase 0)               |
| `src/speech_text/capture.rs`         | Supervisor: target-sink resolver + backend dispatcher + `pactl subscribe` follower            | **Created** (7-prime)               |
| `src/speech_text/parec.rs`           | parec subprocess backend (default)                                                            | **Created** (7-prime)               |
| `src/speech_text/pipewire.rs`        | Native PipeWire client backend (opt-in)                                                       | **Created** (S1)                    |
| `src/speech_text/vad.rs`             | `is_chunk_silent` (S3) + `is_speech_end` (S2) + `high_pass_filter`. Energy-based VAD ported from `vendor/whisper.cpp/examples/common.cpp:597-646` | **Created** (S2 + S3)               |
| `src/control_panel/widgets/speech_text.rs` | Control-panel widget — Start/Stop, session-name `GtkEntry`, "Summarise with Claude" button, periodic 2 s state poll | **Created** (Move 3.b)              |
| `src/speech_text/ringbuf.rs`         | Standalone ring buffer module — currently inline in run_worker                                | Optional (cleanup)                  |
| `src/speech_text/wakeup.rs`          | `eventfd`-backed capture → inference signaling                                                | Pending (S4)                        |
| `src/speech_text/broadcast.rs`       | UDS `SOCK_SEQPACKET` listener + per-subscriber send                                           | Pending (S5)                        |
| `src/speech_text/loop.rs`            | `epoll` + `signalfd` event loop                                                               | Pending (S6)                        |
| `src/speech_text_tail_main.rs`       | `i3more-speech-text-tail` — TUI follower subscribing to the broadcast socket                  | Pending (S7)                        |
| `assets/speech-text.css`             | Gruvbox styling (only when GTK widget lands; not the CLI)                                     | Pending (control-panel widget work) |

### Files touched

| File                          | Change                                                                                                                                                  | Status                  |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| `Cargo.toml`                  | All heavy deps optional + feature-gated: `whisper-rs` and `pipewire` behind `speech-text`; `pam-client` behind `lock`. `default = []`. `[[bin]] i3more-speech-text` has `required-features = ["speech-text"]`. `[[bin]] i3more-lock` has `required-features = ["lock"]`. | **Done**                |
| `src/lib.rs`                  | `#[cfg(feature = "speech-text")] pub mod speech_text;`                                                                                                  | **Done**                |
| `src/control_panel/widgets/mod.rs` | `pub mod speech_text;`                                                                                                                              | **Done**                |
| `src/control_panel/panel.rs`  | New section appended after Background: `build_section("Speech-to-Text", fa::MICROPHONE, widgets::speech_text::build_widget())`                          | **Done**                |
| `Dockerfile`                  | Added `clang`, `libpipewire-0.3-dev`, `LIBCLANG_PATH=/usr/lib/llvm-18/lib`, `LD_LIBRARY_PATH=/usr/lib/llvm-18/lib`                                     | **Done**                |
| `Dockerfile.whisper-cuda`     | Has `clang` + `libpipewire-0.3-dev` + Rust toolchain + GTK4 dev libs                                                                                   | **Done**                |
| `docker-compose.yaml`         | `whisper-build` service: NVIDIA runtime, `cargo-registry` + `whisper-target` volumes                                                                   | **Done**                |
| `.gitmodules`                 | Register `vendor/whisper.cpp`                                                                                                                           | **Done**                |

### Build commands

```bash
# i3more main binary + control-panel widget (dev container, no GPU needed)
docker compose run --rm dev bash -c 'cargo build --release --bin i3more && cp target/release/i3more dist/'

# Lock screen binary (dev container; opts in pam-sys via the lock feature)
docker compose run --rm dev bash -c 'cargo build --release --features lock --bin i3more-lock && cp target/release/i3more-lock dist/'

# Speech-text binary (whisper-build container; CUDA + clang + cmake)
docker compose run --rm whisper-build bash -c 'cargo build --release --features speech-text --bin i3more-speech-text && cp target/release/i3more-speech-text dist/'
```

The dev container intentionally does **not** have CUDA toolkit or
`cmake` — keeping `speech-text` and `lock` feature-gated lets the
non-CUDA build succeed quickly without dragging in the whisper.cpp
build chain.

## Audio capture

Target device: the Bluetooth **Jabra headset** the user wears at work. We tap the monitor of whichever sink the Jabra currently exposes — no virtual sink, no loopback module, nothing is re-routed away from the headphones. Colleagues' voices (Teams / Meet / browser call) play *into* the Jabra's sink, so its `.monitor` source is exactly the audio we want to transcribe.

### Bluetooth profile wrinkle

Bluetooth headsets expose different sinks per profile, and the name changes when the profile changes (e.g. when a call opens the mic):

| Profile            | Usage                  | Example sink name                                    |
| ------------------ | ---------------------- | ---------------------------------------------------- |
| A2DP (stereo)      | Media playback, no mic | `bluez_output.XX_XX_XX_XX_XX_XX.a2dp-sink`           |
| HSP/HFP (mono)     | Calls, mic enabled     | `bluez_output.XX_XX_XX_XX_XX_XX.headset-head-unit`   |

Both belong to the same device but appear as distinct sinks in `pactl list sinks`. The feature must follow the profile switch — otherwise transcription silently dies the moment a Teams call opens.

### Resolving the Jabra sink

- Config field `device_match` in `~/.config/i3more/speech-text.json` — case-insensitive substring matched against each sink's `device.description` / `node.description` (defaults to `"jabra"`).
- On session start: run `pactl list sinks` and pick the first sink whose description matches. If none match, fall back to `pactl get-default-sink` and log a warning that the Jabra is not connected.
- Subscribe to sink events via `pactl subscribe` (reuse the pattern from `src/mic_indicator.rs`). On `change`/`new`/`remove` events for sinks, re-resolve the Jabra sink; if it differs from the active one, restart the `parec` subprocess against the new `.monitor`. This is what keeps transcription alive across A2DP ↔ HSP/HFP switches.

### Capture path — by phase

The capture mechanism evolved as we hit dead-ends:

| Phase | Mechanism                                          | Status   | Notes                                                                                  |
| ----- | -------------------------------------------------- | -------- | -------------------------------------------------------------------------------------- |
| 1     | `parecord` → WAV → `whisper-cli`                   | Smoke-test only | Off-line, used only to validate the audio actually flows.                       |
| 2     | `parec` subprocess (stdout pipe) → `whisper-rs`    | **Current** | Resolves Jabra by `pactl list sinks` description match; in-process whisper-rs. Tumble window. |
| S1    | Direct PipeWire client → in-process ring buffer    | Next     | Eliminates `parec` subprocess; callback-driven; survives Bluetooth profile switches gracefully. |

The `whisper-stream` binary path that originally appeared here (SDL2 + `PULSE_SOURCE` env var) was **abandoned in Phase 1** — SDL2 hides PulseAudio monitor sources from its capture-device list and PulseAudio's `module-remap-source` does not pump on PipeWire's compatibility layer. See **Whisper.cpp integration → How `whisper-stream` receives audio (historical)** below for the full reasoning.

No null-sink, loopback module, or SDL2 capture is involved in the active path — monitor sources serve exactly this role and we read them directly via `parec` (Phase 2) or PipeWire (S1).

## Whisper.cpp integration

- Submodule pinned to **`v1.7.6`** at `vendor/whisper.cpp` (added via `git submodule add https://github.com/ggerganov/whisper.cpp vendor/whisper.cpp` + `git checkout v1.7.6` inside the submodule).
- v1.7.6 is cmake-only (the top-level `make stream` target was dropped). The cmake target name is `whisper-stream`; the output binary lands at `vendor/whisper.cpp/build/bin/whisper-stream`.
- Build-time deps for the stream example: `cmake`, `build-essential`, `libsdl2-dev` (and, for GPU, CUDA). These live in `Dockerfile.whisper-cuda` — see **GPU acceleration (NVIDIA)** below.
- Models are user-downloaded once:
  ```bash
  bash vendor/whisper.cpp/models/download-ggml-model.sh base
  mkdir -p ~/.local/share/i3more/models && cp vendor/whisper.cpp/models/ggml-base.bin ~/.local/share/i3more/models/
  ```
  Same model file works on both CPU and CUDA builds — the backend is selected at `whisper-stream` init, not at model compile.

### How `whisper-stream` receives audio (historical — DO NOT use this path)

> **This section is preserved as a record of why we abandoned the `whisper-stream` binary.** The active capture path is `parec` (Phase 2) → PipeWire (S1) → in-process `whisper-rs`. None of the env vars below are set anywhere in the codebase.

`whisper-stream`'s `-f FNAME` is a *text-output* file flag (writes transcripts), not a PCM input. The binary only supports live capture through SDL2 (`-c ID` picks a capture device). The original plan was:

```
SDL_AUDIODRIVER=pulseaudio \
PULSE_SOURCE=<jabra_sink>.monitor \
whisper-stream -m <model> -l de -t <threads> --step 500 --length 5000 --keep 200
```

This **does not work on this host** for two compounding reasons:

1. **SDL2's PulseAudio backend filters monitor sources out** of its capture-device enumeration (`SDL_GetAudioDeviceName(...)`). Even when `PULSE_SOURCE=<sink>.monitor` is set, SDL still picks an unfiltered source — usually the Jabra microphone — because SDL never sees the monitor as a candidate.
2. **PulseAudio's `module-remap-source` workaround** (which would expose the monitor as a regular non-monitor source) **does not pump audio on PipeWire's pactl compatibility layer.** The remapped source loads, appears in the SDL device list, but the master monitor never feeds it. Confirmed empirically during Phase 1.

Take-aways carried into the active path:
- Reading the monitor directly via `parec --device=<sink>.monitor --raw` works.
- The eventual streaming path (S1) skips PulseAudio's compat layer entirely and connects to PipeWire as a native client, which has full visibility of monitor nodes.
- File-based transcription (for smoke tests and possible future offline transcripts) uses `whisper-cli -f FILE.wav` — also produced by the same cmake invocation (`--target whisper-cli`).

## Rust binary: `i3more-speech-text`

### `src/speech_text.rs` (runtime)

- `SpeechTextConfig` (serde, `~/.config/i3more/speech-text.json`): `model_path`, `language` (default `"de"`), `threads` (default `num_cpus / 2`), `auto_translate` (default `true`), `translate_target` (default `"en"`).
- `struct SpeechSession { parec: Child, whisper: Child, stdout_thread: JoinHandle<()>, _tx: mpsc::Sender<Segment> }` — owns the two processes; `Drop` kills them (reuse the `nix::sys::signal` pattern from existing binaries).
- `start(tx: mpsc::Sender<Segment>) -> Result<SpeechSession>` — spawns `parec | whisper-stream`, reads whisper's stdout line by line on a thread, parses timestamps + text with a small regex (e.g. `^\[\d{2}:\d{2}:\d{2}\.\d+ --> ...\]\s+(.*)$`), pushes `Segment { ts, text }` onto the channel.
- Respects the global `SHUTDOWN` `AtomicBool` from `src/lib.rs`.
- `append_segment(path, seg_de, seg_en)` — appends to `~/.local/share/i3more/stt/YYYY-MM-DD.md` in markdown:
  ```markdown
  - **14:02** — Guten Morgen, wie geht's?
    - _Good morning, how are you?_
  ```

### `src/speech_text_main.rs` (UI)

Single-instance GTK4 `Application` (toggle visibility on re-activate), 520x560 window titled `i3More-speechtext`.

Layout:

```
+------------------------------------------+
| [● Start]  Model: [ base v]  Lang: [de]  |  Top bar: toggle, model, language
+------------------------------------------+
| 14:02  Guten Morgen, wie geht's?         |
|        Good morning, how are you?        |
| 14:03  Können wir kurz sprechen?         |
|        Can we briefly talk?              |
|                                          |
+------------------------------------------+
| Saving to ~/.local/share/i3more/stt/…   |  Status bar
+------------------------------------------+
```

- A `ListBox` inside a `ScrolledWindow` holds one `GtkBox` per segment: German label on top, translated label beneath in a muted color. Auto-scroll to bottom on new segment.
- `glib::timeout_add_local` polls an `mpsc::Receiver<Segment>` every 100 ms (same pattern as `src/translate_main.rs`'s result polling) and appends rows.
- Translation: for each new segment, call `i3more::translate::translate(text, "de", "en")` on a worker thread and append the English line when it arrives. Reuse exactly the existing function — no new translate code.
- Start/Stop button toggles the `SpeechSession`. Disabled while a transition is in flight.
- Copy-on-click per row: clicking a row copies `"<de>\n<en>"` to the clipboard (reuse the clipboard pattern from `src/translate_main.rs`).
- CSS: add `assets/speech-text.css`, loaded with `include_str!` + `CssProvider::load_from_data` (same as translate).

## Persistence

- Config: `~/.config/i3more/speech-text.json` (created on first run from defaults).
- Transcripts: `~/.local/share/i3more/stt/YYYY-MM-DD.md`, appended only. Rotation is date-based — no size caps needed for the expected volume.
- Log: `~/.cache/i3more/speech-text.log` via the existing `init_logging` helper in `src/lib.rs`.

## i3 integration

Document in this file; user adds to their i3 config:

```
bindsym $mod+Shift+s exec i3more-speech-text
for_window [title="i3More-speechtext"] floating enable, border pixel 1, resize set 520 560
```

## Setup (one-time)

```bash
# Submodule + model. `small` (487 MB) is the default — noticeably better
# German recognition than `base`, well within the GPU's real-time budget.
# `base` (150 MB) is a fine fallback for slower setups; just point
# `model_path` at it in the config.
git submodule update --init --recursive
bash vendor/whisper.cpp/models/download-ggml-model.sh small ~/.local/share/i3more/models

# Default config (the binary creates this on first run if absent — only
# needed if you want to override defaults like model path or backend).
cat > ~/.config/i3more/speech-text.json <<'EOF'
{
  "model_path": "/home/$USER/.local/share/i3more/models/ggml-small.bin",
  "language": "de",
  "device_match": "",
  "capture_backend": "parec",
  "chunk_seconds": 5,
  "length_ms": 8000,
  "step_ms": 1500,
  "vad_thold": 0.6,
  "threads": 4,
  "translate_enabled": true,
  "translate_target": "en"
}
EOF
```

## GPU acceleration (NVIDIA)

The user runs an NVIDIA GPU and needs GPU inference. Whisper v1.7.6 supports this via `-DGGML_CUDA=1`, which links `ggml` against cuBLAS + custom CUDA kernels (confirmed in `vendor/whisper.cpp/README.md` §"NVIDIA GPU support"). Runtime API is unchanged — the same `whisper-stream` binary picks the GPU backend at init and logs `ggml_cuda_init: found N CUDA devices` on startup.

Guiding principle: **don't pollute the main dev image with CUDA.** CUDA devel is ~3 GB and only matters for the one-off whisper build. Isolate it in a dedicated container.

### Host requirements (one-time)

1. **NVIDIA driver** installed on the host. Verify: `nvidia-smi` prints the GPU.
2. **CUDA toolkit on the host is NOT required** — the CUDA build happens inside a `nvidia/cuda:*-devel` container.
3. **NVIDIA Container Toolkit** so Docker can pass the GPU into the build container:

   ```bash
   curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey \
     | sudo gpg --dearmor -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg
   curl -s -L https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list \
     | sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://#g' \
     | sudo tee /etc/apt/sources.list.d/nvidia-container-toolkit.list
   sudo apt-get update && sudo apt-get install -y nvidia-container-toolkit
   sudo nvidia-ctk runtime configure --runtime=docker
   sudo systemctl restart docker
   ```

   Smoke-test: `docker run --rm --gpus all nvidia/cuda:12.6.3-base-ubuntu24.04 nvidia-smi` should print the same GPU info as on the host.

### New file: `Dockerfile.whisper-cuda`

```dockerfile
FROM nvidia/cuda:12.6.3-devel-ubuntu24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential cmake pkg-config git ca-certificates \
      libsdl2-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
CMD ["bash"]
```

Notes:
- Base is `nvidia/cuda:12.6.3-devel-ubuntu24.04` to match the existing `ubuntu:24.04` userland in `Dockerfile`; bump the CUDA minor when the host driver supports a newer one.
- Only the deps needed to build `whisper-stream` are installed. No GTK / Rust — those stay in the main `Dockerfile`.

### `docker-compose.yaml` addition

Append this service (keep the existing `dev` and `test-lock` services unchanged):

```yaml
  whisper-build:
    build:
      context: .
      dockerfile: Dockerfile.whisper-cuda
    volumes:
      - .:/src
    working_dir: /src
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: all
              capabilities: [compute, utility]
```

The `deploy.resources.reservations.devices` block is the Compose v2 syntax for exposing GPUs — equivalent to `--gpus all` on `docker run`.

### CUDA architecture pinning (optional)

Default `-DGGML_CUDA=1` builds for a broad set of compute capabilities and Just Works on common GPUs. Pin only if you want a smaller binary / faster link for a known GPU, e.g. `-DCMAKE_CUDA_ARCHITECTURES="89"` for RTX 40-series (Ada), `"86"` for RTX 30-series (Ampere), `"120"` for RTX 50-series (Blackwell). Read the actual capability from `nvidia-smi --query-gpu=compute_cap --format=csv` and set it via a wrapper env var if needed.

## Build

Two-stage build: CUDA container produces `whisper-stream` + its shared libs, regular `dev` container produces the Rust binary. Neither stage pollutes the other. **No artifact is copied to `dist/`** — build outputs stay in `vendor/whisper.cpp/build/` (bind-mounted to the host) and the Rust runtime references them there directly via `LD_LIBRARY_PATH`.

```bash
killall i3more-speech-text 2>/dev/null

# 1. whisper-stream with CUDA (GPU container) — builds in place
docker compose run --rm whisper-build bash -c "\
  cmake -S vendor/whisper.cpp -B vendor/whisper.cpp/build \
    -DGGML_CUDA=1 -DWHISPER_SDL2=ON -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CUDA_ARCHITECTURES=\"89\" && \
  cmake --build vendor/whisper.cpp/build --target whisper-stream -j\$(nproc)"

# 2. i3more-speech-text (regular dev container, no CUDA needed)
docker compose run --rm dev bash -c "\
  cargo build --release --bin i3more-speech-text && \
  cp target/release/i3more-speech-text dist/"
```

Artifacts after build:

| Path                                                         | Role                                                   |
| ------------------------------------------------------------ | ------------------------------------------------------ |
| `vendor/whisper.cpp/build/bin/whisper-stream`                | The executable (root-owned via container, world-exec)  |
| `vendor/whisper.cpp/build/src/libwhisper.so*`                | `libwhisper.so.1` + versioned target                   |
| `vendor/whisper.cpp/build/ggml/src/libggml*.so`              | `libggml-base/cpu/.so`, `libggml.so`                   |
| `vendor/whisper.cpp/build/ggml/src/ggml-cuda/libggml-cuda.so`| CUDA backend                                           |

### Runtime invocation (used by `src/speech_text.rs`)

```bash
WHISPER_DIR=/abs/path/to/repo/vendor/whisper.cpp/build
LD_LIBRARY_PATH="$WHISPER_DIR/src:$WHISPER_DIR/ggml/src:$WHISPER_DIR/ggml/src/ggml-cuda" \
  "$WHISPER_DIR/bin/whisper-stream" -m <model> -l de ...
```

The Rust spawn code computes `WHISPER_DIR` from the repo root (env var `I3MORE_WHISPER_DIR`, with a compile-time default pointing at `vendor/whisper.cpp/build`), sets `LD_LIBRARY_PATH` in the child's env, and execs `bin/whisper-stream`. No copy, no rpath patching, no `ldconfig`.

**CPU-only fallback** (for machines without an NVIDIA GPU — e.g. debugging on a laptop): drop `-DGGML_CUDA=1` in step 1. Runtime path and env-var plumbing are identical.

## Implementation phases

Each phase is an independently commit-able, manually-testable increment. Do not begin a phase before the previous one's **Exit criteria** pass — they are the "definition of done" for that slice.

### Phase 0 — Ground truth: whisper-stream on GPU  *(✅ Done)*

**Goal.** Prove the CUDA build works end-to-end outside any Rust code, so later phases can assume `dist/whisper-stream` is a working black box.

**Touch.**
- `vendor/whisper.cpp` (submodule — already added, pinned to `v1.7.6`).
- `Dockerfile.whisper-cuda` (new).
- `docker-compose.yaml` (new `whisper-build` service).
- Host: install NVIDIA Container Toolkit; run the smoke-test from **GPU acceleration** §"Host requirements".

**Exit criteria.**
- `docker compose run --rm whisper-build nvidia-smi` lists the GPU.
- The built binary runs from its bind-mounted path (with `LD_LIBRARY_PATH` set to the sibling lib dirs; see **Build → Runtime invocation**) and prints usage showing the `-f` flag.
- `espeak -v de -w /tmp/de.wav "Guten Morgen"` followed by invoking `vendor/whisper.cpp/build/bin/whisper-stream -m ~/.local/share/i3more/models/ggml-base.bin -l de -f /tmp/de.wav` transcribes correctly — stderr shows `ggml_cuda_init: found 1 CUDA devices`; stdout contains "Guten Morgen".

### Phase 1 — Shell-level audio pipeline  *(✅ Done — outcome: SDL2 path dead-end, parec path validated)*

**Goal.** Validate live capture from the Jabra monitor via SDL2/PulseAudio before any Rust is written. Catches env-var / device-matching issues cheaply. (The originally-planned `parec | whisper-stream -f -` pipe is not viable — `-f` is an output flag; see **Whisper.cpp integration → How `whisper-stream` receives audio**.)

**Touch.** Nothing in the repo — this is a throwaway bash one-liner.

**Exit criteria.**
- Jabra connected, some German audio playing through it (YouTube, Teams recording, etc.).
- This one-liner prints recognizable German to stdout:
  ```bash
  JABRA=$(pactl list sinks short | awk '/bluez/{print $2; exit}')
  WHISPER_BUILD=vendor/whisper.cpp/build
  LD_LIBRARY_PATH="$WHISPER_BUILD/src:$WHISPER_BUILD/ggml/src:$WHISPER_BUILD/ggml/src/ggml-cuda" \
  SDL_AUDIODRIVER=pulseaudio \
  PULSE_SOURCE="${JABRA}.monitor" \
    "$WHISPER_BUILD/bin/whisper-stream" \
      -m ~/.local/share/i3more/models/ggml-base.bin \
      -l de -t 4 --step 500 --length 5000 --keep 200
  ```
- Ctrl-C cleanly exits the process.
- Log shows `ggml_cuda_init: found 1 CUDA devices` (GPU path still intact).

### Phase 2 — Rust process plumbing (headless)  *(✅ Done)*

**Goal.** Encapsulate Phase 1 behind a `SpeechSession` type. Prove lifecycle (spawn / kill / SHUTDOWN / Drop) without any GUI noise. Single child process (`whisper-stream`); no `parec`.

**Touch.**
- `Cargo.toml` — add `[[bin]] name = "i3more-speech-text"`.
- `src/lib.rs` — `pub mod speech_text;`.
- `src/speech_text.rs` — `SpeechTextConfig`, `Segment`, `SpeechSession::start/stop`, stdout-parse thread, mpsc channel. Spawn sets `LD_LIBRARY_PATH`, `SDL_AUDIODRIVER=pulseaudio`, `PULSE_SOURCE=<jabra>.monitor` on the child's env.
- `src/speech_text_main.rs` — **throwaway** CLI `main()` that calls `SpeechSession::start`, prints segments from the receiver, handles SIGINT/SIGTERM via the global `SHUTDOWN` flag.

**Exit criteria.**
- `cargo run --bin i3more-speech-text` prints the same text Phase 1 produced.
- `kill -TERM <pid>` makes the `whisper-stream` child exit within 1 s (verify via `pgrep`).
- Repeated start/stop in a loop doesn't leak child processes.

### Phase 3 — GTK4 shell (no transcript yet)  *(superseded — folded into the control-panel widget)*

**Goal.** Replace the CLI `main` with the single-instance GTK4 application pattern. Window toggles on re-invoke.

**Touch.**
- `src/speech_text_main.rs` — rewritten: `Application` with `application_id = "com.i3more.speechtext"`, 520×560 `ApplicationWindow` titled `i3More-speechtext`, top bar with a single `Start/Stop` button (not yet wired), empty `ListBox` in a `ScrolledWindow`, status bar with save path.
- Copy the D-Bus-single-instance + `activate` toggle idiom from `src/translate_main.rs`.

**Exit criteria.**
- First `i3more-speech-text` invocation opens the window; second invocation toggles visibility off/on (same PID, verified via `pgrep`).
- Window picks up the Gruvbox theme (stub `assets/speech-text.css` loaded via `include_str!`).
- Start/Stop button visually toggles label only — no backend work yet.

### Phase 4 — Live transcript rendering  *(superseded — replaced by S5 broadcast + S7 TUI follower; later consumed by control-panel widget)*

**Goal.** Wire Phase 2's channel into Phase 3's window. German-only for now.

**Touch.**
- `src/speech_text_main.rs` — Start button calls `SpeechSession::start(tx)`; `glib::timeout_add_local` polls the receiver every 100 ms and appends a row per segment. Auto-scroll to bottom. Stop button kills the session.

**Exit criteria.**
- Playing German audio through the Jabra makes rows appear in the window within ~5 s.
- Stop button cleanly terminates the pipeline; Start again after Stop works.
- Button is disabled for the ~200 ms transition to prevent double-submits.

### Phase 5 — Inline English translation  *(deferred — belongs with the control-panel widget, not the headless CLI)*

**Goal.** Each German row gains an English sub-row rendered beneath it.

**Touch.**
- `src/speech_text_main.rs` — for every incoming `Segment`, spawn a worker thread that calls `i3more::translate::translate(text, "de", "en")`; on completion, post to the GTK main thread (`glib::idle_add_local_once`) to append the English label under the existing row.
- `assets/speech-text.css` — muted color for the translation line.

**Exit criteria.**
- Both German and English text render for each segment.
- If `trans` fails (no network, etc.) the English line shows a muted "`— translation unavailable —`" and the German row stays visible.
- Translation latency does not block ingestion of the next German segment.

### Phase 6 — Persistence  *(subsumed by Phase 6.5)*

**Goal.** Config and transcript survive across sessions.

**Touch.**
- `src/speech_text.rs` — `SpeechTextConfig::load()/save()` at `~/.config/i3more/speech-text.json` (serde); append-to-file helper writing the markdown format from the **`src/speech_text.rs` (runtime)** section to `~/.local/share/i3more/stt/YYYY-MM-DD.md`.
- `src/speech_text_main.rs` — load config on startup, bind model/language dropdowns, save on change.

**Exit criteria.**
- Deleting `~/.config/i3more/speech-text.json`, launching, changing model/language, closing and relaunching restores the chosen values.
- Transcript file contains both German and English lines in the exact markdown shape documented.
- `init_logging("speech-text")` writes to `~/.cache/i3more/speech-text.log`.

### Phase 6.5 — Session metadata + post-process hook  *(✅ Done — CLI side. Claude post-process button waits on the control-panel widget.)*

**Goal.** Make transcripts findable, named, and summarisable. Two user-driven additions captured first in `docs/plan/control-panel.md` §"speech-text — control panel integration".

**Touch.**
- `src/speech_text.rs` — read session name from env `I3MORE_STT_SESSION` (or CLI arg `--session=<name>`). Default to `untitled-<YYYY-MM-DD>-<HH-MM>` when empty. Save path becomes `~/.local/share/i3more/stt/<YYYY-MM-DD>/<session-name>.md` (one file per session, not one shared file per day). Worker writes a small front-matter on first segment: title, started_at, language, model.
- `src/control_panel/widgets/speech_text.rs` (new in control-panel work) — `GtkEntry` for the name; "Summarise with Claude" button that, when a session has stopped, shells out to:
  ```bash
  claude -p "Read the German + English transcript at $TRANSCRIPT and produce a structured markdown summary with: meeting title, date, key decisions, action items (with owners if mentioned), open questions. Save the result to ${TRANSCRIPT%.md}-summary.md. Reply with the file path." \
    --allowedTools 'Read,Write'
  ```
  Output sibling file `<session-name>-summary.md`.

**Exit criteria.**
- Starting with `I3MORE_STT_SESSION="pre-refinement"` produces `~/.local/share/i3more/stt/$(date +%F)/pre-refinement.md` — not the dated default.
- Starting without `I3MORE_STT_SESSION` produces `~/.local/share/i3more/stt/$(date +%F)/untitled-<...>.md` and that path appears in the log.
- Two sequential sessions with the same name in the same day either error cleanly or append (decision: append, with a `---` separator + new front-matter block).
- Manual click on "Summarise with Claude" against a stopped session produces a `<session>-summary.md` next to the raw transcript and surfaces the path in the widget.

**Out of scope (still).**
- Live in-flight summaries.
- Cross-session aggregation.
- Non-Claude LLMs.

### Phase 7 — Bluetooth profile resilience  *(reframed for S1)*

**Goal.** A Teams call (A2DP → HSP/HFP profile switch) does not drop transcription. With S1 in place, this is a PipeWire-stream concern, not a `parec`-restart concern.

**Touch (post-S1).**
- `src/speech_text/capture.rs` — listen on PipeWire's registry for node-add/node-remove events for the Jabra device's monitor. When the active target node disappears (profile switch), re-resolve the new monitor and call `pw_stream_set_active(false)` then re-link to the new node. The same `Stream` object continues; only its target changes.
- Same module — on full Jabra disconnect (no monitors match `device_match`), keep the stream paused and surface a status segment: `[broadcast] {"kind":"info","text":"jabra not connected"}`. Resume on reconnect.

**Touch (pre-S1 fallback, only if S1 is delayed).**
- `src/speech_text.rs` — add a `pactl subscribe` watcher thread (pattern from `src/mic_indicator.rs`); on sink `change`/`new`/`remove` events, re-resolve the Jabra sink by description match; if the active sink changed, kill `parec` and respawn against the new `.monitor`. The whisper-rs context is left intact.

**Exit criteria.**
- Start a session with A2DP music playing → transcripts appear.
- Start a Teams call (triggers HSP/HFP profile switch) → transcripts continue within 1 s (S1 path) / 3 s (pre-S1 path), nothing manual required.
- End the call → transcripts continue when A2DP audio resumes.
- Disconnect the Jabra entirely → session emits a status row "jabra not connected", pipeline pauses, resumes on reconnect.

### Phase 8 — Polish  *(deferred to control-panel widget work)*

**Goal.** Make the feature feel like it belongs next to `i3more-translate`.

**Touch.**
- `assets/speech-text.css` — finalize Gruvbox styling, scrollbar, row hover.
- `src/speech_text_main.rs` — Ctrl+Enter toggles Start/Stop; click-on-row copies `"<de>\n<en>"` to clipboard; window title / icon set.
- `docs/plan/speech-text.md` — add the final i3 binding line at `## i3 integration` (already sketched).

**Exit criteria.**
- All items in **Verification** pass.
- Visual parity with `i3more-translate` (same font, colors, paddings).

## Verification

1. **Smoke — pipeline alive**: play a short German clip (e.g. `espeak -v de "Guten Morgen"` or a YouTube clip) through the headphones. The window should show a German transcript line within ~5 s and an English line within another ~1–2 s.
2. **GPU backend confirmed**: start a session and check `~/.cache/i3more/speech-text.log` (or `whisper-stream` stderr) for `ggml_cuda_init: found 1 CUDA devices`. While active, `nvidia-smi` should show `whisper-stream` as a process using GPU memory. If CPU-only is printed, the CUDA build did not link or the runtime is missing `libcuda.so.1`.
3. **Sink switch**: change the active sink / trigger a Jabra profile switch (A2DP → HSP/HFP by starting a Teams call) while a session is running. The session should restart against the new monitor without needing to toggle Start/Stop.
4. **Persistence**: stop the session, confirm `~/.local/share/i3more/stt/$(date +%F).md` contains each captured pair in the documented markdown format.
5. **Toggle off → GPU/CPU idle**: confirm that after Stop, `pgrep whisper-stream` and `pgrep parec` return nothing; `nvidia-smi` no longer lists whisper-stream; CPU drops to baseline.
6. **Graceful shutdown**: `kill -TERM` the binary — both child processes must die (`SHUTDOWN` flag pathway).
7. **Config round-trip**: delete `~/.config/i3more/speech-text.json`, launch, change model/language in the top bar, close, relaunch, confirm the choice persisted.

## Critical files to read before editing

- `src/translate_main.rs` — the single-instance D-Bus + mpsc + `glib::timeout_add_local` pattern (copy wholesale).
- `src/mic_indicator.rs` — `pactl subscribe` event loop (reuse for default-sink change detection).
- `src/audio_main.rs` — sink/source resolution conventions (`pactl get-default-sink`).
- `src/translate.rs` — `translate(text, source, target)` API (reuse unchanged).
- `src/lib.rs` — `SHUTDOWN` flag + `init_logging` conventions.
- `Dockerfile` — where to add `libsdl2-dev` + whisper build step.

## Streaming architecture (v2 — supersedes the chunked pipeline for Phases 7+)

The Phase 2 pipeline (parec subprocess → 5 s tumble window → whisper-rs `state.full()` → mpsc → stdout/file) works but is **not streaming**. Each output line lags behind speech by the full 5 s buffer, mid-chunk speech is invisible, and silence chunks emit `* Musik *` hallucinations.

This section reshapes the pipeline into a true streaming flow, mirroring (and going beyond) `vendor/whisper.cpp/examples/stream/stream.cpp`. It is decomposed into seven phases (S1–S7). Each is independently shippable; together they replace the audio-capture, inference-dispatch, and output-fanout layers of the current code.

### Why whisper itself is not "streaming"

`whisper.cpp` does not produce token-level streaming output. Its `whisper_full(...)` call is one-shot: encode mel → decode all tokens → return segments. Streaming, in our sense, means **windowed inference with overlap**, plus **VAD-driven commit**, plus **provisional / final** output rows in the UI:

- **Windowed inference** — every `step` ms (e.g. 1 s), run `whisper_full` on the last `length` ms (e.g. 10 s) of audio. Each call is independent.
- **Overlap dedup** — successive windows share the leading `length − step` ms, so their outputs share a common prefix. The runtime emits only the *new suffix* per window.
- **VAD commit** — when energy drops below threshold for ≥ N ms, treat what's been accumulating as a final segment: lock its text, advance the audio head past it, clear the prompt context.
- **Provisional vs final** — UI shows the in-flight transcript in muted style; promotes to normal style on commit. Avoids the "text revising itself in front of you" jank.

Reference: `vendor/whisper.cpp/examples/stream/stream.cpp` (windowed pattern), `vendor/whisper.cpp/examples/common.cpp:610-646` (`vad_simple` energy-based VAD), `vendor/whisper.cpp/include/whisper.h` (`whisper_full_params::no_speech_thold`, `suppress_nst`, `suppress_blank`, `no_context`).

### Audio-capture options

Three viable sources for the Jabra monitor. The streaming refactor moves us off (a) onto (c).

| Option                                | What it is                                                                          | Latency / overhead              | Verdict                |
| ------------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------- | ---------------------- |
| (a) `parec` subprocess + stdout pipe  | Current Phase 2. Out-of-process; line-buffered pipe; ~50–80 ms+ wakeup latency.     | High. Process boundary + SIGPIPE on disconnect. | Replace.               |
| (b) `whisper-stream` binary via SDL2  | What stream.cpp uses. Fails on this host (PipeWire's PA compat hides monitors).     | n/a — cannot select monitor.    | Already ruled out (Phase 1). |
| (c) Direct PipeWire client API        | `pipewire` Rust crate (libpipewire-0.3). Callback-driven. Zero process boundary.    | Lowest. Native pull from monitor node. | **Pick.**          |

### Linux kernel IPC primitives — what we use and why

The user explicitly asked for kernel-IPC research using `~/projects/linux` (now linked at `reference/linux/`). Findings, ranked by use-case fit:

| Primitive                   | Header (under `reference/linux/include/`) | Used in this plan?              | Rationale                                                                 |
| --------------------------- | ----------------------------------------- | ------------------------------- | ------------------------------------------------------------------------- |
| `eventfd`                   | `uapi/linux/eventfd.h`                    | **Yes** (Phase S4)              | Lowest-cost capture-thread → inference-thread wakeup. Replaces mpsc + 10 ms sleep. |
| `epoll`                     | `linux/eventpoll.h`                       | **Yes** (Phase S6)              | One thread, multiple fds (PipeWire fd, eventfd, listen socket, signalfd). |
| `signalfd`                  | `uapi/linux/signalfd.h`                   | **Yes** (Phase S6)              | SIGINT/SIGTERM as an fd in the epoll set; replaces the libc::signal handler. |
| Unix-domain `SOCK_SEQPACKET`| `uapi/linux/socket.h`                     | **Yes** (Phase S5)              | Message-framed local broadcast of segments; control-panel widget + debug viewers subscribe. |
| `memfd_create`              | `uapi/linux/memfd.h`                      | Optional (Phase S2 audio ring)  | Audio ring buffer in anonymous tmpfs; clean lifecycle vs heap `Vec<u8>`. |
| `timerfd`                   | `uapi/linux/timerfd.h`                    | Maybe (Phase S2 step ticker)    | Generates the "1 s step" tick as an epoll-able fd. Alternative: `pipewire`'s timer. |
| `io_uring`                  | `uapi/linux/io_uring.h`                   | No (learning later)             | Overkill for our IO volume. Worth reading just for systems learning.    |
| `pipe` / FIFO               | (no dedicated header — `unistd.h`)        | No                              | Already proven brittle in Phase 1; superseded by SOCK_SEQPACKET.        |
| POSIX shm (`shm_open`)      | `uapi/linux/shm.h`                        | No                              | Multi-process audio sharing — not needed; we run single-process.        |

The plan reaches kernel IPC where it produces a measurable user-visible win (S4, S5, S6). It avoids it where it would just be ceremony (POSIX shm, io_uring) — those are flagged for later kernel-internals study.

### Phase S1 — Direct PipeWire client capture  *(✅ Done — A2DP only; HSP/HFP regression documented)*

**Known limitation discovered during S1.e smoke test (2026-04-25).** On this host's PipeWire 1.0.5, the *native client* monitor capture path (used by both my code and `pw-cat --target=<sink>.monitor`) returns **all-zero buffers** when the Jabra is in **HSP/HFP profile** (`headset-head-unit-msbc`). The same monitor delivers real audio via `parec` (the PulseAudio compat code path) and via *all profiles* in A2DP (`a2dp-sink`, peak ≥ 14 000 confirmed). Reproduced with both `STREAM_CAPTURE_SINK = "true"` and `target.object = "<sink>.monitor"` — the bug is below the property layer.

Implications:
- A2DP playback (Tagesschau, music, browser audio) → transcribes correctly.
- HSP/HFP (Teams calls, anything that opens the mic) → silent capture; `* Musik *` hallucinations only.

Mitigations (not yet implemented; revisit during Phase 7 reframing):
- Detect prolonged silence on the PipeWire path and *temporarily* fall back to spawning a `parec` subprocess against `<sink>.monitor`. Hybrid pipeline; ugly but resilient.
- Switch the card profile to A2DP when starting a session (loses the mic; OK for "transcribe what I hear", not OK for "transcribe the whole call").
- File a PipeWire upstream bug. The reproducer is straightforward.

**Other exit-criteria results (all pass):**
- `nm -D dist/i3more-speech-text | grep pw_stream_connect` → `U pw_stream_connect` ✓
- 30 s capture (A2DP, Tagesschau): five segments of coherent German ✓
- `pgrep -a parec` finds nothing while running ✓
- GPU path intact (0.06–0.17 s inference per 5 s chunk) ✓



**Goal.** Replace the `parec` subprocess + stdout pipe with a native PipeWire client running in the same process. Eliminates SIGPIPE-on-disconnect failure mode and lets the audio callback feed an in-process ring buffer directly.

**Touch.**
- `Cargo.toml` — add `pipewire = "0.8"` (Rust bindings; libpipewire-0.3-dev required at build time, already in `Dockerfile.whisper-cuda` if not, add `libpipewire-0.3-dev`).
- `src/speech_text/capture.rs` (new module) — `CaptureBuilder { device_match, sample_rate, channels, on_pcm: Box<dyn FnMut(&[i16]) + Send> }`. Internally:
  - Connects to PipeWire via `Context::connect(...)`.
  - Resolves the Jabra monitor by walking the registry, matching `Properties::node.description`.
  - Creates a `Stream` of media class `Audio/Capture`, target node = the resolved monitor, format `S16_LE / 16 kHz / mono`.
  - Process callback pushes samples to `on_pcm`. Runs on PipeWire's own thread.
- `src/speech_text.rs` — `SpeechSession::start` constructs a `CaptureBuilder` instead of spawning `parec`. Drop semantics close the stream.

**Exit criteria.**
- `nm -D dist/i3more-speech-text | grep pw_stream_connect` returns a hit (PipeWire linked).
- A 30 s test against live Jabra audio produces transcripts identical in content to Phase 2.
- `pgrep -a parec` finds nothing while the binary is running.
- A Bluetooth disconnect → reconnect cycle no longer kills the session — the stream callback simply stops/resumes.

### Phase S2 — Sliding window inference + overlap dedup  *(✅ Done)*

**Goal.** Replace the 5 s tumble window with a sliding window: every `step_ms` (default 1000), feed the last `length_ms` (default 10000) of audio into `state.full(...)`, emit the *new suffix* of the transcript.

**Touch.**
- `src/speech_text/ringbuf.rs` (new) — fixed-size `i16` ring buffer sized to `length_ms`. Capture callback writes; inference thread reads the most recent `length_ms` worth.
- `src/speech_text.rs` — inference loop:
  ```text
  loop:
    wait for step tick (timerfd or simple sleep_until)
    if energy < silence_thold for ≥ commit_ms → commit + clear prompt
    snapshot = ringbuf.last(length_ms) as f32
    run state.full(samples=snapshot, prompt_tokens=last_tokens, no_context=false)
    new_text = full transcript of this window
    suffix = strip_common_prefix(new_text, prev_text)
    if suffix non-empty → emit Segment::Provisional { suffix }
    prev_text = new_text
  ```
- Dedup helper: `fn strip_common_prefix(new: &str, prev: &str) -> &str` — careful with UTF-8 boundaries.

**Exit criteria.**
- New rows appear ~1 s after the speaker starts a phrase, not 5 s.
- Mid-utterance, the same phrase grows in place (provisional updates) instead of waiting for completion.
- No duplicate text across rows during continuous speech.

### Phase S3 — VAD-driven commit + hallucination suppression  *(✅ Done)*

**Goal.** Decide *when a segment is final*. Suppress `* Musik *` and similar hallucinations on silence.

**Touch.**
- Port `vad_simple` from `vendor/whisper.cpp/examples/common.cpp:610` to `src/speech_text/vad.rs`. ~30 lines: high-pass filter at 100 Hz, energy over the last 1000 ms vs the prior 500 ms, threshold check.
- In the inference loop:
  - On each step, after running inference, check `vad.is_silent(last_500ms)`.
  - If silent for ≥ `commit_ms` (default 700): emit current `Segment::Final { text }`, clear `prev_text`, advance ring-buffer read head past the silent zone.
- `FullParams` tuning (in the same call site):
  - `set_no_speech_thold(0.6)` — drop segments with ≥ 60 % "no-speech" probability.
  - `set_suppress_blank(true)`, `set_suppress_nst(true)` — suppress non-speech tokens.
  - `set_no_context(true)` after a silence-commit, then `false` again on resume — prevents context bleeding through silence.
  - `set_temperature(0.0)` and `set_temperature_inc(0.0)` for deterministic decoding (avoids the "let me hallucinate" temperature ramp).

**Exit criteria.**
- A 30 s capture of pure silence (Jabra connected, nothing playing) produces zero rows.
- Speech → silence → speech yields two distinct **final** rows rather than one provisional run-on.
- Replaying the Phase 6.5 smoke audio no longer prints `* Musik *`.

### Phase S4 — eventfd-driven inference dispatch

**Goal.** Tighten capture → inference latency. Replace the inference-loop's "sleep 10 ms then check" with an `eventfd` that the capture callback signals once per `step_ms`-worth of audio.

**Touch.**
- `src/speech_text/wakeup.rs` (new) — thin wrapper around `nix::sys::eventfd::EventFd`. `signal()` writes 1; `wait()` blocks reading.
- Capture callback (Phase S1) tracks samples-since-last-step; when ≥ `step_ms` accumulated, calls `wakeup.signal()`.
- Inference thread blocks on `wakeup.wait()` instead of sleeping. (Step gating still happens in code — eventfd just removes the polling overhead.)

**Exit criteria.**
- `strace -e read -p <pid>` while running shows reads on the eventfd, not on a timer or sleep loop.
- `perf stat -p <pid>` over 30 s shows fewer wakeups than the Phase S2 baseline.

### Phase S5 — Live transcript broadcast over Unix-domain SOCK_SEQPACKET

**Goal.** Stream provisional + final segments out of the speech-text process to any subscriber: TUI follower, control-panel widget, post-process daemon. One writer, many readers.

**Touch.**
- `src/speech_text/broadcast.rs` (new):
  - On startup, bind a listening socket at `$XDG_RUNTIME_DIR/i3more/speech-text.sock` (typically `/run/user/<UID>/i3more/speech-text.sock`). `SOCK_SEQPACKET` preserves message boundaries — one segment = one packet.
  - Maintain a `Vec<UnixStream>` of subscribers. Accept new ones via `epoll` (Phase S6) or a small accept thread.
  - On each emitted Segment, serialise to a small JSON line (`{"kind":"provisional|final","at":"…","text":"…"}`) and `send` to every subscriber, dropping any that have closed.
- New tiny binary `i3more-speech-text-tail` — connects, prints with provisional in dim and final in bold. ~30 lines.

**Exit criteria.**
- Two subscribers connected concurrently both receive every segment exactly once.
- A subscriber that disconnects rough mid-broadcast does not break the writer.
- Permissions on the socket: `0600` (owner only) — the runtime dir is already private per-UID.

### Phase S6 — `epoll` + `signalfd` event loop

**Goal.** Single main loop multiplexing all the file descriptors the process owns. Replaces the ad-hoc thread + sleep model.

**Touch.**
- `src/speech_text/loop.rs` (new) — `EventLoop` owning an `epoll` instance and the fds: PipeWire stream fd (S1), inference wakeup eventfd (S4), broadcast listen socket (S5), signalfd for SIGINT/SIGTERM/SIGHUP, and `timerfd` for the 1 s step tick.
- Replace the libc-`signal` handler from Phase 2 with `signalfd`. SHUTDOWN flag becomes "epoll loop saw SIGTERM".

**Exit criteria.**
- `ls -l /proc/$(pgrep -f i3more-speech-text)/fd` lists the eventfd, signalfd, timerfd, listen socket, PipeWire socket.
- One thread does the multiplexing; only the PipeWire RT thread and (optionally) inference-on-GPU thread exist beyond it.
- SIGTERM is observed in the loop (not in a signal handler), and the loop drains pending output before exiting.

### Phase S7 — Real-time UI follower

**Goal.** A user-visible window that shows live transcripts at near-zero latency, while we wait for the GTK4 control-panel widget to land.

**Touch.**
- `i3more-speech-text-tail` from Phase S5, dressed up: line-wraps, scrolls, distinguishes provisional from final by colour, prints session name in the title bar.
- i3 binding: launch `alacritty -e i3more-speech-text-tail` next to the existing speech-text trigger; this becomes the user-facing display.

**Exit criteria.**
- Pressing the i3 keybind starts capture **and** opens the follower side-by-side.
- The follower's first row appears within 1.5 s of speech start (i.e. one step + small inference time).
- Closing the follower window does not kill the capture.

### Phase ordering / dependencies

- S1 unblocks S2, S3, S4, S6 (all depend on direct capture).
- S2 unblocks S3.
- S3 is the highest user-visible win (no more silence hallucinations + finalised rows).
- S4 + S6 are systems-internals refinements; S5 + S7 are the user-visible streaming output.

Recommended execution order: **S1 → S2 → S3 → S5 → S7 → S6 → S4** (deliver visible streaming first, internalise the kernel plumbing second).



## Out of scope (future work)

- Multi-speaker diarisation.
- In-window transcript search / export to other formats.
- Vulkan / ROCm backends (only NVIDIA CUDA is in scope — whisper.cpp supports both but we only have an NVIDIA target).
- POSIX shared memory / `io_uring` for the speech-text pipeline (single-process; not needed). Keep around as kernel-internals reading material under `reference/linux/`.
- True token-level streaming via `whisper_encode` + `whisper_decode` directly. The S2/S3 windowed approach is sufficient; full custom decoding is a separate research project.

(Items previously here that are now in scope: voice-activity gating moved to Phase S3; streaming output to Phases S2 / S5 / S7.)
