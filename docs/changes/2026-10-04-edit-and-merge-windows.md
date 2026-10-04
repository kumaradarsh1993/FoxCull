# Edit and Merge as their own windows; merges you can pause, stop and walk away from

Nightly `v1.5.2-nightly.4`. Spec: `docs/design/edit-window-rework.md` (owner,
2026-10-04). The next items (segments in Merge, the library's in/out marking,
the music-synced reel mode) are specced in
`docs/design/segments-and-reel-mode.md` and are NOT in this build.

## Intent

- Edit stops replacing the library: a separate window with the timeline,
  preview, Look and export only. The library is the only media picker; clips
  go across by drag, E, or ⌘C/⌘V, with the in/out ranges marked in Focus as
  separate segments. Side by side with the library.
- Merge becomes a separate window. Closing it must not stop the merge; the
  library's progress panel carries it and can reopen the window onto it.
  While it runs: no clip previews (they cost resources, and playing a 4K
  original from a card was slow), a clean progress screen with Pause, Resume
  and Stop. Failures in words the owner can act on.
- Think through the corner cases: clips that don't match the timeline, files
  that disappear, quitting mid-merge, one merge at a time.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/tool_windows.rs` (new) | architecture | `open_tool_window(kind, payload)` creates/focuses the "edit"/"merge" window (same `index.html`; the frontend picks the view by label) and queues the payload in an inbox the window drains on load and on each `tool-inbox` ping (no lost-message race). `tile_windows` (library left half, tool right half of the library's screen work area). `stash_set/get` (⌘C clips, the drag in progress). `quit_app`. |
| `src-tauri/src/procs.rs` (new) | architecture | Registry of running ffmpeg children: Pause/Resume of the merge's (SIGSTOP/SIGCONT; NtSuspendProcess/NtResumeProcess on Windows), idempotent; a part spawned while paused starts suspended. Scratch paths (convert work dir, copies in flight). `kill_all_and_clean` on quit. Unix test proves stop/resume and cleanup. |
| `src-tauri/src/lib.rs` | architecture | `build()` + `run(event loop)`: ⌘Q / closing the library while work runs → `confirm-quit` to the library instead of quitting; closing the library closes the tool windows; `Exit` always kills ffmpeg and removes partials. |
| `src-tauri/src/commands.rs` | logic | `MergeStatus` (running/done/error/cancelled, pct, detail, paths, times) + `merge_status`, `merge_pause`, `merge_dismiss`; `friendly_merge_error` (space, removed card/drive, vanished folder, damaged clip, permissions). `validate_media_anywhere` for merge/edit/snapshot (clips may be on a drive the library isn't showing now). `cancel_job("merge")` resumes first so a paused ffmpeg can be stopped. Every watched ffmpeg is registered. `media-output` event after merges, exports and saved frames. Activity events carry `paused` and `path`. `list_folder_media` returns each video's marked `ranges` (`Catalog::ranges_under`). Moves copy to a hidden `.name.foxcull-part` and rename into place. |
| `src-tauri/capabilities/default.json` | process | Applies to main, edit, merge; window close/focus/show/title permissions. |
| `src/routes/+layout.svelte`, `src/lib/windows.ts` | architecture | Window dispatch by label (`?window=` in the browser harness). |
| `src/lib/components/EditWindow.svelte` (new) | UX | Hosts the studio: keyboard (moved from the library page), drop anywhere → after the last clip on V1, ⌘V paste, inbox, notices (info fades; warnings stay), timeline saved in localStorage and restored (missing files named, kept dashed). |
| `src/lib/components/EditStudio.svelte` | UX / logic | Media panel and library props removed. `addClips` (segments per range, clamped; photos/missing/other counted, not dropped silently; never overlaps), drop on a track at a time, `serialize/restore/clearTimeline`, mismatch per clip (orientation, size, fps class, HDR, codec for Original) as a ≠ badge and `compatNotes` saying what export will do. "Side by side". |
| `src/lib/components/MergeWindow.svelte` (new) | UX | Review → progress (Pause/Resume, two-step Stop, Close window, ETA, elapsed) → done (Show in folder, YouTube, Merge other clips) / failed / stopped (Back to the clips). One merge at a time. |
| `src/lib/components/MergeDialog.svelte` | UX | Becomes the review step of the window: fills it, no Play (hover-scrub tile), drives fetched itself, Start waits for the backend to report the merge running (refusals stay on the list). |
| `src/routes/+page.svelte` | UX / logic | Edit mode removed (Library/Edit toggle → Edit button that opens the window); E / menu "Add to the Edit timeline"; ⌘C copies clips; drags carry `application/x-foxcull-clips` and park in the backend; ✂N badge on clips with ranges; Loupe reports range changes; merge opens the window; progress panel gets Pause/Resume/"Show merge window" for merges and "Show Edit window" for exports; refresh on `media-output`; quit confirmation. |
| `src/lib/activity.svelte.ts`, `ActivityBar.svelte` | UX | `paused` (no ETA, amber bar), `path` (automatic "Show in folder"), `addActions` providers. |
| `src/lib/api.ts`, `src/lib/types.ts` | logic | Wrappers and types for all of the above (`ClipRef`, `EditInbox`, `MergeInbox`, `MergeStatus`, `MediaItem.ranges`). |
| `src/lib/dev/mock-ipc.ts` | process | Inboxes and stash in localStorage (a second tab is the other window), mock merge status with pause, ranges on some clips, varied probes. |

## Behavior changes

- Edit opens in its own window; the library stays as it is.
- Merge opens in its own window; it can be closed mid-merge, paused, resumed
  and stopped from the window or the progress panel.
- Quitting with a merge, export or copy running asks first.
- Clips marked with in/out ranges show ✂ and arrive in Edit as segments.

## Risks / compat

- **Not run in the real app yet.** Cross-window HTML5 drag is the uncertain
  part (WKWebView/WebView2 may not carry the custom data type between
  windows): the backend-parked drag and ⌘C/⌘V and E are the fallbacks.
- Windows pause uses ntdll's NtSuspendProcess; it compiles only in the
  Windows release job (no Windows toolchain here).
- The Edit window's timeline lives in localStorage (per machine).
- The library's Edit-mode keyboard shortcuts moved to the Edit window.

## Verification actually run (macOS)

- `cargo test --lib` 47 pass (new: process pause/resume/quit cleanup on real
  processes; ranges query). `npm run check` 0/0; `npm run build` OK.
- Browser harness, two tabs standing in for two windows: E with 3 videos + a
  photo → 4 segments (a clip with two ranges), photo left out with a notice,
  vertical clip ≠ badge and notice; reload restores the timeline; drop on V2
  at 10 s places two segments at 10–12 and 12–15 s; ⌘C in the library, ⌘V in
  Edit adds them with a size-mismatch notice. Merge: review at 1120×760 and
  680×480, Merge → progress view → done view; earlier: running, paused.
- Not verified: the real windows in Tauri, Pause on a real ffmpeg inside the
  app (verified at process level), Windows.
