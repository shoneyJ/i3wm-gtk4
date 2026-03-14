# Setup Guide — Ubuntu Server + i3wm

This guide walks through installing all dependencies needed to run i3More on a fresh Ubuntu Server installation with i3 window manager.

## Prerequisites

- **Ubuntu Server 24.04 LTS** (matches the Docker build image)
- **X11 display server** — Wayland is not supported (i3More uses `gdk4_x11`)
- **D-Bus session bus** — standard on Ubuntu, required for system tray

## 1. Install Xorg + i3wm

Ubuntu Server ships without a display server. Install Xorg and i3:

```bash
sudo apt-get install -y xorg i3 xinit x11-xserver-utils
```

After installation, start X with `startx` or configure auto-login to launch i3.

## 2. GTK4 & Graphics Runtime Libraries

i3More is built with GTK4. Install the runtime libraries:

```bash
sudo apt-get install -y \
  libgtk-4-1 \
  libglib2.0-0 \
  libgraphene-1.0-0 \
  libcairo2 \
  libpango1.0-0 \
  libgdk-pixbuf-2.0-0 \
  libdbus-1-3 \
  libx11-6 \
  libxcb1
```

## 3. Icon Themes

Required for system tray and app launcher icon resolution:

```bash
sudo apt-get install -y adwaita-icon-theme hicolor-icon-theme
```

## 4. X11 Utilities

Used for window positioning and type hints by `i3more`, `i3more-launcher`, and `i3more-translate`:

- **`x11-utils`** — provides `xprop` (sets `_NET_WM_WINDOW_TYPE_DOCK`, `_NET_WM_STRUT_PARTIAL`)
- **`xdotool`** — window resize, move, and search

```bash
sudo apt-get install -y x11-utils xdotool
```

## 5. Audio (for `i3more-audio`)

- **`pulseaudio-utils`** — provides `pactl` (volume control, device switching)
- **`libnotify-bin`** — provides `notify-send` (audio change notifications)
- An audio daemon: **PulseAudio** or **PipeWire** with `pipewire-pulse`

```bash
sudo apt-get install -y pulseaudio-utils libnotify-bin
```

If using PipeWire instead of PulseAudio:

```bash
sudo apt-get install -y pipewire-pulse
```

## 6. Translation (for `i3more-translate`)

The translation feature uses [translate-shell](https://github.com/soimort/translate-shell):

```bash
sudo apt-get install -y translate-shell
```

This provides the `trans` command used for text translation and text-to-speech.

## 7. i3 Configuration

Add `tray_output none` to your i3 config's `bar {}` block. This prevents i3bar from claiming the system tray, which would conflict with i3More's tray watcher.

Edit `~/.config/i3/config`:

```
bar {
    tray_output none
    status_command i3status
}
```

Then reload i3 with `$mod+Shift+r`.

## 8. Combined Install Command

All packages in a single command:

```bash
sudo apt-get install -y \
  xorg i3 xinit x11-xserver-utils \
  libgtk-4-1 libglib2.0-0 libgraphene-1.0-0 libcairo2 \
  libpango1.0-0 libgdk-pixbuf-2.0-0 libdbus-1-3 libx11-6 libxcb1 \
  adwaita-icon-theme hicolor-icon-theme \
  x11-utils xdotool \
  pulseaudio-utils libnotify-bin \
  translate-shell
```

## 9. Verify Installation

```bash
# Check that key tools are available
which i3 xprop xdotool pactl notify-send trans

# Run i3more (from within an i3 session)
./i3more
```

If any command is missing, re-run the install for the relevant section above.
