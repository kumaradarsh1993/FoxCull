# FoxCull v1.1.0-nightly.5 — HEIC fixed, safe deletes, cast that follows you

## HEIC photos work now

- **Phone HEIC photos show and open.** Samsung/iPhone `.heic` files were
  showing a grey "HEIC" tile in the grid and "can't preview this file" in
  Focus, while JPEGs worked fine. The cause: phone HEICs are stored as a *grid
  of tiles*, and the decode step used a filter that can't sit on top of the
  tile-stitching ffmpeg does to reassemble them — it failed before producing
  anything. Fixed; full-res HEIC now decodes, scales, and rotates correctly
  everywhere. No Windows codec packs involved or needed — HEIC is handled
  entirely inside FoxCull.
- When an image genuinely can't be decoded, the reason now lands in the log
  instead of being swallowed — the next such bug is a one-look diagnosis.

## Deletes can't freeze the app anymore

- Deleting a huge clip that something was still reading (a preview build, a
  playing video) used to grind the whole app into "not responding" — the
  fallback quietly copied the entire multi-GB file and then failed anyway.
  Now: background work is cancelled first, the delete runs off the UI thread,
  and if a file is still locked you get a clear "couldn't delete — still in
  use?" notice instead of a frozen window.

## Casting follows you now

- **Start casting once — the TV then shows whatever you're on.** Move through
  photos and videos with the arrow keys and the TV keeps up (small debounce so
  holding a key doesn't spam the TV). Photos and videos both, one session.
- **HEIC and RAW cast correctly** (previously the TV got a format it can't
  render and showed nothing) — they cast their high-res preview instead.
- Videos still stream the untouched original file — your 4K60 HEVC plays at
  full native quality via the TV's own decoder.
