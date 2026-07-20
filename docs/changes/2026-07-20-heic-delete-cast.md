# 2026-07-20 — HEIC decode fix · delete-hang fix · cast follows browsing

**Author:** base-machine agent (Fable) · **Tag:** rolls into v1.1.0-nightly.5
**Scope note:** the Focus-filmstrip/Live-Scrub regression is deliberately NOT
touched here — RCA delivered separately, fix awaits owner sign-off.

## Intent

Three user-reported failures from live use on the internal-HDD library:
HEIC files unusable (grid placeholder + "can't preview"), app froze
("not responding") deleting a 17 GB in-use clip, and casting was one-shot
(TV didn't follow while browsing).

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/video.rs` → `decode_still()` | **logic (bugfix)** | `-vf` → `-filter_complex` for the scale. Tiled phone HEICs (Samsung/iPhone store a grid of HEVC tiles) make ffmpeg auto-insert a complex filtergraph to stitch tiles; a simple `-vf` on top errors out ("Simple and complex filtering cannot be used together") so every tiled HEIC failed to decode. Also: stderr now captured into the error string instead of `Stdio::null()` — the failure was invisible in `foxcull.log`, which is why it survived this long. |
| `src-tauri/src/commands.rs` → `move_into_recycle()` | **logic (safety)** | Removed the copy+delete fallback after a failed rename. Recycle dir is always same-volume, so rename failure ⇒ file locked/in use, NOT cross-device; the fallback was copying 17 GB on one HDD spindle and then failing the remove anyway. Now fails fast with a clear "file is in use" error. |
| `src-tauri/src/commands.rs` → `dispose_rejected()` | **architecture (threading)** | Sync → `async` command (returns `Result<TrashOutcome, String>`; frontend contract unchanged — Ok unwraps to the same shape). Sync Tauri commands run on the main thread; the slow dispose froze the window event loop → "not responding". |
| `src/routes/+page.svelte` → `executeDelete()` | **logic** | Before disposing: `cancelAllSprites()` + `cancelWarm()` + 350 ms drain (sprite builds cancel between frame extractions), so background ffmpeg releases file handles before the rename. After: failures surface via `activity.error` instead of silently leaving files in the grid. |
| `src/routes/+page.svelte` → cast block | **logic + UX** | New `castDevice` retained for the session; `$effect` on the active item re-LOADs it to the same TV (350 ms debounce so key-holds send one LOAD, not 20). `castablePath()`: videos + web-safe stills (jpg/png/webp/gif/bmp) cast the original; HEIC/RAW/TIFF cast the cached 1920 px loupe JPEG (the Default Media Receiver's Chromium cannot decode HEIC/RAW — previously cast as `image/heic`, rendering nothing). Backend untouched: `cast_start` already reuses the live connection for same-device LOADs. |
| `RELEASE_NOTES.md`, `docs/DECISIONS.md`, `docs/changes/*`, `CLAUDE.md` | **docs/process** | This ledger convention (see CLAUDE.md → "Per-push change ledger"); ADR log seeded with the HEIC-decode-strategy and cast-quality decisions. |

## Behavior changes visible to the user

- HEIC/HEIF thumbnails render and open in Focus (no OS codec involvement).
- Deleting an in-use file: no freeze; either succeeds after background work is
  cancelled, or reports "N files couldn't be deleted (still in use?)".
- Cast: start once, then the TV mirrors whatever you browse — photos and
  videos, including HEIC/RAW (via their preview JPEGs).

## Risks / compatibility

- `decode_still` is also used by RAW-adjacent still paths only for HEIC/HEIF
  (thumbs.rs gate) — no other formats routed through it; JPEG/PNG untouched.
- `dispose_rejected` return-type change is invisible to JS (`invoke` resolves
  Ok identically); no other caller exists (`grep` verified).
- Cast follow only fires while `castStatus.connected` — zero cost otherwise.

## Verification

- `cargo check` clean; `svelte-check` 0 errors / 0 warnings; `npm install`
  synced (`@types/node` from the audit merge).
- HEIC: exact old command reproduced the failure against a real Samsung
  Note 10+ `.heic` from the F: library; new command decodes full-res
  3024×4032 and scales correctly (verified with the shipped ffmpeg binary).
- Delete/cast: logic verified by reading all call sites; live verification on
  the owner's hardware pending the next nightly install.
