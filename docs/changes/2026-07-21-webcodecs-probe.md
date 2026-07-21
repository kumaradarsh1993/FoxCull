# 2026-07-21 — Architecture C decided; WebCodecs feasibility probe (GREEN)

## Intent

Close the video-player migration question. A ground-up review (Fable) found the
libmpv effort was solving a layering problem that only exists because the
pixels left the DOM; mpv's actual advantage is a *seek policy* (keyframe-only
while dragging, precise on release) that WebCodecs lets us implement inside the
webview. This push archives the libmpv work, records the decision, and lands
the measured go/no-go probe plus the scaffolding the real engine will reuse.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src-tauri/Cargo.toml` | process | `crate-type = ["rlib"]` (from the archive branch) — local `tauri dev` can link on windows-gnu; cdylib/staticlib were unused mobile defaults |
| `src-tauri/src/commands.rs` | logic | NEW `read_file_range` (binary IPC via `tauri::ipc::Response`, 64 MB cap) — the scrub engine's I/O primitive; NEW `scrub_probe_report` (dev probe verdict → `%TEMP%/foxcull-scrub-probe.json` + app log) |
| `src-tauri/src/lib.rs` | logic | register both commands |
| `src/lib/api.ts` | logic | `readFileRange` wrapper returning ArrayBuffer |
| `src/lib/scrub-probe.ts` | logic | NEW dev-only probe: moov-only mp4box parse (handles moov-at-end), hvcC/avcC description extraction, `isConfigSupported`, timed decode of 6 real keyframes on one persistent decoder, canvas paint + pixel readback |
| `src/lib/mp4box.d.ts` | process | ambient types for mp4box.js (ships none) |
| `src/routes/+page.svelte` | logic | dev-only dynamic import of the probe in onMount (inert without `VITE_SCRUB_PROBE`) |
| `package.json` | process | + `mp4box` |
| `docs/design/video-player-migration.md` | architecture | brought to `main` from the archive branch; added §10 (ground-up review + Architecture C design) and §11 (probe results); §12 open items updated |
| `docs/design/libmpv-transplant.md` | architecture | brought to `main`, marked ARCHIVED with pointers to `archive/libmpv-A/B` |
| `CLAUDE.md` | process | doc-map row for the migration brief |

## Behavior changes

None for users. The probe only runs in dev with `VITE_SCRUB_PROBE` set.
`read_file_range` is a new command surface (path-unrestricted read, same trust
level as the existing loupe/thumbnail path handling).

## Branch archival

`archive/libmpv-A-mpv-in-front` (4ae90ae, the working in-front build) and
`archive/libmpv-B-transparent-webview` (5dd3d35, proven impossible) created and
pushed. `feat/libmpv-video` pushed at the same tip as B. Owner: keep these
frozen as fallback; do not delete.

## Verification actually run

- `npm run check` — 0 errors / 0 warnings.
- `cargo check` — clean (after killing a stale dev app holding `libresource.a`).
- Probe executed in `tauri dev` against `DJI_20260622190543_0628_D.MP4`
  (Osmo Pocket 3, 3840×2160 HEVC Main 10, 546 s, on the P: HDD):
  hardware config supported; moov parse 316 ms / 4.9 MB; 1092 keyframes
  (GOP ≈ 30); hot decode 16–19 ms per 4K keyframe; sample reads 16–47 ms;
  canvas pixel readback real. Verdict: GREEN — 20–30 full-res scrub fps with
  zero preparation. Full JSON in `video-player-migration.md` §11.
