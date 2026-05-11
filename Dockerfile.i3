FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Build tooling + every i3 build dep from vendor/i3/DEPENDS.
# Ubuntu 24.04 (Noble) package names.
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential \
      meson \
      ninja-build \
      pkg-config \
      ca-certificates \
      git \
      # XCB / X11
      libxcb1-dev \
      libxcb-util-dev \
      libxcb-cursor-dev \
      libxcb-icccm4-dev \
      libxcb-keysyms1-dev \
      libxcb-randr0-dev \
      libxcb-shape0-dev \
      libxcb-xkb-dev \
      libxcb-xrm-dev \
      libxcb-xinerama0-dev \
      libxkbcommon-dev \
      libxkbcommon-x11-dev \
      libxcb-ewmh-dev \
      # Event loop + JSON + regex
      libev-dev \
      libyajl-dev \
      libpcre2-dev \
      # Startup notification
      libstartup-notification0-dev \
      # Text + graphics (i3 title rendering)
      libpango1.0-dev \
      libcairo2-dev \
    && rm -rf /var/lib/apt/lists/* \
    && ldconfig

WORKDIR /src
CMD ["bash"]
