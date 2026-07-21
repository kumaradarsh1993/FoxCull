# 2026-07-22 — Glimpse, grid skim on the decoder, sprite retirement, polish

## Intent

Owner feedback on v1.2.0-nightly.1: scrubbing works, so (a) wipe all cached
renders for an unbiased retest, (b) chase a one-frame "blip" when opening a clip
in Focus, (c) add a fast keyframe browse for culling long clips, (d) extend the
live decoder to grid tiles and drop video pre-caching, (e) toolbar polish.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src-tauri/src/video.rs` | logic | `make_poster` takes `at_s`. Grid poster stays ~1 s (frame 0 is often black); **Focus poster moves to t=0** so it matches the frame `<video>` paints — the blip. |
| `src-tauri/src/commands.rs` | architecture | `warm_thumbnails` heavy no longer calls `ensure_filmstrip`: Prepare builds posters only for video. |
| `src/lib/scrub-engine.ts` | logic | `keyPos()` extracted (binary search); NEW `nextKeyTimeAfter()` for Glimpse's forward walk. |
| `src/lib/components/Loupe.svelte` | UX | NEW Glimpse: `toggleGlimpse()`, ~9 steps/s, speed × realtime floored to a 4 s minimum sweep, lands via the exact-frame + hand-off path. Button beside play (accent while running, disabled without the decoder). Stops on play/pause, drag, item change, teardown. |
| `src/lib/components/Thumb.svelte` | architecture | Armed+hovered tile opens its own `ScrubEngine` and paints decoded frames into a canvas; sprite build held back while the decoder is opening and skipped entirely when it's up; engine closed on leave/disarm/unmount. |
| `src/routes/+page.svelte` | UX / logic | Ctrl+Space → Glimpse (checked before plain Space) + shortcuts-panel row; Glimpse speed slider in Settings; neighbour-prefetch effect and its setting removed; Arrange rows gained glyphs and a prominent direction button; Prepare min-width 96→84 px and its bolt 11→14 px in `--star` gold. |
| `src/lib/settings.svelte.ts` | logic | NEW `glimpseSpeed` (40). `scrubPrefetch` deprecated to optional — old stored values still load. |
| `src/lib/scrub-probe.ts` | process | Self-test now walks `nextKeyTimeAfter` and asserts strictly-increasing, on-keyframe results. |
| `docs/design/precache-policy.md` | architecture | New section: video pre-caching retired everywhere; machine block updated (filmstrip = fallback only, `scrubPrefetch` removed, `glimpseSpeed` added). |

## Behavior changes

- Opening a clip in Focus no longer swaps frames a few ms in.
- **Ctrl+Space / the ⏩ button sweeps a clip by its keyframes.** ~14 s for a
  9-minute clip at the default 40×; short clips never sweep faster than ~4 s.
- Armed grid tiles skim by decoding real frames — full resolution, no build,
  no "scrub 40%" chip.
- Prepare on a video folder is much faster and writes far less: posters only.
- Neighbour scrub prefetch is gone (setting removed).

## Risks / compat

- Cached `w`-prefix Focus posters made before this build hold the 1 s frame and
  would still blip. Not an issue here — the caches were wiped this session — but
  a machine with old caches needs them cleared to see the fix.
- Two decoders can now be alive at once (Focus + a filmstrip tile hover). Each
  holds ≤2 frames, so the ceiling is ~48 MB; acceptable, not yet measured under
  a deliberate stress.
- Clips the decoder rejects still fall back to sprites, now built on demand only.

## Verification actually run

- `npm run check` 0 errors / 0 warnings; `cargo check` clean.
- Engine self-test in `tauri dev` against the Osmo 4K60 HEVC clip: open 197 ms,
  6 frames painted across a simulated drag, exact seek 192 ms landing 1 ms from
  target (read 117 / flush 69, no timeout), keyframe walk strictly increasing at
  an exact 0.5 s cadence.
- One probe assertion was itself wrong and got fixed: it rounded keyframe times
  to 3 dp before comparing, which puts the probe just *before* the keyframe and
  reported a false failure on a working walk.
- **Not verified on device:** Glimpse's on-screen pacing, the grid tile canvas,
  and the poster fix all need a hand on the mouse.

## Data wipe (owner-requested, this session)

Removed: `thumbs/` and `catalog.sqlite*` from `_FoxCull` on D:, E:, F:, P:, plus
`%APPDATA%/com.foxcull.app` (settings + logs) and the empty LocalAppData twin.
**Kept on instruction:** every `recycle/` folder — 9 culled video files, 18.6 GB.
`D:\FoxCullLibraryResetBackup-20260709-024847` (16.7 MB of old catalog backups)
was also left alone as the only recovery path for the wiped culling marks.
