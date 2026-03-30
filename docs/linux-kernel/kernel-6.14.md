# Kernel 6.14 Specific Features

| Term                  | Description                                                      |
| --------------------- | ---------------------------------------------------------------- |
| EEVDF scheduler       | Replaced CFS as the default scheduler (stabilized in 6.6+)       |
| io_uring improvements | Continued zero-copy and fixed-buffer optimizations               |
| Landlock v4           | Enhanced unprivileged sandboxing with network rules              |
| BPF token             | Delegated eBPF program loading for unprivileged users            |
| NTSYNC                | Windows NT synchronization primitive emulation (for Wine/Proton) |
| bcachefs              | New copy-on-write filesystem (experimental, merged 6.7+)         |
| Rust in-kernel        | Rust language support for kernel modules (ongoing since 6.1)     |
