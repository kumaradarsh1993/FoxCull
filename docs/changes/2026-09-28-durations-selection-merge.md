# Video lengths on tiles, selection totals, and a lossless "Merge videos"

## Intent

The owner's second workflow, separate from the Edit studio's Instagram
crop/trim/grade flow: take a trip's Osmo Pocket 3 clips, drop the vertical ones
and photos, and join the rest end to end in shooting order for YouTube, with no
re-encoding ("I don't have time and these are super large videos"). Asked for,
in their words:

1. The video length on grid tiles, where landscape clips leave letterbox space
   anyway, plus a setting for what tiles show.
2. Range-select, then add or remove items with modifier-clicks, as in other apps.
3. Right-click a selection → merge, LosslessCut-style: a simple list with
   compatibility checks, not the timeline.
4. (Mid-task) A status line for a selection: count, total length, total size.
   And sanity checks before a merge: the estimated length and size, a choice
   of destination (external SSD ↔ internal drive), and whether it fits. The
   Mac has a 512 GB SSD with ~76 GB free, holding ~97 GB of these clips.

## What the real clips are (probed, not assumed)

`~/Downloads/Adarsh seattle temp osmo pocket 3`: 57 MP4 (+56 `.LRF` proxies,
5 JPG), 97 GB.

| Clips | Stream signature |
|---|---|
| 41 (98.5 min) | HEVC Main 10, yuv420p10le, 3840×2160, 59.94 fps, AAC 48 kHz stereo 317 kb/s |
| 8 (28.2 min) | HEVC **Main (8-bit)**, 3840×2160, **29.97 fps** |
| 5 | HEVC Main 10, **1728×3072** (vertical), 59.94 fps |
| 2 | HEVC Main 10, **3072×3072**, 59.94 fps |
| 1 | HEVC Main 10, 3840×2160, **23.98 fps** |

