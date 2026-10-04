# Segments everywhere, and the Reel window

Nightly `v1.5.2-nightly.5`. Spec: `docs/design/segments-and-reel-mode.md`
(owner, 2026-10-04, "a major item"). Builds on the Edit/Merge windows of
nightly.4 (`2026-10-04-edit-and-merge-windows.md`).

## Intent

- **A. Library in/out cleanup.** `[` / `]` build several segments per clip
  with the owner's rules (again moves; segments never overlap), drawn as spans
  with markers that drag frame by frame; right-click removes an in, an out, a
  segment or all; drag one segment or all of them to Edit.
- **B. Merge with segments.** A Segments column with a checkbox per clip that
  has segments (default: use them), a tri-state header, totals that follow the
  choice, a preview that starts at the first in and plays only the checked
  segments with a scrub bar, and "Show in library".
- **C. Reel (mode 3).** Its own window: clips (segments as separate pieces)
  and a song with beats found automatically, a section of the song, then the
  beat board: filmstrip per piece, a beat-length window to move and resize
  (snapping to beats), later pieces re-flow keeping their offsets, overflow in
  red, hover preview, portrait preview with the song, export.

## Decisions taken (the spec left them open)

| Question | Decision |
|---|---|
| `[` inside an existing segment | Moves THAT segment's in (refines it). With an in already open, it's refused with a note. Never overlaps. |
| `]` with nothing open | Extends the last segment that ends before the playhead; before every segment, makes one from the start. |
| `]` running into the next segment | Stops at its start, with a note. |
| Remove in point / out point | In: the segment starts where the previous one ends (or at 0). Out: the segment opens again (its in waits for a `]`), or with another in already open, runs to the next segment. |
| Lossless merge of segments | Each piece is copied out as MPEG-TS (`-ss` before `-i`, `-c copy`) and the pieces are joined: starts at the keyframe at or before the in (Osmo: ≤ 0.5 s early). Concat-demuxer inpoints and cut-MP4 parts both produced backwards timestamps on B-frame footage. Convert re-encodes with exact `-ss`/`-t`. The window says so. |
| One clip, two segments | A valid merge (pieces ≥ 2, not clips ≥ 2). |
| Segments a window sees | `video_ranges`: from each clip's own drive catalog, so a library now showing another drive doesn't make them vanish. Re-read when the window comes to the front. |
| Beat detection | Own Rust (no new crates): spectral flux → tempo by autocorrelation with a 120 BPM prior (the envelope smoothed first, or 120 BPM read as 60) → Ellis DP tracker → downbeats = the beat phase of four with the most bass. Tests: click tracks at 92/120/140 BPM, beats within 40 ms, downbeats exact. |
| Major / minor beats | Major = downbeats (bar starts), minor = the other beats. |
| Default cut lengths | Each piece gets its fair share of what's left of the section, snapped to the nearest downbeat that fits the clip (on a tie, the later one: lean long); a clip shorter than a bar gets the longest beat that fits; shorter than one beat runs to the next beat and shows red. No beats found → even shares. |
| Re-flow | Resizing a window pins its length; later pieces keep their offsets and re-flow with the default rule; a pinned later piece keeps its length re-snapped to the nearest beat. "auto" / double-click on the edge unpins. |
| Overflow at export | Blocked with a message; "Fix for me" slides the window back if the clip is long enough, else shortens it to the longest beat-aligned length that fits. Freeze-frame filling would desync the cuts from the beats. |
| Filmstrips | Whole-clip pieces use the Focus filmstrip; segments get their own range strip (`video_range_strip`, ~2 frames/s of the segment), else a 5 s segment of a 20 min clip would show one frame. |
| Song drop | The Reel window keeps Tauri's native file-drop handler (paths), so its reordering uses pointer events, not HTML5 drag. Clips come from the library menu or ⌘C/⌘V. |
| Export | `reel_export` builds an Edit export (1080×1920 crop-fill, SDR, AAC 192k) with `music_start_s` and a 1 s fade-out; its own job "reel-export" and Stop, so it can't cancel an Edit export. Cut points are rounded on the frame grid from the reel's start, so a dozen cuts can't drift off the beat. |

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/segments.ts` (new) | logic | The `[`/`]` rules as pure functions (markIn/markOut/moveEdge/removeIn/removeOut/…), node-tested. |
| `src/lib/components/Loupe.svelte` | UX | Multi-segment marks: spans, markers (drag, frame snap, ←/→ nudge), right-click menu, Clip tools list, Play segments, Save as clips, drag a segment / all to Edit. Legacy trim migrates to one segment. |
| `src/routes/+page.svelte` | UX | Loupe key routing (←/→ nudge, Esc), `library-reveal` listener (select a clip, opening its folder), "Create a reel synced to music…", "Show Reel window" on the reel export job. |
| `src/lib/components/MergeDialog.svelte` | UX / logic | Segments column + tri-state header, totals/space follow the choice, `parts` in the request, keyframe note and "Convert (exact cuts)", row menu: use segments / whole clip, Show in library. |
| `src/lib/components/SegPlayer.svelte` (new) | UX | The merge preview: opens at the first in, plays only the chosen parts in sequence, scrub bar with lit parts, hover glimpse from the cached filmstrip, frees its decoder per clip. |
| `src/lib/components/MergeWindow.svelte` | UX | "6 clips (8 parts)". |
| `src/lib/reel.ts` (new) | logic | Pieces, layout on the beats, snapping, overflow, longest fit, default section; node-tested. |
| `src/lib/components/ReelWindow.svelte` (new) | UX | Step 1 (pieces, pointer reorder, segments toggle, song drop/choose, waveform section with downbeat snapping and zoom, audition), inbox/paste/native drop, saved in localStorage. |
| `src/lib/components/ReelBoard.svelte` (new) | UX | Step 2: rows with frame strips, beat ticks, windows (move / resize with snapping), overflow, Fix for me, portrait preview (A/B videos synced to the song), export panel with progress and Stop. |
| `src-tauri/src/beats.rs` (new) | logic | Decode with ffmpeg, FFT, onset envelopes, tempo, DP tracker, downbeats, waveform outline; tests. |
| `src-tauri/src/commands.rs` | logic / architecture | `MergeRequest.parts`, `MergeItem`, `merge_items_copy` (TS pieces), convert with cuts, `merge_copy_with`; `MergeStatus.parts`; `video_ranges`; `analyze_beats` (+ asset scope for the song, memo); `reel_export` + `reel_edit_request`; `run_edit_export` shared by Edit and Reel; `music_start_s`, `music_fade_s`, `friendly_name` on Edit exports; `video_range_strip`; more audio extensions. |
| `src-tauri/src/video.rs` | logic | `ensure_range_strip` (sprite of one part of a clip; `ensure_sprite` takes a range). |
| `src-tauri/src/tool_windows.rs`, `lib.rs`, `capabilities/default.json` | architecture | "reel" window (keeps the native drop handler), `show_in_library`, new commands registered. |
| `src/lib/api.ts`, `types.ts`, `windows.ts`, `+layout.svelte`, `activity.svelte.ts` | architecture | Wrappers/types (`BeatInfo`, `ReelInbox`, `parts`), window kind "reel", "reel-export" classified as an export. |
| `src/lib/dev/mock-ipc.ts` | process | Segments persist across tabs, `video_ranges`, beats (112 BPM), reel export, strips from `static/dev-sprite.jpg` (generated, gitignored). |

## Behavior changes

- Focus marks several segments per clip with `[`/`]`; the old single trim
  becomes the first segment the first time the clip opens.
- Merge uses a clip's segments by default when it has any.
- A third tool window, Reel.

## Risks / compat

- **Not run inside the real app yet** (same as nightly.4's windows). Real
  ffmpeg paths are verified by opt-in tests: segment merge on a real Osmo clip
  (nightly.4 work), reel export (`real_reel_export`: 1080×1920, 30 fps,
  7.77 s for 7.75 s of pieces, the song from 2.5 s, −31 dB → −46 dB over the
  last second).
- Beat detection is tuned on synthetic click tracks only; real songs (calm,
  rubato, swing) need the owner's ear. Downbeats assume 4/4.
- The native drop handler in the Reel window: if WKWebView/WebView2 behave
  differently there, the Choose button is the fallback.
- Range strips are new cache files (`r<in>-<out>-<hash>.jpg` beside the
  filmstrips; precache policy updated).
- Landscape clips in a reel are centre-cropped; choosing the crop is the
  spec's later to-do.

## Verification actually run (macOS)

- `cargo test --lib`: 50 pass (new: beats ×3), 4 ignored (real-media opt-ins;
  `real_reel_export` run and checked with ffprobe as above).
- `npm run check` 0/0. `src/lib/segments.ts` and `src/lib/reel.ts` rule tests
  in node (esbuild bundle): all pass.
- Browser harness: Focus `[`/`]` on a clip (two segments, spans, pills);
  Merge window with mixed clips (segments column at 1280 and ≤ 900 px,
  tri-state header, totals, preview playing 2–6, 10–14, 20–25 s then back to
  2 s, scrubbing, row menu, the request's `parts`); Reel: inbox → 9 pieces
  (segments read from the catalog), song drop → beats and default section,
  section resize snapping to a downbeat, zoom, board defaults on downbeats,
  edge resize → overflow red → Fix for me, window move, playback switching
  clips at the cuts, export request (total = section length), restore after
  reload, 880×580.
