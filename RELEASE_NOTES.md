# FoxCull v1.1.0-nightly.3 — the video preview rework

**This build reworks the video preview system end to end.** Hover-scrubbing
over video thumbnails and seeking in the Focus player are the headline: both
were rebuilt for latency, cancellation, and Final Cut-style fluidity.

## Video previews, reworked

- **Live Scrub is fast now.** Hover strips used to be built by decoding *every
  frame* of a clip (minutes for a long 4K60 HEVC Osmo clip, and it couldn't be
  stopped once started). They're now extracted with per-frame keyframe seeks —
  a couple of seconds per clip, any length, with a live "scrub %" tag on the
  tile while it builds.
- **Moving on actually stops the work.** Leaving a tile cancels that clip's
  build mid-flight; switching folders cancels the whole backlog. Sweeping the
  cursor across a row of videos no longer piles up work that hangs the drive.
- **Prepare now covers videos properly.** The Prepare button pre-builds video
  posters *and* hover scrub strips, so a prepared folder skims instantly, like
  plugging an SSD into Final Cut. Scrub caches stay tiny (well under 0.05% of
  the footage size) and are cleaned up when files are deleted or moved.
- **Focus seeking is fluid.** The seek bar always gets a frame filmstrip now;
  while you drag, the frame under the cursor fills the whole stage instantly
  (no decoder in the loop), and releasing lands a frame-accurate seek. Held
  step-seeks shuttle smoothly instead of stalling per press. Clips also paint
  their poster immediately instead of opening on black.
- HEVC proxy conversion now uses hardware decode where available (NVDEC on the
  Alienware's GTX 1070), so "Convert & play here" is much quicker.

## New: cull from the couch with a PS5/PS4 controller

Pair a DualSense/DualShock over Bluetooth (step-by-step guide in Settings →
Controller) and review on the TV: d-pad to move, ✕ Pick, ○ Reject, △ clear
marks, □ play/pause, L1/R1 grid⇄Focus, analog L2/R2 shuttle inside videos,
Options for fullscreen play mode, Create for an on-screen button guide.
**Every button is remappable** — press-to-bind in Settings → Controller. The
mouse's Back/Forward thumb buttons go through the same mapper.

Fullscreen (F) now keeps the bottom filmstrip — photo + strip is the new
"play mode" layout for TV review (set Filmstrip to Off for a bare photo).
