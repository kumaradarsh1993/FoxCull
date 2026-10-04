# Redesign 2/3: the library

Nightly `v1.5.2-nightly.11`. Audit: `docs/UX-AUDIT-2026-10.md` (L1–L8, A2,
T1). Builds on the foundations of nightly.10.

## Intent

Cull without clutter: the toolbar for looking, the bottom bar for deciding,
the sidebar for going places, and what's filtered always visible.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/routes/+page.svelte` | UX | **Toolbar:** Reject, Clear and the red hold-to-Delete removed (L1). **Bottom bar:** Clear (icon + menu opening upward) after Pick/Reject; "Delete rejected" (hold, quiet until hovered) beside the ✓/✕ counts. **Filters:** active filters as removable chips with "N of M" (L3); opening a toolbar menu closes the others (L2); the menu's segmented rows match Settings, a 58 px column label no longer leaves a gap above the tag and event lists, the selected tag/event row is tinted, not filled. **Sidebar:** sections Drives, Pinned (folder menu: Pin to / Unpin from the sidebar), Review (Picks, Rejected, Not decided, Missing on this drive; each toggles a filter or opens the list), Events (each toggles the event filter) (L5). Open becomes an icon when the sidebar is narrow. **Tiles:** `data-badges` (L4), rejected pictures at 55 % and desaturated instead of 35 % (L7), tile radius 8. 28 px icon buttons (A2). |
| `src/lib/components/Thumb.svelte` | UX | A video tile waiting for its poster shimmers with a small play mark; the file type only shows when no poster can be made (L8). |
| `src/lib/components/SettingsSheet.svelte` | UX | Appearance → Layout → Marks on tiles: Minimal / Standard / Everything. |
| `src/lib/settings.svelte.ts` | logic | `tileBadges` (default standard), `pinned`. |
| `src/lib/dev/layout-audit.ts` | process | Clear menu probe follows the button. |

## Behavior changes

- Rejecting from the top toolbar is gone (P / X, the bottom bar, the menu
  and the controller remain). Clear and Delete rejected moved to the bottom
  bar.
- Standard tiles hide the segment (✂), event (✦) and tag marks until you
  point at the tile or select it.
- Clicking Picks / Rejected / Not decided / an event in the sidebar filters
  the grid; click again to clear. Folders can be pinned.

## Risks / compat

- People used to the toolbar's Reject button: the bottom bar has the same
  one, under the picture.
- `tileBadges: standard` is the default; Everything restores the old tiles.

## Verification actually run

- `npm run check` 0/0; `npm run build`.
- Harness: sidebar sections render with counts (Picks 29, Rejected 24, Not
  decided 164) and two events; Picks → chip "Picks ×" and "31 of 227";
  opening Arrange closes Filters; `__sweep` at 1440×900 and 1024×700:
  nothing new (the Open/Grid overlap it found at 1024 after the Mac padding
  was fixed); remaining findings are the two pre-existing ones for part 3.
