# FoxCull v1.1.0-nightly.7 — skimming that actually works, honest deletes, undo that reaches into the Trash

## Live Scrub: the whole thing, gone over properly

Skimming had accumulated three separate faults that hid each other. All three
are fixed.

- **The clip you click is now the clip that skims.** Selecting a video arms it
  for skimming — but the pointer is already sitting on the tile when you click,
  so the app never noticed the hover it had been waiting for, and quietly built
  a preview for every tile you'd swept *past* and none for the one you'd chosen.
  You saw the scrub bar appear and the picture never change. Click a clip, move
  across it, frames follow.
- **Portrait clips skim at a sane speed.** The timeline used to be squeezed into
  just the part of the tile the picture covers — on a 9:16 phone clip that's a
  narrow strip down the middle, so a centimetre of pointer travel jumped a third
  of the video while the space either side did nothing. Skimming is now measured
  across the whole tile, portrait and landscape alike.
- **A preview being built is no longer thrown away.** Drifting off the tile used
  to cancel a build that was seconds from finishing, and coming back started it
  from zero — which is why it could feel like it never worked at all. Once
  you've selected a clip, its preview finishes.

**In Focus view, the preview starts when you open the clip.** Previously it
waited until your pointer happened to touch the seek bar, then made you watch a
ten-second build with no indication of what was happening. Now, with Live Scrub
on, it begins immediately and a small counter at the top-left tells you how far
along it is. Live Scrub off still means genuinely off — opening a video does no
preview work at all.

**New: pre-build nearby clips** (Settings, appears when Live Scrub is on). While
you're watching one clip, the three either side quietly get their previews
ready, so stepping to the next one can be skimmed the moment you arrive. Off by
default — it's real background work, and on a slow drive you may not want it.

## Deletes tell you the truth now

Deleting a couple of large clips could fail with "still in use?" when nothing
was using them. The real cause was Windows refusing permission — the same reason
Explorer asks for administrator rights on those files — and the app was guessing
wrong and telling you to close a preview that didn't exist.

- A file that's genuinely open by another program, and a file Windows won't let
  us touch, now say so separately, by name, in a message you can read in full.
- A read-only file is un-set and retried automatically instead of failing.
- If you're hitting the permissions case on a whole drive, it's an ownership
  leftover from reinstalling Windows, not something inside FoxCull — taking
  ownership of the folder once (Properties → Security → Advanced → Change owner,
  apply to contents) clears it for good.

## Undo can bring deleted files back

Ctrl+Z after a delete now offers to restore that batch out of the in-app Trash —
with a confirmation first, showing how many files will come back, because
stepping back through a long undo history shouldn't silently start moving files
around. Deletes are never *re*-done by Ctrl+Y, for the same reason.

## Filmstrip on the left

The filmstrip can now sit between the folder tree and the picture, not only at
the bottom or on the right. **Settings → Filmstrip → Left.** Drag its edge to
resize as usual.

## Prepare, but only what you want

Prepare still covers the whole folder when you click it. The new ▾ beside it
narrows the job: just the selection, just the videos, or just the photos & RAW —
each showing how many items that is. Useful when a folder is twenty 4K clips and
you only care about three of them.
