# Scroll-freeze investigation — handover (2026-08-02)

Self-contained handover so a fresh session can continue without the chat history.
Read this, then `docs/design/rendering-rework-2026-08.md` (the rework plan) and the
newest `CLAUDE_CODE_HANDOVER.md` section.

## TL;DR

FoxCull freezes: the viewport goes black/dead (input dead, sometimes needs
relaunch) — reliably on a **fast scroll**, and sometimes on slow scroll past the
2nd screen. Reproduces in grid, grouped grid, and the Focus filmstrip, on photo
AND video folders, on stable `v1.2.1` too (so it is **not** a regression from the
visual refit). Two distinct causes were found by live measurement:

- **Mode B — idle / on-launch paint stall.** WebView2 native-window-occlusion
  misfire. **FIXED** (`--disable-features=CalculateNativeWinOcclusion` in
  `lib.rs`, v1.3.0-nightly.9). Confirmed: booted unstuck.
- **Mode A — fast-scroll compositor stall.** **NOT yet fixed.** Root cause is now
  isolated to the **image fetch/decode path**, not the DOM. This is the open item.

## How it was measured (reuse these)

- **Process monitor:** `scratchpad/foxmon.ps1` (in the session scratchpad, NOT the
  repo). Samples `foxcull` + `msedgewebview2` working set / private bytes / handle
  & thread counts + `nvidia-smi` every ~1.2 s to `scratchpad/foxmon.csv`. Launch
  it `run_in_background` before reproducing. Recreate it if the scratchpad is gone
  (it's ~30 lines of PowerShell).
- **App paint heartbeat:** `+page.svelte` runs a rAF loop advancing `rafFrames`,
  printed as `raf=N` on the 20 s `MEM tick` line, and logs `PAINT-RESUME gap=Nms`
  when rAF resumes after a >1.5 s stall. **A `raf` that stops advancing while
  `MEM tick` lines keep coming = the compositor died while the JS thread lived.**
- **Log location:** `%APPDATA%\com.foxcull.app\foxcull.log`. Logging is now
  robust to relaunch races (append-based, per-pid fallback — see `log.rs`). Grid
  scrolls log `grid-scroll top=… pool=…`.

## Mode A — the measured signature (two clean captures + one on the recycler build)

On a fast scroll:
- `raf` stops for **7–12 s** (`PAINT-RESUME gap=7544ms` / `8545ms` / `12474ms`) —
  the screen paints nothing; input dead. Usually drains and recovers; sometimes
  doesn't (→ relaunch).
- WebView2 **handle count spikes ~20k → ~36–38k** (+16–18k) over the stall, then
  drains back to ~20k. RAM +~900–1150 MB, then drains.
- **GPU util stays 1–14 % (idle) the entire time; GPU memory flat.** So it is
  **NOT** a GPU/TDR problem. (The machine HAS a history of `LiveKernelEvent 117`
  TDR dumps, but they do not coincide with these freezes — do not chase TDR.)
- `foxcull` main process stays `Responding=True`, ~33 MB, JS heap flat (~10 MB).
- Correlates with an image-load backlog (`pending=35 queue=32 inflight=2-3`).

### What has been RULED OUT (do not re-investigate)

- **GPU / driver TDR** — GPU idle throughout every capture.
- **JS main-thread deadlock** — `setInterval` heartbeat keeps firing; heap flat.
- **Backend / thumbnail-loader deadlock** — the loader drains, queue returns to 0,
  it recovers.
- **DOM cell mount/unmount churn** — the placeholder gate (nightly.8/.9) did NOT
  stop it, and **cell recycling (v1.4.0-nightly.1) did NOT stop it either**: the
  +16k handle spike still happened with a stable recycled pool (`pool=117`, zero
  mount/unmount). This is the key result — it eliminates the DOM as the cause.

### What is CONFIRMED as the remaining cause

The **image fetch/decode path**. With recycling, a fast scroll still rapidly
repoints ~cols tiles' `<img src>` per row (asset-protocol fetch + Chromium decode
+ bitmap/section creation per image). The handle spike grows ~1000/s during the
stall and drains after — consistent with decoded-bitmap shared-memory sections /
asset-protocol request objects piling up behind a compositor that can't keep up,
then being released. GPU idle rules out the upload itself; the cost is in the
fetch/decode/section churn.

## The open fix — options (ranked), for the next session

