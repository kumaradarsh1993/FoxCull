# 2026-08-02 — Grid/filmstrip freezes: two measured root causes fixed

Targets `v1.3.0-nightly.9`. Supersedes the nightly.8 attempt
(`2026-08-02-fast-scroll-freeze.md`), whose gate had the right idea but the wrong
trigger and never fired.

## How this was diagnosed (measure, don't infer)

A live session with the owner using a background monitor
(`scratchpad/foxmon.ps1`) sampling `foxcull` + `msedgewebview2` process memory,
handle/thread counts and `nvidia-smi` every ~1.2 s, cross-referenced with the
app's own `raf=` paint heartbeat in `foxcull.log`. Two clean captures (one photo
folder, one video folder) gave an unambiguous signature and split the problem in
two.

### Mode A — fast-scroll compositor stall (the original bug)

```
grid-scroll top=1700 ...
PAINT-RESUME gap=12474ms          # rAF (paint) stopped for 12.5 s
```
During the stall the monitor showed WebView2 **handles climbing 26k→38k** and
**RAM +900 MB** (frame/backpressure objects piling up behind a stalled
compositor), then **draining back to ~20k** on recovery — while the **GPU stayed
at 4–7 % (idle)** and `foxcull` stayed responsive. Reproduced on both photos and
videos.

Conclusion: **not** a GPU/TDR problem (that theory, and the `--disable-gpu`
path, are dead — GPU was idle throughout), and **not** a JS-thread or loader
deadlock (timers ran, the queue drained). It is a WebView2 **compositor pipeline
choke** on the burst of tile mount/unmount + per-tile image work that a fast
scroll produces. It usually drains in ~10–12 s; sometimes it does not, and the
app must be relaunched (the owner's "permanent freeze").

### Mode B — idle / on-launch paint stall

Separately, the app sometimes opened or sat **frozen while completely idle**:
monitor showed *normal* handles (~20k, no spike), flat memory, idle GPU,
`Responding=True` — the process healthy but not painting. This is the known
WebView2 **native-window-occlusion** misfire (Chromium decides the window is
occluded and stops presenting). Confirmed: launching with the occlusion feature
disabled opened unstuck.

### A logging bug that kept blinding us

`log::init` truncated the logfile every launch and swallowed open failures with
`.ok()`. Combined with `tauri-plugin-single-instance`, a relaunch that raced a
still-closing/stuck instance produced a session with logging silently dead — on
exactly the frozen sessions we needed to see. Fixed.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src-tauri/src/lib.rs` | logic | On Windows, set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--disable-features=CalculateNativeWinOcclusion` before the webview is created (idempotent, preserves any existing value). Fixes Mode B. |
| `src-tauri/src/log.rs` | logic | Robust `init`: append by default (truncate only when the file > 5 MB), and fall back to a per-pid file if the primary can't be opened. Session banner now includes the pid. No launch can silently lose logging again. |
| `src/lib/components/VirtualGrid.svelte` | logic + UX | Corrected fast-scroll gate: engage `isScrolling` on a big single jump (`delta > rowH*1.5`) OR fast cumulative motion (`recentMove > rowH*2.5`, events chained <120 ms), never a gentle notch. Renders image-free `.cellskeleton` placeholders during fast motion, real tiles on the 160 ms settle. Logs `grid-gate ON …` when it engages. Fixes Mode A. |
| `src/lib/components/SectionedGrid.svelte` | logic + UX | Same corrected gate; headers stay live. |
| `src/lib/components/VirtualStrip.svelte` | logic + UX | Same gate applied to the filmstrip (both orientations) — the bottom strip in Focus chokes the same way. Uses `step`-based thresholds. |

## Why the nightly.8 gate didn't fire

It accumulated distance only across events <90 ms apart and needed >3 rows within
such a burst. Real flings arrive as **one big jump** (e.g. top 1500→5900 in a
single event), so the burst counter kept resetting and the gate never engaged —
the change was inert, which matches "nightly.8 did nothing / felt worse". The new
trigger fires on the single-jump case directly and logs each engagement so we can
confirm it from `foxcull.log`.

## Behavior changes

- Fast scrolling (grid + filmstrip) shows neutral placeholders during the fling
  and paints thumbnails on settle (~160 ms after motion stops). Gentle scroll is
  unchanged and fully live.
- WebView2 native-window-occlusion detection is disabled on Windows.
- `foxcull.log` is append-based, pid-stamped, robust to relaunch races, and logs
  gate engagements.

## Risks / compat

- Occlusion flag: standard, low-risk for a single-window desktop app; Windows-only
  (ignored by WKWebView/WebKitGTK). No effect on macOS/Linux.
- Gate is presentation-only; virtualization, keyed identity, async loading,
  cancellation and scroll-position sync untouched; preserves the documented traps
  (no rAF in the correctness path; primitive keying kept; Thumb internals
  untouched).
- If Mode A still recurs, the log now shows `grid-gate ON` (did it fire?) plus the
  `raf=` gap, which tells us whether to escalate to true cell recycling or a
  WebView2 present-mode flag.

## Verification actually run

- `npm run check`: 0 errors / 0 warnings.
- `cargo check`: passed (lib.rs + log.rs changed).
- Device QA is the real test (freeze only reproduces on real WebView2). Occlusion
  fix already confirmed live (booted unstuck). Fast-scroll gate to be confirmed on
  the installed nightly.9 with the monitor running.
