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
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app

CMD ["bash"]
