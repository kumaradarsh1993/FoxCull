# Scroll-freeze — consolidated handover (2026-08-03)

**Read this first if you are picking up the FoxCull scroll-freeze.** It is
self-contained: the full issue evolution, every nightly tried and its on-device
result, the RCAs (including the wrong ones), what is ruled out, and the live leads.
Supersedes the running narrative in `FREEZE-INVESTIGATION-2026-08-02.md` (still
useful for the Mode-B occlusion fix and the measurement tooling).

**Status: RESOLVED IN SOURCE (v1.4.0-nightly.7 candidate).** The grid
live-decode feature was wrongly removed and remains restored. Do not remove it
again. The fix and native stress evidence are in §R below.

> ## ⚠ READ §0 FIRST — the RCA was inverted on 2026-08-03 (late)
>
> Everything below §0 that says *"the compositor stalled while JS kept running"*
> is **WRONG** and is kept only as investigation history. Detailed owner symptom
> reporting on nightly.4/.5 proves the opposite: **the JS main thread is blocked;
> the renderer is healthy.** Start at §0.

---

## R. Resolution — bound the loader's main-thread work (2026-08-03)

The bisection and corrected symptoms converge on the thumbnail loader, not the
decoder or compositor:

1. `v1.1.0` was clean.
2. `dabfb9a` in the v1.2 line added on-demand thumbnail progress. Once visible,
   `jobReport()` wrote the reactive activity store on every enqueue, cancel and
   completion. A fast traversal made the app re-render a progress value hundreds
   of times for a denominator that changed under it — the reported backwards
   percentage.
3. The later fling gate reduced mid-scroll work but accumulated it. On settle,
   the old recursive `pump()` synchronously resolved every memo hit in the queue
   (up to 240), causing one turn to contain the activity writes, promise
   continuations and `<img src>` assignments together. That is the long JS task;
   the handle/RAM spike is its downstream resource burst.
4. Cancellation also searched/spliced the array per tile, and removed queued
   work without settling its promise. Those were amplifiers and correctness
   hazards even when they were not the original regression.

The replacement has hard structural bounds:

- live queued work is indexed by key, so cancellation is O(1) and always settles
  the waiter;
- the priority array is a compacted tombstone log, capped at 240 live requests;
- dispatch is frame-paced: at most 12 cached assignments (one grid row) per
  paint, never a recursive settle burst;
- real backend work stays capped at 6 total / 2 heavy;
- "Loading thumbnails" is indeterminate and writes the reactive store only on
  start/end transitions, never per tile.

**Native verification on the real 6,825-item F: library:** a temporary dev-only
probe drove 800 three-row jumps at a 4 ms cadence, then was removed. There was no
`PAINT-RESUME` gap. Peak logged loader state after the traversal was 43 queued +
6 in flight; it drained to zero. JS heap fell 92 → 47 MB after drain. The FoxCull
WebView2 tree settled at ~332 MB private / ~3,270 handles (not the previous +1 GB
/ +16k spike), the activity indicator cleared, every tile populated, and a real
Right-arrow action immediately moved selection from item 312 to 313.

---

## 0. The corrected model — the JS main thread is BLOCKED (2026-08-03, late)

### The observation that settles it

During a freeze the owner reports he CAN still:

- **scroll** the grid with the wheel, and with ↑/↓ (the view really moves),
- get **hover highlights** on toolbar buttons, folder-tree rows, thumbnails,
- get **native tooltips** on hover.

…and CANNOT:

- **click anything** (buttons highlight, tooltips appear, click does nothing —
  Edit, Clear, Reject, folder tree right-click, filmstrip: all dead),
- **move the blue selection** (the outline stays on one tile while arrow keys
  are pressed),
- **populate tiles** (they never fill in, even after motion stops),
- **advance the loading chip** (it sticks, and had been counting backwards).

**The split is exactly "needs JavaScript" vs "does not".** Native scrolling of an
`overflow-y:auto` div, CSS `:hover`, and `title` tooltips are all done by the
browser **without the main thread**. Clicks, Svelte state→DOM updates, tile
loading and the activity chip **all require the main thread**.

**Conclusion: the JS main thread is blocked/saturated. The compositor and the GPU
are FINE.** The renderer keeps compositing the last DOM it was given, which is why
the frozen picture is still interactive-looking.

