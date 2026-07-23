# 2026-07-23 — Chromecast session recovery and predictable trigger seeking

## Intent

Continue the exhausted 2026-07-22 session: fix Chromecast sessions where the
first item loaded but follow/play/pause/seek were inert, and investigate the
owner's report that DualSense L2/R2 fast-back/forward felt glitchy.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/cast.rs` | logic | `CastStatus.connected` becomes true after the synchronous TCP/TLS handshake instead of racing the actor's first writes. Removed the premature `playing_path` assignment from `cast_start`; the actor remains the only writer after LOAD is sent. |
| `src/routes/+page.svelte` | logic / resilience | Cast follow and polling key off the intended device session rather than a stale `connected` snapshot. Poll results refresh UI state. Start records intent before awaiting LOAD and clears it honestly on failure. |
| `src/routes/+page.svelte` | observability / UX | Added `cast-ui:` log evidence for discovery, LOAD/follow, superseded requests, transport sends/skips, status death, errors, and stop. Cast UI distinguishes Connecting from Casting. |
| `src/lib/gamepad.svelte.ts` | input UX | Trigger repeat grace changed 220→500 ms and cadence 120→250 ms. |
| `src/routes/+page.svelte` | controller logic | L2/R2 now seek a fixed ±5 seconds per event instead of using a threshold-sensitive 1–5 second analog curve. |
| `scripts/verify-windows-runtime.ps1` | release safety | Refuses every Windows-GNU distributable (the loader can exist beside the raw exe yet be absent from NSIS), requires the FFmpeg sidecar, and launch-smokes the supported MSVC executable. |
| `.github/workflows/release.yml` | release safety | Runs the Windows runtime gate before publishing; portable ZIP now includes and verifies `ffmpeg.exe`. |
| `RELEASE_NOTES.md`, `CLAUDE.md` | UX / process | Corrected the local-build claim, documented the GNU packaging trap, and made the nightly's hardware-verification status explicit. |
| `CLAUDE_CODE_HANDOVER.md`, `docs/HANDOVER-2026-07-22-cast-broken.md`, `docs/PROJECT-LOG.md` | process | Recorded the confirmed cause, fix, verification, and still-pending hardware QA without rewriting the earlier failed-release history. |

## Behavior changes

- A successful cast handshake is immediately presented as a live connection,
  so initial LOAD no longer disables every subsequent cast feature.
- Changing the active item can reissue/recover the cast session even if the
  last frontend status snapshot was stale.
- The status poll can now repair stale UI state and honestly folds up a dead
  connection.
- Cast failures and early-return reasons are visible in the normal app log.
- L2/R2 gives one consistent five-second skip per squeeze; holding repeats only
  after a deliberate half-second hold.

## Risks / compatibility

- Chromecast protocol behavior cannot be proven without the owner's Sony
  Bravia. The change fixes a deterministic frontend/backend race confirmed by
  code, but hardware QA remains mandatory.
- Transport commands issued before the receiver provides a media session id
  are still intentionally dropped by the Rust actor; logs identify that case.
- Trigger seeking no longer varies with analog pressure. This sacrifices
  pressure-sensitive speed for predictable culling behavior.

## Verification actually run

- `npm run check` — pass, 0 errors and 0 warnings.
- `npm run build` — pass.
- `cargo check` — pass.
- `CARGO_BUILD_JOBS=2 npm run tauri -- build` — pass in 25m35s; local Windows
  executable and NSIS installer produced, **but runtime failed**:
  `WebView2Loader.dll was not found`. This artifact is invalid and must not be
  distributed. The new runtime gate rejects the entire Windows-GNU distribution
  path before handoff.
- `cargo fmt -- --check` — reports broad pre-existing formatting drift across
  untouched Rust modules; no bulk formatting was applied.
- Hardware cast/controller test — pending owner use.
