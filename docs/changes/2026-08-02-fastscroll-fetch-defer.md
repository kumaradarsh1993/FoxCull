# 2026-08-02 — Defer thumbnail fetches during a fast fling (v1.4.0-nightly.2)

The remaining scroll freeze, fixed at the loader.

## Why

With cell recycling in place (v1.4.0-nightly.1) a fast scroll STILL froze paint
8.5 s with the SAME +16k WebView2 handle / +1.1 GB spike (GPU idle) — proving the
DOM was never the cause. The owner spotted the tell: the freeze coincides with the
"Loading thumbnails" activity, whose percentage even runs backwards as scrolling
enqueues faster than it completes. Root cause: a fast fling rapidly repoints
recycled tiles' `<img src>` (each an asset-protocol fetch + Chromium decode);
memoized/cached URLs churn fastest of all. The fetch/decode burst is what spikes
handles and stalls the compositor. `warm_thumbnails` was ruled out — it runs once
per folder-open, images-only, so it is a no-op on the video folders that froze.

## What changed (`src/lib/thumbnail-loader.ts` + the three virtualizers)

- `thumbnail-loader.ts`: new module flag `scrolling` + `setScrolling(v)`. `pump()`
  returns early while `scrolling` — no fetch dispatch during a fling. The memo
  fast-path in `enqueue` is bypassed while `scrolling` (cached URLs are queued too,
  since instant resolution IS the churn), and `run()` re-checks the memo so a
  deferred cached hit resolves instantly with no fetch the moment we settle.
  Turning `scrolling` off calls `pump()` to drain the settled viewport.
  `resetThumbs()` clears the flag (a folder switch is never "still flinging").
- `VirtualGrid` / `SectionedGrid` / `VirtualStrip`: `onScroll` measures velocity
  (single-jump `delta` or fast cumulative `recentMove`) and calls
  `setScrolling(true)` on a fast fling, `setScrolling(false)` on the 160 ms settle.
  Cleanup effects call `setScrolling(false)` so unmounting mid-fling can never
  leave the loader paused.

## Behavior

- Fast fling: no new image fetches issued; already-visible tiles keep their
  pixels (recycling); new tiles are neutral until you settle (~160 ms), then fill
  — cached ones instantly. No blank-flash of staying tiles, no bulk mount/unmount.
- Gentle/normal scroll: `scrolling` never trips, loads happen live as before.
- No Thumb changes → live-scrub (arm→hover→pointer) untouched. Backend, disk
  cache, RAW/HEIC, folder-switch cancellation all untouched.

## Risk / rollback

- If the flag were stuck true, loads would stall — guarded three ways (settle
  timer, unmount cleanup, `resetThumbs`). Falsifier if the freeze persists: the
  handle spike would then be coming from something other than image fetches (e.g.
  the per-tile cached-filmstrip probe IPCs in `Thumb`, still un-gated) — the
  monitor + `raf=` log will show it. Rollback: `v1.4.0-nightly.1`.

## Verification

- `npm run check` 0/0. Device QA: hard-fling the 6k video folder with the monitor
  running — expect NO `raf` gap and WebView2 handles to stay near the ~20k baseline
  instead of spiking to ~36k.