### This also re-reads the older "proof" correctly

The earlier signature — rAF stops 7–12 s (`PAINT-RESUME gap=…`) while 20 s
`MEM tick` lines keep appearing — was read as "compositor dead, JS alive." That
was wrong: **a blocked main thread stops rAF *and* `setInterval` alike**; a 20 s
interval simply resumes afterwards and prints, so its survival proves nothing.
The rAF gap is a direct measure of **main-thread block duration**. GPU idle
(1–14%) fits this perfectly — nothing is wrong with the GPU because nothing is
being asked of it.

### The 12 rows × 9 columns constant

Consistently exactly **12 rows × 9 cols** of tiles remain, "stuck in the middle,"
blank above and below; this number appeared in the ORIGINAL bug report too (i.e.
it predates the v1.4.0 recycler). That is simply **the last virtual window JS
managed to render**: 8 visible rows + 2 overscan rows top and bottom = 12. Cells
are absolutely positioned inside a tall canvas, so once JS stops updating them,
native scrolling reveals empty canvas everywhere else. It is a **symptom of the
main-thread block, not a virtualization bug** — and it explains why the "dead
zone" and the "freeze" are the *same event*, as the owner suspected.

### Consequences for where to look

Stop looking for GPU-layer / compositor-resource explanations (that is what sent
nightly.4 after the grid decoder — which the on-device test then disproved). Look
for **what saturates or blocks the main thread during rapid scroll**, e.g.:

- an unbounded/very large synchronous burst of work per scroll event,
- unbounded queue/map growth with O(n) scans per tile repoint,
- a burst of hundreds of `<img>` src changes resolving at once (decode +
  `onload` handlers + layout on the main thread),
- reactive-store write storms (one `$state` write per queued/cancelled fetch)
  driving derived recomputation and re-render on every event,
- memory pressure (+1 GB observed) pushing the renderer into GC thrash.

The **+16k handle** spike is then a *consequence* of the fetch/decode burst, not
the mechanism of the freeze.

### A gate that can latch (introduced by this investigation — fix it)

