# 2026-08-02 — Cell recycling in VirtualGrid (rework/virtual-recycler)

Branch `rework/virtual-recycler`, test tag `v1.3.0-recycler.1`. First step of the
audited rework (Aamir, 2026-08-02): replace the four hand-rolled virtualizers'
destroy-and-recreate model with cell recycling. This commit does the **main grid
only**; SectionedGrid / VirtualStrip / DetailsView still use the nightly.9 gate
and will be migrated once the grid is confirmed on device.

## Why (measured)

nightly.9 proved the placeholder gate is the wrong layer: with the gate firing
(`grid-gate ON` in the log) a fast scroll STILL produced `PAINT-RESUME
gap=7544ms` — a 7.5 s compositor stall — and the gate's mass skeleton↔real swap
was itself the "blank everything then reload" flicker the owner saw. Root cause
(from the audit + captures): a window shift keyed by item index destroys and
recreates all ~108 heavy cells at once; the burst chokes WebView2's compositor
(handles spike ~18k, GPU idle — a pipeline stall).

## What changed

`src/lib/components/VirtualGrid.svelte` — recycling rewrite, **same public API**
(`+page.svelte` untouched):

- A grow-only pool of cell slots; item `i` always renders in slot `i % poolSize`,
  with `poolSize` sized to `(rowsInView + 2·overscan + 2)·cols` so the pool
  exceeds the visible window and at most one visible item maps to any slot.
- `{#each slotItem as idx, s (s)}` keyed by **slot number** (stable), not item
  index. On any re-window — small scroll OR big jump — the slots shared by the
  old and new windows (~poolSize − 2·cols of them) go valid→valid, so Svelte
  updates the existing `Thumb`'s `item` prop **in place** (recycle, no
  mount/unmount). Only ~2·cols edge slots toggle. Bounded churn per re-window
  regardless of jump distance (verified by the modulo arithmetic), vs. the old
  full ~108-cell teardown/rebuild.
- The nightly.8/.9 `isScrolling` placeholder gate is **removed** from the grid —
  recycling makes it unnecessary, and its removal deletes the blank/reload
  flicker. Tiles that stay visible keep their pixels; only genuinely new tiles
  lazy-load (loader still caps 6 inflight, so no load storm on a big jump).
- Scroll-position handling unchanged (direct read + 160 ms settle; no rAF
  dependency). `scrollToIndex` / `columnCount` unchanged. `Thumb` unchanged — it
  already resets on `mediaLoadKey`, so a recycled slot cleanly cancels the old
  item's load and starts the new one.

## Expected on-device behavior

- **Main grid** fast scroll: no 10 s freeze, no blank/reload flash; tiles fill in
  as you settle.
- Filmstrip (Focus) and grouped grid: **still on the old gate** — may still
  stall; don't judge those yet. Details view unchanged.

## Verification

- `npm run check`: 0/0. (No Rust changed; `cargo check` not required for this
  step — occlusion/log fixes already validated on nightly.9, which this branch
  includes.)
- Device QA is the test: fast-scroll the MAIN GRID on the 6k folder with the
  monitor running; confirm `raf=` no longer gaps and `grid-scroll pool=…` lines
  show a stable pool.

## Addendum — all four surfaces recycled (v1.4.0-nightly.1)

Following the plan in `docs/design/rendering-rework-2026-08.md` and the owner's
call (work on `main`, bump base to 1.4.0, single build), the recycler was applied
to the remaining surfaces and the placeholder gate removed everywhere:

- `SectionedGrid.svelte` — cells recycle by `gi % poolSize`; section headers stay
  rendered directly (few, image-free). `sectioned-grid-scroll … pool=` logging.
- `VirtualStrip.svelte` — 1-D recycling (both orientations); the blank-placeholder
  gate is gone; native/smooth wheel + active-cell reveal unchanged.
- `DetailsView.svelte` — rows recycle by `index % poolSize` and are positioned
  with `top` instead of a CSS `transform` (dropping a per-row compositor layer).

All keep the same public APIs, so `+page.svelte`, `Thumb.svelte`, the loader and
folder-switch cancellation are untouched. `npm run check` 0/0. Base version bumped
to **1.4.0**; shipped as `v1.4.0-nightly.1` for full-device QA per the checklist in
the design doc (fast grid, slow multi-screen, filmstrip fling, grouped grid,
details, live-scrub, folder switch, small/RAW/HEIC folders).
