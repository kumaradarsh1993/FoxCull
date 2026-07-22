# 2026-07-22 — Edit mode: unbounded per-file fan-out on a 229-clip folder

## Intent

Owner opened Edit on **229 Osmo Pocket 3 4K60 HEVC clips, 2-15 minutes each**.
The WebView2 process group reached **7,940 MB** (renderer 4,557 MB, Utility
2,167 MB, Utility 1,202 MB) at 92% of system memory, and every item in the
source pane sat on "Reading details...". Called critical by the owner.

## What was proven, by reading

**1. EditStudio's source pane is not virtualized.** `{#each filteredSources as
item (item.path)}` (EditStudio.svelte:2320) with **zero** uses of
`VirtualGrid`/`VirtualStrip` in the file (`grep -c` = 0), while the library grid
in `+page.svelte` uses both. So Edit mounts **one `Thumb` per file** — 229 of
them at once. Each `Thumb` on mount fires `loadVideoPoster()` (an ffmpeg poster
extraction) plus `videoFilmstripCached` and `videoScrubstripCached` IPC calls:
**~687 calls and up to 229 ffmpeg invocations against multi-GB files**, none of
it gated on whether the tile is anywhere near the screen.

**2. The probe sweep was wrong in both directions at once.**
`for (const src of filteredSources.slice(0, 80)) ensureProbe(src)` — so:

- Items **81 through 229 were never probed at all**. `sourceSubline()`
  (EditStudio.svelte:890) renders "Reading details..." whenever
  `probes[src.path]` is absent, so for 149 of the 229 clips that string was
  **permanent, not slow**. This is the reported symptom, exactly.
- The first 80 fired ffmpeg on open, competing for the same disk with the 229
  poster extractions from (1) — which is why even those crawled.

## What was NOT proven — stated plainly

**The 8 GB is not attributed.** Renderer bytes (4.5 GB) are JS heap and DOM;
Utility bytes (3.3 GB across two processes) are Chromium's hardware video
decode. Those have different causes, and the fixes here address work that
provably should not have been happening rather than a measured allocation.

Two hypotheses were considered and neither could be confirmed by reading:
- ~20 MB/clip × 229 ≈ the renderer figure is arithmetically tidy but weak
  evidence; tidy ratios have misled this project before.
- The `ScrubEngine` (3 days old) was suspected because Utility processes are
  where decoders live, but `Thumb`'s engine is gated on `armed && hovering`
  and EditStudio passes no `armed` prop (defaults false), so no engine opens
  from this path. **This makes the scrub engine an unlikely source here, not a
  cleared one** — its lifecycle under virtualized-list recycling was never
  audited. Two subagents dispatched to investigate both angles independently
  died on an account spend limit before running.

Hence the instrumentation below rather than a claimed fix.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src/lib/components/Thumb.svelte` | architecture | NEW `IntersectionObserver` (`rootMargin: 400px`) sets `onScreen`; the load effect returns early while false, so no poster extraction and no cached-strip IPC happens for an off-screen tile. `onScreen` **latches** — a tile that has loaded never unloads, so scrolling doesn't thrash. No-op for the virtualized library grid; the whole fix for the unvirtualized Edit pane. |
| `src/lib/components/EditStudio.svelte` | architecture | Blind 80-item probe sweep replaced by a `probeOnView` action (`rootMargin: 200px`, disconnects after one hit) on each `.sourceItem`. Probes now happen for every item, but only when seen. |
| `src/lib/components/EditStudio.svelte` | process | NEW `edit-mem` log line via `api.logNote` on Edit open, again at +15 s, and on close, carrying source count and `performance.memory.usedJSHeapSize`. |

## Behavior changes

- Opening Edit does bounded work proportional to what is visible, not to folder
  size.
- Every source eventually shows real details; nothing is permanently stuck.
- `foxcull.log` gains `UI edit-mem open|open+15s|close sources=N heap=NMB`.

## Risks / compat

- `onScreen` latching means a very long session scrolling the whole 229-item
  pane still ends with 229 loaded tiles. This bounds the *rate*, not the
  ceiling. Real virtualization of the source pane is the actual fix and is not
  done here.
- `probeOnView` disconnects after firing once; if a probe fails, it is not
  retried until the pane is rebuilt. Previously the sweep also never retried.
- `performance.memory` is Chromium-only and heap-only — it will not show the
  Utility-process decode memory, which is precisely the part still unexplained.

## Verification actually run

- `npm run check` 0 errors / 0 warnings. `npm run build` clean.
- Static reading only. **Nothing here was verified against a running app**, and
  the memory hypothesis is explicitly unconfirmed — the instrumentation exists
  because of that, not as decoration. The next Edit-mode open on the owner's
  229-clip folder produces the numbers that settle it.
