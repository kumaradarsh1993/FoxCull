# 2026-07-20 (3) — Minimal video transport, hi-res poster, fullscreen filmstrip

**Author:** base-machine agent (Opus) · **Tag:** rolls into v1.1.0-nightly.5
**Basis:** owner feedback on the 2026-07-20 device test (still on an older
installed nightly). Video-focus UX pass + first-frame quality.

## Intent

Make the Focus/full-screen video experience get out of the picture's way and
look sharp:

1. The transport bar ate vertical space and covered the frame permanently.
   Owner wants a thin, unobtrusive progress line that expands to the full bar
   only on hover, then collapses again — VLC/YouTube-style — as an option.
2. The `<video>` poster (first frame shown before playback / autoplay-off) was
   a pixelated 480px blowup on a large stage.
3. In play mode (F) the bottom grid filmstrip stayed pinned, competing with the
   picture; owner wants it hidden with a hover-to-reveal at the bottom edge.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/video.rs` → `make_poster` | **logic (perf/quality)** | Parametrized on `box_px`; added `-q:v 3`. One codepath now serves both the light grid poster and the sharp Focus poster. |
| `src-tauri/src/video.rs` → `poster_path`/`poster_hires_path`/`ensure_poster_hires` | **architecture** | Split the poster cache into two keys: `v…` (480px, grid) and `w…` (1280px, Focus). Hi-res is generated lazily, only for clips opened in Focus, so the grid's poster memory is unchanged. |
| `src-tauri/src/commands.rs` → `video_poster_hires` | **plumbing** | New read/generate command mirroring `video_poster`. |
| `src-tauri/src/lib.rs` | **plumbing** | Registered `video_poster_hires`. |
| `src/lib/api.ts`, `src/lib/thumbnail-loader.ts` | **plumbing** | `videoPosterHires` wrapper + `loadVideoPosterHires` (separate queue key `vidhi:`). |
| `src/lib/components/Loupe.svelte` | **UX (major)** | Focus poster now uses the hi-res loader. Transport moved from a panel *below* the video to an **overlay on the stage**. New `bottomHover` state + `onStagePointerMove` (bottom ~26%/≥90px reveal band); `showTransport = !minimal \|\| bottomHover \|\| scrubbing \|\| clipToolsOpen`. Collapsed state renders a 3px `.thinline` progress hairline. Transport fades/slides via `.shown`. |
| `src/lib/settings.svelte.ts` | **logic** | New `minimalVideoBar: boolean` (default **true**). |
| `src/routes/+page.svelte` | **UX** | Settings popover toggle for Minimal video bar. Fullscreen: `.bstrip` lifts out of flow (`position:absolute`, `translateY(100%)`) and slides up on `fsStripHover`; a bottom `.fsStripSensor` (22px) plus the strip's own pointer-enter drive the reveal. Non-fullscreen behavior unchanged. |

## Behavior changes visible to the user

- **Minimal video bar ON (default):** opening a video shows the picture
  edge-to-edge with only a thin progress hairline at the bottom. Move the
  pointer to the bottom of the frame → the full transport (play, time, scrub
  track, Info, Clip tools) slides up. Move away while playing → it collapses.
  Scrubbing or having Clip tools open keeps it open. Turn the setting **Off**
  to pin the bar always-visible.
- **Sharper first frame:** the Focus/full-screen poster is now ~1280px. Grid
  thumbnails are unchanged (still the light 480px poster).
- **Play mode (F):** the bottom filmstrip is hidden and reveals when you reach
  the very bottom edge, so F is a clean full picture.
- **In/out marks already persist** (no change needed): `get_trim`/`set_trim`
  restore in/out on reopen, and marked segments via `get/set_video_segments`.
  The owner's "doesn't persist" report was against an older installed nightly.

## Risks / compat

- Hi-res posters are a new cache key: first Focus-open of each clip generates a
  1280px frame once (keyframe seek, ~cheap). Old `v…` posters keep serving the
  grid. Cache GC remains the separate pre-existing P0 in BACKLOG.
- Transport is now an overlay: it covers the very bottom of the frame while
  shown (like every modern player). The `minimalVideoBar=false` path keeps it
  visible but as an overlay scrim, not the old solid panel below the video.
- Fullscreen filmstrip reveal relies on a 22px bottom sensor; on odd WMs where
  `setFullscreen` is refused the chrome still hides and the sensor still works.

## Verification

- `svelte-check` 0 errors / 0 warnings; `cargo check` clean.
- Owner device checklist: (a) minimal bar hairline + hover-expand + collapse in
  Focus and in F; (b) first frame sharp on a 1440p/4K stage; (c) F hides the
  filmstrip, bottom-edge hover reveals it; (d) in/out marks survive leaving and
  returning to a clip; (e) toggle Minimal video bar Off → bar stays pinned.
