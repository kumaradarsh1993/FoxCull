# 2026-07-21 (2) — sprite unification, fullscreen docks, measured build cost

## Intent

Owner's second round of feedback on the same session's nightly.7 build. The
arm-then-hover fix worked ("scrub seems to be working fine"), which exposed the
next layer: the build visibly **restarted** when a skimmed clip was opened in
Focus, fullscreen dropped a left/right filmstrip entirely, the portrait hover
preview was oversized, and builds were slow enough (~40 s on a 1.46 GB clip) to
question the whole approach. Also the standing question — *is this a hard-drive
problem?* — which was answered by measurement rather than assertion.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/thumbnail-loader.ts` | **architecture** | New `loadVideoFilmstrip`/`cancelVideoFilmstrip`. The dense filmstrip now goes through the shared queue, so grid and Focus requests for the same clip **join** one promise. `loadVideoScrubstrip` retained read-only for legacy caches. |
| `src/lib/components/Thumb.svelte` | **architecture** | Grid tiles build the *dense* filmstrip (`f`) instead of the light `s` strip — one artifact for both views. Paints either cached sprite for free on mount. Activity id `scrub:` → `strip:`. |
| `src/lib/components/Thumb.svelte` | **logic (bug)** | Teardown no longer cancels the build of an **armed** tile. Double-clicking into Focus unmounted the tile, which cancelled the build, which is half of why it "restarted at 10%". |
| `src/lib/components/Loupe.svelte` | **logic (bug)** | `ensureFilmstrip` goes through the shared loader instead of a direct `invoke`. A direct invoke took a fresh backend cancel-token (`new_sprite_token` cancels any in-flight build for the same key) — the other half of the restart. |
| `src/lib/components/Loupe.svelte` | logic | `strip.src` is now hydrated to an asset URL at exactly one point per path; `stripSrc` no longer re-converts. |
| `src/lib/components/Loupe.svelte` | UX | Timeline hover preview sized by both edges (`PREVIEW_MAX_H` = 132) — a 9:16 clip was rendering 200×356 and covering a third of the picture. |
| `src/routes/+page.svelte` | **UX (bug)** | Fullscreen mode 1 keeps the filmstrip in **whichever** dock is set (bottom/left/right), dimmed, **and resizable**. Previously `.app.fs` hid `.lstrip`/`.rstrip`/`.vsplit`/`.hsplit` outright, so a side dock vanished on `F` and the bottom dock lost its resize grip. The tree splitter got its own `.treeSplit` class so it alone still hides. |
| `src/routes/+page.svelte` | UX | Bare-filename `title` removed from grid and strip tiles (the OS tooltip popped up over the tile being skimmed). Stack tooltips kept. |
| `src/routes/+page.svelte` | logic | Neighbour prefetch builds the unified sprite. |
| `src-tauri/src/commands.rs` | logic | Prepare (`warm_thumbnails` heavy) builds `ensure_filmstrip`, not `ensure_scrubstrip`. |
| `src-tauri/src/commands.rs` | **UX (bug)** | `reveal` drives `explorer.exe /select,<path>` for files on Windows; the opener plugin reused an existing Explorer window and left nothing highlighted. Directories unchanged. |
| `src-tauri/src/video.rs` | **perf** | `SPRITE_PARALLEL` (const 2) → `sprite_parallel()` = `(cores/3).clamp(2,4)`. Justified by benchmark, not intuition — see below. |
| `docs/design/precache-policy.md` | process | Rewritten for one-sprite; new §5.1 records the benchmark. |
| `BACKLOG.md` | process | Grid info overlay (owner: don't build yet) + "sprite extraction without a process per frame". |

## The measurement (why the parallelism constant moved)

Bundled ffmpeg, Osmo 4K60 HEVC Main10 `.mov`, **internal HDD**, 12-core machine,
same command line the sprite builder issues:

| Test | Result |
|---|---|
| 6 cold frames, `-hwaccel auto` | 5.92 s (~0.99 s/frame) |
| 6 cold frames, software decode | 6.22 s |
| the SAME 6 frames, OS cache warm | 5.04 s |
| 1 frame, `-f null` (no scale/encode) | 0.80 s of the 0.99 s |
| 12 cold frames at parallel 2 / 4 / 6 | 6.16 / 4.40 / 3.61 s |

**Sprite building is CPU/process bound, not disk bound** — cache-warm is only
15% faster than cold, so the drive is not the constraint (which matches the
owner finding his SSD barely beat his HDD). ~0.8 s of each frame's ~1.0 s is
ffmpeg startup plus re-parsing a multi-GB container index, paid once per frame.
hwaccel is worth ~5% on a single keyframe, so removing it would be a wash. It
parallelises, so parallelism was the change to make.

This **contradicts the existing doctrine comment** that framed the old value of
2 as protecting a USB SSD's read queue; the comment has been replaced with the
data. The clamp keeps a 4-core XPS 13 at 2.

## Behavior changes

- Skimming a clip and then opening it in Focus continues one build instead of
  restarting a second, different one. Total extraction per video is halved.
- Folders Prepared before this change still skim from their cached `s` strips;
  nothing re-extracts.
- Fullscreen honours the filmstrip dock and stays resizable.
- Portrait hover previews are no longer oversized.
- No filename tooltip on grid/strip tiles.
- Show in Explorer selects the file.

## Risks / compat

- **Cache churn:** clips whose only cached sprite is the legacy `s` strip will
  build an `f` strip the first time they're skimmed *after* this build. Existing
  `s` files are never deleted and still paint, so the visible behaviour is never
  worse — but a previously-Prepared folder is no longer fully "prepared" in the
  strict sense. Re-running Prepare brings it fully forward.
- **Parallelism** is up to 4 concurrent ffmpeg processes inside one build, and
  sprite builds are still serialized process-wide, so the ceiling is 4 — not
  4 × N. Measured as a win on 12 cores; the clamp protects small machines.
- `explorer.exe /select` opens a new Explorer window rather than reusing one.
  That is the trade for actually selecting the file.

## Verification actually run

- `npm run check` — 274 files, **0 errors, 0 warnings**.
- `cargo check` — clean.
- The ffmpeg benchmark above, executed against the owner's real 4K60 HEVC
  footage on the machine in question.
- Not run: `cargo test` (GNU-toolchain link limit — CI only), local installer
  build (CI on tag).
