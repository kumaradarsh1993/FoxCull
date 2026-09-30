# Trash mode, a compact sidebar, and new folders that actually appear

## Intent

Three owner reports after installing v1.5.1:

1. **Trash.** "It just shows up as a regular window in the viewport, with some
   top icons that may not be applicable. No way to go back to the previous
   folder… a proper audit of the Trash experience and UI." They asked for how
   other apps do it, and explicitly **not an overlay**.
2. **Bug.** A folder created from the sidebar (under MAHINDRA) showed in
   Finder but never in FoxCull.
3. **Sidebar.** Make the folder pane more compact and space-efficient without
   losing quality.

## Audit of the old Trash (what was wrong)

- It was just the `FoxCull Trash` folder opened like any other, so the
  toolbar kept Prepare, Reject, Clear, "Delete N rejects", Arrange, Filters and
  Edit, none of which mean anything for deleted files.
- Library filters still applied (a rating filter could hide trashed files), and
  it sorted by name, not by when things were deleted.
- The bottom bar offered stars, colour labels, Pick/Reject and tags for deleted
  files. Keyboard culling (P/X/1–5) worked on them.
- No Back: leaving meant finding your folder in the tree again.
- The context menu led with Previous/Next. Restore sat mid-menu, and "Came
  from" was a disabled line rather than an action.
- It lived in the tree as a folder named "FoxCull Trash", mixed in with the
  drive's own folders, and there was no count anywhere.

Reference patterns: Apple Photos "Recently Deleted" (a sidebar entry, a
contextual toolbar with Recover/Delete, per-item age), Google Photos Trash (Back
arrow, "Empty trash", selection actions), Finder Trash (pinned sidebar item,
"Empty", "Put Back", original location).

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/routes/+page.svelte` | UX / logic | **Trash mode**, keyed on `inTrashFolder`. Its own top bar: Back (to the folder and photo you came from), title with drive · count · size, Grid/Details/Focus, Restore all, Empty Trash…, Settings. Bottom bar: name or selection summary, "Deleted … · from <folder> · size", Restore, Delete permanently. Tiles caption "<origin folder> · <time ago>"; newest-deleted first; no filters, grouping or stacks; tiles not draggable. Keys: culling off; R restores, Delete asks and then deletes permanently, ⌘[ / Alt+← / Esc (in the grid, with 0–1 selected) go back; cut/paste disabled. Controller marks ignored; mouse Back leaves. Rewritten menu (Restore, Delete permanently…, Preview in Focus, Open in system player/default app, Show original folder, Reveal). Empty state with Back. **Pinned sidebar entry** "Trash" with a count, for the active drive. Esc that closes a context menu no longer also acts on the page. The view switcher and settings gear became snippets shared by both bars. |
| `src/lib/components/TreeNode.svelte` | UX / logic | **Fix:** `treeGen` makes every expanded node re-list its children (keeping what's open), and `revealPath` expands ancestors and flashes the row. **Redesign:** 24px rows (were 29px), SVG chevron that rotates, drive/home/folder icons, one faint guide line per level with 12px indent, bolder drive rows, accent-edged selection, zero counts hidden. |
| `src-tauri/src/commands.rs` | UX | `list_tree` omits the Trash folder (it has its own entry now). |
| `src/lib/dev/mock-ipc.ts` | process | A fake Trash (14 rows, restore/purge) and `create_folder` that the tree lists. |

## Behavior changes

- The Trash is entered from the sidebar's pinned **Trash** entry (or Settings →
  Open Trash) and left with **Back**. You land back on the folder and photo you
  came from.
- **New subfolder** now appears immediately, with its parents expanded; ↻
  also re-lists folders made outside FoxCull. Changing Excluded folders no
  longer collapses the whole tree (it used to remount it).
- The tree shows about 20% more rows in the same height.

## Risks / compat

- `lastDir` can be the Trash folder. After a restart there's no Back target, so
  Back goes to the drive root.
- The Trash entry follows the active drive. Another drive's Trash is reached by
  opening a folder on that drive first.
- The Details view in the Trash still shows the library columns (Marks etc.);
  no "Deleted" or "From" columns yet.

## Verification actually run (macOS, browser pane + fake backend)

- Sidebar: 24px rows, drive/home/folder icons and guides; "FoxCull Trash" absent
  from the tree; "Trash 14" pinned.
- New subfolder under a collapsed drive: appears at once, parent expanded,
  other expanded folders still open.
- Trash: bars and captions as above; culling keys ignored. R restored one
  (14→13); Delete asked, then deleted (13→12). Esc returned to SD_Card with the
  same photo active. Esc to close the menu stayed in the Trash. Empty Trash
  asked "…14 files (1.7 GB)…" and showed the empty state; Restore all/Empty
  disabled; badge gone.
- `__audit`: Trash clean at 1280×788 and 1049×645 (TV). `__sweep` on the
  library shows only the expected Details header and Edit "Look" tab.
- `npm run check` 0/0; `npm run build` OK (no dev tooling in `build/`);
  `cargo check` clean; `cargo test --lib` 37/37.
- Not run: the real app against a real Trash on disk; Windows.
