# FoxCull v1.2.0-nightly.3

A bug fix that deserved the deep look you asked for, plus the smaller things
around it.

## The picture freezing while the audio played on

You found a real one. Clicking or dragging the timeline during playback could
leave the frame stuck while the sound carried on — intermittently, and once it
started on a clip it kept happening on that clip.

**What was happening.** Releasing the playhead starts two things at once: FoxCull
decodes the exact frame you landed on, and the video itself seeks to it.
Whichever finishes second was supposed to hand the picture back to live video.
If the video won that race, the still was cleared correctly — and then the
late-arriving decoded frame put itself back on screen, with nothing left to ever
take it down again. Hence a frozen picture over a perfectly healthy, playing
video.

That also explains why it felt random but then stuck: which side wins depends on
how fast that particular file seeks, so a clip that loses the race once loses it
every time.

**The fix** tags each decode with the hand-off it belongs to, so a frame that
arrives after the video has already taken over is discarded — it would only have
shown the frame the video is already displaying. On top of that, FoxCull now
checks four times a second that a playing video is never sitting behind a still,
so anything similar recovers on its own instead of freezing.

## Scrubbing readouts

- The timestamp that floated in the middle of the picture while dragging is
  **gone** — it read as a glitch, not a readout.
- The transport's own clock takes over the job: the current position is bigger,
  white and bold, and legible over a bright frame.

## Hide the filmstrip — press B

The filmstrip now hides and comes back, like the folder panel: press **B**, or
click the small chevron on the divider above it. What's left behind is a 14-pixel
rail, so there is always something to click to bring it back. Hiding remembers
where the strip was docked, so it returns to the bottom, left or right as you had
it.

## "Is it doing anything?"

Opening a folder whose thumbnails aren't built yet used to render in silence —
sometimes for ten or fifteen seconds — because only bulk jobs like Prepare ever
reported progress, and video posters weren't part of those at all. Loading now
reports itself like any other job, so you can see it working and roughly how far
along it is.

The activity indicator has also moved to the **bottom-left**, where a status
readout belongs, and it no longer disappears when you collapse the folder panel.

## Still worth testing

Glimpse's pacing — tell me if 40× is right as a default. And phone (H.264)
video, which takes a slightly different path inside and still hasn't met a real
file.
