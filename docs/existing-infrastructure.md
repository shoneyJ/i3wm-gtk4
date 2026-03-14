# Existing Infrastructure

The dotfiles repo already has several building blocks relevant to i3More:

| Asset                   | Path                                                             | Status                                                                    |
| ----------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------- |
| App icon resolver       | `eww/.config/eww/scripts/resolve-app-icon.py`                    | Working — resolves WM_CLASS to icon file path with caching and batch mode |
| Workspace data provider | `eww/.config/eww/scripts/get-workspaces.sh`                      | Working — outputs JSON with workspace info and app icon paths             |
| Font Awesome icon map   | `i3/.config/i3/app-icons.json`                                   | 115 app-class-to-icon-name mappings (used by `i3-workspace-names-daemon`) |
| Auto-renumber script    | `i3/.config/i3/scripts/auto-renumber-workspaces.sh`              | Working — keeps workspace numbers sequential on close                     |
| EWW bar + widgets       | `eww/.config/eww/`                                               | **Disabled** — causes high CPU usage (see Known Constraints)              |
| QuickShell Workspaces   | `quickshell/.config/quickshell/ii/modules/ii/bar/Workspaces.qml` | Reference only — built for Hyprland, not i3                               |
