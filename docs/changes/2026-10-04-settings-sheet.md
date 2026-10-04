# Settings as one sheet

Nightly `v1.5.2-nightly.7`. Owner, 2026-10-04: the settings bar "has become
too long and clunky … fix the UX and UI for settings as a whole. Also
Prepare can be moved to this bar. Rethink overall layout and structure,
segmentation, modern 2026 app UI level."

## Intent

- One place for every setting, grouped so each is easy to find, with what it
  does written under it (the popover kept that in hover tooltips).
- Prepare (build every preview of a folder) lives in Settings, with its
  progress.
- The three separate panels it opened (Excluded folders, Controller, About &
  updates) become pages of the same sheet.

## Decisions taken

| Question | Decision |
|---|---|
| Popover, sheet or own window? | A centred sheet over the library (940×660 max) with a sidebar, like System Settings. An OS window would need settings synced live across windows for no gain. ⌘, / Ctrl+, opens it, as in every Mac app. |
| Sections | Appearance · Playback · Speed & storage · Files & catalog · Controls · About & updates. Each is a page of cards; each row = name + one-line description + control. |
| On/off settings | Real switches instead of On/Off chip pairs. Choices stay segmented controls. |
| Theme | Four tiles that preview each theme's colours (drawn from its tokens, so a tile shows its theme whichever is active). |
| Prepare | Speed & storage → Previews: "Prepare this folder" with the open folder's name and count, a progress bar and Stop while it runs (the same `prepare` job as the folder menu, so the job centre still shows it), and the size of this drive's preview cache with Show. The folder menu entry stays ("Prepare this folder (build previews)"). |
| Sub-pages | Excluded folders (Files & catalog) and Game controller (Controls) open inside the sheet with a back link; Esc goes back, then closes. Excluded-folder rules are applied once, when the sheet closes, and only if they changed. |
| Mouse buttons | Moved out of the Controller panel into Controls (they aren't the controller). |
| About & updates | The shared `UpdatePanel` embedded as is (it's byte-identical across the four apps). The gear shows a dot when an update is available, and so does the sidebar entry. |
| Search | Filters every row of every section by name, description and synonyms ("dark", "sd card", "gamepad"); results are the live controls. ⌘F focuses it. |
| Narrow windows | Below 760 px the sidebar becomes a scrolling row of tabs above the content. |
| Keys while open | The sheet is modal: library shortcuts don't fire behind it. |

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/components/SettingsSheet.svelte` (new) | UX | The sheet: sidebar with search, six sections, row registry (`ROWS`) rendered as cards or as search results, switches, segmented controls, theme tiles, Prepare with progress/Stop, cache size, library location, mouse buttons, embedded sub-pages and update panel. |
| `src/routes/+page.svelte` | UX / architecture | The popover, the Controller/Excluded/About modals and their state are gone; `openSettings(page?)` / `closeSettings()` (re-applies exclusions if changed), `setFilmstripDock`, ⌘, , modal key guard, update dot on the gear, Welcome's "Excluded folders" opens the sheet there, folder-menu label "Prepare this folder (build previews)". Dead popover CSS removed. |
| `src/lib/components/ExcludePanel.svelte` | UX | Now a page inside the sheet: no backdrop/header/footer, Reset stays at the bottom. |
| `src/lib/components/ControllerPanel.svelte` | UX | Now a page inside the sheet: a status card with a "Use a controller" switch replaces the header; Esc while listening for a button cancels only that (capture phase); mouse buttons moved out. |
| `src-tauri/src/commands.rs`, `lib.rs` | logic | `cache_usage`: size and file count of the active drive's `_FoxCull/thumbs`, walked off the main thread. |
| `src/lib/api.ts` | architecture | `cacheUsage()`. |
| `src/lib/dev/mock-ipc.ts` | process | `cache_usage`; `warm_thumbnails` takes 180 ms per heavy chunk so Prepare's progress can be seen. |

## Behavior changes

- The gear opens the Settings sheet (it used to drop a popover). ⌘, too.
- Prepare is back in plain sight, in Settings → Speed & storage.
- Controller remapping and Excluded folders are reached through Settings.

## Risks / compat

- No settings were renamed or migrated; the store is untouched.
- The Shortcuts guide and the controller's button guide still say where
  things are; both were updated to "Settings → Controls → Game controller".
- Not run in the real app (WebKit); built and checked in the Chromium
  harness at 1440×900 and 720×620.

## Verification actually run

- `npm run check` 0/0, `npm run build`, `cargo test --lib` (50 pass).
- Harness: gear and ⌘, open the sheet with the search focused; every section
  rendered at 1440×900; search "sd card" finds Prepare; Prepare runs with a
  progress bar ("160 of 217") and a spinner on the sidebar entry, then shows
  "Previews ready for DCIM · 217 items"; cache size shows (mock 1.9 GB);
  Files → Excluded folders → Esc returns to Files; Controls → Game controller
  page with its status card; About shows the update panel full width; at
  720×620 the sidebar turns into tabs with no horizontal overflow.
