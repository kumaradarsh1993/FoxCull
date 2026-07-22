<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body, and the GitHub release title already carries the tag. A
     heading here goes stale the moment you forget to bump it — which is exactly
     what happened on nightly.5 and .6, both of which announced themselves as
     "nightly.4". Write what changed; let the tag say which build it is. -->

> **This is the A/B build — the one WITHOUT the Chromecast changes.** It is
> v1.2.0 plus the new controller layout, the in/out-point fix and the filmstrip
> behaviour, and nothing else. Casting behaves exactly as it did in v1.2.0.
> The next nightly is this build plus the cast work; install that one unless
> you're trying to isolate a casting problem.

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

## The filmstrip gets out of the way

It hides itself in Grid, where it just repeats what's already on screen, and
comes back in Focus, where it's the only way to see where you are in the folder.
Toggle it by hand and that view remembers your answer.

## Everything from v1.2.0

Instant full-resolution scrubbing with nothing pre-built, Glimpse (Ctrl+Space),
the reliable cast session, and the Edit-mode memory fix — all unchanged from the
v1.2.0 stable release.
