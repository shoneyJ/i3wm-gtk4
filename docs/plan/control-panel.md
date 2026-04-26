# Plan: Dedicated Android-Style Control Panel

## Context

Control widgets (Volume, Backlight, Background) previously lived inside the notification panel (`src/notify/panel.rs`). This plan extracts them into a dedicated, Android-style control panel that:

- Opens from a new sliders icon in the i3More bar (not the bell)
- Floats over windows (non-tiling overlay)
- Groups widgets with section headers and icons
- Places frequently-used controls at the top

## Phases

### Phase 1: Control Panel Window (Empty Shell)

Created `src/control_panel/mod.rs` and `src/control_panel/panel.rs` with a `ControlPanel` struct mirroring `NotificationPanel`:

- `new(app)`, `toggle()`, `is_visible()`, `hide()`
- 400x520px floating window, title `"i3more-control-panel"`
- Same i3-msg positioning pattern (150ms delay after present)
- Added `mod control_panel;` to `src/main.rs`

**Files**: `src/control_panel/mod.rs`, `src/control_panel/panel.rs`, `src/main.rs`.

### Phase 2: Toggle Icon in Bar

Added a SLIDERS icon (`\u{f1de}`) to the bar between tray icons and bell:

- Added `SLIDERS` constant to `src/fa.rs`
- Added clickable label + overlay in `src/navigator.rs` `right_box` (between `tray_box` and `bell_overlay`)
- Returns `ControlPanelHandles` from `build_navigator()`
- Click handler in `src/main.rs` calls `control_panel.toggle()`
- `.control-panel-icon` CSS in `assets/control-panel.css`

**Files**: `src/fa.rs`, `src/navigator.rs`, `src/main.rs`, `assets/control-panel.css`.

### Phase 3: Move Widgets to Control Panel

Relocated widget files from notification to control panel:

- Moved `src/notify/widgets/{volume,backlight,background}.rs` to `src/control_panel/widgets/`
- Created `src/control_panel/widgets/mod.rs`
- Removed `pub mod widgets;` from `src/notify/mod.rs`
- Removed widget_box + separator from `src/notify/panel.rs`
- Added widget building to `src/control_panel/panel.rs`
- Reduced notification `PANEL_HEIGHT` from 500 to 400
- Added `hide()` method to `NotificationPanel`

**Files**: `src/control_panel/widgets/`, `src/notify/mod.rs`, `src/notify/panel.rs`, `src/control_panel/panel.rs`.

### Phase 4: Layout and Grouping

Organized widgets into Android-style sections:

```
+-----------------------------------+
|  Control Panel              [x]   |
+-----------------------------------+
|  [speaker] Audio                  |
|  [mute] ========o======= 75%     |
+-----------------------------------+
|  [sun] Display                    |
|  [sun] ======o=========== 60%    |
+-----------------------------------+
|  [image] Background               |
|  [preview]  [Folder] Mode:[fill]  |
|  [thumb grid]                     |
+-----------------------------------+
```

Created `build_section(title, icon, content) -> gtk4::Box` helper:

- `.cp-section` container with `.cp-section-header` row (FA icon + bold title)
- Content widget appended below header
- Sections ordered: Audio (top), Display (if available), Background

Close button (X) in panel header.

**Files**: `src/control_panel/panel.rs`.

### Phase 5: CSS Styling

Created `assets/control-panel.css` with:

- Panel: `.control-panel` (bg `#1d2021`, border `#3c3836`)
- Header: `.control-panel-header`, `.control-panel-title`
- Sections: `.cp-section` (bg `#282828`, rounded corners 6px, card-like), `.cp-section-header`
- Moved all `.widget-*` classes from `assets/notification.css` to `assets/control-panel.css`
- Loaded via `i3more::css::load_css("control-panel.css", ...)` in `ControlPanel::new()`

**Files**: `assets/control-panel.css`, `assets/notification.css`, `src/control_panel/panel.rs`.

### Phase 6: Mutual Exclusion and Polish

- Opening control panel hides notification panel (and vice versa)
- Bell click handler: hides CP before toggling notification panel
- CP click handler: hides NP before toggling control panel
- Auto-hide timer: `connect_notify_local(Some("is-active"))` pattern (5s timeout on focus loss)
- Close button in header

**Files**: `src/main.rs`, `src/control_panel/panel.rs`.

## File Summary

| Action     | File                                                                            |
| ---------- | ------------------------------------------------------------------------------- |
| **Create** | `src/control_panel/mod.rs`                                                      |
| **Create** | `src/control_panel/panel.rs`                                                    |
| **Create** | `src/control_panel/widgets/mod.rs`                                              |
| **Create** | `assets/control-panel.css`                                                      |
| **Create** | `docs/plan/control-panel.md`                                                    |
| **Move**   | `src/notify/widgets/volume.rs` -> `src/control_panel/widgets/volume.rs`         |
| **Move**   | `src/notify/widgets/backlight.rs` -> `src/control_panel/widgets/backlight.rs`   |
| **Move**   | `src/notify/widgets/background.rs` -> `src/control_panel/widgets/background.rs` |
| **Delete** | `src/notify/widgets/` (entire directory)                                        |
| **Modify** | `src/main.rs`, `src/navigator.rs`, `src/fa.rs`                                  |
| **Modify** | `src/notify/mod.rs`, `src/notify/panel.rs`                                      |
| **Modify** | `assets/notification.css`                                                       |

