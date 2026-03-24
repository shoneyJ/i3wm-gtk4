# Translation Utility

A standalone GTK4 translation popup (`i3more-translate`) that replaces the existing EWW-based translator. Uses `trans` (translate-shell) CLI as the translation engine, reuses shared i3More modules (Font Awesome, i3 IPC), and follows the Gruvbox dark theme.

## Motivation

The previous EWW translator had bugs (long text not visible, always opens on primary monitor) and was a bash/EWW dependency the project aims to eliminate.

## Architecture

- **Separate binary**: `i3more-translate` (defined in `Cargo.toml` as `[[bin]]`)
- **Shared modules**: imports `i3more::fa`, `i3more::ipc`, `i3more::translate` via `src/lib.rs`
- **No new dependencies**: uses only existing crate dependencies

### Files

| File                    | Purpose                                                  |
| ----------------------- | -------------------------------------------------------- |
| `src/translate_main.rs` | GTK4 application, UI, signal wiring, window management   |
| `src/translate.rs`      | Translation backend — wraps `trans` CLI                  |
| `assets/translate.css`  | Gruvbox dark theme for the translator window             |
| `src/lib.rs`            | Re-exports `pub mod fa; pub mod ipc; pub mod translate;` |

## User Interface

### Layout (400x420 dialog)

```
+---------------------------------+
| [Source v]  <=>  [Target v]     |  Language selector row (DropDown + swap)
+---------------------------------+
|  Source text input               |  TextView in ScrolledWindow
|  (multiline, editable)          |  (~120px min height)
+---------------------------------+
|        [ Translate ]            |  Button (disabled during translation)
+---------------------------------+
|  Translation output              |  TextView in ScrolledWindow
|  (multiline, read-only)         |  (~120px min height, scrollable)
+---------------------------------+
|  [Copy]  [Speak]  [Clear]      |  Action buttons with FA icons
+---------------------------------+
```

### CSS architecture (`assets/translate.css`)

- `window` -- dark background (`#1d2021`)
- `.translate-main` -- main container, 8px padding
- `.lang-dropdown` -- dark input (`#282828`), gruvbox text (`#ebdbb2`), 1px border
- `.swap-button` -- transparent bg, muted text, hover brightens
- `.text-input` / `.text-output` -- dark input bg (`#282828`), 1px border, word-wrap
- `.translate-button` -- blue (`#4c7899`), hover brightens (`#5a8daa`), disabled grays out
- `.action-button` -- dark bg, muted text, hover brightens
- Scrollbar sliders -- dark gray (`#504945`), hover lightens

## Translation Backend (`src/translate.rs`)

### API

- `translate(text, source, target) -> Result<String, String>` -- shells out to `trans -brief -s {source} -t {target} -- {text}`
- `speak(text, lang)` -- fires `trans -speak -t {lang} -- {text}` in a background thread (fire-and-forget)
- `list_languages() -> Vec<String>` -- runs `trans -list-languages`, falls back to hardcoded list (30 languages)

### Off-thread execution

Translation runs off the GTK main thread to avoid UI freezes:

- `std::sync::mpsc::channel` sends the result from a spawned thread
- `glib::timeout_add_local` polls the receiver every 50ms and updates the output `TextView`
- The translate button is disabled during translation to prevent double-submits

## Language Selection

- `gtk4::DropDown` with `StringList` model and `set_enable_search(true)` for type-to-filter
- Language list populated from `trans -list-languages` on startup
- Hardcoded fallback list if `trans` is unavailable
- Defaults: English (source) -> German (target)
- Swap button (FA exchange icon): swaps selected languages **and** input/output text

## Action Buttons

| Button | FA Icon   | Behavior                                                                  |
| ------ | --------- | ------------------------------------------------------------------------- |
| Copy   | COPY      | `gdk::Display::default().clipboard().set_text()` -- native GTK4 clipboard |
| Speak  | VOLUME_UP | `trans -speak` in background thread                                       |
| Clear  | ERASER    | Reset both TextViews, refocus source input                                |

Keyboard shortcut: **Ctrl+Enter** triggers translate (via `EventControllerKey` on the window).

## Window Management

### Single-instance toggle

GTK4 `Application` with fixed `application_id` (`com.i3more.translate`) provides single-instance via D-Bus. Second invocation sends `activate` to the running instance, which toggles `window.set_visible(!window.is_visible())`.

### Monitor positioning

- Queries focused workspace output via `i3more::ipc::I3Connection` -> `get_workspaces()` -> find `focused: true` -> read `output` field
- Matches GDK monitor by connector name
- Computes centered position for 400x420 window on that monitor's geometry
- Uses `xdotool` (200ms delayed) as X11 fallback to move the window to the computed position

## Build & Deploy

```bash
killall i3more 2>/dev/null; docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more target/release/i3more-translate dist/"
```

Produces both binaries in `dist/`:

- `dist/i3more` (~7.2 MB)
- `dist/i3more-translate` (~3.5 MB)

## i3 Integration

Add to i3 config:

```
bindsym $mod+Shift+t exec i3more-translate
for_window [title="i3More-translate"] floating enable, border pixel 1, resize set 400 420
```

## Key Findings

- **`glib::MainContext::channel` not available in glib 0.21**: Used `std::sync::mpsc` + `glib::timeout_add_local` polling (50ms) as the async result pattern instead.
- **`gdk::Toplevel::set_size` not available in gtk4 0.10**: Window sizing handled by `default_width` / `default_height` on `ApplicationWindow` plus i3 `resize set` window rule.
- **CSS loaded via `include_str!`**: Same pattern as `navigator.rs` -- embedded at compile time, loaded via `CssProvider::load_from_data()`.
- **Font Awesome registration**: Reuses `i3more::fa::register_font()` from the shared library.
- **Window title `"i3More-translate"`**: Enables i3 `for_window` rules for floating/border/resize.

## Improvements

- The last selected source language and destination language should be persistant.
- When users copies a text and then opens the translator the copied text should be added in the language source. The persistant destination languages translated value should be already avaialable in the text.

## Build

We use docker to build the project

```bash
killall i3more-translate 2>/dev/null; docker compose run --rm dev bash -c "cargo build --release && cp target/release/i3more target/release/i3more-translate dist/i3More-translate"

```

## UI

- ~~currently user copies the text and then opens the translator app for the copied text to be translated. Along with it, if user only selects a text and opens the translator. The selected text should be translated.~~
- **Done**: `auto_paste_and_translate` now checks X11 primary selection (`display.primary_clipboard()`) first, then falls back to the regular clipboard. Selected text is auto-populated and translated on open.

### popup-translation

- When user selects a text on a window, then with a shortcut key, a popup opens right above where the selection ended.
  The popup should show a translated text.
- search gtk4 submodule for this feature implementation.
