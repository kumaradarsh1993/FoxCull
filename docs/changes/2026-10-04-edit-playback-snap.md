# Edit: playback past the first clip, snapping that snaps

Nightly `v1.5.2-nightly.6` (fixes only). Reported by the owner while testing
nightly.5: clips spread over V1/V2 alternately, Play ran the first clip to
its end, then the preview looped that clip while the playhead sat at its
end; dragging clips "didn't snap".

## Intent

- Program playback must carry on across every clip, on any track.
- Snapping must be felt at every zoom level.

## Cause

- **The freeze.** A clip's length comes from the probe, which reads the
  container. Phone footage often has a container a few ms longer than its last
  frame. The video then stops (`ended`) a few ms short of the clip's out point:
  `sampleFromVideo` never saw the out point (tolerance 1 ms), and the next
  engine tick's `play()` on the ended video restarted it from 0. Result: the
  first clip looped and the playhead stayed at its end. `onMeta` only
  corrected the length when it differed by more than 10 ms. Reproduced in the
  harness with a 40.005 s probe on a 40.000 s file: stuck at 0:40.
- **A second mapping bug.** A clip on V2 that starts under a V1 clip is shown
  from where the V1 clip ends, so its program segment starts inside the clip.
  `sampleFromVideo` mapped source time through the segment's start instead of
  the clip's, so that clip jumped ahead by the overlap and was cut short.
- **Snapping.** The reach was a fixed 0.16 s: 1–4 px at the usual zooms. And
  a moving clip only snapped by its start edge.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/components/EditStudio.svelte` | logic | `sampleFromVideo`: maps through `clip.start`; an `ended` video ends its segment; a video that jumped back on its own is put back at the playhead. `engineTick` never calls `play()` on an ended video. `onended` on the program video. `onMeta` also clamps `outS` to the playable length for any difference. |
| `src/lib/components/EditStudio.svelte` | UX | Snap reach is 10 px on screen (`SNAP_PX / timelineScale`); a moving clip snaps by whichever edge is nearer (`snapMove`, also for audio clips); a dashed guide line shows where it snapped; the Snap chip is a toggle; ⌥ while dragging places freely. |
| `src/lib/dev/mock-ipc.ts` | process | `probe_media_info` reports 40.005 s for the 40.000 s sample, so the harness keeps covering the freeze. |

## Behavior changes

- Play runs through the whole timeline: clip to clip, track to track, through
  gaps, and stops at the end.
- Clips snap to clip edges, the playhead and 0 within 10 px, by either edge.
  The Snap chip turns it off; ⌥ skips it for one drag.

## Risks / compat

- Which clip shows where clips overlap is unchanged: the upper row (V1) wins,
  as before. NLEs like Premiere do the opposite (V2 over V1); worth asking
  the owner if that trips them up.
- Not run in the real app (WebKit); the harness is Chromium. The `ended`
  path is the standard media event in both.

## Verification actually run

- Harness, before the fix: 4 clips × 40.005 s, play from 0:37: stuck at 0:40,
  video `ended`.
- After: the owner's layout (V1 0 s, V2 38 s, V1 76 s, V2 114 s): play from
  0:36 crosses into the V2 clip at 0:40 showing its 2.7 s at playhead 40.7 s
  (it starts at 38 s), keeps going; from 2:31 it stops at 2:34 with Play
  shown again.
- Snapping: a clip dragged 1.7 s lands exactly on the neighbour's end
  (116.005 s), guide drawn while dragging, gone on release.
- `npm run check` 0/0, `npm run build`, `cargo test --lib` (50 pass).
