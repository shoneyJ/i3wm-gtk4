## Having system info in the i3More bar

## Static infos

- Add system battery status.
- Current date and time.

## On Hover widget

- When user hovers on system icon and popup drop down shows the
  -- Current cpu usage
  -- current Temprature
  -- current ram usage

The information pooling should only happen as long as the pop is open after hovering onto it.

## Key findings

- **Thermal zone selection**: `/sys/class/thermal/thermal_zone0` (`INT3400 Thermal`) reports a
  fixed virtual temperature (~20 °C) on many laptops. The reader now scans all zones and prefers
  `x86_pkg_temp`, `TCPU`, `TCPU_PCI`, or `coretemp` for an accurate CPU temperature.
- **Popup styling**: The popover background uses the same `#1d2021` dark background as the bar.
  Inside the popup, keys (CPU, Temp, RAM) are shown in a muted gruvbox color (`#a89984`) and
  values in bright foreground (`#ebdbb2`) via Pango markup for clear visual separation.
- **GTK `connect_notify` requires `Send + Sync`**: The popover visibility handler uses
  `connect_notify_local` instead, since all state (`Rc<RefCell>`, GTK labels) is main-thread only.
- **Battery graceful degradation**: On desktops without a battery, the battery label is simply
  not added to the bar — no error or empty slot.
- **Font Awesome 6 Free Solid** bundled in `assets/fonts/`, loaded via fontconfig FFI at startup.
- **Popup uses FA icons** (microchip, temperature, memory) instead of text keys for CPU/Temp/RAM.
- **Bar battery uses tiered FA battery glyphs** based on capacity level (full/¾/half/¼/empty/bolt).
- **Stats trigger uses FA gauge glyph** instead of GTK theme icon (`utilities-system-monitor`).
