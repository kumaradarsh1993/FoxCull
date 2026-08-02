# 2026-08-02 — Fast-scroll Grid freeze: churn gate + compositor-liveness probe

Targets `v1.3.0-nightly.8`.

## Intent

Stop the long-standing failure where flinging the Grid (fast wheel throw or a
held arrow key) blacks out the viewport and wedges the whole window — input dead,
permanent until relaunch — while gentle scrolling is fine. **This is not a
regression from the visual refit:** the owner reproduced the same freeze in
`v1.2.1` stable (`e951f74`), which predates the refit and all three prior grid
fixes. Nightlies .5/.6/.7 each attacked it at the JS-windowing / CSS-layer level
(double whole-app transform, rAF→direct scroll sync, removing per-cell
`will-change`) and none held, because the failure is **downstream of the DOM**.

## Diagnosis (what the evidence supports vs. what is still inference)

Proven by prior sessions' telemetry, at the moment of freeze: JS heartbeat keeps
firing, JS heap flat ~18–19 MB (no leak, event loop alive), thumbnail loader
idle (`pending=0 queue=0 inflight=0`, not backend saturation), VirtualGrid
reports the **correct** rows and mounted-cell count (windowing math is correct),
no crash report, and the WebView2 **GPU** subprocess holds ~227 MB after the
freeze. Owner also confirms even native cursor feedback dies and only relaunch
recovers it. That profile — correct DOM, live JS, dead screen/input, stuck GPU
process — points at the **WebView2 GPU/compositor process wedging**, not a Svelte
bug.

Mechanism (inference, not yet measured): the one variable separating "slow = fine"
from "fast = dead" is the **rate of image-tile mount/unmount**. Two amplifiers in
the code: (1) `onScroll` commits `scrollTop` on every native scroll event with no
throttle (deliberate, for position correctness), and (2) `{#each … (i)}` is keyed
by absolute index, so a large jump destroys and rebuilds **all** ~117 tiles —
each a full `Thumb` with effects, a `ResizeObserver`, and an `<img>` (a GPU
texture upload). A fling churns thousands of texture alloc/free cycles per second,
which is the plausible tip-over for the GPU process (likely a context-loss/TDR the
embedded webview never recovers from).

## Modules touched

| File | Level | Change |
|---|---|---|
| `src/lib/components/VirtualGrid.svelte` | logic + UX | `onScroll` accumulates distance travelled within one continuous motion (events chained <90 ms apart); once it exceeds `rowH*3` an `isScrolling` flag renders image-free `.cellskeleton` placeholders instead of the `cell` snippet. Real tiles swap back on the existing 160 ms settle. Scroll-position handling is otherwise unchanged (no new rAF dependency). |
| `src/lib/components/SectionedGrid.svelte` | logic + UX | Same gate; section headers stay live (they carry no textures). |
| `src/routes/+page.svelte` | logic (diagnostics) | New rAF loop advancing `rafFrames`; the 20 s MEM heartbeat now prints `raf=N`. Two consecutive ticks with the same `raf` = compositor dead while renderer alive (vs. both stopping = wedged renderer, vs. both advancing = present-only failure). Logs `PAINT-RESUME` if it ever recovers from a >1.5 s gap. |

## Why key on distance, not event count

Windows native smooth-scroll can emit several scroll events for one gentle wheel
notch. An event-count trigger would blank tiles during exactly the monitoring
scroll the owner says works today. Distance-in-one-motion (>~3 rows) cleanly
separates a fling / held key from a gentle scan; a single notch totals well under
a row and never gates.

## Behavior changes

- Fast Grid scrolling shows neutral placeholders during the fling; thumbnails
  paint on settle (~160 ms after motion stops). Gentle scroll is unaffected.
- `foxcull.log` gains `raf=N` on each `MEM` line and occasional `PAINT-RESUME`.

## Risks / compat

- The gate is presentation-only; virtualization, keyed identity, async loading,
  cancellation, and scroll-position sync are untouched. Preserves the documented
  traps (no rAF in the correctness path; primitive `(i)` keying kept; Thumb
  internals untouched).
- If this does not fully fix it, the `raf=` divergence in the log confirms the
  GPU-process theory and the next lever is WebView2 browser args (`--disable-gpu`
  / `--disable-features=CalculateNativeWinOcclusion`) or true cell recycling —
  neither taken here to keep the nightly focused and low-risk.

## Verification actually run

- `npm run check`: 0 errors / 0 warnings.
- `cargo check`: passed (no Rust changed; ran per the local-gate convention).
- Device verification is the point of the nightly: the freeze only reproduces on
  real WebView2/GPU, not locally or in a plain browser. Owner to fling the 6,000-
  item folder on the Alienware and, if it still freezes, send the tail of
  `foxcull.log`.
