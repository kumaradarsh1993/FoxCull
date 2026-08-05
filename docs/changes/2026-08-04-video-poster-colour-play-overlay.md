# Washed-out video stills, paused-state overlay, and the workflow doc

## Intent

The owner reported that a video's still in Focus view looks "very washed out",
with colour snapping back the instant playback starts and never returning to
washed-out on pause. He also asked for a proper play/pause overlay, and for a
written explanation of Events / moves / relink so he can plan a workflow.

## 1. The washed-out still — measured, not guessed

**The symptom pinned the mechanism before any code was read.** Colour correct
after play, and *not* washed out again on pause, is the signature of an HTML
`<video poster>`: the poster is shown only until the first frame is available and
never returns. So the defect had to be in the poster JPEG, not in playback, not
in CSS, and not in a paused state.

**Cause.** The owner's footage is `yuv420p(tv, bt709)`. JPEG/JFIF means BT.601,
and ffmpeg's mjpeg encoder **labels** its output `bt470bg` without converting the
coefficients. The webview honours the label, so it decoded BT.709 pixels with the
BT.601 matrix — a desaturated still. `<video>` then decoded the same clip
correctly, which is why colour "returned" on play.

**A wrong hypothesis, discarded by measurement.** The first theory was a
limited/full range mismatch (the usual cause of washed-out video stills). Adding
`in_range=auto:out_range=pc` produced a **byte-identical** file — swscale already
converts `yuv420p` → `yuvj420p` range on its own. The range theory was dead
before it could be written into the code.

**Verification.** Same frame decoded straight to RGB as ground truth (PNG has no
YCbCr ambiguity), then PSNR of each candidate against it, on a real 1080p60
Note 10+ clip:

| Poster command | PSNR vs RGB truth |
|---|---|
| current — label only | 38.36 dB |
| **`scale=…:out_color_matrix=bt470bg`** | **41.19 dB** |
| round-trip via `format=rgb24` | 38.97 dB |

Saturation confirms the direction: `SATAVG` 12.69 → 13.30.

**Fix.** `make_poster` asks swscale to genuinely convert the matrix. One filter
option; applies to both the 480 px grid poster and the 1280 px Focus poster.
(The grid looked "fine" only because a hue/saturation shift is far less visible
at 176 px than full-screen.)

**Known gap, deliberately not guessed at:** an HDR source (PQ/HLG — S23 Ultra,
Osmo Pocket 3) extracted without tone-mapping will still look flat, and would
look *much* worse than this. Every clip reachable for testing on this machine was
SDR `bt709`, so there was nothing to verify a tone-map chain against. The proven
`TONEMAP_CHAIN` in `commands.rs` is the piece to reuse when an HDR clip is
available. Shipping an unverified filter chain was the wrong trade.

## 2. Paused-state overlay

A centred play button over the picture whenever the video is paused, YouTube
style — the owner's read was right that a stopped clip looked like "a slightly
odd photo" rather than a paused video. Hidden while scrubbing or glimpsing, where
the user is watching frames and a button in the middle is in the way. It sits at
`z-index: 3`, below the scrub layers and the transport, so it never covers
anything interactive.

## 3. `docs/EVENTS-MOVES-AND-RELINK.md`

New doc covering Events, in-app moves and the relink pass — happy paths, the
awkward cases, and a blunt table of what does **not** work. The load-bearing
entries: a move+rename in one pass is unrecoverable by relink; cross-drive moves
lose marks entirely (per-drive catalogs); and *Forget* on a disconnected drive
would delete marks for files that are merely offline.

Also documents the three overlapping refresh actions that exist today, and
records two **proposals awaiting the owner's sign-off**: a CLI adapter on the
same binary so an external agent can report its moves/tags (the only thing that
can survive move+rename), and the Trash rework to a visible flat
`<drive>\FoxCull Trash` with provenance mirrored into a `_trash-index.json`.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src-tauri/src/video.rs` | logic | `make_poster` converts to BT.601 rather than mislabelling BT.709 as it. |
| `src/lib/components/Loupe.svelte` | UX | Paused-state play overlay + styles. |
| `docs/EVENTS-MOVES-AND-RELINK.md` | docs (new) | Workflow reference + two proposals. |
| `CLAUDE.md` | docs | Doc-map row for the above. |

## Behavior changes

- Video stills (grid and Focus) render with correct colour. **Existing cached
  posters keep the old colour** — the cache key is (path, mtime, size, max-edge)
  and none of those changed. They regenerate for any file that changes; a user
  who wants them all rebuilt now can clear `<drive>\_FoxCull\thumbs`.
- A paused video in Focus shows a play button over the frame.

## Risks / compatibility

- `out_color_matrix` is a swscale option present in every ffmpeg the project
  bundles; no new filter dependency (unlike zscale, which the tone-map path
  guards with a fallback for exactly that reason).
- The overlay is `z-index: 3`, deliberately under `.scrubStage`/`.liveScrub` (4–5)
  and the transport, and is suppressed during scrub/glimpse.

## Verification actually run

- `cargo check`: passed. `npm run check`: 0/0. `npm run build`: passed.
- Poster colour: three candidate ffmpeg command lines executed against real
  footage, each compared by PSNR to an RGB ground-truth decode of the same frame,
  plus `signalstats` saturation (table above). The range hypothesis was tested
  and falsified by byte-identical output before being discarded.
- **Device QA pending:** the overlay and the corrected posters need the built app,
  and posters only regenerate for files whose cache entry does not already exist.
