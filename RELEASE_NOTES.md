# FoxCull v1.2.0-nightly.2

Built on your feedback from nightly.1. The scrubbing engine now runs everywhere
it should, and there's a new way to see what's inside a long clip.

## Glimpse — press Ctrl+Space

The culling problem: a long clip's cover frame tells you almost nothing, and
dragging the playhead to find out is work you have to do by hand.

**Glimpse sweeps the whole clip for you.** Press `Ctrl+Space`, or the ⏩ button
beside play, and FoxCull flips through the clip's keyframes fast enough to be
quick and slow enough to read — the way you'd thumb through footage in an
editor. A nine-minute clip takes about fourteen seconds. Press it again, hit
space, or grab the playhead to stop; it lands cleanly on wherever you stopped.

Short clips are never rushed: however high the speed, a sweep always takes at
least a few seconds. **Settings → Glimpse speed** runs from 10× to 100× real
time if you want it brisker or calmer.

## Skimming in the grid is decoded now too

Hovering an armed tile in the grid used to paint small frames extracted in
advance. It now decodes the real frame under your cursor, exactly like Focus.
Full resolution, and it works on a clip the first time you touch it.

**So video pre-caching is gone entirely.**

- **Prepare** no longer builds scrub frames for videos — just the poster. On a
  folder of 4K clips that was most of the work it was doing.
- The "Pre-build nearby clips" setting is gone; there is nothing left to
  pre-build.
- Nothing is written to your drives for scrubbing any more, anywhere.

Clips whose codec can't be decoded this way quietly fall back to the old
behaviour, building frames on demand as before.

## The blip when opening a clip

You spotted a frame appearing and then being replaced a few milliseconds later.
It was real: the still shown while a clip opens was taken **one second in**,
while the video itself starts at **zero** — two different moments of the same
clip, swapping. The Focus still is now taken at zero, so there's nothing to see.
(The grid's thumbnail keeps its one-second frame, since frame zero is often
black.)

## Smaller things

- **Arrange** has icons beside Sort, Group and Subgroup, and the sort-direction
  arrow is now a proper button instead of a faint mark.
- **Prepare** is narrower, and its bolt is bigger and gold.

## Worth testing

Glimpse's pacing is the main thing — tell me if 40× feels right as the default,
or if it wants to be quicker. Also worth a look: grid skimming on portrait
clips, and phone (H.264) video, which takes a slightly different path inside
and hasn't met a real file yet.
