# 2026-08-02 — audit safe-batch fixes (v1.4.0-nightly.3)

Contained, verified fixes from the 2026-08-02 perf + correctness audits
(`docs/design/AUDIT-2026-08-02.md`). Higher-risk / measurement-dependent findings
(localhost thumbnail server P2, O(N²) stack re-rooting P1, swallowed mark errors
C1, Edit virtualization P3, …) are deferred there, not done here.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src-tauri/src/video.rs` | logic | `trim` captures stderr and returns the real ffmpeg error; `-map 0:v -map 0:a?` instead of `-map 0` so data/timecode tracks can't fail a stream-copy cut. |
| `src-tauri/src/commands.rs` | logic | Lossless-concat list file name is pid+sequence unique (was whole-second `now()` → overlapping exports collided). `restore_trash` cross-volume fallback deletes its copy if the source removal fails (no duplicate/second-restore). |
| `src/lib/components/Thumb.svelte` | logic | The two `*_cached` sprite probes (fired per video tile / per recycle, bypassing the loader) are gated behind Live Scrub and collapsed to the single dense probe. |
| `src/routes/+page.svelte` | logic | Folder-open warm sends only the first 600 image paths (its cap), not thousands over IPC. Removed dead `relatedScore`; corrected the stale "rebuilds on mark keystroke" comment. |

## Behavior / risk

- No user-facing behavior change except: failed trims now show a real reason;
  scrolling video folders does slightly less per-tile IPC. All contained; no
  scroll/loader/recycler path touched.
- Verification: `npm run check` 0/0; `cargo check` passed.
