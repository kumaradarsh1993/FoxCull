# In/out segments everywhere, and the music-synced reel mode

**Status:** owner spec, 2026-10-04 (voice brief, captured close to their
words). A major item. Builds on `docs/design/edit-window-rework.md`.
**Built in `v1.5.2-nightly.5`**: the decisions this spec left open, and what
was verified, are in `docs/changes/2026-10-04-segments-and-reel.md`. Still to
do from here: the landscape crop (below), and tuning the beat detection on
the owner's real songs.

FoxCull has three edit workflows:

1. **Merge** ("dump the Osmo clips into one file for YouTube"): quick, clean,
   what LosslessCut was used for. Its own window.
2. **Edit** (Premiere-like): the general, robust timeline. Its own window, fed
   by the library.
3. **Reel** (new): a quick "Instagram hack": clips cut to the beats of a song.
   Its own window, its own workflow, not bolted onto Edit.

## A. In/out points in the library (cleanup)

The library is where clips are chosen and trimmed, for all three workflows.
In Focus, on a video:

- `[` marks an **in**, `]` marks an **out**. Between them is a **segment**,
  drawn as a highlighted span on the scrub bar (or a layer right next to it),
  so it's always clear where the in and out points are.
- **Several segments per clip** (a 20-minute Osmo clip may give two, three,
  four pieces). The keys follow the playhead and build segments in order:
  - `[` with no open segment starts a new one (its in).
  - `[` again while that in is still open **moves** the in (to wherever the
    playhead is now, left or right). No new segment.
  - `]` closes the open segment (its out). `]` again moves that out.
  - Then `[` again starts the **second** segment, `]` closes it, and so on.
  - Typically while playing or scrubbing forward, but marking also happens
    after moving the playhead by mouse to the exact spot.
- **Segments never overlap.** (Decide what happens when a new in falls inside
  an existing segment: e.g. it moves that segment's in instead, or is refused
  with a message — never two overlapping segments.)
- **Adjust by dragging** an in or out marker; dragging steps **frame by frame**
  so the exact frame can be nailed down.
- **Remove:** right-click a marker → "Remove in point" / "Remove out point";
  right-click the span between them → "Remove this segment"; also "Remove all
  segments". (Work out the details.)
- The owner remembers earlier trouble drawing overlays on the video player;
  if the UI hits limits, say so and agree a way around them.

### Carrying segments

- Drag a clip (grid tile or **from Focus**) to the Edit window: all its
  segments go on the timeline, side by side (no segments = whole clip).
- **Drag one segment** (its highlighted span) → only that segment goes.

## B. Merge (mode 1) with segments

Today Merge joins the selected clips whole ("blindly", after the
compatibility check). With segments:

- The list gets a **Segments** column. A clip with none says so ("No segments
  marked") and goes in whole.
- A clip with segments gets a **checkbox**: checked = only its segments go in
  (in order); unchecked = the whole clip.
- **Default: checked** (segments). The checkbox exists because segments are
  sometimes marked for another use (an Instagram cut) and this merge wants
  the whole clip.
- A **header checkbox** for the column applies to all clips that have
  segments: check all / uncheck all, and shows the partial (indeterminate)
  state when mixed. Only clips with segments have a checkbox.
- Selecting a row: the **preview** (top right) loads that clip **from its
  first in point** and plays only the checked segments, one after another
  (the whole clip when unchecked). **Scrubbing** (hover/drag) works in it
  too: it's for scrubbing through, not just playback.
- Right-click a row → **"Show in library"** (focus that clip in the library
  window). (Not "Show in folder".)
- Lengths and sizes in the summary follow the choice (segments vs whole).
- The lossless join of segments needs keyframe-accurate cuts or a re-encode:
  decide and say which in the window (see Notes).

## C. Reel (mode 3): clips cut to the beat of a song

**Use cases.** (a) A couple of dozen 5–10 s portrait clips shot on the S23
Ultra, meant to be synced to music with cuts on beats. (b) Long Osmo Pocket 3
clips (often landscape 4K60) with in/out segments marked, clipped into a
reel. Reels are portrait.

**Entry.** Select clips in the library, right-click → "Create a reel synced to
music…". A new window.

**Step 1 — clips and song.**
- The clips as a list, **reorderable**. Segments as in B: checkbox per clip;
  checked → each segment is its **own item** in the list.
- **Drop an audio file** (any song downloaded from the internet) somewhere on
  the page (or pick one). Choose **which section of the song** to use.
- **Beats are detected automatically.** It need not be scientific — "goes by
  vibes" — but it must work for any music: calm, peppy, travel-log tracks.
  Give **major and minor** beats if possible (both offered as snap points).
- **Next.**

**Step 2 — the beat board** (the heart of it; replaces the Edit timeline for
this mode).
- Every clip/segment **one below the other** (vertical list), each shown as a
  **filmstrip of its frames** (like the S23 Ultra Gallery app's frame strip
  under a playing video). Roughly 7–8 clips visible at once.
- On each strip, a **window** (overlay) whose length is that clip's beat
  interval: the song's cuts give 3 s, 3 s, 5 s, 3 s, 3 s, 5 s…, so clip 1 gets
  a 3 s window, clip 2 a 3 s window, clip 3 a 5 s window, and so on.
- **Default:** each window starts at the beginning of its clip.
- **Move** a window left/right along its strip: changes which part of the clip
  is used, never the length (the beat structure stays).
- **Resize** by dragging the right edge: snaps to the **next beats** (major and
  minor offered) and shows soft boundaries where they are: 3 → 6 → 11 s.
  Instagram's gripe to avoid: it cuts a 10 s clip to 2 s because a beat came
  at 2 s, when it could have run 8 s to a later beat. So defaults should lean
  longer when the clip allows, and the owner can stretch.
- When a window grows, **later clips keep their start offsets** (they may have
  been tweaked; don't reset them), and **their lengths re-flow** to the beats
  that are left: the next one might become 5, or 3, or 12 if that's the next
  beat interval. If a clip is shorter than its window, the **overflow shows in
  red** so the owner can shorten the previous one or reorder.
- **Preview** (portrait): hovering a strip previews the frame under the
  pointer, like scrubbing; each clip shows its window's start frame. A small
  per-clip preview and the overall preview playing the reel with the song.
- Reorder clips here too (move up/down).
- **Export** (Instagram settings as in Edit), or hand it to the Edit window
  for finer work.

**Later / to-do.** Landscape sources (Osmo 4K60, drone) need a **crop** per
clip to portrait (choose what part of the frame). That makes it
multi-layer; maybe done by sending the reel to the Edit window, which has
aspect and crop. Figure out later.

## Notes for the build (not decided yet)

- Segments are already stored per clip (`video_segments`), and the library
  already carries them (`MediaItem.ranges`, the ✂ badge, drags to Edit).
- A stream-copy join can only cut at keyframes (Osmo: every ~1 s); cutting at
  exact in/out points means re-encoding those clips. Merge with segments must
  either snap to keyframes (and say so) or re-encode; tell the owner.
- Beat detection: an onset-strength envelope (spectral flux) from the decoded
  audio, then a tempo estimate (autocorrelation) and a beat grid aligned to
  the strongest onsets; "major" = downbeats/strong onsets, "minor" = the
  rest. Decoding: ffmpeg to mono PCM (any format the owner drops).
