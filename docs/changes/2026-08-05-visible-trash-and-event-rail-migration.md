# The visible Trash, and why the event banner never appeared

## Intent

The owner installed `v1.5.0-nightly.4` and reported that neither of the two
things he expected was there: the Trash was still a popup, and events still drew
the old cover-art blocks. Both are on me — one was a real bug, one was a bad
scoping call.

## 1. The event banner never appeared — a settings trap I built

**Not a rendering bug.** Read back from his machine:

```json
"groupBy": "event",
"sortBy": "name",
"eventRail": true
```

Two things followed from that, both "working as designed":

- `groupBy: "event"` still selected the album-block view, so he saw cover art.
- `eventRailPossible` requires a time axis (capture/modified sort, or a date
  grouping). Sorted by name with `groupBy: "event"`, it was false — the banner
  was switched off and nothing on screen said why.

The root cause is a decision from the previous session: he asked for the
lightweight banner *instead of* the blocks, and I kept both "for when you want
the album read". That was optionality he had not asked for, and it silently
pinned an upgrading user to the old mode.

**Fix — the grouping mode is gone.**

- `"event"` removed from `GroupBy`; the Group/Subgroup selects lose the option;
  `SectionCover.svelte`, the cover band, `eventStats`, `eventRank`,
  `eventDateRange`, `eventCovers` and `eventOrder` all deleted.
- **A migration, because removing an enum value is not enough.** A stored
  `groupBy: "event"` proves the user was trying to look at events, so it maps to
  `groupBy: "none"` **and** `sortBy: "capture"` — the state where the replacement
  is actually visible. Mapping only the grouping would have dropped him on a
  plain grid with the new feature still silently off: the same dead end.
- When the sort makes the banner impossible, the Arrange row now says so with a
  one-click *"Sort by capture date to use it"*.
- Creating an event turns the banner on and switches to a capture sort if needed.
  Creating one and seeing nothing change is the exact failure being fixed.

## 2. The Trash is now a real folder

I had held this pending his review of the ~19 GB of orphans. That conflated two
different things: **not deleting his files** (correct) with **not changing the
mechanism** (wrong — he had asked for it explicitly). Built now.

- Storage moves from the hidden `<drive>\_FoxCull\recycle` to a visible
  **`<drive>\FoxCull Trash`** at the drive root.
- **Flat.** A mirrored twenty-deep tree is hostile to a folder you actually
  browse. Collisions uniquify as before.
- It appears in the folder tree like anything else, so opening it gives the
  **normal grid** — thumbnails, size, sorting, Focus, playback. That answers "I
  couldn't see the size or play the clip to remember why I deleted it" with no
  new UI at all.
- It is skipped when walking a *parent* folder, so trashed files never reappear
  in the library; browsing into it directly still works, because the walk starts
  at the folder you opened.
- **The modal is deleted** (`TrashPanel.svelte` removed). Restore and Delete
  permanently are right-click actions inside the folder; the info bar shows
  `🗑 from <original path>`; the menu shows `Came from: …`.
- **Provenance is doubled.** `_trash-index.json` beside the files mirrors the
  catalog's `stored → orig` mapping and is rewritten on every dispose, restore,
  purge and adoption. This is the direct fix for the orphan class: a flat folder
  loses the positional information the mirrored tree used to encode, so the
  sidecar carries it instead — and unlike the catalog it travels with the files.
- **Migration is automatic and instant.** `migrate_recycle` renames each file
  (same volume, so an 18 GB clip is metadata-only), re-keys the catalog rows to
  the new flat names, and **adopts orphans on the way** using their old mirrored
  position as their origin. Nothing is deleted.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src-tauri/src/commands.rs` | architecture | `TRASH_DIRNAME`, sidecar read/write, `migrate_recycle`, flat `move_into_recycle`, trash excluded from parent walks, sidecar-aware orphan adoption. |
| `src/lib/settings.svelte.ts` | logic | `"event"` off `GroupBy`; `eventOrder`/`eventCovers` gone; migration of a stored event grouping. |
| `src/routes/+page.svelte` | UX | Cover-band path removed; Trash-folder mode (provenance rows, Restore/Purge menu, origin in the info bar); panel wiring removed. |
| `src/lib/components/SectionedGrid.svelte` | UX | Cover-band header removed; back to plain section headers. |
| `SectionCover.svelte`, `TrashPanel.svelte` | — | Deleted. |

## Risks / compatibility

- **The migration moves real files.** It is rename-only within one volume and
  never deletes; a rename that fails leaves the file where it is and simply
  doesn't re-key that row. The old `recycle` directory is removed only after the
  loop.
- Anyone who liked the album-block view loses it. That is the owner's explicit
  call ("rather than building on that separate whole thing for events let's keep
  it light"), recorded here so it isn't quietly reverted.
- The Trash folder is now visible in Explorer too. That is intended — but it does
  mean a user can move or delete its contents behind FoxCull's back. The orphan
  adoption path already covers files appearing; files vanishing are reaped by the
  existing stale-row sweep in `list_trash`.

## Verification actually run

- `cargo check`: passed, 0 warnings. `npm run check`: 0/0. `npm run build`: passed.
- The event-banner diagnosis came from **reading his actual settings file**, not
  from reasoning about the UI — `groupBy: "event"` + `sortBy: "name"` explains
  both symptoms exactly.
- **Device QA pending, and this one genuinely needs it:** the migration relocates
  ~19 GB across E: and P: on first launch per drive. It is rename-only, but it
  has not been exercised against those volumes.