**First: rule out environment.** The owner had NOT rebooted the machine in a long
time and only noticed this freeze "over the last day". Baseline is ~20k WebView2
handles across 38 helper processes before any scroll. A reboot may reset
accumulated WebView2/driver state. **Get the owner's post-reboot fast-scroll
result before more code.** If a reboot alone fixes it, this was partly a stale-
state issue and the code below becomes hardening, not the cure.

If it still freezes after reboot, in order of preference:

1. **Suppress image FETCH during a fast fling (react-window `isScrolling`
   pattern), done at the load level — not the DOM level.** While the viewport is
   moving fast (velocity/large-jump; use a GENEROUS threshold so ordinary
   monitoring scroll stays fully live — the owner disliked the nightly.9 gate
   because it triggered on moderate scroll and blanked *staying* tiles), recycled
   tiles show a neutral background and do NOT issue new `loadThumb`/`loadVideoPoster`
   or change `src`; on settle, they load. With recycling there is no mount/unmount,
   so this adds no churn and no "blank everything then reload" flash — only genuinely
   new tiles are neutral during the fling (owner already accepted "thumbnails can
   be shown as black"). This directly caps the fetch/decode churn that spikes the
   handles. **Most likely the fix.** Tune the threshold on-device with the monitor.
2. **Serve thumbnails through a faster path than the Tauri asset protocol.** On
   Windows, WebView2 routes custom-scheme (`asset:`/`convertFileSrc`) requests
   through IPC, which is comparatively heavy; hundreds of rapid requests may be the
   handle/section source. Options: a localhost `tiny_http` range server (the cast
   module `cast.rs` already embeds one — reuse the pattern) serving the cache dir,
   or inline small grid thumbs as `data:` URLs (they're tiny JPEGs). Bigger change;
   do only if (1) is insufficient.
3. **Investigate the asset-protocol handle cost directly** — instrument how many OS
   handles a single thumbnail fetch creates on this WebView2 build; if it is
   pathological, that reframes the fix.

Falsifier for (1): if suppressing fetch during the fling still spikes handles, the
churn is in the compositor's handling of the recycled `<img>` elements themselves,
and (2) becomes primary.

## What is already done (don't redo)

- Occlusion flag (Mode B fix) — `lib.rs`, confirmed working.
- Robust logging — `log.rs` (append + per-pid fallback + pid-stamped banner).
- rAF paint heartbeat + `grid-gate`/`grid-scroll pool=` diagnostics — `+page.svelte`,
  the virtual components.
- **Cell recycling across all four surfaces** — `VirtualGrid`, `SectionedGrid`,
  `VirtualStrip`, `DetailsView` (grow-only slot pool, item `i` in slot
  `i % poolSize`, keyed by slot). Removed the placeholder gate. This is correct and
  worth keeping (it removed real DOM-churn cost and the flicker) even though it did
  not fix Mode A. `npm run check` 0/0.

## Key files

- `src/lib/components/VirtualGrid.svelte` / `SectionedGrid.svelte` /
  `VirtualStrip.svelte` / `DetailsView.svelte` — the recyclers.
- `src/lib/components/Thumb.svelte` — per-tile: image effect (`mediaLoadKey`),
  live-scrub arm/hover (`ScrubEngine`), sprite fallback. This is where a
  fetch-suppression flag (option 1) would read a `scrolling` prop.
- `src/lib/thumbnail-loader.ts` — LIFO queue, 6 inflight / 2 heavy, cancellation,
  folder-switch generation token. The load calls to gate live here.
- `src-tauri/src/thumbs.rs` / `media.rs` / `video.rs` — decode/cache/poster (good,
  leave alone). `commands.rs::thumbnail` / `video_poster` — the asset-served
  endpoints. `lib.rs` — WebView2 args. `cast.rs` — has a reusable `tiny_http`
  server pattern for option 2.

## Version / repo state

- `main` @ `v1.4.0-nightly.1` (base bumped from 1.3.0 to mark the recycler rework).
- Rollback: `v1.3.0-nightly.9` (occlusion + gate, freeze not fixed) or
  `v1.2.1` stable (also freezes).
- FoxCull ships via tags; push to `main` ships nothing; tag `v*` → CI builds.
- Owner (Aamir) direction: root fix, modern 2026 standard, NO refresh/blank bloat,
  unifying grid/focus experience is welcome, don't reinvent the wheel.
