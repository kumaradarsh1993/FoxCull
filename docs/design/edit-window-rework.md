# Edit rework: separate windows, library-driven timeline

**Status:** owner spec, 2026-10-04. Not started. Planned as the nightly AFTER
the fixes batch (`v1.5.2-nightly.3`), which deliberately ships fixes only.

This records what the owner asked for, close to their words, so the build can
be checked against it. Design decisions taken while building go in a section at
the bottom, not in the spec above it.

## The owner's spec

### 1. Edit becomes its own window, not a tab

Today the app has two modes in one window: Library and Edit, and Edit replaces
the library. Instead:

- **Edit is a button that opens a separate OS window** (alt-tab to it, put it
  beside the library). It holds **only the timeline and the final preview**.
- The **library window keeps everything it does now**: browsing all media,
  picking, rating, highlighting. That is where clips are chosen.
- The Edit window's **left source panel goes away** ("it just messes things
  up"). No media picker in the Edit window: the library is the picker.
- "We will consolidate our app later on." Fragmentation into windows is
  accepted for now.

### 2. Three edit workflows

1. **Merge** (Osmo Pocket 3 clips → one file for YouTube). The existing merge
   flow "works really well"; keep it. It may become **its own window too** (not
   an overlay): alt-tab to it, side by side with the library. It must be
   minimisable/closable while it runs, with progress in the job centre (done in
   the fixes nightly for the overlay).
2. **Timeline editing** (the Instagram flow): clips dragged from the library
   onto the timeline in the Edit window.
3. **Single-clip work** (grading/LUTs): "even to change LUT I would prefer to go
   to the edit window. I will work with just one clip."

### 3. Side by side

- When the owner is on a video in the library and opens Edit, the two windows
  should sit side by side. They'd like **an in-app setting or quick shortcut**
  to snap library left / edit right, like OS window snapping. If that can only
  be done well by the OS (macOS tiling, Windows Snap), that's acceptable, but
  an in-app way is preferred.

### 4. In/out points live in the library

- In the library (grid or Focus) the owner sets **in and out points on any
  video, one pair or several pairs**.
- **Drag from the library to the Edit window** (mouse drag or three-finger
  drag):
  - no in/out set → the **whole clip** goes on the timeline;
  - one pair → **that segment** goes on;
  - several pairs → **every segment, individually, side by side** in order.
- **Where it lands:**
  - dropped **on a specific track at a specific time** → placed there, like
    Premiere Pro;
  - dropped **anywhere else** on the Edit window (e.g. the preview) → added to
    the default track **right after the current last segment**.
  - several clips dragged at once → **concatenated** in order.

### 5. What stays

- Export settings for Instagram, the **time estimate**, and the indication of
  whether a change **needs a re-encode or can be done like LosslessCut**
  (stream copy). The owner wants re-encoding avoided whenever possible.
- Looks/LUTs stay, applied in the Edit window.

## Notes for the build (not decided yet)

- Tauri 2 can open more `WebviewWindow`s from the same SvelteKit build (e.g. a
  `/edit` and a `/merge` route). Windows talk through Tauri events and the
  backend; there is one catalog.
- The catalog already stores **several segments per clip**
  (`video_segments(rel, idx, in_s, out_s)`), so multi-pair in/out has a home.
- Cross-window drag: test HTML5 drag between two webviews on both WKWebView
  and WebView2 early. Fallback: the library tells the backend what is being
  dragged on `dragstart`, and the Edit window reads it on `drop`.
- Snapping: `currentMonitor()` work area + `setPosition`/`setSize` on both
  windows gives an in-app "Side by side" without OS help.

### 6. Owner follow-up (same day): carrying clips over, and corner cases

- **Selection carries over.** Whatever is selected in the library (shift and
  ⌘/Ctrl-click) is what goes to the timeline, by drag **or by keyboard: copy in
  the library, paste in the Edit window**. Paste lands where a plain drop would
  (after the current last segment), or at the playhead/track if one is active.
- **No silent failures.** Clips that don't match the timeline (resolution,
  aspect ratio, frame rate, codec/format, HDR vs SDR, photos, missing files)
  must be handled gracefully and the owner must be told what will happen,
  before or as it happens, never discovered at export. "Think through all the
  permutations of corner cases."

#### Corner cases to design for (first pass, to be reviewed with the owner)

| Case | Proposed handling |
|---|---|
| Same format as the timeline | Placed as is; export can stream-copy (LosslessCut-style) if nothing else forces a re-encode. |
| Different resolution, same aspect | Placed, scaled to the timeline; a badge on the segment; export says "re-encode needed: N clips differ in size". |
| Different aspect (vertical into landscape…) | Placed with fit/fill choice (default fit, letterboxed); badge; the drop toast says so with an Undo. |
| Different frame rate class (30 vs 60) | Placed; badge "converted to 60 fps"; re-encode noted. VFR averages of the same class are not a difference (`fps_class`). |
| HDR into SDR timeline (or reverse) | Placed with a tone-map warning; offer "make the timeline HDR" if it's the first clip. Never a silent colour change. |
| Codec the decoder can't preview | Placed; preview uses the proxy path; message names the clip. |
| Photo dropped | Becomes a still segment of a default length (setting), or refused with a message — decide with the owner. |
| Missing ("?") item in the selection | Skipped, with "2 missing items weren't added" in the drop toast. |
| Several in/out pairs on one clip | Each pair becomes its own segment, in order (spec §4). |
| In/out pair shorter than a frame or inverted | Ignored with a message naming the clip. |
| Drop while an export is running | Allowed (the timeline is separate from the running export); the export uses the timeline as it was when it started. |
| Paste with nothing copied, or a non-media clipboard | Nothing happens except a short message. |

The first clip into an empty timeline sets the timeline's format (as Premiere
does); the owner can change it in timeline settings.
