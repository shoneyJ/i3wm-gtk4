# App Search

## Current Approach

rofi is used to open a popup to search and open the application.

- shortcut key is mod+space

## gtk4 native application

- a stand alone executable `i3more-launcher` which replaces rofi and bash script.

## Reference files

```bash
ls ~/dotfiles/i3/.config/i3/
ls ~/dotfiles/rofi/.config/rofi/
```

## Architecture

```
src/launcher.rs        — Library module: .desktop parsing, search/filter, exec handling
src/launcher_main.rs   — Binary: GTK4 UI (SearchEntry + ListBox)
assets/launcher.css    — Gruvbox dark theme styling
src/icon.rs            — Shared: desktop file discovery, icon resolution (moved to lib.rs)
```

**Binary pattern:** Same as `i3more-translate` — GTK4 Application with D-Bus single-instance toggle.

**Search:** Case-insensitive substring match on Name + GenericName + Keywords + Categories.
Ranked: name-starts-with > name-contains > keyword/category match. Capped at 50 results.

**Launch:** Strips Exec field codes (%U, %F, etc.), spawns via `sh -c`. Terminal=true apps
wrapped with `$TERMINAL` / `x-terminal-emulator` / `xterm`.

---

## Implementation Phases

### Phase 1: Move icon.rs to shared library ✅

**Goal:** Make `IconResolver`, `IconResult`, `find_desktop_files()`, and `resolve_icon_in_theme()`
available to all binaries via `i3more::icon`.

**Files modified:**

- `src/lib.rs` — added `pub mod icon;`
- `src/icon.rs` — made `DesktopEntry`, `find_desktop_files()`, `build_desktop_index()`,
  `parse_desktop_entry()`, `resolve_icon_in_theme()` public
- `src/main.rs` — removed `mod icon;`, use `i3more::icon::IconResolver`
- `src/navigator.rs` — changed `crate::icon` → `i3more::icon`

---

### Phase 2: Create launcher library module ✅

**Goal:** Desktop entry parsing, search/filtering, and app launching logic.

**New file:** `src/launcher.rs`

**Key types:** `LauncherEntry` (name, generic_name, exec, icon, terminal, search_haystack)

**Key functions:**

- `load_entries()` — scans .desktop files, parses Name/Exec/Icon/Keywords/Categories/Terminal/NoDisplay,
  resolves icons, sorts by name, deduplicates
- `filter_entries(entries, query)` — substring match ranked by relevance, capped at 50
- `launch(entry)` — strips field codes, spawns process, handles Terminal=true

**Files modified:**

- `src/lib.rs` — added `pub mod launcher;`

---

### Phase 3: Create launcher binary + CSS ✅

**Goal:** GTK4 search dialog — SearchEntry + scrollable ListBox with icons.

**New files:**

- `src/launcher_main.rs` — binary entrypoint
- `assets/launcher.css` — Gruvbox styling

**UI structure:**

- SearchEntry (auto-focused, placeholder "Search applications...")
- ScrolledWindow → ListBox (rows: 32x32 icon + name + generic name)
- Keyboard: Escape hides, Down arrow moves to list, Enter launches

**Features:**

- Single-instance toggle via GTK Application D-Bus activation
- Debounced search (50ms)
- Centered on focused monitor via i3 IPC + xdotool
- Re-run clears search and re-focuses

**Files modified:**

- `Cargo.toml` — added `[[bin]] name = "i3more-launcher"`

---

### Phase 4: i3 config integration ✅

**Goal:** Wire the launcher to the i3 keybinding.

**Files modified:**

- `~/dotfiles/i3/.config/i3/config` — replaced rofi bindings (`$mod+space`, `$mod+d`) with
  `i3more-launcher`, added floating window rule

---

## Features Checklist

| Feature                                 | Phase | Status |
| --------------------------------------- | ----- | ------ |
| .desktop file scanning and parsing      | 2     | ✅     |
| Icon resolution (theme + file path)     | 1     | ✅     |
| Searchable app list                     | 3     | ✅     |
| Ranked search results                   | 2     | ✅     |
| App launching (Exec field codes)        | 2     | ✅     |
| Terminal=true app support               | 2     | ✅     |
| NoDisplay=true filtering                | 2     | ✅     |
| Single-instance toggle                  | 3     | ✅     |
| Gruvbox CSS theme                       | 3     | ✅     |
| Centered on focused monitor             | 3     | ✅     |
| Keyboard navigation (Escape/Down/Enter) | 3     | ✅     |
| Debounced search                        | 3     | ✅     |
| i3 keybinding integration               | 4     | ✅     |

## BUG

### Bug 1: Enter key from SearchEntry does not launch app

**Symptom:** User types a query, presses Enter — nothing happens. Clicking a row works.

**Root cause:** `launcher_main.rs` line 141-166 only handles Escape and Down keys.
The `ListBox::row_activated` signal (line 113) only fires when a **row** has focus.
When the SearchEntry has focus, Enter emits `SearchEntry::activate` — but no handler is connected.

**Fix:** Connect `search_entry.connect_activate()` to launch the currently selected row:

```rust
// In on_activate(), after the row_activated handler:
let entries_for_activate = entries.clone();
let listbox_for_activate = listbox.clone();
search_entry.connect_activate(move |_| {
    if let Some(row) = listbox_for_activate.selected_row() {
        let idx_str = row.widget_name();
        if let Ok(idx) = idx_str.parse::<usize>() {
            if let Some(e) = entries_for_activate.get(idx) {
                i3more::launcher::launch(e);
            }
        }
    }
    std::process::exit(0);
});
```

**File:** `src/launcher_main.rs` — add `connect_activate` after the `connect_row_activated` block (line 127).

---

### Bug 2: Launcher process stays alive after launching an app

**Symptom:** After an app is opened (via click), the launcher window hides but the process remains.

**Root cause:** `launcher_main.rs` line 121-125 calls `window.set_visible(false)` which hides the window
but keeps the GTK Application running as a D-Bus service.

**Fix:** Replace `set_visible(false)` with `std::process::exit(0)` in the `row_activated` handler:

```rust
// Replace the row_activated handler (lines 111-127):
listbox.connect_row_activated(move |_, row| {
    let idx_str = row.widget_name();
    if let Ok(idx) = idx_str.parse::<usize>() {
        if let Some(entry) = entries_ref.get(idx) {
            i3more::launcher::launch(entry);
        }
    }
    std::process::exit(0);
});
```

**File:** `src/launcher_main.rs` — modify `connect_row_activated` block (lines 111-127).
