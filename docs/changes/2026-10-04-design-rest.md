# Redesign 3/3: Focus, Details, menus, Merge, Reel

Nightly `v1.5.2-nightly.12`, the last of the three redesign nightlies.
Audit: `docs/UX-AUDIT-2026-10.md` (L10–L13, T2, T3).

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/components/DetailsView.svelte` | UX | The name column takes the spare width (`minmax(…, 1fr)`); FPS and Codec off by default (Columns menu); slimmer defaults, so Size and Date fit at 1440 px (L10). |
| `src/lib/components/Loupe.svelte` | UX | The hidden video transport fades in place instead of sliding 14 px below the stage (the long-standing "Focus overflows" audit finding); a calmer 58 px play button; the Focus surround follows Behind the pictures when it's black, grey or light (L11). |
| `src/routes/+page.svelte` | UX | Tab in Focus hides/shows the sidebar (in the shortcut guide); the tile menu leads with Open, Pick/Reject, Edit/Merge/Reel, and ends with Previous/Next (L12); "Video · 1 of 227" instead of upper case. |
| `src/lib/components/ContextMenu.svelte` | UX | Drops leading, trailing and doubled dividers. |
| `src/lib/components/ActivityBar.svelte` | UX | Housekeeping jobs (thumbnails) appear only after a second of running (L13); sentence-case section heads. |
| `src/lib/components/ReelWindow.svelte` | UX | A stepper header (numbered dots joined by a line); the waveform 140–220 px tall instead of 120 (T3). |
| `MergeDialog`, `ReelBoard`, `EditStudio` (export dialog), `ControllerPanel`, `DetailsView`, `+page` | UX | Every upper-case micro-label in sentence case, as in Settings and Edit (T2). |

## Behavior changes

- Tab in Focus toggles the sidebar (elsewhere Tab still moves keyboard
  focus).
- Right-click menus are in a new order.

## Risks / compat

- Details at 1024 px still scrolls sideways to reach Camera (a table
  scrolling in its own frame, by design).

## Verification actually run

- `npm run check` 0/0; `npm run build`.
- `__sweep` at 1440×900: no findings at all (the two pre-existing ones are
  gone). At 1024×700: only the Details sideways scroll.
- Harness: Reel stepper and taller waveform.
