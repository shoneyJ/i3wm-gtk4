# Linux Kernel Terms — Ubuntu x86 (Kernel 6.14.0-37-generic)

---

## I/O & Event Notification

| Term          | Description                                                                |
| ------------- | -------------------------------------------------------------------------- |
| epoll         | Scalable I/O event notification for monitoring many file descriptors       |
| io_uring      | Async I/O interface using submission/completion ring buffers (kernel 5.1+) |
| select / poll | Legacy I/O multiplexing; predecessors to epoll                             |
| eventfd       | Lightweight kernel object for event signaling between threads/processes    |
| signalfd      | Converts signals into file descriptor events for epoll integration         |
| timerfd       | Delivers timer expiration events via file descriptors                      |
| inotify       | File system change notification (file create/modify/delete)                |
| fanotify      | Advanced filesystem notification with access decisions and marks           |
| splice / tee  | Zero-copy data transfer between file descriptors via kernel pipe buffers   |
| AIO (libaio)  | Legacy async I/O for direct disk access; largely replaced by io_uring      |

## Process & Scheduling

| Term                  | Description                                                                     |
| --------------------- | ------------------------------------------------------------------------------- |
| fork / clone / exec   | Process creation syscalls; clone allows shared memory/fd spaces                 |
| CFS                   | Completely Fair Scheduler — default process scheduler using red-black tree      |
| EEVDF                 | Earliest Eligible Virtual Deadline First — CFS replacement (kernel 6.6+)        |
| SCHED_FIFO / SCHED_RR | Real-time scheduling policies (FIFO priority, round-robin)                      |
| SCHED_DEADLINE        | Earliest Deadline First real-time scheduler                                     |
| cgroups v2            | Resource limits (CPU, memory, I/O) for process groups; unified hierarchy        |
| namespaces            | Isolation primitives: pid, net, mount, user, uts, ipc, cgroup, time             |
| waitpid / wait4       | Wait for child process state changes                                            |
| setsid                | Create new session and process group                                            |
| prctl                 | Process control operations (set name, seccomp mode, signal handling)            |
| pidfd                 | File descriptor referring to a process; race-free signal delivery (kernel 5.3+) |

## Memory Management

| Term                | Description                                                                               |
| ------------------- | ----------------------------------------------------------------------------------------- |
| mmap / munmap       | Memory-mapped files and anonymous mappings                                                |
| mlock / mlockall    | Lock pages in RAM, prevent swapping (critical for security)                               |
| madvise             | Advise kernel on memory usage patterns (MADV_DONTNEED, MADV_HUGEPAGE)                     |
| OOM killer          | Kernel mechanism to kill processes under memory pressure; oom_score_adj controls priority |
| Huge Pages / THP    | 2MB/1GB pages reducing TLB misses; THP = Transparent Huge Pages (automatic)               |
| Copy-on-Write (COW) | Shared pages after fork; copied only on write                                             |
| NUMA                | Non-Uniform Memory Access — memory locality on multi-socket systems                       |
| userfaultfd         | Userspace page fault handling (live migration, lazy restore)                              |
| memfd_create        | Create anonymous file in memory; used for shared memory without filesystem                |
| shmem / tmpfs       | Kernel shared memory backed by swap; powers /dev/shm and tmpfs mounts                     |
| KSM                 | Kernel Same-page Merging — deduplicates identical memory pages                            |

## Filesystem & VFS

| Term              | Description                                                            |
| ----------------- | ---------------------------------------------------------------------- |
| VFS               | Virtual Filesystem Switch — abstraction layer above all filesystems    |
| ext4              | Default Ubuntu filesystem with journaling, extents, delayed allocation |
| Btrfs             | Copy-on-write filesystem with snapshots, checksums, compression        |
| overlayfs         | Union mount filesystem; powers container image layers                  |
| procfs (/proc)    | Virtual filesystem exposing process and kernel state                   |
| sysfs (/sys)      | Virtual filesystem exposing kernel objects (devices, drivers, buses)   |
| devtmpfs (/dev)   | Auto-populated device nodes                                            |
| cgroup filesystem | Virtual filesystem for cgroup hierarchy management                     |
| fuse              | Filesystem in Userspace — allows userspace filesystem implementations  |
| io_uring_cmd      | Filesystem-specific commands via io_uring (kernel 6.x+)                |

## Networking

