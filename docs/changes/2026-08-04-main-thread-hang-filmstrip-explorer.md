# Main-thread hang, filmstrip rewind, Explorer reveal, orphaned Trash

## Intent

Four defects the owner hit on `v1.5.0-nightly.1`, three reported and one found
while investigating.

## 1. Clicking a drive root froze the whole app (root cause: main thread)

**Symptom.** Clicking `D:\` left the window "Not Responding" for minutes. It did
eventually recover.

**Evidence, not inference.** `foxcull.exe Responding=False` while *every*
`msedgewebview2.exe` child stayed `Responding=True`. That asymmetry rules out the
webview and points at the native message pump. The log confirms it: the last
heartbeat is `UI MEM tick D:` and then silence — no `SCAN dir=` line for D:,
because the walk never finished.

**Cause.** Tauri runs a **synchronous** command on the main thread — the same
thread that pumps the window's messages. `list_folder_media` was synchronous.
On a normal folder that is invisible (8,403 files on `F:\` in 390 ms); on `D:\`
it descended into `node_modules`, a shared cargo target dir and a Steam library
and blocked the pump for minutes. The async/sync split in this file was already
doing real work — `thumbnail`, `warm_thumbnails`, `capture_dates`,
`folder_counts` and `dispose_rejected` are all `async` — and `list_folder_media`
was simply on the wrong side of it.

**Fix.**
- `list_folder_media`, `list_edit_sources`, `move_media_files` and `list_trash`
  are now `async` and do their filesystem work on a blocking worker.
- The walk is **cancellable**. `warm_gen` was already bumped on every folder
  open; it now doubles as the walk's cancellation token, so opening another
  folder abandons the previous walk instead of racing it.
- The frontend guards on an `openGen` counter, so a late reply from a superseded
  scan can never overwrite the folder the user actually landed on.
- The walk skips directories that cannot contain the user's photos but can hold
  hundreds of thousands of files (`node_modules`, `$RECYCLE.BIN`,
  `System Volume Information`, `Windows`, `Program Files`, `SteamLibrary`, …).
  Deliberately conservative: only unambiguously machine-owned names.
- Progress is now visible. The walk emits a live file count every 400 ms to the
  activity chip, the chip lifts to an accent-tinted "busy" state while any job
  runs, and the scanning screen echoes the count plus "you can pick a different
  folder on the left at any time".

## 2. The filmstrip rewound to the start every time it appeared

**Symptom.** Press `B`, or go Grid → Focus, or leave full screen: the strip
appears at photo 1, races horizontally to the current photo, and reloads
thumbnails on the way.

**Cause.** `VirtualStrip`'s reveal effect scrolls the active cell into view with
`behavior: "smooth"`. A freshly mounted strip has `scrollLeft = 0`, so revealing
photo 750 animated across all 750 — and rendered/requested a thumbnail for every
cell it passed.

**Fix.** Instant placement for the first positioning after mount and for any jump
longer than a viewport; the smooth glide is kept only for ordinary arrow-key
stepping, the one case where the animation conveys something. `positioned` is a
plain `let`, not `$state` — the effect reads and writes it, and reactive state
there is the self-invalidation trap already documented on `Thumb.tilePending`.

## 3. "Show in Explorer" opened OneDrive\Documents

**Symptom.** Reported as "broken on the external drive P:".

**Cause — not the drive.** `explorer.exe` parses its own command line and splits
`/select,…` on spaces, so an unquoted path containing a space loses everything
after the space and Explorer falls back to Documents. The trigger was the space
in `All media MASTER`; it would have failed identically on any drive.

**Fix.** Quote the path *inside* the `/select,` token, via `raw_arg` — `arg()`
applies Rust's own MSVC-style quoting on top and produces a doubly-quoted token
Explorer rejects the same way.

**Verified by execution, both directions**, using `ProcessStartInfo.Arguments`
(verbatim command line) and reading back Explorer's real location through the
`Shell.Application` COM window list:

| Command line | Explorer landed on |
|---|---|
| `explorer.exe "/select,P:\All media MASTER\…\100_5097-edited.JPG"` (old) | `C:/Users/kadar/OneDrive/Documents` ❌ |
| `explorer.exe /select,"P:\All media MASTER\…\100_5097-edited.JPG"` (new) | `P:/All media MASTER/Pics/2010/Takeout` ✅ |

## 4. Files in the in-app Trash the app could not see (found, not reported)

While auditing the `_FoxCull` folders for the owner's requested fresh start:

| Drive | `_FoxCull` size | Trash rows | Files in `recycle/` |
|---|---|---|---|
| D: | 24 MB | 0 | 0 |
| E: | **18,051 MB** | 0 | **1** (an 18 GB merged Dubai-trip MP4) |
| F: | 127 MB | 0 | 0 |
| P: | **1,161 MB** | 14 | **22** (8 untracked DJI Mavic Mini clips, ~1.0 GB) |

~19 GB of real media sat in the per-drive recycle folders with **no `trash`
row**, so the Trash panel showed nothing, Restore could not reach them, and the
owner reasonably believed his Trash was empty — while about to authorise
deleting the folders holding them. None of the originals are back on disk.

**Fix.** `list_trash` now adopts orphans before listing. Every media file under
`recycle/` with no row gets one, reconstructed from its position: the recycle
layout mirrors the original rel-path (`stored == orig` for every row FoxCull
writes), so an adopted file restores to exactly where it came from. Nothing is
deleted — it is made visible and restorable.

## Risks / compatibility

- `list_folder_media`, `list_edit_sources`, `move_media_files` and `list_trash`
  changed from sync to async. `move_media_files` and `list_trash` now return
  `Result`, so `invoke` can reject where it previously always resolved; both
  frontend call sites already sit in `try/catch`.
- Returning `Ok(vec![])` from a cancelled scan is safe because the frontend
  discards superseded replies by generation; the empty vector is belt-and-braces
  so a partial list never leaves the function looking authoritative.
- The directory skip list is additive to the existing dotfolder / `_FoxCull` /
  reparse-point rules. If a user genuinely keeps photos in a folder named
  `node_modules`, they are now invisible — judged acceptable and worth naming
  here so the trade is a decision rather than a surprise.
- Trash adoption runs on every Trash-panel open. It walks only `recycle/`, on a
  blocking worker, and inserts with `INSERT … ON CONFLICT` so repeats are inert.

## Verification actually run

- `cargo check`: passed, 0 warnings.
- `npm run check`: 0 errors, 0 warnings.
- `npm run build`: passed.
- Explorer reveal: both command-line forms executed against the real file, with
  Explorer's resulting location read back programmatically (table above). The
  two test windows were closed afterwards.
- Process/log forensics on the live wedged instance: `Responding` flags per
  process, and the app log read out from under the running app's open handle.
- **Device QA pending:** the filmstrip and drive-root behaviour need the built
  app. Shipped as `v1.5.0-nightly.2`.
