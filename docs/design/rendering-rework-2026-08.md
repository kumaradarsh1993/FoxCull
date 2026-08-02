# Media rendering rework — cell recycling (2026-08)

**Status:** in progress · **Owner ask (Aamir, 2026-08-02):** stop patching the
scroll freeze; fix it at the root with a modern, simpler, more performant design
that matches August-2026 industry standard for this stack (Tauri 2 + SvelteKit 2
+ Svelte 5 runes). No visual "refresh/blank" bloat. Unifying the grid/focus/strip
experience is welcome. This doc is the researched, adversarially-checked plan; the
implementation follows it.

## 1. The problem, proven

Fast scrolling (and sometimes slow scrolling past the second screen, and the
Focus filmstrip) froze the app: paint dead 7–12 s (`raf` stops), WebView2 handle
count spiking ~20k→38k and RAM +900 MB then draining, **GPU idle at 4–7 %
throughout**, process responsive. Two live monitored captures (photos and videos)
plus the app's own `raf=`/`grid-gate` log lines established this is a **WebView2
compositor-pipeline stall**, not GPU/TDR and not a JS or backend deadlock. A
separate idle/on-launch freeze was the WebView2 native-window-occlusion misfire
(fixed in nightly.9 by `--disable-features=CalculateNativeWinOcclusion`).

Root cause of the scroll freeze: each of the four render surfaces
(`VirtualGrid`, `SectionedGrid`, `VirtualStrip`, `DetailsView`) is a hand-rolled
virtualizer that keys its `{#each}` by **item index**, so every window shift
**destroys and recreates all ~108 visible cells** — each a heavy `Thumb`
(image fetch + `ResizeObserver` + live-decode engine). The burst of
teardown/rebuild overwhelms the compositor. nightly.8/.9's placeholder gate only
*relocated* the burst (to the settle) and added a blank/reload flicker; nightly.9
logs prove the freeze survived with the gate on.

## 2. Industry standard (August 2026) — what modern grids actually do

Verified against current sources (see §7): every serious large-list/grid
implementation — AG Grid, TanStack Virtual, react-virtuoso, Wijmo — uses **DOM
node recycling**: keep a small pool of DOM nodes sized to the viewport, and as the
user scrolls **reposition and repoint existing nodes to the next data window**
instead of creating/destroying them. "Render cost stays closer to viewport size
than dataset size." `content-visibility: auto` (Baseline 2025) is a *complementary*
paint-skipping optimization for when many off-screen nodes exist — irrelevant once
windowing already bounds mounted nodes, so we do **not** add it now (no change
without data; avoids bloat).

Decision (confirmed with owner): an **in-house recycling primitive**, not a
library. Reasons: fixed-size media cells make the math simple; no dependency in a
Tauri app; and the recycler must integrate tightly with `Thumb`'s live-scrub
arm/hover and the existing loader/cancellation — all of which we own.

## 3. Target architecture

**One recycling technique, applied to every surface** (and, where the owner is
flexible, converging their behavior):

- A **grow-only pool of cell slots**. Item `i` always renders in slot
  `i % poolSize`, `poolSize` sized to `(rowsInView + 2·overscan + 2)·cols` so the
  pool exceeds the visible window and at most one visible item maps to a slot.
- `{#each … as …, s (s)}` keyed by **slot number**. On any re-window (small scroll
  or big jump) the slots shared by old and new windows go valid→valid and Svelte
  updates the existing `Thumb`'s `item` prop **in place** (recycle); only the
  ~2·cols edge slots toggle. Churn per re-window is **bounded and independent of
  jump distance** — the modulo arithmetic guarantees it.
- **No placeholder/blank gate.** Tiles that remain visible keep their pixels;
  only genuinely-new tiles lazy-load through the existing 6-inflight loader (so no
  load storm, no flicker). This is the "no refresh bloat" the owner asked for.
- Everything else is preserved unchanged: the per-drive disk thumbnail cache and
  its keying, DCT-scaled decode, embedded-thumb fast path, HEIC→ffmpeg, RAW
  embedded-JPEG, ICC, the LIFO loader, folder-switch cancellation, and the
  live-scrub arm→hover→pointer feature.

