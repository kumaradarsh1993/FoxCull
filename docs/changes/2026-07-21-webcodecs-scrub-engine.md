# 2026-07-21 — Focus scrubbing decodes real frames (WebCodecs engine)

## Intent

Deliver Architecture C: replace sprite-sheet scrubbing in Focus view with
on-demand decoding of the actual frame under the cursor. Removes the
per-clip build, the low-resolution stand-ins, and the wait — dragging works the
moment a clip opens. Grid tiles keep sprites (a decoder per tile isn't viable).

## Modules touched

| File | Level | Change |
|---|---|---|
| `src/lib/scrub-engine.ts` | architecture | NEW. `ScrubEngine.open()` → moov-only mp4box index (all samples, keyframe list, rotation, hvcC/avcC description) + a persistent hardware `VideoDecoder`. `request(t, exact, cb)` with latest-wins coalescing; coarse = nearest keyframe, exact = GOP-forward decode to the real frame. `paintFrame()` handles container rotation + HiDPI sizing. `stats` exposes the last decode's read/flush timings. |
| `src/lib/components/Loupe.svelte` | architecture / UX | Engine opened per video on Focus open; full-stage `<canvas>` shows the decoded frame while dragging and holds until `<video>` lands on the same frame (`onseeked`); hover thumbnail decodes too; `previewBox` aspect now rotation-aware from the index; `applySeek` never touches `currentTime` while moving; sprite blocks kept as the fallback branch. |
| `src/lib/settings.svelte.ts` | logic | NEW `liveDecodeScrub` (default **true**). |
| `src/routes/+page.svelte` | UX | Settings: new "Focus scrub — Live decode / Sprites" row; existing Live Scrub relabelled "(grid tiles)" with an explanatory title. |
| `src/lib/scrub-probe.ts` | process | Now imports the engine's parser instead of a forked copy (a copy would stop testing the real thing), and gained phase 4: drives the real `ScrubEngine` through a simulated 24-request drag + an exact request, reporting what actually painted. |
| `docs/design/precache-policy.md` | architecture | New section "Focus view no longer pre-caches anything"; machine-readable block updated (`liveDecodeScrub`, filmstrip re-scoped to grid + fallback). |

## Behavior changes

- Focus scrubbing is full-resolution and immediate; no "scrub preview %" build.
- Nothing is written to the cache for Focus scrubbing.
- Live Scrub (grid) is unchanged and still default-off.
- Unsupported clip → silent per-clip fallback to the previous sprite behaviour,
  including the sprite build that was held back while indexing.

## Risks / compat

- Non-MP4/MOV containers and codecs WebCodecs rejects take the fallback path;
  H.264 (`avcC`) is handled but has not been exercised on a real phone clip yet.
- The `<video>` element remains the sole playback path, so audio, autoplay,
  trim/segments, EditStudio and export are untouched by design.
- `read_file_range` reads arbitrary paths from the frontend (same trust level as
  the existing loupe/thumbnail commands).

## Verification actually run

`npm run check` 0/0. `cargo check` clean. Engine driven end-to-end in
`tauri dev` against a real Osmo 4K60 HEVC Main 10 clip (546 s, on the P: HDD):

| | Before fixes | After |
|---|---|---|
| Frames painted during a 24-request simulated drag | **1** | **6**, spread across the clip (0 → 535 s), 31–78 ms each |
| Exact-frame decode on release | **4118 ms** (hit the 4 s timeout) | **147 ms** (read 79, flush 63) |
| Frame it landed on | — | 1 ms from the request; the keyframe alone would have been 185 ms off |
| Index open | 262 ms | 262 ms |

Two real bugs found by that self-test, both fixed and commented at the site:

1. **The picture froze while dragging.** `pump()` skipped painting whenever a
   newer request had arrived — which during a real drag is *always*, since
   requests arrive ~3× faster than a frame decodes. Now every decoded frame
   paints; being one step behind the cursor is the correct output.
2. **`flush()` deadlocked on multi-frame decodes.** Collecting a whole GOP's
   `VideoFrame`s before choosing starved the hardware decoder's output-texture
   pool (1 sample flushed in 13 ms; 12 samples timed out at 4 s, with reads
   measured at 112 ms ruling out I/O). Now only the best-matching frame is held
   — ≤2 alive at any moment, ~24 MB instead of ~144 MB.

Not yet verified on device: the Loupe wiring itself (canvas reveal, hand-off on
release, hover thumbnail) — that needs a hand on the mouse, hence the nightly.
