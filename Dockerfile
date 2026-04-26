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
  # PipeWire client lib (used by the speech-text capture module's
  # opt-in pipewire backend; required at link time even when only
  # the parec backend is selected at runtime).
  libpipewire-0.3-dev \
  # libclang (for bindgen used by pam-sys). The meta `clang` package
  # installs the unversioned symlinks that the bare libclang-dev package
  # misses; clang-sys' dlopen needs them.
  clang \
  libclang-dev \
  # Testing: virtual X11 server and input simulation
  xvfb \
  xdotool \
  xauth \
  && rm -rf /var/lib/apt/lists/* \
  && ldconfig

# clang-sys (used by pam-sys / bindgen) doesn't auto-find libclang on
# Ubuntu 24.04 without LIBCLANG_PATH set; also add the dir to
# LD_LIBRARY_PATH so dlopen("libclang.so") resolves at runtime.
ENV LIBCLANG_PATH=/usr/lib/llvm-18/lib
ENV LD_LIBRARY_PATH=/usr/lib/llvm-18/lib

# Test PAM service: always-accept auth (used by integration tests only)
RUN echo "auth    required  pam_permit.so" > /etc/pam.d/i3more-lock-test \
 && echo "account required  pam_permit.so" >> /etc/pam.d/i3more-lock-test

WORKDIR /app

CMD ["bash"]
