<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

Fixes found while testing the previous nightly. Nothing new to learn.

## Edit

- **Play no longer stops at the end of the first clip.** With clips on more
  than one track, Play ran the first clip to its end, then the preview kept
  looping that clip while the playhead stayed put. Many phone clips end a
  few milliseconds before their stated length, and the player waited for a
  moment that never came. Play now runs clip to clip, track to track, and
  stops at the end of the timeline.
- **A clip on V2 that starts under the end of a V1 clip** now picks up from
  the right moment when the V1 clip ends. Before, it jumped ahead and was cut
  short.
- **Snapping works.** Dragging a clip, or the edge of one, catches on the
  other clips' edges, the playhead and the start of the timeline. Both ends
  of a moving clip snap, and a dashed line shows where. It used to reach only
  a few pixels, so it seemed off. Click **Snap** to turn it off, or hold ⌥
  (Alt) while dragging to place a clip freely.

## Reel

- Dropping a new song on the beat board takes you back to step 1 with the new
  song, instead of a blank board.
