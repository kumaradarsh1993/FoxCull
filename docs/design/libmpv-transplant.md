# Design: native video playback via libmpv ("the VLC transplant")

**Status:** in progress (branch `feat/libmpv-video`) · **Started:** 2026-07-21
**Owner call:** yes — owner chose the native-surface route over an in-webview
WebCodecs player, accepting the higher integration effort for maximum playback
fidelity. Owner also lifted the "no local builds" rule for this work, so the
native compositing can be developed and verified locally before shipping.

## Why

The one real ceiling of the Tauri/WebView2 stack is `<video>` playback and
scrub: the element re-buffers on every seek, so dragging the playhead on a 4K60
HEVC clip (Osmo Pocket 3) stutters and lags the cursor, versus VLC's instant
frame-step on the same hardware. Everything else in FoxCull (decode, I/O,
caching, export) is already native Rust/ffmpeg and is not the bottleneck.

libmpv (the engine inside mpv, a cousin of VLC) keeps the decoder hot and
frame-steps precisely. Embedding it gives butter-smooth playback and scrub, and
— importantly — **loses none of the editing model**: in/out points, segments,
EditStudio Looks, and export are all UI state + backend ffmpeg, independent of
whatever draws the pixels.

## Non-negotiable guardrails

1. **Never break the culling shell.** The native player is gated behind an
   experimental setting (`experimentalNativeVideo`, default **false**). Any
   failure — dll missing, window creation fails, codec unsupported — falls back
   to the current `<video>` element automatically.
2. **No risk to the existing build/CI.** libmpv is loaded at **runtime via
   `libloading`**, not linked at build time. The normal `cargo build`/CI
   release does not gain a link dependency on mpv; a missing dll just disables
   the feature. (Same spirit as the ffmpeg sidecar.)
3. **Isolated branch** (`feat/libmpv-video`) until proven. `main` stays the
   clean nightly source. Spike nightlies, if needed, are tagged from the branch
   so `main` is never destabilised.
4. **Preserve the overlays we just built** — minimal hover transport, sprite
   scrub preview, fullscreen filmstrip. The transplant replaces the *pixel
   surface*, not the UI.

## The hard problem (Windows + WebView2)

Tauri hosts a windowed WebView2 that fills the client area. libmpv wants to draw
into a window (`--wid` embedding → a child HWND). Getting our **HTML overlays on
top of a native video surface** is the crux, because WebView2 composites via
DirectComposition and a plain sibling HWND behind it does not show through its
transparent pixels. Candidate approaches, to be decided by the M2 probe:

- **A. mpv child HWND on top of the stage rect; overlays as a separate
  borderless always-on-top child window** synced to the bottom of the stage.
  Reliable rendering, more windowing code.
- **B. Transparent WebView2 + mpv surface behind**, via a DirectComposition
  visual tree. Cleanest overlay story but needs bypassing Tauri's default
  windowed controller — deep surgery.
- **C. mpv on top for playback; during scrub, cover it with the existing
  webview sprite overlay** (we already render decode-free sprite frames). Hybrid
  — native playback, sprite scrub — least code, pragmatic.

The M2 probe (a throwaway "just render mpv over the stage and report what you
see") picks between these from real observation rather than guesswork.

## Milestones

- **M0** — local `tauri dev` loop running (test harness).
- **M1** — `mpv.rs`: libloading binding + safe wrapper; bundle `libmpv-2.dll`;
  `mpv_start/command/stop` commands behind the flag. Compiles + runs, no UI wire.
- **M2** — child HWND over the stage, mpv `--wid` embed, load current clip.
  **Decide A/B/C** from local observation. *Make-or-break.*
- **M3** — overlays working over the native surface (per M2).
- **M4** — play/pause/seek/scrub JS→mpv, frame-accurate scrub, position sync,
  `hwdec` for 4K60; flag + graceful fallback solid.
- **M5** — macOS NSView path; CI fetch/bundle libmpv per platform; merge + tag.

## FFI surface (libloading, hand-declared — small on purpose)

`mpv_create`, `mpv_initialize`, `mpv_set_option` (for `wid`, INT64),
`mpv_set_option_string` (vo, hwdec, etc.), `mpv_command` /
`mpv_command_string` (loadfile, seek), `mpv_set_property_string` (pause),
`mpv_terminate_destroy`. No render-context API needed for `--wid` embedding.

## Local build note (important, discovered 2026-07-21)

`tauri dev` had **never linked** on the local windows-gnu toolchain: the lib's
`crate-type` included `cdylib`, and GNU ld auto-exports every symbol of a
cdylib — the full Tauri app has ~165k, far past GNU ld's **65k DLL
export-ordinal limit** (the same limit that blocks `cargo test` locally, per the
workspace CLAUDE.md). The team only ever built via CI (MSVC, no limit), so this
was latent. Fix on this branch: `crate-type = ["rlib"]` — the desktop **binary**
links the lib as an rlib and doesn't hit the limit; `cdylib`/`staticlib` were
mobile-only and unused. Desktop CI is unaffected (it builds the binary). This is
what makes the local test loop for M2 possible at all.

Corollary for FFI here: keep native Win32 usage **hand-declared** (as `mpv.rs`
does for `user32`) rather than pulling `windows-sys`/`windows` features — those
add ~100k exported symbols and, if the cdylib ever comes back, re-trip the limit.

## M2 probe result (2026-07-21)

First on-device test: toggling **Native video ON/OFF made no visible difference**
— the `<video>` element kept playing either way, no mpv picture appeared. So the
child HWND is **not compositing over the WebView2**. Two candidates, not yet
distinguished (the command swallows errors and nothing is logged):
1. `native_video_start` errored (dll resolve / `hwnd()` / mpv init) → silent
   fallback. Unlikely for the dll (verified present) but unconfirmed.
2. The child HWND renders **behind** WebView2's DirectComposition surface, so
   it's occluded — the expected Windows hurdle.

**Next M2 step (before M3):** add `eprintln` logging to `native_video_start`
(window handle, rect, mpv load result) so the dev log tells us which; and force
the child to the top of the z-order (`SetWindowPos(HWND_TOP, SWP_SHOWWINDOW)`
and/or `BringWindowToTop`) to test candidate 2 directly. Re-probe with the user
watching. This is the single blocking observation for the whole overlay design.

## Open questions / risks

- HEVC hwdec path on the two real machines (GTX 1070 → d3d11va/nvdec). Verify
  `hwdec=auto-safe` actually engages NVDEC on the Osmo footage.
- DPI scaling of the child HWND vs the CSS stage rect.
- Audio device selection (mpv opens its own WASAPI stream).
- macOS embedding differs entirely (NSView, `--wid` takes an NSView pointer).
- libmpv-2.dll size (~40 MB) and where CI sources it (shinchiro/mpv builds).