`thumbnail-loader.ts`'s fast-fling gate (`setScrolling`) holds ALL dispatch while
`scrolling === true`, and is cleared only by `VirtualGrid.onScroll`'s 160 ms
settle `setTimeout`. **If the main thread is blocked, that timer never runs, so
the gate latches ON permanently** — after which no thumbnail ever loads again for
the rest of the session, and the activity chip sticks. That precisely matches
"tiles don't populate any more, loader stuck" persisting after the freeze. This
is a v1.4.0 (this investigation's) amplifier, not the v1.2.0 root cause, but it
converts a recoverable stall into a permanent dead grid and must be made
un-latchable (absolute max defer, cleared on any settle path).

### Owner's product constraint restated (drives the fix)

Scrolling must **never** change the selected tile (only click / arrow keys do) —
already true in `VirtualGrid` (scroll updates `scrollTop` only). And heavy
per-tile work must wait for a **deliberate dwell**, so fast pass-through mounts
nothing. He also asks the bigger question: Windows Explorer lazy-loads the same
1000+ clip folder with free fast scrolling and no visible caching — so the whole
thumbnail/caching model is open to being simplified toward that.

### Reported alongside (unconfirmed, likely same cause)

A clicked selection appearing "retained"/stale. Consistent with the frozen DOM
(the `class:active` outline is whatever JS last painted). Owner mentioned
screenshots; **none arrived in the session** — re-request if needed.

---

## 1. The bug, in the owner's words

Two distinct symptoms, both on a **video** folder and (at least in current builds)
photo folders, triggered by **fast wheel-scroll** or **holding the ↓ arrow**:

1. **Freeze.** Viewport goes black/blank, the whole app + the mouse cursor lock
   up; sometimes needs a relaunch. Slow/gentle scrolling is fine.
2. **Blank/dead zone + stuck tiles** (the sharpest, newest clue — 2026-08-03).
   Scrolling into fresh territory, all tiles **vanish** (just the theme
   background). Scroll back up and you see ~8×9 tiles again; scroll further and
   it's blank again — "the tiles get stuck in the middle." The left-side loading
   bar sticks, and thumbnails don't refresh with images even after settling.

**Bisection (owner installed the actual stable builds and tested):**
- **v1.0.1 — CLEAN.** Fast scroll and fast arrow-key nav both lazy-load smoothly,
  no freeze, no blank zone.
- **v1.1.0 — CLEAN.** Same.
- **v1.2.0 — BROKEN.** The freeze begins here. v1.2.0 = the video-playback
  overhaul.

So it is a genuine **regression introduced in v1.2.0**, and v1.1.0 is the known-good
reference for how grid scrolling should behave.

---

## 2. What v1.2.0 changed (the suspect surface)

v1.2.0 replaced the pre-built **sprite-sheet** scrub system with a **WebCodecs
live-decode** engine (`docs/design/video-player-migration.md`, Architecture C).
The `git diff v1.1.0..v1.2.0` hot-path changes:

| Area | Change | Notes |
|---|---|---|
| `scrub-engine.ts` (new) | WebCodecs `VideoDecoder` scrub engine | Focus + grid |
| `Loupe.svelte` | Focus view scrubs live (its own engine) | **The feature the owner loves — do not touch.** |
| `Thumb.svelte` | Armed+hovered **grid** tile opens its own decoder + `<canvas>` | Intentional extension (see §5) |
| `Thumb.svelte` | per-tile `IntersectionObserver` (`onScreen` gate) | later gated off for the grid via `deferUntilVisible=false` |
| `Thumb.svelte` | per-video-tile `videoFilmstripCached` IPC probe | later gated behind `liveScrub` (P8) |
| `thumbnail-loader.ts` | `activity` progress reporting per enqueue/cancel/finish | reactive-store churn on the hot path |
| `settings.svelte.ts` | `liveDecodeScrub` default **true** | governs Focus + (formerly) grid |
| backend `commands.rs` | `read_file_range` (binary IPC) | scrub-engine I/O |
| backend `video.rs` | poster `at_s` param; Prepare no longer builds filmstrips | less work |

The owner's own recollection of the whole caching evolution: images needed
thumbnail caching → then video sprites → sprites were slow/"shitty" overhead → the
1.2 overhaul moved to live decode and **removed the sprite build entirely**, which
"worked magically." He now questions whether ANY of the caching is needed (see §6).

---

## 3. Nightlies tried, and their on-device results

Numbers are the `v1.3.x` / `v1.4.0-nightly.N` line worked during the investigation.

| Build | What it tried | On-device result |
|---|---|---|
| v1.3.0-nightly.9 | **Mode B fix:** `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--disable-features=CalculateNativeWinOcclusion` (`lib.rs`) for the idle/on-launch paint stall | **FIXED** the idle/launch freeze. Confirmed. (Separate from the scroll freeze.) |
| v1.4.0-nightly.1/.2 | DOM **cell recycling** across grid/grouped-grid/strip/details; loader **`setScrolling` fetch-deferral** gate (hold image fetches during a fling) | Scroll freeze **persisted**. |
| v1.4.0-nightly.3 | Audit safe-batch: P8 gated the per-tile `videoFilmstripCached` probe behind `liveScrub`; video trim/concat/trash fixes | Scroll freeze **persisted**, incl. on keyboard nav. |
| v1.4.0-nightly.4 | **Removed the grid WebCodecs decoder + `<canvas>` entirely** (kept Focus). Hypothesis: decoder/canvas GPU-layer + handle churn was the freeze | **DISPROVED IT.** Freeze STILL happened (black zone, then recovered enough to scrub). Also the blank-zone/stuck-tiles symptom present. → **The grid decoder is NOT the cause.** |
| v1.4.0-nightly.5 | **Restored** grid scrubbing (owner relies on it) + guards: `DECODER_DWELL_MS=320`, refuse to open while `isScrolling()`. Kept loader activity-chip quieting during flings | current build; freeze still open |
| v1.4.0-nightly.7 candidate | O(1) cancellable live queue; frame-paced settle (12 assignments/paint); activity writes only on start/end | **Native 6,825-item stress passed**; §R. |

---

## 4. RCAs — including the wrong ones (keep, so nobody re-runs them)

- **WRONG #1 — "not a regression" (early).** Concluded from "reproduces on v1.2.1
  too." True but mis-scoped: v1.2.1 is *after* the overhaul. The clean line is
  v1.1.0, which wasn't tested until the owner bisected. It **is** a regression.
- **WRONG #2 — GPU/TDR.** The live monitor showed the GPU **idle (1–14%)** during
  the freeze. Not a TDR / GPU-compute stall. Abandoned `--disable-gpu`.
- **WRONG #3 — DOM destroy/recreate churn.** Recycling removed all mount/unmount
  churn; freeze persisted with the same handle spike. So it is not DOM node churn.
- **WRONG #4 — the grid WebCodecs decoder + canvas.** The strongest-looking
  theory (a live `<canvas>` is its own WebView2 GPU layer; `VideoDecoder.configure`
  pins D3D11/MF handles; navigation churns them). **nightly.4 removed it and the
  freeze remained** → disproved on device. Feature restored.

**Measured signature (still the best evidence):** on a fast scroll the app's rAF
paint heartbeat stops for **7–12 s** (`PAINT-RESUME gap=…ms`) while the JS thread's
`MEM tick` lines keep coming — i.e. **the compositor died while JS lived**.
WebView2/`foxcull` **handle count spikes ~+16k** and RAM ~+1 GB during the stall;
GPU idle throughout. Usually drains and recovers; sometimes needs a relaunch.

---

## 5. The grid live-decode feature — scope it correctly, do NOT remove it

**This was intentional and is valued.** Sequence per the owner: the overhaul first
solved Focus-view fast seeking; he then asked to extend the **same** live-decode
mechanism to the **grid** so the slow, overhead-heavy sprite pre-caching could be
dropped entirely. It was, and "it worked magically" — full-res grid skimming, no
sprites. (The original `video-player-migration.md` §2/§10 says "decoder per grid
tile is not a thing" — that doc is **stale**; the decision was later changed and the
doc never updated. Don't anchor on it.)

**The exact UX policy the owner wants (confirmed 2026-08-03):**
- There is always one **selected** tile (like any file browser). Selection moves
  **only** on click or arrow key — **never on scroll**.
- Scrub (Final-Cut-style live seek) is available **only on the selected tile**,
  **when hovered**. Explicitly NOT "scrub wherever the mouse goes." The code
  already implements this (`armed && hovering`); it is correct, not mis-built.

**Guards now in place (nightly.5):** decoder opens only after a 320 ms dwell and
only when `!isScrolling()`. Rationale: fast scroll / held-arrow sweep the armed
tile past the pointer well under the dwell, so the effect re-runs and clears the
timer before it fires — navigation never spins a decoder up, deliberate skimming
does.

---

## 6. Owner product direction (design constraints for the fix)

1. **Scroll must not change the selected tile.** Confirmed already true at the
   `VirtualGrid` level (scroll only updates `scrollTop`, not `activeIndex`).
   Double-check `+page.svelte` has no scroll→active coupling and keep it that way.
2. **Dwell/timeout before any heavy per-tile mount** (his suggestion, now applied
   to the decoder). Generalize this if other per-tile work is found on the hot
   path: fast pass-through should mount nothing.
3. **Bigger open question — do we need caching at all?** He points out Windows
   Explorer lazy-loads the SAME video (and photo) folder with free fast scroll,
   no freeze, no visible caching. He is open to rethinking the whole
   thumbnail/loader model toward "just lazy-load like the OS does." Not a now-task,
   but the direction he'd endorse if the current model is the root problem.

---

## 7. Historical leads (superseded by §R; use only if the issue recurs)

1. **The virtualization scroll→range sync is the prime suspect now** (the
   blank-zone/stuck-tiles symptom). In `VirtualGrid.svelte`, `scrollTop` is a
   `$state` updated only in `onScroll`. Hypotheses to test:
   - During/after a fast fling the `onScroll` events stop being processed (the
     compositor stall itself blocks the event loop), so `scrollTop` goes **stale**
     → the rendered window freezes at an old position = "tiles stuck in the
     middle," blank elsewhere. Freeze and blank-zone may be the SAME event.
   - Recycling range math (`slotItem`, `poolSize`, `firstRow/lastRow`) may under-
     cover after a large jump. **Test:** does v1.2.0 **stable** (destroy/recreate,
     no recycling) show the blank-zone? If NO, the blank-zone is a recycling bug I
     introduced in v1.4.0 and is separate from the v1.2.0 freeze. If YES, it's the
     v1.2.0 regression. This one experiment splits the two.
2. **Image-decode memory / asset-protocol pressure (audit P2).** Uncapped WebView2
   off-heap decoded-image memory; every tile via `convertFileSrc` →
   `http://asset.localhost`. Consistent with +1 GB spikes. Consider a localhost
   `tiny_http` range server for the cache dir (reuse `cast.rs`), `Cache-Control`/
   `ETag`, capped concurrency; keep the JS heap out of it (no data-URLs). Instrument
   handle-count-per-fetch first.
3. **Diff v1.1.0 vs the current loader/virtualizer behavior directly.** v1.1.0's
   grid (destroy/recreate + sprites) scrolled smoothly. Rather than more theory,
   compare what v1.1.0 did on scroll that the current build doesn't. The owner
   endorses returning to a simpler model if that's what's robust.
4. **Compare against Windows Explorer** as the owner suggests — it lazy-loads the
   same folder with no freeze. What is it doing that we aren't (thumbnail request
   throttling, no synchronous per-item work, OS thumbnail cache)?

---

## 8. How to measure (reuse these — the whole investigation lived on them)

- **Cross-process monitor:** `scratchpad/foxmon.ps1` (session scratchpad, ~30 lines
  PS; recreate if gone). Samples `foxcull` + `msedgewebview2` working-set / private
  bytes / **handle & thread counts** + `nvidia-smi` ~every 1.2 s to a CSV. Launch
  `run_in_background` before reproducing.
- **App paint heartbeat:** `+page.svelte` runs a rAF loop → `raf=N` on the 20 s
  `MEM tick` line, and logs `PAINT-RESUME gap=Nms` after a >1.5 s stall. A gap
  measures a blocked main thread. A later `MEM tick` proves only that the thread
  eventually resumed; it never proved that JS ran during the gap.
- **Log:** `%APPDATA%\com.foxcull.app\foxcull.log` (robust to relaunch races;
  append + per-pid fallback, see `log.rs`). Grid scrolls log
  `grid-scroll top=… first=… last=… pool=…` from `VirtualGrid.onScroll` — watch
  whether `top`/`first`/`last` keep updating through a freeze (if they stop, the
  scroll handler stalled → confirms lead #1).

---

## 9. Key files

- `src/lib/components/VirtualGrid.svelte` — recycling grid + scroll→range sync
  (**prime suspect**). Also `VirtualStrip.svelte`, `SectionedGrid.svelte`,
  `DetailsView.svelte` (same recycling pattern).
- `src/lib/components/Thumb.svelte` — per-tile: image/poster load, sprite scrub,
  grid live-decode (restored + guarded). Reads `isScrolling()`.
- `src/lib/thumbnail-loader.ts` — the throttled loader: LIFO queue, `MAX_INFLIGHT=6`
  / `MAX_HEAVY_INFLIGHT=2`, memo LRU, generation-token cancellation, the
  `setScrolling`/`isScrolling` fling gate, and the `activity` progress job.
- `src/lib/components/Loupe.svelte` — Focus view + its own `ScrubEngine`. **Working;
  leave alone.**
- `src/lib/scrub-engine.ts` — WebCodecs engine (well-built; frame lifecycle careful).
- `src-tauri/src/lib.rs` — the occlusion flag (Mode B fix). `log.rs`, `commands.rs`
  (`read_file_range`, `warm_thumbnails`), `video.rs`, `thumbs.rs`, `media.rs`.

## 10. Constraints (workspace + repo rules)

- **Ships via tags:** commit to `main` (push ships nothing), then tag
  `v1.4.0-nightly.N` → GitHub Actions builds installers → draft prerelease.
  Stable promotion only on the owner's explicit "ship it".
- **No heavy local builds** (RAM/LTO). Local gates: `npm run check` (0/0) and
  `cargo check`. `cargo test` is CI-only (GNU 65k export limit).
- Commit author `kumar.adarsh.cse12@itbhu.ac.in`; trailer
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Single-writer per repo: `git fetch`/reconcile before work; commit code+docs and
  push at session end. Per-push change ledger in `docs/changes/`.