Surfaces:
- `VirtualGrid` — uniform grid; slot = `i % pool`, position = row/col. **Done.**
- `SectionedGrid` — headers stay rendered as-is (few, cheap, image-free); the
  cells recycle by the same modulo rule, positioned by their row `y`.
- `VirtualStrip` — 1-D (horizontal/vertical); slot = `i % pool`, position along
  the axis. Keep native/smooth scroll.
- `DetailsView` — rows recycle by the same rule; drop the CSS `transform`
  row-positioning for plain `top` (consistency with the grids; transforms were a
  compositor-layer smell).

## 4. Adversarial check (stress-testing my own audit)

- **"Is recycling really the root fix, or a second guess?"** The captures show the
  burst→stall causally; recycling removes the burst. But it is **not yet
  device-proven** — that is exactly what the `v1.3.0-recycler.1` grid-only build
  tests. Falsifier: if the main grid *still* freezes with recycling, the burst was
  not the (sole) cause and the next suspects are the Tauri **asset-protocol**
  fetch path and image decode/upload volume. The `raf=`/`grid-scroll pool=` logs
  will show it either way. I will not call this fixed until the log shows no `raf`
  gap on a hard fling.
- **"Could recycling introduce correctness bugs?"** Yes — the real risks:
  (a) a recycled `Thumb` must fully reset for its new item — it already keys off
  `mediaLoadKey` and its effect cleanup cancels the old load, so this is covered,
  but must be verified; (b) live-scrub arm/hover on a recycled tile; (c) active-
  cell highlight and keyboard nav landing on the right slot; (d) folder switch and
  small folders (many `-1` slots). All are on the QA checklist below.
- **"Does the modulo mapping degrade on a huge jump?"** No — worked example:
  pool 117, cols 9, window 108; jumping from items [0..107] to [900..1007] leaves
  99 slots valid→valid (recycled in place) and toggles only 9+9 at the edges.
  Bounded regardless of distance.
- **"Why not just TanStack Virtual?"** It is excellent and would work, but adds a
  dependency, its Svelte-5 adapter is newer, and fitting the live-scrub/sprite/
  loader machinery into a headless external virtualizer is more integration risk
  than a ~120-line in-house recycler we fully control. Revisit only if we later
  need variable-size rows or advanced features.
- **"Does this add bloat?"** It removes code (the placeholder gate) and the
  blank/reload flicker. Net simpler.

## 5. Migration & rollback

Incremental, each step gated by `npm run check` + a device fling test with the
process monitor + `raf=` logging running:
1. `VirtualGrid` (done, shipped as `v1.3.0-recycler.1` for isolated grid QA).
2. `SectionedGrid`, `VirtualStrip`, `DetailsView`.
3. Base version bumped to **1.4.0**; first full build `v1.4.0-nightly.1`.

Rollback: nightly.9 (`v1.3.0-nightly.9`) remains a tagged, working-except-freeze
fallback. Each surface's change is self-contained.

## 6. QA checklist (device)

- Hard fling main grid on 6k folder → no `raf` gap, no freeze, no blank flash.
- Slow scroll past several screens → no accumulation freeze.
- Focus filmstrip hard fling → no freeze.
- Grouped (month/type) grid → recycles correctly, headers correct.
- Details view fast scroll → no freeze, rows correct.
- Live-scrub: click a clip to arm, hover, drag pointer → frames still track.
- Folder switch mid-load → old work cancels, new folder enumerates, no stuck job.
- Small folder (< one screen) and RAW/HEIC/video folders render correctly.

## 7. Sources

- TanStack Virtual (headless recycling; Svelte adapter): https://tanstack.com/virtual/latest/docs/introduction
- AG Grid DOM virtualisation (node recycling): https://www.ag-grid.com/javascript-data-grid/dom-virtualisation/
- react-virtuoso — offloading virtualization via CSS containment: https://github.com/petyosi/react-virtuoso/discussions/959
- Frontend virtualization overview (recycle DOM nodes, render viewport-size): https://dev.to/zeeshanali0704/frontend-system-design-virtualization-handling-large-data-sets-29nf
