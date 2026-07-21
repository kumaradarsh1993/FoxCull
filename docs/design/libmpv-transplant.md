# Design: native video playback via libmpv ("the VLC transplant")

**Status: ARCHIVED 2026-07-21 — superseded by Architecture C (WebCodecs), see
`video-player-migration.md` §10.** The code lives on branches
`archive/libmpv-A-mpv-in-front` and `archive/libmpv-B-transparent-webview`
(both pushed); do not delete them — they are the deliberate fallback if
WebCodecs ever hits a wall. This doc is kept as the historical record.

**Original status:** in progress (branch `feat/libmpv-video`) · **Started:** 2026-07-21
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

## RESOLVED (2026-07-21): the surface works, and it goes BEHIND the webview

Everything below this section is superseded — kept because the wrong turns are
the useful part of the record.

### What the three failures actually were

1. **`loadfile` rejected every Windows path.** `mpv_command_string` runs mpv's
   own parser, and inside a quoted argument **backslash is an escape character**,
   so `loadfile "P:\All media MASTER\…"` was read as the escapes `\A`, `\M`, `\P`
   and returned error -4. mpv had a perfectly good window and had been handed
   nothing to play. Fixed by using `mpv_command`'s **argv** form, which parses
   nothing. *(This is what the original "no picture" probe was actually seeing.)*
2. **A diagnostics read taken 1.2 s after start** reported `vo-configured=no`.
   D3D11 context creation plus first-run shader compilation takes ~6 s. The
   sample was simply too early. Now sampled later, and mpv's own verbose log
   (`%APPDATA%/com.foxcull.app/mpv.log`) is enabled while the flag is on.
3. **`SWP_NOZORDER` on every reposition** preserved whatever order existed, so a
   webview recreation could put the video permanently on the wrong side. Z-order
   is now restated explicitly on every move.

Once those were fixed the surface rendered correctly: 3840×2160 HEVC Main 10,
`hwdec=d3d11va` on the GTX 1070, flip-model presentation, and the owner's verdict
on seeking was *"this is the kind of behavior I was expecting."*

### The layering decision, and why it changed

**First working version put mpv IN FRONT** (`HWND_TOP`, commit `4ae90ae`). It
composited fine. But a window in front of the page cannot be drawn over by
anything *in* the page, so every overlay in the app — Settings, Prepare, Filters,
Arrange, context menus, the Info panel, dialogs — was invisible behind it. The
workaround was to hide the video whenever an overlay opened, which produced a
visible frame-switch each time, plus a reserved transport strip, a pinned bar,
and `overlayOpen` plumbing through the component tree. The owner's read was
blunt and correct: *"these trade-offs are just piling up… it just switches
frames, it looks odd."* The trade-offs were a symptom of the layering choice.

**Current design: mpv BEHIND a transparent webview.**

- The window is created with `transparent: true`.
- The page still paints an opaque background **by default**, so with no video
  showing the app looks and behaves exactly as before — the transparency is
  invisible.
- While the native surface is up, `<html>` gets the `nativeHole` class
  (`app.css`). That drops the page background and the `.viewport` background, so
  the Focus video stage becomes a genuine hole. Every other region paints its own
  background (see the `:global(html.nativeHole)` rules in `+page.svelte`), or the
  desktop would show through the gaps.
- mpv's child window sits at `HWND_BOTTOM` (`VIDEO_Z` in `mpv.rs`) and shows
  through the hole.

What this buys, all of it deleted rather than worked around:

- Menus, dialogs, the Info panel and the transport are **ordinary HTML on top**.
- Semi-transparent overlays (the transport's gradient) **alpha-blend over the
  video** for free.
- The minimal hover-reveal bar works again; no reserved strip, no pinned bar.
- Clicking the picture is a plain HTML handler (`.nativeHit`) — mpv never sees a
  click, so its own `MBTN_LEFT` binding is redundant.
- Fullscreen needs no special casing.

**The hole is only opened once mpv has actually started** (`nativeReady`).
Punching it earlier would briefly show the desktop, because there is no video
window behind it yet.

### How to revert to the in-front design

1. `VIDEO_Z = HWND_TOP` (null) in `mpv.rs`.
2. `"transparent": false` in `tauri.conf.json`.
3. Restore the reserved-strip / `overlayOpen` / hide-on-overlay code from
   `4ae90ae`.

Worth keeping documented because the behind-approach depends on WebView2
transparency, which is the piece most likely to misbehave on another machine or
a future WebView2 release. `native_video_set_visible` is retained for that path.

### Still open

- In/out point marking doesn't take effect in the native path (keys are wired
  and exported; cause not yet diagnosed — do not guess at it).
- Fullscreen (F) positioning of the surface is untested.
- Prepare has no cancel button (owner request, unrelated to this work).
- macOS embedding is a different API entirely (NSView) — untouched.

### Dev-environment trap that wasted a round of testing

`tauri dev` on this machine had a **72 KB stub `ffmpeg.exe`** beside the built
exe (the placeholder that satisfies `cargo check`), not the real 144 MB binary.
Every ffmpeg operation therefore failed silently in dev — video posters, scrub
sprites, HEIC decode, proxies — and was misreported as thumbnail/live-scrub
regressions. Videos still *played*, because mpv brings its own decoders. Copy the
real binary to `src-tauri/binaries/ffmpeg-<triple>.exe` (gitignored) before
trusting any media behaviour seen in a dev run.

## M2 probe result (2026-07-21) — superseded, see above

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