| Term                 | Description                                                                  |
| -------------------- | ---------------------------------------------------------------------------- |
| Netfilter / nftables | Kernel packet filtering framework; nftables replaces iptables                |
| netlink              | Kernel ↔ userspace socket protocol for networking config and events          |
| Unix domain sockets  | Local IPC via socket API (SOCK_STREAM, SOCK_DGRAM, SOCK_SEQPACKET)           |
| eBPF (XDP)           | Programmable packet processing at driver level; near-wire-speed filtering    |
| TCP BBR              | Bottleneck Bandwidth and RTT congestion control algorithm                    |
| MPTCP                | Multipath TCP — multiple network paths for a single connection (kernel 5.6+) |
| SO_REUSEPORT         | Allow multiple sockets to bind same port; kernel load-balances               |

## Security

| Term                  | Description                                                                         |
| --------------------- | ----------------------------------------------------------------------------------- |
| seccomp / seccomp-bpf | Syscall filtering using BPF programs for sandboxing                                 |
| capabilities          | Fine-grained privilege splitting (CAP_NET_RAW, CAP_SYS_RESOURCE, etc.)              |
| LSM                   | Linux Security Modules framework (AppArmor on Ubuntu by default)                    |
| AppArmor              | Mandatory access control via path-based profiles (Ubuntu default LSM)               |
| SELinux               | Mandatory access control via labels (alternative to AppArmor)                       |
| Landlock              | Unprivileged sandboxing LSM for filesystem access control (kernel 5.13+)            |
| keyring               | In-kernel credential and key storage                                                |
| PAM                   | Pluggable Authentication Modules — userspace auth framework with kernel interaction |
| dm-crypt / LUKS       | Block device encryption via device-mapper                                           |
| IMA / EVM             | Integrity Measurement Architecture — file integrity verification                    |

## Device & Driver Layer

| Term                  | Description                                                                 |
| --------------------- | --------------------------------------------------------------------------- |
| udev                  | Userspace device manager; receives kernel uevents, creates /dev nodes       |
| DRM / KMS             | Direct Rendering Manager / Kernel Mode Setting — GPU and display management |
| ALSA                  | Advanced Linux Sound Architecture — kernel audio subsystem                  |
| PulseAudio / PipeWire | Userspace audio servers on top of ALSA                                      |
| input subsystem       | Kernel layer for keyboard, mouse, touch events (/dev/input/event\*)         |
| evdev                 | Generic input event interface for userspace                                 |
| sysfs class devices   | /sys/class/backlight, /sys/class/power_supply, /sys/class/thermal           |
| eBPF                  | Programmable kernel hooks for tracing, networking, security                 |
| VFIO                  | Virtual Function I/O — userspace device drivers, GPU passthrough            |

## IPC Mechanisms

| Term                  | Description                                                                 |
| --------------------- | --------------------------------------------------------------------------- |
| pipes / FIFOs         | Unidirectional byte streams between processes                               |
| Unix domain sockets   | Bidirectional local IPC via socket API                                      |
| futex                 | Fast Userspace Mutex — kernel primitive behind pthread_mutex and Rust Mutex |
| eventfd               | Lightweight counter-based signaling between threads/processes               |
| shared memory (shmem) | Memory regions shared between processes via mmap or shmget                  |
| message queues (mq)   | POSIX or SysV message passing between processes                             |
| D-Bus (kdbus history) | Userspace IPC bus; kernel kdbus was rejected but concept remains relevant   |
| memfd + sealing       | Anonymous shared memory with immutability guarantees                        |

## Tracing & Observability

| Term              | Description                                                         |
| ----------------- | ------------------------------------------------------------------- |
| eBPF              | Attach programs to kernel tracepoints, kprobes, uprobes             |
| ftrace            | Kernel function tracer (/sys/kernel/tracing)                        |
| perf              | Performance counters and sampling profiler                          |
| tracepoints       | Static instrumentation points in kernel code                        |
| kprobes / uprobes | Dynamic probes on kernel/userspace functions                        |
| BPF CO-RE         | Compile Once, Run Everywhere — portable eBPF across kernel versions |

## Virtualization & Containers

| Term           | Description                                                       |
| -------------- | ----------------------------------------------------------------- |
| KVM            | Kernel-based Virtual Machine — hardware-assisted virtualization   |
| namespaces     | Process isolation (pid, net, mount, user, uts, ipc, cgroup, time) |
| cgroups v2     | Resource accounting and limiting for containers                   |
| seccomp        | Syscall filtering for container sandboxing                        |
| overlayfs      | Union filesystem for container image layers                       |
| vhost / virtio | Paravirtualized I/O for VMs                                       |
| io_uring       | Used by modern VM backends for async I/O                          |
