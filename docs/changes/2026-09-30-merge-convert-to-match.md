# Merge: honest compatibility checks, and "Convert to match"

## Intent

The owner tried to merge 12 clips from Meta Ray-Ban Display glasses and the
merge window flagged 7 of them ("Vertical" highlighted on vertical clips, and
frame rates of 29.88 / 29.73 / 29.99 against 30). They asked whether it was
ffmpeg or just FoxCull's logic stopping them, and why "Vertical" was flagged
when every clip is vertical. They need one file for YouTube.

What the clips actually are (ffmpeg, 2026-09-29): HEVC Main 10, HLG
(bt2020/arib-std-b67), ~15 Mbps, AAC 48 kHz stereo, **variable frame rate**
(frames dropped, so averages of 29.73-30), and **a different crop per clip**:
1376×1824, 1376×1840, 1392×1856, 1424×1888, 1488×1984. The parameter sets live
only in each file's `hvcC` header (no in-band VPS/SPS/PPS).

Tested with the bundled ffmpeg 9.0.2:

- Same size, different average rate (29.73 + 30 + 30), stream copy: 4151 of
  4151 frames, 0 decode errors, timestamps monotonic. **The fps flag was
  FoxCull being too strict.**
- Different sizes (1376×1840 + 1392×1856), stream copy: the second clip decodes
  with the first clip's SPS: `cu_qp_delta` / `CABAC_MAX_BIN` errors and a green
  frame after the join. **The size flag was right; the "Vertical" label hid the
  reason.**
- One ffmpeg with 12 inputs into the `concat` filter (the obvious way to
  re-encode): all frames kept, but 3 min 16 s of frozen picture inserted at six
  joins. Reproduced with 3 inputs: the gap after clip 2 equals clip 1's length
  minus clip 2's. **Not used.**
- Each clip encoded on its own with identical VideoToolbox settings, then a
  stream-copy join: `hvcC` byte-identical across 4 clips of 4 sizes, 0 decode
  errors, no gaps. **This is the shipped design.**

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/commands.rs` | logic | `fps_class()`: the nominal rate from ffmpeg's average (the first class of 12/15/24/25/30/48/50/60/72/90/100/120/240 at or above 0.995 × average), so VFR and NTSC fall in one class. `MergeClip` gains `fps_class`, `vbitrate`, `color` (hlg/pq/sdr); the signature uses the class and the colour. |
| `src-tauri/src/commands.rs` | architecture | `MergeRequest.convert: Option<MergeConvert>` (size, rate, 10-bit, colour, bitrate), validated. `merge_videos` now splits into `merge_copy` (the unchanged lossless join) and `merge_convert`, which encodes each clip to a part in a dot-folder beside the destination (scale to fill + centre crop, `fps=`, HEVC with the source's HDR tags, AAC copied when it's already 48 kHz stereo, silence for clips without audio) and then joins the parts with `merge_copy`. The encoder is picked on the first clip (VideoToolbox → x265 on Mac, NVENC → x265 elsewhere) and kept, and the parts' `hvcC` must match before the join or it fails with nothing written. Progress is split per part (`ExportWatch.base_pct/span_pct`). The space check counts the parts too (2× the estimate). A failed merge deletes its half-written file. |
| `src-tauri/src/video.rs` | logic | `mp4_codec_config()`: the `hvcC`/`avcC` box of an MP4, for the part check. |
| `src/lib/components/MergeDialog.svelte` | UX / logic | Frame column shows width×height (the chip adds the shape); FPS shows the class (29.97 stays 29.97; a VFR 29.73 shows 30 with an explanatory tooltip); HDR shows as "HEVC HLG" / "HEVC HDR10". Each issue knows whether a conversion fixes it: size within 3% aspect, frame rate, codec and audio do; another shape, HDR vs SDR, photos don't (`hard`). "How to join" (Lossless / Convert to match) appears when a conversion would bring clips in. Convert mode: target chips, fixable cells turn blue ("adjusted to match"), only hard items block, summary shows bitrate vs the sources and the free space needed, button reads "Convert and merge N clips". Target = the dominant clip's rate, colour and depth at the size of the largest included clip, 2× the sharpest source's bitrate scaled to that size (8-150 Mbps). |
| `src/lib/types.ts`, `src/lib/api.ts` | logic | `MergeClip` fields, `MergeConvert`, `mergeVideos({… convert })`. |
| `src/lib/dev/mock-ipc.ts` | process | The fake probe adds VFR clips (must not flag), a smaller crop (fixable) and the new fields. |

## Behavior changes

- Meta glasses clips: lossless mode flags only the six with a different crop,
  with the sizes in the cell. "Convert to match" merges all twelve.
- Clips whose average rate differs only through VFR/NTSC are no longer flagged.
- HDR and SDR clips are flagged against each other (they used to pass if
  codec and pixel format matched).

## Risks / compat

- A conversion is a second generation. At 2× the source bitrate with
  VideoToolbox HEVC it is visually the same; it is still not bit-exact, which
  is why Lossless stays the default and the choice is explicit.
- Clips are scaled to the largest one and centre-cropped to its aspect (at most
  3% of an edge, since larger aspect differences are refused).
- On Windows without NVIDIA the fallback is libx265, which is slow (not run on
  Windows yet).
- A conversion needs twice the result's size free while it works (parts and
  the joined file coexist). The first real run, started outside the app's
  up-front check, filled the owner's 250 GB external SSD and the join failed at
  4.3 GB ("No space left on device"). `merge_convert` now re-checks the space
  before the join, and a failed merge deletes its partial file.
- The fps class of a heavily dropped VFR clip can land one class low (a 60
  averaging 45 reads 48). That only produces a flag, never a broken file.

## Verification actually run (macOS, Apple Silicon)

- `cargo test --lib`: new tests for fps classes, Meta banner parsing
  (VFR same signature, crop and HDR/SDR different), convert validation and the
  part command's arguments.
- The real 12 Meta clips (19:19, 1376×1824 … 1488×1984, VFR 29.73-30) through
  `merge_convert` (opt-in test `real_convert_merge`, the function the app
  calls), to the Mac's Movies folder: 3 min 56 s with VideoToolbox. Result
  1488×1984 HEVC Main 10 HLG (bt2020/arib-std-b67 kept), 30 fps CFR, 41 Mbps,
  5.97 GB. Video 1159.33 s, audio 1159.35 s = the sum of the clips' lengths.
  34,666 frames, full decode with 0 errors. Timeline gaps only where a source
  clip's audio outlasts its video (up to 1.24 s, as recorded); none of the
  concat-filter kind. A frame after each of the 11 joins viewed: all clean,
  including the size changes that turned green in the stream-copy test.
  Work folder removed afterwards.
- Browser harness at 1280×788 and 1049×645: lossless and convert modes,
  "Select these" + Delete, blue adjusted cells; `__audit` shows only the
  right pane scrolling under its pinned action bar.
