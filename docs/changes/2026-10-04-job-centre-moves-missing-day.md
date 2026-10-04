# Job centre, drag-to-folder moves, missing items in bulk, Day groups

Fixes-only nightly (`v1.5.2-nightly.3`). The Edit rework the owner described the
same day is the next nightly: spec in `docs/design/edit-window-rework.md`.

## Intent

The owner's list, 2026-10-04:

1. Dragging a selection onto a sidebar folder floated the whole selection as
   the drag image; it should be a small stack with a count. A drop that copies
   (to another drive) should show its progress in FoxCull with a time estimate.
2. The bottom-left progress indicator should grow up: concurrent jobs, an
   expandable list, ETAs, what each job is doing, finished results delivered
   and dismissable. "Imagine it's a modern 2026 application."
3. Removing missing ("unlinked") media took a right-click per tile. Allow it per
   folder (right-click in the sidebar, any depth) and for a selection, mixed or
   not. Check relinking is Lightroom-like: locate one file and its neighbours
   reconnect.
4. Group by Day, for trips.
5. Picked/rejected visible at a glance: a green tab on the top edge of picked
   tiles, a red one on the bottom edge of rejected ones.
6. The merge window can't be minimised or closed while it runs; it should be,
   with progress in the job centre.
7. Why is a FoxCull merge slower than LosslessCut's "5-10 seconds"?
8. Is the Prepare button still needed?

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/commands.rs` | architecture | `Activity` events gain optional `detail`, `unit` ("bytes"), `cancellable`; `emit_job`. A job cancel registry (`JOB_CANCELS`, `job_token`, `job_finished`, `cancel_job` command; "edit-export" maps to `export_gen`). |
| `src-tauri/src/commands.rs` | logic | `move_media_files(paths, dest, copy?, job?)` → `TransferPlan` + `transfer_files` + `transfer_catalog` (Tauri-free, so tested on real volumes): rename on one volume (instant); otherwise `copy_file_progress` (8 MB chunks, bytes reported every 150 ms, mtime/atime/created kept, `sync_all` before deleting the original on a move, size verified, partial copy removed on failure/cancel, never overwrites). Destination may be on another drive: `drive_root(dest)` picks the catalog, and the records go there via `export_entries` → `import_entries`, then `forget` here for a move. `transfer_dest_allowed`/`hidden_dest_component`: no drops into a library, a Trash, app data or anything the tree hides. Returns `copied`, `cross_drive`, `cancelled`. Logged as `MOVE …`. |
| `src-tauri/src/commands.rs` | logic | `ExportWatch` gets a job id, an optional generation, an optional stop flag, a `detail`, `expect_bytes` (bytes written / speed from ffmpeg's `total_size`). Merges report as job "merge" with their own flag (an Edit export no longer cancels a background merge), one merge at a time, `MERGE start` / `MERGE ok … secs= MBps=` in the log, done/cancelled/error as job states. |
| `src-tauri/src/catalog.rs` | logic | `MediaMeta`, `export_entries`, `import_entries` (events matched by name, created if absent), `copy_media_entries`. |
| `src/lib/activity.svelte.ts` | architecture | The job model: kind, quiet (housekeeping), unit, cancellable, queued, actions, started/ended. Quiet jobs vanish 2 s after ending; owner jobs go to Recent (15 min / 20 entries; errors stay until dismissed). Time-window rate (8 s) for ETA and speed (a 16-sample window never qualified at 150 ms updates). `start/update/finish/notify/cancel/dismiss/clearFinished`; `local/end/error` kept. `finish` replaces the running detail with "4.2 GB in 38 s". |
| `src/lib/components/ActivityBar.svelte` | UX | The job centre. One job: icon, title, sizes · speed · file, time left, %, bar, Stop, action chips. Several: "N tasks running", combined bar, the longest time left. Just finished: the result with its follow-up for 6 s, then a "N recent tasks" footer; failures keep a red count. The list floats above the card (340-400 px, a 230 px sidebar truncated everything), sections In progress / Recent, closes on outside click. |
| `src/routes/+page.svelte` | UX / logic | Drag image: `setDragGhost` (up to 3 thumbnails, count badge, removed after the snapshot). `movePathsTo` queues moves, one job each ("Moving 24 items to Seattle on MAHINDRA"), Stop works queued or running, results with "Open folder". `driveRootOf` mirrors the backend's drive rule (`rootForDir` can answer Home). Missing items: folder menu "Remove N missing items…" (refreshes the count from `list_missing` when the menu opens), grid menu "Select all N missing here", "Select only the N that exist", "Remove the 3 missing of these 12…"; locating one file also relinks its old folder from the new one (not from the drive root). Day grouping (`wallTime`: videos use the camera's file-name clock when it has one, so evening clips stay on their day; capture sort uses it too). Pick/reject tabs. Prepare removed from the toolbar; `prepareFolder` behind folder menu "Build previews for this folder" with Stop. Merge window hide/show wiring; keys go back to the grid while it's hidden. Floating dock lifted above the status bar. |
| `src/lib/components/MergeDialog.svelte` | UX | "Run in background" (button and the header's –) while merging; progress and bytes/speed from the job centre; Stop via `cancel_job("merge")`; hidden = `display:none`, keys ignored. |
| `src/lib/components/TreeNode.svelte` | UX | Drop pill "Move 24" / "Copy 24" (Option on Mac, Ctrl elsewhere); spring-loaded folders (700 ms hover opens). |
| `src/lib/drag.svelte.ts` | logic | Shared drag count + platform copy modifier. |
| `src/lib/settings.svelte.ts` | logic | `GroupBy` adds "day". |
| `src/app.css` | UX | `.dragGhost` styles (global: built outside Svelte). |
| `src/lib/dev/mock-ipc.ts`, `layout-audit.ts` | process | Fake moves/merges stream job events with Stop; some missing items; `__sweep` audits the job panel instead of the gone Prepare menu. |
| docs | process | This ledger; handover; project log; release notes; precache policy §7, README, STORAGE, CLAUDE.md (Prepare moved, moves across drives); `docs/design/edit-window-rework.md` (next nightly's spec and corner cases). |

## Behavior changes

- Drag shows a small stack with a count; the folder under it says what the drop
  will do. Option/Ctrl-drag copies.
- **Moves across drives work.** They used to be refused (external → Mac) or,
  worse, accepted with the marks left in the wrong catalog (Mac → external,
  because `/Volumes/X` is under `/`). Now they copy with progress, verify,
  delete the original, and the marks follow into the destination's catalog.
- Every move/copy is a job with bytes, speed, time left and Stop; a second drop
  queues behind the first.
- A merge can run in the background; the job centre carries it, with "Show
  merge window", Stop, and "Show in folder" when done.
- Missing items can be removed per folder or per selection; locating one file
  reconnects the rest of its folder.
- Group/Subgroup by Day.
- Picked tiles carry a green tab on top, rejected a red tab underneath.
- The toolbar has no Prepare button; it's "Build previews for this folder" in
  the folder menu.

## Merge speed (owner question 7), measured

- FoxCull's join is the same kind of operation as LosslessCut's (ffmpeg concat
  demuxer, stream copy). On the internal SSD, 11.3 GB synthetic 4K60 HEVC:
  FoxCull's arguments 9.4 s, LosslessCut's 20.6 s.
- Real Osmo clips read from MAHINDRA (exFAT over USB, macOS FSKit), written to
  the Mac: plain read 6.0 GB in 9.4 s (~640 MB/s); FoxCull merge 6.4 GB in
  15.3 s (~420 MB/s); LosslessCut merge 7.1 GB in 23.7 s (~300 MB/s).
- LosslessCut is slower because its default `movflags +faststart` rewrites the
  whole file a second time to put the index first. FoxCull doesn't (YouTube
  doesn't need it).
- So a merge is bounded by the disks: the owner's merges were 15-70 GB
  (`MERGE ok` lines in the log), which is 40 s to 3 min from that SSD and
  longer from an SD card. 5-10 s is what a few GB on the internal SSD takes in
  either app. The job centre now shows the bytes and speed so the reason is
  visible, and the log records `secs=`/`MBps=` per merge.
- Not done: a custom MP4 concatenation reading each file sequentially (rather
  than ffmpeg's interleaved demux) might approach the plain-read speed. Large
  and risky; noted for later.

## Risks / compat

- Cross-drive move opens the destination drive's catalog as a second SQLite
  connection (WAL); its marks land there before they're removed here. If the
  import fails, files are moved but marks stay in the source catalog and show
  as missing there (reported as "catalog update failed").
- A move flushes each copy to the device before deleting the original
  (`sync_all`): slower than an unflushed copy, deliberately.
- Copy within one APFS volume is a real copy, not a clone (no progress API for
  clones). Rare (Option-drag in one drive).
- Day grouping reads a video's day from its file name when it has a camera
  timestamp; a renamed file falls back to its container time (UTC).
- One merge at a time (the backend refuses a second; the menu brings the
  running one back).

## Verification actually run (macOS, Apple Silicon)

- `cargo test --lib`: 46 pass, 2 ignored. New: catalog records travel between
  catalogs (marks, tags, trims, segments, captures, durations, events) and copy
  within one; chunked copy keeps bytes and mtime, refuses to overwrite (caught
  a bug where a failed open deleted the existing file), a stop leaves nothing;
  move destinations follow the tree's rules; a real move and a real copy on
  one drive through `transfer_files` + `transfer_catalog` (the code the
  command runs: bytes, mtime, originals, marks re-keyed or duplicated).
- `real_cross_drive_move` (opt-in, `FOXCULL_XFER_DEST`) against a fresh exFAT
  disk image mounted through FSKit: copied across devices, bytes and mtime
  identical, originals deleted only after the verified copy, marks found in the
  image's own `_FoxCull/catalog.sqlite` and gone from the source catalog. Pass.
- `cargo clippy --lib`: 22 warnings before and after (all pre-existing).
- `npm run check` 0/0; `npm run build` OK.
- Browser harness (fake backend, 1280×800 and 1049×645): drag ghost (3 cards,
  badge "3", removed after dragstart), "Move 3"/"Copy 2" pills, spring-loaded
  folder, cross-drive move with bytes/speed/ETA, two queued moves, Stop on a
  running and on a queued move, finished results with "Open folder", merge in
  background (hidden window, keys back to the grid, "Show merge window"),
  folder "Remove 11 missing items…" + confirm, mixed-selection relink menu,
  Month › Day sections, pick/reject tabs, floating dock with the sidebar hidden.
  `__sweep()`: clean except a Details column scrolled off at those widths
  (pre-existing, the table scrolls).
- Merge speed numbers above (ffmpeg directly, the same arguments the app uses).
- Not verified: the move through the running app (the command is now a thin
  wrapper over the tested functions), Windows, the real drag image in WKWebView
  (the synthetic drag checks the DOM ghost; the OS snapshot needs a real mouse).
