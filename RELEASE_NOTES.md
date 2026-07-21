# FoxCull v1.1.0

The first stable since 1.0.1, and a big one. Casting, a real editing Look
panel, controller culling, RAW→JPEG export, stacks, undo/redo — and a long
run of work on video: previewing it, skimming it, playing it, deleting it.

This release is also a deliberate checkpoint. Everything here plays video
through the built-in web view, which is what limits scrub smoothness on large
4K footage. The next line of work replaces that player outright; 1.1.0 is the
mark of how far the current approach goes.

---

## Video

**Skimming (Live Scrub).** Hover a clip and watch it move without opening it.
Off by default; turn it on in Settings.

- **Click a clip to arm it, then skim it.** Only the selected tile responds, so
  sweeping across a folder of 4K clips doesn't set the whole drive working.
- The timeline maps across the whole tile, so portrait and landscape clips skim
  at the same speed.
- Frames are extracted once and kept on the drive, so a clip you've skimmed
  before is instant forever — on any machine that reads that drive.
- One set of frames now serves both the grid and the Focus timeline. It used to
  build two, which is why opening a clip you'd just skimmed appeared to start
  over.
- Extraction is keyframe-based and runs several frames at a time: a strip that
  once took over a minute on a long clip now lands in seconds.

**Playback and the transport.**

- **The bar gets out of the way.** A video plays edge-to-edge with a thin
  progress line at the bottom; move toward it and the full controls rise. Prefer
  a permanent bar? Settings → Minimal video bar → Off.
- **Sharp first frame.** The still before playback is generated at high
  resolution for Focus and full-screen instead of being a blown-up thumbnail.
- **Shift+←/→** steps 5 seconds; `,` and `.` do the same.
- Dragging the playhead shows the frame under your cursor immediately and lands
  the exact one when you let go.
- **Clips the app can't decode play anyway** — a capped H.264 version is made
  once in the background and used for preview. Trimming still cuts the original.
- Trim, in/out points and marked sub-clips persist per clip.

## Culling

- **PS5 / PS4 controller support.** Pair a pad and cull from the couch; every
  button is remappable, with a pairing guide and an on-screen button map. Your
  mouse's extra Back/Forward buttons are remappable too.
- **Play mode (F) is a 3-step cycle**: picture with the filmstrip (dimmed so
  your eye stays on the shot) → bare picture → back to normal. `Esc` always
  exits. The filmstrip stays wherever you docked it and stays resizable.
- **Filmstrip docks bottom, left or right** — left sits between the folder tree
  and the picture.
- **Stacks.** RAW+JPEG pairs and edits/exports group under their original, fold
  and unfold, and are labelled by what they are.
- **Filters** on rating (≥ ≤ =), multiple colour labels at once, kind, and tags.
- **Undo / redo** across ratings, labels, picks, rejects and tags — and now
  **deletes**: Ctrl+Z after a delete offers to restore that batch from the
  in-app Trash, asking first. (Ctrl+Y deliberately never re-deletes.)
- **Prepare** caches a whole folder up front so a culling pass has no waiting.
  The ▾ beside it narrows the job to your selection, just videos, or just
  photos & RAW.

## Photos

- **HEIC works.** Phone HEICs — stored as a grid of tiles — now decode, scale
  and rotate correctly everywhere. No Windows codec pack needed.
- **RAW→JPEG export** in bulk, pulling the camera's own embedded preview.
- **The Look panel** was rebuilt: 12 grouped presets and sliders that actually
  move the image, with the on-screen preview and the exported file matched by
  construction rather than by eye.
- Flipping between photos no longer flashes a blur — the next shot swaps in only
  once it's fully decoded.

## Cast to TV

- **Casting follows you.** Start it once and the TV shows whatever you're on as
  you arrow through the folder — photos and videos in one session.
- **HEIC and RAW cast correctly** (they send their high-resolution preview).
- Videos stream the untouched original, so the TV's own decoder plays your 4K60
  HEVC at full quality.

## Deleting

- **Deletes explain themselves.** A file genuinely held open by another program
  and a file Windows won't give permission for are different problems, and now
  say so separately, by name, in a message you can read in full. Read-only files
  are handled automatically.
- **Deleting a huge clip can't freeze the app.** Background work is cancelled
  first and the delete runs off the UI thread.
- Everything deleted goes to a per-drive Trash you can browse, restore from, or
  purge.

## Smaller things

- **Show in Explorer selects the file**, rather than dropping you in a folder of
  six hundred.
- A keyboard shortcut guide on `?`, light-dismiss on every menu, honest
  time-remaining estimates, and no more filename tooltip popping up over the
  tile you're skimming.
- Instagram-ready export with quality shown as real time cost.