Video bitrate is ~70–110 Mb/s. Every file also carries `djmd` (27 kb/s), `dbgi`
(**~5 Mb/s** debug), `tmcd` and a 1280×720 MJPEG cover as a second "video"
stream. A single folder mixes five incompatible signatures, which is why the
existing `concat_needs_reencode` (size + codec only) was not enough: it would
stream-copy 59.94 and 29.97 fps into one broken file.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/video.rs` | logic | `mp4_duration`: length from the `moov/mvhd` box, seeking past `mdat` (32/64-bit sizes, v0/v1 mvhd). Unit-tested with a synthetic file. |
| `src-tauri/src/catalog.rs` | architecture | New `durations` table (rel, duration, mtime, size) + `durations_under` / `set_duration_many`, mirroring `captures`. |
| `src-tauri/src/commands.rs` | logic | `video_durations` (cached, warm pool); `merge_probe` (per-clip signature: codec, profile, pix_fmt, size, fps, rotation, audio codec/rate/layout; skips attached pictures; sorted by creation_time, then name); `merge_videos` (concat demuxer, `-map 0:V:0 -map 0:a:0? -c copy`, `hvc1` tag for HEVC, first clip's creation_time, pre-flight free-space check with 1 GB margin, progress/cancel via `ExportWatch`); `disk_free`; helpers `file_stamp`, `ffmpeg_banner`, `clip_length`, `split_top`, `friendly_file_stem`, `iso_utc`. Three unit tests on real Osmo banner text. |
| `src-tauri/src/lib.rs` | process | Registers the four commands. |
| `src-tauri/Cargo.toml` | process | `fs4 = "0.13"` for free space (pure Rust: rustix + windows-sys). |
| `src/lib/components/MergeDialog.svelte` | UX | New. Summary (clips, length, format, ≈size), set chips when signatures differ (default: the longest), clip list in shooting order with mismatch reasons, file name, destinations with live free space (clips' folder, this computer's Movies/Videos, each non-system drive, any folder), not-enough-space warning that disables Merge, progress + ETA + Stop, done screen with Show in folder / Open YouTube upload. |
| `src/routes/+page.svelte` | UX / logic | Tile length badge in a bottom-right cluster with the tag/event glyphs; optional file-name caption row; `durations` map fetched per folder in batches of 64; selection summary in the info bar ("N selected · V videos · h:mm:ss · P photos · size"); Cmd/Ctrl+Shift-click adds a range; "Merge N videos into one…" in the item menu; Settings → Tile details (Video length / File name); shortcuts panel lists the click modifiers. |
| `src/lib/settings.svelte.ts` | logic | `tileInfo: { duration: true, name: false }`, merged over stored values. |
| `src/lib/types.ts`, `src/lib/api.ts` | logic | `MergeClip`, `MergeOutcome`; `videoDurations`, `mergeProbe`, `mergeVideos`, `diskFree`. |
| `src/lib/dev/mock-ipc.ts` | process | Fake handlers for the four commands (mixed signatures, a nearly full SD card). |
| `docs/design/precache-policy.md` | process | Row for the durations cache. |

## Behavior changes

- Video tiles show their length (bottom-right). Settings → Tile details turns it
  off, or adds a file-name caption.
- With 2+ items selected, the info bar reads e.g. `12 selected · 8 videos ·
  1:02:13 · 4 photos · 18.4 GB`. `≥` prefixes the length while some durations
  are still loading.
- Cmd/Ctrl-click (toggle one) and Shift-click (range) already existed;
  **Cmd/Ctrl+Shift-click now adds a range** to the selection, keeping the anchor.
- Right-click a selection with 2+ videos → **Merge N videos into one…**.
  Photos are left out and counted. Only clips matching the chosen signature can
  be ticked. The output keeps the original video and audio bit for bit, drops
  DJI's debug/metadata/cover streams, and is written as `<name>.mp4`
  (uniquified, never overwritten).

## Risks / compat

- Merge output size is estimated as the sum of the input sizes. The real
  output is ~5% smaller (the dropped debug track), so the check errs safe.
- `-map 0:V:0` needs ffmpeg ≥ 4 for the `V` specifier (the bundled one is 9.0.2).
- Order is container `creation_time` (UTC). Clips without it sort last by name.
- No direct-to-YouTube upload (see the owner answer in the handover): that
  needs Google OAuth, an API project and daily quota, and must stream
  fragmented MP4. The dialog instead lets the output go to another drive, so the
  Mac's own disk is never used.
- Windows not exercised (the free-space call goes through windows-sys there).

## Verification actually run (macOS)

- `cargo test --lib` 37/37 (new: 2 `mp4_duration`, 3 `merge_tests`); `cargo
  check` clean; `npm run check` 0/0; `npm run build` OK with no dev tooling in
  `build/`.
- Temporary ignored test over the real 57 clips (removed after): header lengths
  of all 57 read in **21 ms total**, max difference from ffmpeg **0.005 s**;
  signature grouping reproduced the five sets above exactly.
- Real merge, with the exact command, of three main-set clips (143 + 179 + 79
  MB) into scratch: 0.26 s; duration 37.06 s = the sum of the inputs; HEVC Main
  10 hvc1 + AAC 317 kb/s; decoding windows across both joins had no errors;
  audio 37.05 s. Deleted afterwards.
- Browser pane with the fake backend at 1280×788: badges render; selection
  sequence (range → Cmd remove → Cmd+Shift add → Cmd toggle) gave 6 → 5 → 10 →
  9 with correct summaries; Select All + right-click showed "Merge 43 videos";
  the dialog grouped 34 / 6 / 3, left out 161 photos, marked the nearly full SD
  card "not enough space" with Merge disabled, merged to the external drive and
  showed the done screen; `__audit` clean on the dialog, and `__sweep` clean
  apart from the expected Details header.
- Not run: a full 60+ GB merge, Windows.
