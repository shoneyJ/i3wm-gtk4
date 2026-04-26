# NVIDIA Container Toolkit

Reference note on the host-side prerequisite for GPU-accelerated whisper.cpp builds used by the **speech-to-text** feature (`docs/plan/speech-text.md`).

## What it is

The NVIDIA Container Toolkit is the piece that lets a Docker / Podman / Kubernetes container see and use the host's NVIDIA GPU. Out of the box, a container is walled off from the host's GPU hardware — it cannot touch `/dev/nvidia*`, the kernel driver, or CUDA. The toolkit registers a thin runtime with Docker that:

1. Recognises the `--gpus` flag on `docker run` (and `deploy.resources.reservations.devices` in Compose v2+).
2. When that flag is present, mounts the host's NVIDIA driver files (`libnvidia-*.so`, `/dev/nvidiactl`, `/dev/nvidia0`, …) into the container at start.
3. Adjusts the container's library search path so programs link against those mounted driver libs at runtime.

The container then runs CUDA code against the host's physical GPU — no virtualization, no passthrough — with near-native performance.

## What it is NOT

| It is not...                       | Why that matters                                                               |
| ---------------------------------- | ------------------------------------------------------------------------------ |
| A container image                  | You still `FROM nvidia/cuda:*` to get CUDA tooling inside the container.       |
| The CUDA toolkit                   | The SDK that compiles CUDA code. Only needed on the host for non-Docker builds.|
| The NVIDIA kernel driver           | Your existing `nvidia.ko` module is untouched; the toolkit reuses it.          |
| A replacement for `docker`         | It plugs into Docker; Docker itself keeps working for non-GPU containers.      |

## Why this project needs it

`docs/plan/speech-text.md` ships a `whisper-build` Compose service that builds `whisper-stream` with `-DGGML_CUDA=1` inside `nvidia/cuda:*-devel-ubuntu24.04`. The build must link against the host's CUDA driver; the runtime smoke test (Phase 0 exit criteria) must call into the GPU. Both require the toolkit.

Without the toolkit, `docker run --gpus all` fails with:

```
docker: Error response from daemon: could not select device driver "" with capabilities: [[gpu]]
```

## Components installed

One apt package — `nvidia-container-toolkit` — brings:

- `nvidia-container-runtime` — a thin wrapper around `runc` that injects GPU mounts.
- `nvidia-container-cli` — the low-level tool that performs the injection.
- `nvidia-ctk` — small CLI for configuring Docker / containerd / CRI-O integration.

It does **not** install CUDA, does **not** update the NVIDIA driver, does **not** require a reboot.

## Install (Ubuntu / Debian-family)

```bash
# 1. Add NVIDIA's apt repo
curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey \
  | sudo gpg --dearmor -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg
curl -s -L https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list \
  | sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://#g' \
  | sudo tee /etc/apt/sources.list.d/nvidia-container-toolkit.list

# 2. Install the package
sudo apt-get update
sudo apt-get install -y nvidia-container-toolkit

# 3. Register the runtime with Docker (edits /etc/docker/daemon.json)
sudo nvidia-ctk runtime configure --runtime=docker

# 4. Restart the docker daemon so it picks up the new runtime
sudo systemctl restart docker
```

Source: official NVIDIA docs at <https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html>.

## Verify

```bash
# Docker sees the nvidia runtime
docker info | grep -i runtime
# → Runtimes: io.containerd.runc.v2 nvidia runc

# Container can access the GPU
docker run --rm --gpus all nvidia/cuda:12.6.3-base-ubuntu24.04 nvidia-smi
# → Prints the same GPU info as running nvidia-smi on the host
```

If both pass, the `whisper-build` service in `docker-compose.yaml` will work.

## Troubleshooting

| Symptom                                                             | Root cause                                                  | Fix                                                                        |
| ------------------------------------------------------------------- | ----------------------------------------------------------- | -------------------------------------------------------------------------- |
| `could not select device driver "" with capabilities: [[gpu]]`       | Toolkit not installed, or daemon not restarted after config | Run the install steps above in full.                                       |
| `nvidia-smi` inside container prints different driver than host      | Container's image ships its own driver stubs (unusual)      | Use an official `nvidia/cuda:*` image.                                      |
| `Failed to initialize NVML: Unknown Error` inside container          | Cgroups v2 + older toolkit                                  | `sudo apt-get upgrade nvidia-container-toolkit`; toolkit ≥ 1.14 handles v2.|
| Compose rejects `deploy.resources.reservations.devices`              | Using Compose v1 (`docker-compose`)                         | Use Compose v2 (`docker compose`, space, not hyphen) — v1 is EOL.          |
| Host driver too old for chosen CUDA image                            | `nvidia/cuda:12.6.3-*` needs driver ≥ 560                    | Check `nvidia-smi`'s "Driver Version". Bump driver or pick an older image. |

Host driver ↔ CUDA runtime compatibility matrix: <https://docs.nvidia.com/deploy/cuda-compatibility/>.

## Relation to the plan phases

- **Phase 0 — Ground truth** (`docs/plan/speech-text.md` §"Implementation phases"): this toolkit must be installed before the `whisper-build` service can run. Phase 0's exit criteria explicitly calls out `docker compose run --rm whisper-build nvidia-smi` as the smoke-test gate.
- **Phases 1–8**: toolkit is not re-invoked during later phases. Once the `whisper-stream` binary is built and copied to `dist/`, it runs directly on the host — the host's NVIDIA driver is all it needs. The toolkit matters only at *build time* (and for re-builds when bumping the whisper.cpp version).

## Uninstall

```bash
sudo apt-get purge nvidia-container-toolkit nvidia-container-runtime
sudo rm /etc/apt/sources.list.d/nvidia-container-toolkit.list
sudo rm /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg
# Optional: remove the docker daemon config entry the toolkit added
sudoedit /etc/docker/daemon.json   # remove the "nvidia" runtime block
sudo systemctl restart docker
```

No side effects on the NVIDIA driver, CUDA, or unrelated Docker images.
