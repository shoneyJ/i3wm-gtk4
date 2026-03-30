# Sequential Implementation: Replacing Docker with Rust

Bash prototype (`scripts/claudei.sh`) is phase 0. Each subsequent phase replaces one Docker
abstraction with direct kernel calls in Rust. Ordered by difficulty and dependency.

## Phase 0: Bash Wrapper (current)

**Difficulty**: trivial
**What you learn**: Docker CLI flags → kernel concepts mapping
**File**: `scripts/claudei.sh`

Wraps `docker run` with the right flags. No kernel programming. Proves the workflow.

## Phase 1: Namespaces — Process Isolation

**Difficulty**: moderate
**Kernel concepts**: `clone(2)`, `unshare(2)`, `setns(2)`, `/proc/PID/ns/`
**Rust crates**: `nix` (clone, unshare), `libc`
**What to build**: Spawn a child process in new pid + mount + uts namespaces

```rust
// Core concept
use nix::sched::{clone, CloneFlags};
let flags = CloneFlags::CLONE_NEWPID
    | CloneFlags::CLONE_NEWNS
    | CloneFlags::CLONE_NEWUTS;
```

**Why first**: Namespaces are the foundation — everything else happens inside them.
Doing this without networking (CLONE_NEWNET) simplifies the first step.

## Phase 2: Filesystem — Mount + chroot/pivot_root

**Difficulty**: moderate
**Kernel concepts**: `mount(2)`, `pivot_root(2)`, `chroot(2)`, bind mounts, overlayfs, tmpfs
**Rust crates**: `nix::mount`, `std::fs`
**What to build**: Inside the namespace from Phase 1, set up the filesystem

```
1. Mount overlayfs (or just bind mount a rootfs directory for now)
2. Bind mount the user's project dir → /workspace
3. Mount tmpfs at /tmp, /run, ~/.npm
4. pivot_root to the new root
5. Remount / as read-only
```

**Why second**: Without a filesystem, the isolated process has nothing to run.
Start simple — skip overlayfs, just bind-mount an extracted rootfs directory.

## Phase 3: cgroups v2 — Resource Limits

**Difficulty**: easy (just file writes)
**Kernel concepts**: `/sys/fs/cgroup/`, `memory.max`, `cpu.max`, cgroup delegation
**Rust crates**: `std::fs::write`
**What to build**: Create a cgroup scope, write limits, move the child PID into it

```rust
// Create cgroup
fs::create_dir_all("/sys/fs/cgroup/claudei.scope")?;
// Set memory limit (8GB)
fs::write("/sys/fs/cgroup/claudei.scope/memory.max", "8589934592")?;
// Set CPU limit (6 cores = 600ms per 100ms period)
fs::write("/sys/fs/cgroup/claudei.scope/cpu.max", "600000 100000")?;
// Move process into cgroup
fs::write("/sys/fs/cgroup/claudei.scope/cgroup.procs", pid.to_string())?;
```

**Why third**: File writes are simple, but you need the PID from Phase 1 and the
mount namespace from Phase 2 to be set up first.

## Phase 4: Security — seccomp + no_new_privs + user switching

**Difficulty**: moderate-to-hard (seccomp BPF is tricky)
**Kernel concepts**: `prctl(PR_SET_NO_NEW_PRIVS)`, `seccomp(SECCOMP_SET_MODE_FILTER)`, BPF, `setuid/setgid`
**Rust crates**: `nix::sys::prctl`, `libseccomp` or `seccompiler`
**What to build**:

```
1. prctl(PR_SET_NO_NEW_PRIVS) — one line, blocks setuid escalation
2. setgid(1001) + setuid(1001) — drop to non-root user
3. seccomp BPF filter — whitelist ~300 safe syscalls (start by copying Docker's default profile)
```

**Why fourth**: Security is a hardening layer on top of a working sandbox.
Get the sandbox running first, then lock it down.

## Phase 5: PTY — Interactive Terminal

**Difficulty**: moderate
**Kernel concepts**: `openpty(2)`, `ioctl(TIOCSCTTY)`, `SIGWINCH`, terminal raw mode
**Rust crates**: `nix::pty`, `termion` or `crossterm`
**What to build**: Allocate a PTY pair, connect stdin/stdout of the host to the child's PTY

```
Host terminal ←→ PTY master (parent) ←→ PTY slave (child in namespace)
```

**Why fifth**: Until now you can test with simple command output piped to stdout.
PTY is needed for interactive use (vim, claude cli prompts, etc.)

## Phase 6: OCI Image Parsing — Replace `docker pull`

**Difficulty**: hard
**Kernel concepts**: OCI image spec, tar layer extraction, overlayfs layer composition
**Rust crates**: `oci-distribution`, `flate2`, `tar`
**What to build**:

```
1. Pull image manifest from registry (HTTP API)
2. Download and extract tar.gz layers to directories
3. Compose overlayfs lowerdir string from layer directories
4. Mount overlayfs with all layers
```

**Why last**: This is the most complex part and the least kernel-related.
You can defer it indefinitely by extracting the rootfs from the existing Docker image:

```bash
# One-time export — gives you a rootfs directory to use in Phases 1-5
docker export $(docker create workspace-claude-cli:latest) | tar -xf - -C rootfs/
```

## Phase 7 (optional): Networking — veth + bridge

**Difficulty**: hard
**Kernel concepts**: `CLONE_NEWNET`, veth pairs, bridges, netlink, iptables/nftables
**Rust crates**: `rtnetlink`, `netlink-packet-route`
**What to build**: Create a veth pair, attach one end to a bridge, configure NAT

**Why optional**: `claudei` doesn't need network isolation for the base use case.
The container can share the host network namespace initially.

## Difficulty Summary

| Phase | What | Difficulty | Lines of Rust (est.) | Key Syscalls |
| --- | --- | --- | --- | --- |
| 0 | Bash wrapper | trivial | 0 | — |
| 1 | Namespaces | ★★☆☆☆ | ~50 | `clone`, `unshare` |
| 2 | Filesystem | ★★★☆☆ | ~120 | `mount`, `pivot_root`, `chroot` |
| 3 | cgroups | ★☆☆☆☆ | ~30 | file writes to `/sys/fs/cgroup` |
| 4 | Security | ★★★☆☆ | ~80 | `prctl`, `seccomp`, `setuid` |
| 5 | PTY | ★★★☆☆ | ~100 | `openpty`, `ioctl` |
| 6 | OCI images | ★★★★☆ | ~300 | HTTP + tar + overlayfs |
| 7 | Networking | ★★★★☆ | ~200 | netlink, veth, bridge |