## speech-text — control panel integration

The standalone speech-to-text feature is being built in `docs/plan/speech-text.md` (Phases 0–8). This section captures only what the **control panel** needs to surface, plus two new follow-up requirements that don't yet exist in the speech-text plan.

### Status (as of 2026-04-26)

| What                                          | Where                                       | Status                                                                              |
| --------------------------------------------- | ------------------------------------------- | ----------------------------------------------------------------------------------- |
| `dist/i3more-speech-text` binary              | speech-text Phase 2 + S2 + S3 + 5 + 7-prime | **Done.** GPU @ ~85× RT, sliding window, VAD, inline German+English, parec default + auto-follow default sink. |
| Save German text to a file                    | speech-text Phase 6.5                       | **Done.** `~/.local/share/i3more/stt/<date>/<session>.md` with front-matter, German + English pairs. |
| Show German + English live on screen          | speech-text Phase 5                         | **Done in CLI.** Widget can also tail the .md file or subscribe to S5 socket — see Pending below. |
| Toggle from control panel                     | `src/control_panel/widgets/speech_text.rs`  | **Done.** Start/Stop button, pgrep-detect, SIGTERM stop, periodic 2 s state poll.   |
| Name a session ("pre-refinement", "stand up") | widget + speech-text Phase 6.5              | **Done.** `GtkEntry` in widget → sanitised → `I3MORE_STT_SESSION` env on spawn.    |
| Claude CLI post-process transcript → markdown | widget                                      | **Done (mechanism).** "Summarise with Claude" button shells out `claude -p $PROMPT --allowedTools 'Read,Write'`; greyed while running; status label shows result path. |
| Live in-widget transcript view                | widget — pending                            | Not yet — needs file-tail (simple) or S5 broadcast subscriber (clean). Currently only the running `i3more-speech-text` stdout / on-disk .md show transcripts. |

Run command (CLI form, used by the toggle until Phase 3 GTK lands):

```bash
~/projects/github/shoneyj/i3More/dist/i3more-speech-text
```

### Control-panel widget

Built in v1 (Apr 2026); `build_widget()` in `src/control_panel/widgets/speech_text.rs` returns a `gtk4::Box` plugged into `panel.rs` between the Background and Speech-to-Text sections. Mirrors the existing audio / brightness widget pattern:

- **Display.** A row labelled "Speech-to-Text (DE)" with a toggle switch on the right. When active, also show: current session name (or "Untitled"), elapsed time, segment count.
- **Toggle ON behaviour.** Spawn `i3more-speech-text` as a detached child (or send a D-Bus `activate` to it once Phase 3 wires single-instance D-Bus). The control panel must NOT load whisper itself — it stays a thin GTK process.
- **Toggle OFF behaviour.** Send `SIGTERM` to the running `i3more-speech-text` process. Worker honours the existing `SHUTDOWN` flag and reaps `parec` cleanly.
- **Discovery.** `pgrep -fx 'i3more-speech-text'` to detect "running" state for the switch's initial position. Re-check on the same `pactl subscribe` event loop the volume widget already uses (cheap, already polling).

### New requirement A — name the session

User wants to label each session ("pre-refinement meeting", "daily standup") so transcripts on disk are findable later.

- **Where the name comes from.** A small `GtkEntry` in the control-panel widget, *next to* the toggle. Filled in BEFORE pressing the toggle. If empty when toggling on, default to `untitled-<YYYY-MM-DD>-<HH-MM>`.
- **How it gets to the transcript.** Pass via env var `I3MORE_STT_SESSION` (or a CLI arg `--session=<name>`) read once on startup in `src/speech_text.rs`. The save path becomes `~/.local/share/i3more/stt/<YYYY-MM-DD>/<session-name>.md` (one file per session, not one shared file per day).
- **Editable mid-session?** No (v1). To rename, stop and restart with the new name. Avoids merge edge cases.

### New requirement B — Claude CLI post-process

User wants to run Claude CLI over the saved transcripts to produce a properly contextualised markdown summary.

- **Trigger.** Manual: a button "Summarise with Claude" on the control-panel widget that fires once per stopped session (greyed out while a session is active).
- **Invocation.** `claude` CLI is already on the user's PATH (this is a Claude Code workstation). The button shells out:
  ```bash
  claude -p "Read the German + English transcript at $TRANSCRIPT and produce a structured markdown summary with: meeting title, date, key decisions, action items (with owners if mentioned), open questions. Save the result to ${TRANSCRIPT%.md}-summary.md. Reply with the file path." \
    --allowedTools 'Read,Write'
  ```
- **Where the output lands.** Sibling file `<session-name>-summary.md` next to the raw transcript. Visible in the control-panel widget after completion.
- **Out of scope (v1).** Live in-flight summaries; cross-session aggregation; non-Claude LLMs.

### Cross-link

Both new requirements (session naming + Claude post-process) need a new phase in `docs/plan/speech-text.md`. Suggested placement: insert between current Phases 6 and 7 as **Phase 6.5 — Session metadata + post-process hook**, since they touch the persistence layer that Phase 6 establishes.
