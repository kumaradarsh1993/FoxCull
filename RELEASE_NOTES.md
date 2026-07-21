# FoxCull v1.1.0-nightly.6 — video-view polish: fullscreen, seeking, calmer scrub

## Fixes in this build (over nightly.5)

- **Fullscreen `F` is now a clean 3-step cycle.** Press once for play mode with
  the filmstrip (dimmed ~20% so your eye stays on the photo), again for just the
  photo/video, again to exit. No more hunting the very bottom edge to coax the
  strip back — it's simply there or not. `Esc` exits from anywhere.
- **Shift+←/→ scrubs a video** ±5 s in Focus view (it used to only extend the
  selection). `,` / `.` still step too.
- **Grid skimming stopped misfiring.** Hover-scrub now only runs on the clip you
  actually **click to select** — sweeping the pointer across a wall of videos no
  longer kicks off (and cancels) a build on every tile. Click a clip to arm it,
  then hover it to skim frames.

## The video view gets out of the way

- **The transport bar is gone until you want it.** Open a video and you see the
  picture edge-to-edge with just a thin, quiet progress line at the bottom.
  Move your pointer to the bottom of the frame and the full controls — play,
  time, scrub track, Info, Clip tools — slide up; move away while it's playing
  and they tuck back down. It's the VLC/YouTube feel. Prefer the old
  always-visible bar? **Settings → Minimal video bar → Off.**
- **The first frame is sharp now.** The still shown before a video plays (and
  with autoplay off) used to be a pixelated low-res blowup on a big screen.
  It's now generated at high resolution for Focus and full-screen. Grid
  thumbnails are untouched, so this costs the grid nothing.
- **Play mode (F) is a clean full picture.** The bottom filmstrip no longer
  sits in play mode — reach the very bottom edge and it slides up when you want
  it.

## Video previews respect you again

- **Live Scrub OFF now means OFF.** The previous nightly built a scrub
  filmstrip for every video the moment you opened it in Focus — a minute-plus
  of disk and CPU churn per clip on a hard-drive library, toggle be damned.
  Opening a video now does zero preview work: anything already built still
  shows for free, and the plain seek bar just works.
- **Live Scrub ON got much faster and politer.** The filmstrip builds only
  when you actually reach for the timeline (hover, drag, or step keys) — and
  it now decodes on your GPU, so a build that took ~70 seconds lands in
  roughly 10–20. Fewer, smarter frames too; you won't see the difference,
  your disk will.

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
