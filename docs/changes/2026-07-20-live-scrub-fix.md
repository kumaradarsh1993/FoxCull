# 2026-07-20 (2) — Focus filmstrip: gate restored, GPU decode, intent-driven build

**Author:** base-machine agent (Fable) · **Tag:** rolls into v1.1.0-nightly.5
**Basis:** owner-approved fix plan from the 2026-07-20 RCA (`.rca-live-scrub.md`
at repo root — full cost model, git archaeology, and the SSD-vs-HDD analysis).

## Intent

Undo the nightly.3 regression where Focus view built the dense scrub filmstrip
**unconditionally on open**, ignoring the Live Scrub setting — ~70 s of
unwanted software-decode ffmpeg work per clip on the owner's HDD library, and
the open file handles that fed the delete-freeze. Keep the rework's genuinely
better keyframe-seek extractor.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/components/Loupe.svelte` | **logic (regression fix)** | Focus open is now **cached-only**: paints the coarse hover strip and/or dense strip if a previous session built them, builds nothing. New `ensureFilmstrip()` starts the dense build only when **Live Scrub is ON** *and* the user shows scrub intent — first hover/drag on the timeline (`onTrackMove`/`onTrackDown`) or a key/controller seek (`seekBy`). `denseRequested` guards one build per item; epoch guard + existing `cancelSprite` cleanup still cancel on item switch. |
| `src-tauri/src/video.rs` → `extract_frame_at()` | **logic (perf)** | `-hwaccel auto` on every frame extraction (was only on the rare fullscan fallback). NVDEC/d3d11va decodes 4K HEVC keyframes ~5–10× faster than software; transparent software fallback. |
| `src-tauri/src/video.rs` → `ensure_filmstrip()` | **logic (perf)** | Dense strip frame cap 100 → **48** (≈ a frame every 2% of the seek bar — visually indistinguishable, half the work on long clips). Existing cached 100-frame sprites keep working (geometry lives in the JSON sidecar). |
| `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src/lib/api.ts` | **plumbing** | New read-only `video_filmstrip_cached` command (mirrors `video_scrubstrip_cached`): fetch dense-strip geometry IF cached, never build. |

## Behavior changes visible to the user

- **Live Scrub OFF:** opening any video does zero preview work — no "Building
  scrub filmstrip" job, no disk/CPU churn, exactly like pre-nightly.3. Plain
  seek bar still seeks; already-cached strips still show.
- **Live Scrub ON:** watching a clip costs nothing; the build starts on your
  first reach for the timeline and lands in ~5–20 s (GPU) instead of ~70 s.
- Prepare (explicit) still pre-builds hover strips — unchanged.

## Risks / compat

- Cached sprites from earlier versions load unchanged (sidecar-driven).
- `-hwaccel auto` on a codec/GPU with no decoder falls back to software
  inside ffmpeg; the existing exact-seek retry also remains.
- Intent trigger fires on timeline *hover* — a stray mouse pass over the
  transport starts a (cancellable, serialized, GPU) build; judged acceptable.

## Verification

- `svelte-check` 0 errors / 0 warnings; `cargo check` clean.
- Owner live-verification checklist (from the RCA): OFF → several uncached
  2 GB+ HEVC clips open with no build job; ON → build fires on first
  timeline touch and completes in the predicted window; delete-mid-build
  cancels and succeeds; A/B SSD vs HDD.
