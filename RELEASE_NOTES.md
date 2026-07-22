<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body, and the GitHub release title already carries the tag. A
     heading here goes stale the moment you forget to bump it — which is exactly
     what happened on nightly.5 and .6, both of which announced themselves as
     "nightly.4". Write what changed; let the tag say which build it is. -->

> **Install this one.** It is the previous nightly plus the Chromecast work.
> The `-nightly.1` build is a deliberate A/B twin with the cast changes left
> out — only reach for it if casting misbehaves and you want to know whether
> these changes caused it.

Everything below carries forward from the v1.2.0 stable release; the new
sections are the controller, in/out points, the pairing guide, the filmstrip
and the TV following your player.

## Scrubbing works the way it should have from the start

Drag the playhead on a 4K60 clip and the picture follows your cursor. No lag, no
waiting, full resolution.

**And nothing is pre-built any more.** No "preparing scrub frames", no progress
bar to sit through, no cache written to your drives. Open a clip and skim it
immediately — including the first time you ever touch it.

What changed underneath: FoxCull now decodes the exact frame you're pointing at,
on demand, using your GPU. The old approach extracted hundreds of small frames
to disk first and painted those. That was minutes of work per folder for a
blurry result.

This works in the **grid** too — click a clip to select it, then hover across
its tile to skim through it.

## Glimpse — press Ctrl+Space

A long clip's cover frame tells you almost nothing. Glimpse plays through it
fast so you can see what's in it, then stop and cull.

It runs at a **plain multiple of real time**, like a player's 2x or 5x — so the
pace is the same on every clip. At the default 5x, a 20-second clip takes 4
seconds and a 10-minute clip takes 2 minutes. **Settings → Glimpse speed**
adjusts it from 2x to 10x.

Press Ctrl+Space again, hit Space, or grab the playhead to stop; it lands
cleanly wherever you stopped.

## Casting to your TV is reliable now

Quality was always good — FoxCull sends the original file untouched and lets the
TV decode it. But the session used to fall apart as you browsed. Three separate
faults, now fixed:

- **The receiver app on your TV closes itself when idle**, and FoxCull only ever
  launched it once. After it closed, everything you selected silently went
  nowhere while the button still said "Casting". It's relaunched as needed now.
- FoxCull marked a file as "on the TV" when it *decided* to send it rather than
  when it actually went — so a failed send looked like a success and nothing
  retried.
- Two fast presses of → could arrive out of order, leaving the TV on the earlier
  shot.

It now also notices when a session has genuinely ended instead of claiming a
connection that's gone.

## Culling from the couch, on a PlayStation controller

The controller layout has been rebuilt around a full review session done from
across the room — you shouldn't need to walk back to the laptop for anything.

- **Press the touchpad** to open the selected shot in Focus, press it again to
  go back. It's the pad's Enter key.
- **✕ rejects, △ picks, ○ clears every mark** on the shot, **□ plays and
  pauses**.
- **Ratings live on the left stick** — flick up, right, down, left for one to
  four stars, click the stick in for five.
- **Colour labels live on the right stick**, the same way: blue, purple, red,
  green, and yellow on the click. (Same order as the 6/7/8/9/0 keys.)
- **L1 and R1 mark a video's in and out points**; the triggers still shuttle.
- **Create/Share hides and shows the filmstrip**, the **PS button** goes
  fullscreen, and **Options** brings up the button guide.

All of it is still remappable, and the Controller panel now has a **button
tester** — press anything and it tells you what the pad reported, so you can
check the PS button and touchpad work on your setup before relying on them.

Because the layout changed shape, **any bindings you'd customised are reset
once** to pick up the new defaults.

## The pairing guide is actually readable now

Two side-by-side cards — pairing to this PC, and pairing back to your PS5 —
each a short numbered list instead of one block of prose. The console direction
is documented for the first time: plug in USB-C, press PS, done.

## Your video in/out points are saved now

Mark an in and out point on a clip, move to the next one, come back — the
markers are still there, committed or not. They're stored per drive alongside
your ratings and labels.

They were never being saved before. Not "lost on navigation" — never written at
all, and the failure was being discarded silently, which is why it looked like a
missing feature rather than a bug. The same fault also disabled the **Cut**
button in Focus, which should now work.

## The TV follows what you're doing

With a cast session running, pausing or scrubbing on the laptop now pauses and
scrubs on the TV. Casting already followed whatever you selected — including in
the grid — and that's unchanged.

## The filmstrip gets out of the way

It hides itself in Grid, where it just repeats what's already on screen, and
comes back in Focus, where it's the only way to see where you are in the folder.
Toggle it by hand and that view remembers your answer.

## Fixes

- **A frozen picture with the audio still playing.** Clicking the timeline
  during playback could leave the frame stuck. Two things race there — the
  decoded frame and the video's own seek — and when the seek won, a late frame
  put itself back on screen with nothing left to take it down. Fixed, with a
  second check four times a second so anything similar recovers on its own.
- **Edit mode on a big folder.** Opening Edit on 229 clips used ~8 GB and left
  every item reading "Reading details..." — items past the 80th were in fact
  never loaded at all. Now ~0.5 GB, and everything loads as you scroll.
- **Opening a clip no longer blips**, showing one frame and then swapping to
  another a moment later.
- Thumbnail loading reports progress instead of leaving you looking at a blank
  grid wondering if anything is happening.

## Smaller things

- **Press B** to hide or show the filmstrip; a slim rail stays behind to bring
  it back. It remembers which side it was docked to.
- The activity indicator moved to the **bottom-left**, and no longer disappears
  when you collapse the folder panel.
- The timestamp that floated over the middle of the picture while dragging is
  gone; the transport clock is bigger and readable over a bright frame.
- **Arrange** has icons beside Sort/Group/Subgroup, and the sort-direction arrow
  is a proper button.

## Known limits

- Skimming needs a codec your GPU can decode. Everything else falls back to the
  older behaviour automatically — you'll see it disable itself, not break.
- Edit mode's source list loads as you scroll, but a very large folder scrolled
  end to end still adds up. Fine at a few hundred clips.
