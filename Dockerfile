FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Rust toolchain
RUN apt-get update && apt-get install -y --no-install-recommends \
  curl ca-certificates \
  && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
  && rm -rf /var/lib/apt/lists/*

ENV PATH="/root/.cargo/bin:${PATH}"

# GTK4 and build dependencies — matches Ubuntu 24.04 (Noble) packages exactly
RUN apt-get update && apt-get install -y --no-install-recommends \
  # GTK4 + GLib
  libgtk-4-dev \
  libglib2.0-dev \
  libgraphene-1.0-dev \
  libcairo2-dev \
  libpango1.0-dev \
  libgdk-pixbuf-2.0-dev \
  # Build tooling
  pkg-config \
  build-essential \
  # Icon theme resolution
  adwaita-icon-theme \
  hicolor-icon-theme \
  # D-Bus (for zbus/system tray)
  libdbus-1-dev \
  # X11
  libx11-dev \
  libxcb1-dev \
  # PAM (for i3more-lock authentication)
  libpam0g-dev \
  # libclang (for bindgen used by pam-sys)
  libclang-dev \
  # Testing: virtual X11 server and input simulation
  xvfb \
  xdotool \
  xauth \
  # Debugging: LLDB + GDB for container debug sessions
  lldb \
  gdb \
  # Tracing: strace for syscall inspection (epoll, socket, mount, clone)
  strace \
  && rm -rf /var/lib/apt/lists/*

# Symlink versioned LLDB binaries so CodeLLDB extension finds them
RUN ln -sf /usr/bin/lldb-server-18 /usr/bin/lldb-server \
 && ln -sf /usr/bin/lldb-18 /usr/bin/lldb \
 && ln -sf /usr/lib/x86_64-linux-gnu/liblldb-18.so /usr/lib/liblldb.so

# Test PAM service: always-accept auth (used by integration tests only)
RUN echo "auth    required  pam_permit.so" > /etc/pam.d/i3more-lock-test \
 && echo "account required  pam_permit.so" >> /etc/pam.d/i3more-lock-test

WORKDIR /app

CMD ["bash"]
