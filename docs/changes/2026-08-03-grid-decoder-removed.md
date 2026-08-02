# 2026-08-03 — Remove the WebCodecs decoder from grid tiles (v1.2.0 scroll-freeze regression)

## Intent

Root-cause and fix the fast-scroll / held-arrow **freeze** (viewport goes black,
whole app + cursor frozen, needs relaunch). The owner bisected it precisely
against installed stables: **v1.0.1 and v1.1.0 are clean** (fast scroll and fast
arrow-key nav both lazy-load smoothly); **v1.2.0 introduced it.** v1.2.0 was the
video-playback overhaul. This change targets the specific piece of that overhaul
that reached the grid scroll hot path.

## RCA — what v1.2.0 changed in the grid, and why it freezes

The overhaul was scoped, in the DECIDED architecture
(`docs/design/video-player-migration.md` §2, §10), to **Focus view only**:

> "Sprites stay for grid tiles (a decoder per tile is not a thing), exactly per
> the owner's scoping."

But a later commit in the same release — `93b9328` *"Glimpse, grid skim on the
decoder, and the end of video pre-caching"* — extended the WebCodecs
`ScrubEngine` **into grid tiles** (an armed+hovered tile opened its own
`VideoDecoder` and painted frames into a per-tile `<canvas>`), and
`a2dcf6b` fixed the self-invalidation bug that had kept it from ever opening — so
in v1.2.0 stable the grid decoder is live. `liveDecodeScrub` also defaults **on**.
This went out without a fast-scroll test (owner: *"there was much thought… but I
never got to test the scroll thing"*).

Why it stalls the compositor (matches every measured symptom — GPU **idle**,
WebView2 handle count **+16k**, RAM +1 GB, 8–12 s black freeze, input dead):

- A live `<canvas>` is its **own WebView2 GPU compositor layer**, and
  `VideoDecoder.configure()` pins **D3D11 / Media-Foundation** resources (texture
  pools, device references = kernel handles).
- Arming/hovering tiles is not a rare event during navigation: a fast wheel fling
  with the pointer over the grid, and especially **held arrow-key nav** (each
  step moves the armed tile), repeatedly opened/closed decoders and mounted/unmounted
  canvases. WebCodecs teardown is async, so `close()` does not free the GPU
  resources immediately — rapid churn accumulates layers + handles faster than
  they drain, until the compositor surface blacks out. GPU stays idle because
  nothing is *decoding for playback* — it is a **pipeline/resource stall, not a
  GPU-compute one**, exactly as the live monitor showed.

This is consistent with photos being *less* affected (photo tiles never open a
decoder or a canvas) and with the freeze worsening as DOM recycling (v1.4.0) made
tile arming/hover churn during scroll more frequent.

## The fix

Return the grid to the **decided** architecture: **WebCodecs lives in Focus view
only** (`Loupe.svelte`, untouched — that is where live scrubbing was proven on
4K60 HEVC and is the feature the owner values). Grid tiles go back to
poster + optional pre-built sprite skim, exactly as v1.1.0 when fast scroll was
smooth.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src/lib/components/Thumb.svelte` | architecture | Removed the grid live-decode path entirely: `ScrubEngine`/`paintFrame` import, `tileEngine`/`tileReady`/`tilePending`/`tileCanvas`/`tilePainted` state, the decoder-open `$effect`, `paintTile()`, `closeTileEngine()`, the `<canvas class="scrubLayer live">` element and its CSS. `canSkim` is now just `settings.s.liveScrub`; the sprite-build effect and template lose their decoder guards. Focus is unaffected. |
| `src/lib/components/Thumb.svelte` | logic | The item-load effect now also resets `hovering = false` on item change, so a **recycled** slot cannot carry a stale pointer-hover into the new item it was repointed to (which would auto-arm skim work mid-scroll). |
| `src/lib/thumbnail-loader.ts` | logic | `jobReport()` returns early while `scrolling` is true — the enqueue/cancel churn no longer fires a reactive `activity` store write per tile during a fling (hundreds/sec previously; also the source of the "36% → 28% going backwards" progress whipsaw). `setScrolling(false)` calls `jobReport()` once on settle so the chip catches up. |

`liveDecodeScrub` remains a setting (it still governs Focus scrub: live-decode vs
sprites) — it is simply no longer read by the grid. No settings UI change: the
toggle is already labelled "Focus scrub".

## Behavior changes

- Grid tiles no longer skim by live-decoding frames. Skimming an **armed +
  hovered** grid clip works via the pre-built sprite sheet only when **Live
  Scrub** (sprite fallback) is enabled — the v1.1.0 behavior. Focus-view
  scrubbing (drag the timeline) is **unchanged**: full-resolution live decode.
- No progress-chip flicker/whipsaw during a fast scroll.

## Risks / compat

- Owners who enabled the default live-decode grid skim lose the *grid* live
  decode (Focus keeps it). This is the intended trade to kill the freeze and
  matches the original scoping; grid skim can be re-introduced later off the
  scroll hot path if wanted.
- Hypothesis-driven: the RCA is strongly supported by the diff + every measured
  symptom, but the freeze is a device-side compositor effect that cannot be
  reproduced by the local gate. **Confirmation is the owner's next fast-scroll
  test** (see the test plan in `FREEZE-INVESTIGATION-2026-08-02.md`).

## Verification actually run

- `npm run check` — 0 errors / 0 warnings.
- No Rust changed (`cargo check` not required).
- Grepped `Thumb.svelte` for every removed symbol: only the explanatory comment
  mentions `ScrubEngine` now.
