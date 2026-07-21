# FoxCull v1.2.0-nightly.1

**Scrubbing a video in Focus now shows the real frame under your cursor.**

Grab the playhead on a 546-second 4K60 clip and drag. The picture follows the
cursor at full resolution — no waiting, nothing to prepare, no cached frames,
no progress chip. It works the instant the clip opens, on any clip.

This is the feature the last release was a checkpoint for, and it arrived by a
different route than announced. v1.1.0 said the next step was to replace the
video player with a native one. That was tried properly and abandoned for a
better answer — the full story is in the repo, but the short version:

- Playback in the web view was never the problem. Your clips already decode on
  the graphics card.
- The problem was **how seeking was asked for**. Every drag position asked for
  an exact frame, which means rewinding to the previous keyframe and decoding
  forward — thirty times a second, while you drag. That is what lagged.
- Players like VLC feel instant because while you drag they show the nearest
  *keyframe* — cheap — and only compute the exact frame when you let go.

FoxCull now does exactly that, inside the app, with a decoder of its own. On the
test machine a full-resolution 4K frame lands in about 40 milliseconds, and the
exact frame you release on takes about 150.

### What you'll notice

- **Dragging is smooth and sharp.** The old preview frames were small stand-ins
  extracted in advance; these are the real thing at full size.
- **No preparation.** No "scrub preview 40%" chip, no build to wait through, no
  disk filling with preview frames. Opening a clip is enough.
- **Hovering the timeline** shows a real decoded frame in the thumbnail too.
- **Letting go doesn't jump.** The frame you released on stays on screen until
  the video has caught up to that same frame underneath.
- **Live Scrub is now about grid tiles only** — skimming a clip by hovering its
  thumbnail in the grid still uses prepared frames, because a video decoder per
  tile isn't practical. Settings labels it that way now.

### If a clip can't do it

Some containers and codecs can't be decoded this way. Those clips quietly fall
back to the previous behaviour — same as v1.1.0, nothing to configure. There's a
"Focus scrub" setting (Live decode / Sprites) if you ever want to compare, but
you shouldn't need it.

### Please test

This is the first build with the new engine wired into the interface. The engine
itself has been measured hard, but the on-screen behaviour — dragging, releasing,
hovering, fullscreen, portrait clips — wants real hands. Try it on the Osmo
footage and on phone clips; H.264 phone video takes a slightly different path
inside and hasn't been exercised on real files yet.
