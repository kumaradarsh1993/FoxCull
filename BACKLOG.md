# FoxCull Backlog

Prioritized from the July 2026 holistic audit
([docs/AUDIT-2026-07.md](docs/AUDIT-2026-07.md) has the full analysis behind
each item). Target hardware for perf items: Alienware 15 R4 (GTX 1070) and
XPS 13 (4-core, iGPU). Mac-only issues are P2 by decision.

Priorities: **P0** do next · **P1** high value soon · **P2** scheduled ·
**P3** nice-to-have.

## P0 — correctness / perf with daily impact

- [ ] **Stable, portable preview-cache keys** (audit PERF-1): key
      thumbs/posters/strips/proxies by *library-relative* path with an
      explicitly stable hash (fnv/xxhash) instead of absolute path +
      `DefaultHasher`. Today the cache regenerates per machine (E:\ vs D:\ vs
      /Volumes/…) and a Rust toolchain bump can orphan it wholesale. Ship
      together with PERF-2 so the orphaned old entries get cleaned.
- [ ] **Thumbs-cache garbage collection + size visibility** (PERF-2): sweep
      unreferenced/stale entries (esp. H.264 proxies), show cache size and a
      Clear button in Settings. Unbounded growth on the user's photo SSD.
- [x] **Hardware-accelerated / keyframe-only sprite generation** (PERF-3):
      DONE 2026-07-19, and further than proposed — sprites are now built by
      per-timestamp keyframe SEEKS (~40 single-keyframe decodes instead of a
      full decode of the clip), with cancellation, progress events, and a
      `-hwaccel auto` full-scan fallback for unseekable containers. Proxy
      builds also decode via `-hwaccel auto` now (NVDEC on the 1070).

## P1 — high value

- [ ] **Persist media probes in the catalog** (PERF-4 follow-up): `probes`
      table keyed rel+mtime+size (mirror `captures`); reuse at export time so
      each clip isn't probed 2–3× per session.
- [ ] **Try `hevc_nvenc` for Keep-HDR exports, and NVENC for proxy builds**
      (PERF-5): the GTX 1070 does 10-bit HEVC in hardware; libx265-medium is
      the slowest path in the app. Keep the software fallback ladder.
- [ ] **Filename/text search** in the library toolbar (works with existing
      filters; at 10k files filters alone don't isolate a shot).
- [ ] **Measure & tame mark-keystroke reactivity on huge folders** (PERF-6):
      log `relatedIndex`/sort timings via the existing logEvent plumbing; if
      >10 ms at 5k items, cache sort keys and memoize stack grouping
      independently of mark changes.
- [ ] **Split the three monoliths** for maintainability: extract stacks logic
      + culling actions from `+page.svelte`, the export dialog + timeline
      engine from `EditStudio.svelte`, and group `commands.rs` into
      submodules. Pure refactor, no behavior change; add unit tests for
      `buildRelatedIndex` while extracting it.
- [ ] **Play the audio lane in the edit preview** (currently export-only —
      cuts can't be timed to music).
- [ ] **Catalog safety net**: rotating `catalog.sqlite` backup (2–3
      generations) written on library switch/close; removable drives get
      yanked mid-write.

## P2 — scheduled (includes all Mac items)

- [ ] **Mac: replace `window.confirm()`/`alert()`** with in-app dialogs —
      WKWebView doesn't reliably implement them, so the JPEG-export confirm
      can be a silent no-op on macOS (MAC-2).
- [ ] **Mac: sign + notarize the .dmg** (needs Apple Developer account; add
      notarytool step to release.yml) (MAC-1).
- [ ] **Mac: verify the shared-SSD story end-to-end** once P0 cache keys land
      (MAC-4): same drive on Windows→Mac should reuse catalog *and* previews.
- [ ] **Path-guardrail consistency** (SEC-1): validate `cast_start`,
      `export_raw_jpegs`, `raw_embedded_probe`, and the read-side media
      commands against the active library root like the destructive commands.
- [ ] **Cast hardening + status polling** (SEC-2 + UX): clear the media-server
      allowlist on stop, expire tokens, poll `cast_status` while the chip
      shows "casting" so a dropped TV session doesn't leave a stale indicator.
- [ ] **Pin ffmpeg sidecar downloads** in release.yml to exact release tags +
      checksums (SEC-3); document the bundled build's GPL license in the
      release notes/about.
- [ ] **Folder badges: skip system directories** (PERF-7) so expanding `C:\`
      doesn't recursively count Windows/Program Files on the warm pool.
- [ ] **Loupe cap from display size** (PERF-8): derive the 1920 preview cap
      from the largest connected monitor (4K external looks soft today).
- [ ] **Blur-up placeholder uses the grid tier** instead of always 320px
      (avoids an extra cache variant per photo).
- [ ] **Trash auto-expiry option** (e.g. purge items older than 30 days, off
      by default) + show trash size.
- [ ] **`clean_segments` zero-length edge**: clamp order after max(0) so a
      fully-negative segment can't survive to a per-segment export error.

## P3 — nice-to-have

- [ ] Undo no-op check without full `JSON.stringify` on 10k-item selections.
- [ ] `list_drives`: probe drive letters with a timeout / skip A:–B: to avoid
      card-reader stalls.
- [ ] Dedupe the repeated `CREATE_NO_WINDOW` blocks and `uniquify` copies into
      shared helpers; single `ffmpeg_cmd()` constructor (also the natural home
      for `-hwaccel auto`).
- [ ] Volume/mute control on the Focus video player.
- [ ] Cache-size + library stats panel (files, ratings coverage) in Settings.
- [ ] Auto-updater (tauri-plugin-updater) once builds are signed.
- [ ] Crop keyframes ("moving portrait window following a subject") — the
      long-standing roadmap idea; sits on top of the existing per-clip crop.
- [ ] Custom Cast receiver for true full-res photo casting (Default Media
      Receiver caps stills ~720–1080p).

## Done (2026-07-19 video-preview rework session)

- [x] PERF-3 (see above): keyframe-seek sprite extraction + cancellation +
      hwaccel; the single biggest Live Scrub / seek-latency fix.
- [x] Live Scrub hover builds are cancellable end-to-end (pointer-leave stops
      the backend mid-build; folder switch cancels ALL sprite work).
- [x] Prepare also pre-builds video hover scrub strips (posters already were).
- [x] Focus video: cached poster paints instantly; the seek bar always gets a
      filmstrip (coarse cached strip first, dense one replaces it); drag-scrub
      shows a full-canvas sprite frame while the decoder chases; step/shuttle
      seeks are optimistic + throttled with a trailing accurate seek.
- [x] PS5/PS4 controller culling: remappable bindings (press-to-bind panel,
      pairing guide, button-guide overlay), mouse extra-button mapper, and
      fullscreen "play mode" now keeps the bottom filmstrip.

## Done (this audit)

- [x] CI stamps the tag version into Cargo.toml (window title showed a stale
      version on every tagged build).
- [x] Prepare pre-builds RAW previews + video posters (was silently a no-op on
      RAW/video folders).
- [x] Edit-mode ffmpeg probe storm throttled to 4 concurrent.
- [x] Stack-index bucket fill de-quadratified (rebuilds on every mark key).
- [x] Cross-volume move no longer leaves a duplicate when source removal fails.
- [x] macOS AppleDouble `._*` files skipped by all media walkers.
- [x] Push/PR CI (svelte-check, build, cargo check + test).
- [x] `@types/node` added — svelte-check fully clean for the first time.
