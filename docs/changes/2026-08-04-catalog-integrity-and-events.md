# Catalog integrity (Lightroom-style relink) + Events

## Intent

Two owner-requested features, both about making metadata survive the real world.

1. **Moving photos must not destroy the catalog.** FoxCull keys every rating,
   label, flag, tag and trim by path-relative-to-the-drive, so a file moved in
   Explorer silently orphaned its marks. The ask was explicitly Lightroom's
   model: flag what can't be found with a "?", never delete it, auto-figure-out
   whole folders that moved, let the user remap the rest — and make the primary
   path *dragging photos onto a folder in the left pane*, with the ability to
   create subfolders there.
2. **Events** — a virtual collection that sits at the same level as tags ("Monar
   trip", "Rashi's birthday"), spans every folder, and renders as a visually
   distinct block with cover art the way Google Photos fronts an album, so a long
   recursive feed becomes readable.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src-tauri/src/catalog.rs` | architecture / schema | New `events`, `event_members` and `missing` tables. New APIs: `list/create/rename/delete_event`, `add_to/remove_from_event`, `set_event_cover`, `events_under`; `tracked_rels`, `set_missing`, `clear_missing`, `missing_under`. `forget` and `move_media_entries` now carry event membership + cover, and clear the missing flag. |
| `src-tauri/src/commands.rs` | logic | `create_folder`; `catalog_scan` (two-pass verify + relink); `list_missing`, `relink_missing`, `relink_folder`, `forget_missing`; seven event commands. `MediaItem` gains `events` + `missing`; `list_folder_media` attaches events and appends "?" placeholders for missing entries in scope. |
| `src-tauri/src/lib.rs` | wiring | Registers the 14 new commands. |
| `src/lib/types.ts` · `src/lib/api.ts` | logic | Typed wrappers for all of the above, plus `pickMediaFile` for the relink picker. |
| `src/lib/settings.svelte.ts` | logic | `GroupBy` gains `"event"`; new `eventOrder`, `eventCovers`, `scanOnLaunch`. |
| `src/routes/+page.svelte` | UX / logic | Event model (`eventStats`, `eventRank`, cover + date-range resolution), event section keys/labels, event filter, event chips on the info bar, event entries in the media context menu, an event manage list in Arrange. Missing-file handling: "?" tiles, relink/forget menu, launch scan, Settings toggles. Folder management: "New subfolder…" in the tree header and folder menu. `Ask` modal extended into a name prompt. |
| `src/lib/components/SectionedGrid.svelte` | UX | Per-section header heights and an event "album band" header (cover, title, date range, count). |
| `src/lib/components/SectionCover.svelte` | UX (new) | The band's cover image, fetched through the existing bounded loader. |
| `src/lib/components/Thumb.svelte` | UX | Missing entries render a hatched "?" plate and issue no fetches. |
| `src/lib/components/Loupe.svelte` | UX | Focus view explains a missing entry instead of showing a black frame. |
| `src/lib/components/TrashPanel.svelte` · `EditStudio.svelte` | types | Synthesised `MediaItem`s carry the two new fields. |

## Behavior changes

**Catalog integrity**

- On launch (after the folder is on screen; `Settings ▸ Check catalog on launch`,
  default on) FoxCull verifies every metadata-carrying entry against the disk.
- The check is two-pass and cheap in the common case: an existence check over the
  tracked rel-paths only, and the library is walked **only if something is
  actually missing**.
- Relink is cohort-first: absent entries are grouped by their old folder, and a
  folder that moved is recognised as one move when at least half its filenames
  turn up together under a single new directory. Leftovers relink on a *unique*
  filename match; ambiguity is left for the user.
- A file that already carries its own metadata is never adopted as a relink
  target — "fixing" one photo must not overwrite another's marks.
- Unresolved entries are flagged, never deleted. They appear in the grid as "?"
  placeholders carrying their full marks, with right-click → Locate this file… /
  Locate the folder it moved to… / Forget. `forget_missing` is the only path in
  the app that deletes marks for a missing file.
- Drag-and-drop from the grid onto a folder in the left pane already moved files
  *and* their metadata; that path is unchanged and is now the documented one.
  New: "New subfolder…" from the tree header and the folder context menu, which
  also moves any staged (cut) files into the folder it creates.

**Events**

- An item can join any number of events; the **first** it joined is its primary
  and decides which block it renders in.
- `Arrange ▸ Group ▸ Event` (also available as Subgroup) turns the grid into
  event blocks that hold together across every subfolder in view.
- Blocks are ordered by name or by each event's earliest capture ("Order by
  date"), and both honour the existing ascending/descending button. The two
  event checkboxes are **disabled** outside an event grouping rather than hidden.
- With "Cover art" on, each block is fronted by a 128px band showing the cover
  frame, the event name, its date range and its count. The cover is the member
  explicitly set via "Use as cover of …", else the first real member.
- `Filters ▸ Event` isolates one event across the whole view — the virtual
  collection read of the same metadata.
- Events are managed (rename, delete, isolate) from a list inside Arrange while
  an event grouping is active. Deleting an event never touches files.
- Event membership travels with a file through a FoxCull move, and through a
  relink.

**Housekeeping**

- "?" entries are excluded from thumbnail warming, Prepare, drag-to-move, and the
  pick/reject counts that drive the delete sweep.

## Risks / compatibility

- **Schema is additive only.** Three new tables via `CREATE TABLE IF NOT EXISTS`;
  no existing table changed shape. An older FoxCull build opening the same
  catalog ignores them.
- **`tracked_rels` deliberately excludes default-valued decision rows** and the
  `captures`/`dir_counts` caches, so merely *looking* at a file can never turn it
  into a "missing photo" the user has to resolve. The set is marks, tags, trims,
  segments and event membership.
- `catalog_scan` walks the library only when something is missing; the walk uses
  the same bounded warm pool as folder counts, so it cannot starve the
  foreground. The launch scan runs after the folder renders.
- Section ordering: the grouped sort compares section keys directly and never
  multiplies by `sortDir`, so event direction is baked into a numeric rank rather
  than applied at compare time. `NO_EVENT_KEY = "999999"` parks unassigned shots
  after every real block under the numeric collator.
- `Ask` gained an optional `input`; all three existing call sites were converted
  to `openAsk(...)` so the field always starts clean. The global key handler bails
  on INPUT targets, so Enter/Escape are handled on the field itself.
- Events are **outside the undo stack** (like tags' own add/remove menu entries):
  the stack's snapshot shape covers marks only, and every event action is
  reversible from the same menu. Called out here so it isn't read as an oversight.

## Verification actually run

- `cargo check` (src-tauri): passed, 0 warnings.
- `npm run check` (svelte-check): 0 errors, 0 warnings.
- `npm run build`: passed.
- Dev server in the browser pane (no Tauri backend, so IPC is inert): page
  renders, console clean. Confirmed the Arrange popover lists Event under both
  Group and Subgroup; confirmed the two event checkboxes report
  `disabled: true` with `groupBy = none` and `disabled: false` after selecting
  Event; confirmed the Filters popover rows are
  `Type · Status · Rating · Label · Tag · Event · Scope`.
- **Not yet verified natively**: relink against a real moved folder, the "?"
  tiles, and the event band's cover rendering all need the built app against a
  real catalog. These ship as `v1.5.0-nightly.1` for device QA.
