# Docker-to-Kernel Mapping

Every `docker run` flag maps to a Linux kernel primitive:

| Docker Flag                        | Kernel Subsystem             | What Happens Under the Hood                                                              |
| ---------------------------------- | ---------------------------- | ---------------------------------------------------------------------------------------- |
| `docker run` itself                | `clone(2)` + namespaces      | Creates pid, net, mount, user, uts, ipc, cgroup namespaces via `clone()` syscall         |
| `--memory=8g`                      | cgroups v2 `memory.max`      | Writes `8589934592` to `/sys/fs/cgroup/<id>/memory.max`; kernel OOM-kills if exceeded    |
| `--cpus=6.0`                       | cgroups v2 `cpu.max`         | Sets CFS bandwidth quota in `/sys/fs/cgroup/<id>/cpu.max` (e.g. `600000 100000`)         |
| `--read-only`                      | overlayfs (no upper dir)     | Image layers composed via overlayfs with no writable upper directory                     |
| `--tmpfs /tmp:size=512M`           | tmpfs (RAM-backed fs)        | Kernel mounts `tmpfs` with `size=512M` in container's mount namespace                    |
| `--security-opt=no-new-privileges` | `prctl(PR_SET_NO_NEW_PRIVS)` | Bit set on process; prevents privilege escalation via setuid/execve for all descendants  |
| `-v path:/workspace`               | bind mount (VFS layer)       | `mount --bind` in container's mount namespace; same inode, different mount point         |
| `-v claude-home:/home/...`         | Docker named volume          | Block storage managed by Docker volume driver; survives `--rm`                           |
| (default) seccomp profile          | seccomp-bpf                  | BPF program filters syscalls; ~300 allowed, dangerous ones (reboot, kexec, etc.) blocked |

## Kernel Deep Dive: What Happens When You Run `claudei`

1. **Namespace creation** — `clone(2)` with `CLONE_NEWPID | CLONE_NEWNS | CLONE_NEWNET | ...` creates isolated process tree, mount table, network stack
2. **cgroup setup** — Docker creates a new cgroup under `/sys/fs/cgroup/system.slice/docker-<id>.scope/`, writes memory and CPU limits
3. **overlayfs mount** — Image layers (node:20-slim + claude-code) stacked as `lowerdir` with no `upperdir` (read-only)
4. **Bind mount** — Host project directory bind-mounted at `/workspace` inside the container's mount namespace
5. **tmpfs mounts** — Kernel creates RAM-backed filesystems at `/tmp`, `/run`, `~/.npm` within container
6. **seccomp filter** — BPF program loaded via `seccomp(2)` syscall, filtering every subsequent syscall
7. **no_new_privs** — `prctl(PR_SET_NO_NEW_PRIVS, 1)` called; inheritable by all child processes
8. **entrypoint.sh** — `execve()` replaces init process; sentinel file pattern for first-run init

## Inspecting Kernel State From Host (while container runs)

```bash
# Find container's cgroup
CONTAINER_ID=$(docker inspect --format '{{.Id}}' <container>)
cat /sys/fs/cgroup/system.slice/docker-${CONTAINER_ID}.scope/memory.max
cat /sys/fs/cgroup/system.slice/docker-${CONTAINER_ID}.scope/cpu.max

# See container's namespaces
PID=$(docker inspect --format '{{.State.Pid}}' <container>)
ls -la /proc/$PID/ns/

# See mount table inside container's mount namespace
cat /proc/$PID/mountinfo | grep -E "overlay|tmpfs|workspace"

# See seccomp status
grep Seccomp /proc/$PID/status
```

## What Docker Abstracts: Dockerfile + Compose Parsed

### Dockerfile Breakdown — What Each Instruction Does at the Kernel Level

```dockerfile
FROM node:20-slim
```

- Docker pulls an OCI image (tar layers) and unpacks them as overlayfs `lowerdir` layers
- **Rust equivalent**: Download tar archives, extract to directories, build an overlayfs mount string
- **Kernel syscall**: `mount("overlay", "/merged", "overlay", 0, "lowerdir=...:...,upperdir=...,workdir=...")`

```dockerfile
RUN apt-get install -y git curl vim postgresql-client jq
```

- Docker creates a temporary container (clone + namespaces), runs the command, snapshots the filesystem diff as a new layer
- **Rust equivalent**: `clone(2)` → `chroot(2)` → `execve("/bin/sh", ["-c", "apt-get ..."])` → diff the filesystem → store as tar layer
- Each `RUN` = one new overlayfs layer

```dockerfile
RUN useradd -ms /bin/bash -u 1001 claude-user
```

- Writes to `/etc/passwd`, `/etc/shadow`, `/etc/group` inside the container layer
- Creates `/home/claude-user` with bash shell
- **Rust equivalent**: Write passwd/group entries directly, `mkdir` + `chown` the home dir

```dockerfile
RUN mkdir -p /workspace ... && chown -R claude-user:claude-user ...
```

- Filesystem operations inside a layer
- **Rust equivalent**: `mkdir(2)` + `chown(2)` syscalls, or `std::fs::create_dir_all` + `nix::unistd::chown`

```dockerfile
USER claude-user
WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["bash"]
```

- These are **metadata only** — stored in the OCI image config JSON, not filesystem layers
- At runtime: `setuid(1001)` + `setgid(1001)`, `chdir("/workspace")`, `execve("entrypoint.sh", ["bash"])`

### Compose YAML Breakdown — What Each Setting Maps To

| Compose Key | What Docker Does | Kernel Syscall / Subsystem | Rust Crate |
| --- | --- | --- | --- |
| `volumes: - ../../:/workspace` | Bind mount host dir | `mount(src, "/workspace", NULL, MS_BIND, NULL)` | `nix::mount::mount` |
| `volumes: - claude-home:/home/claude-user` | Create/reuse named volume, bind mount it | `mount` on a managed directory | `nix::mount::mount` |
| `read_only: true` | Remount rootfs read-only | `mount(NULL, "/", NULL, MS_REMOUNT\|MS_RDONLY, NULL)` | `nix::mount::mount` |
| `tmpfs: - /tmp:size=512M` | Mount tmpfs | `mount("tmpfs", "/tmp", "tmpfs", 0, "size=512M")` | `nix::mount::mount` |
| `memory: 8G` | Write cgroup memory limit | Write `8589934592` to `memory.max` | `std::fs::write` |
| `cpus: "6.0"` | Write cgroup CPU quota | Write `600000 100000` to `cpu.max` | `std::fs::write` |
| `security_opt: no-new-privileges` | Set prctl bit | `prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)` | `nix::sys::prctl` |
| `stdin_open: true` + `tty: true` | Allocate PTY, connect stdin | `openpty(2)` / `ioctl(TIOCSCTTY)` | `nix::pty::openpty` |
| `env_file` / `environment` | Set env vars before exec | `execve` env parameter | `std::env::set_var` |
| `networks: fx-db-net` | Create/join network namespace + veth pair | `clone(CLONE_NEWNET)` + `ip link add veth` | `nix::sched::clone` + netlink |

### Entrypoint.sh — What It Does in Syscalls

| Script Line | Kernel Equivalent |
| --- | --- |
| `mkdir -p ~/.claude ~/.config` | `mkdir(2)` with `O_CREAT` semantics |
| `touch ~/.init_done` | `open(O_CREAT\|O_WRONLY)` + `close` |
| `touch ~/.claude/.write_test` | `open(O_CREAT\|O_WRONLY)` — tests filesystem is writable |
| `exec "$@"` | `execve(2)` — replaces shell PID with the target command |
