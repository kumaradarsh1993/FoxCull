<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## Merge a trip's videos into one, at full quality

Select your clips, right-click, **Merge N videos into one…**. FoxCull joins
them end to end **without re-encoding**: the file is exactly the camera's video
and audio, so it's the best thing you can upload to YouTube, and it takes about
as long as copying the files.

- **Everything you selected is listed, oldest first**, with each clip's length,
  frame size, frame rate, codec and audio in columns.
- **FoxCull works out the main format and highlights what doesn't match** — a
  30 fps night sequence among 60 fps clips, a vertical clip, a photo — with the
  reason. Nothing is dropped behind your back; you remove things yourself: the
  **−** button, right-click → Remove, or select rows and press **Delete**.
- **Drag rows to change the order**, or put it back with **Sort by time shot**.
- **Click any row to preview it** on the right: hover a video to scrub through
  it, or press Play.
- **It tells you the length and size before you start**, and which drives have
  room: the clips' own folder, your computer, or any plugged-in drive. Merge
  only lights up once nothing is left to fix.
- The window only closes when you say so. A stray click outside won't lose your list.
- When it's done: **Show in folder** or **Open YouTube upload**.

## Video lengths and selection totals

- **Video tiles show their length**, in the corner. Choose what tiles show in
  **Settings → Tile details**: video length, file name, or both.
- **Select several things and the bottom bar adds them up**: how many, total
  video length, and total size.
- **⌘-click** (Ctrl on Windows) adds or removes one item, **Shift-click**
  selects a range, and **⌘+Shift-click** adds another range without losing
  what you'd already picked.

## Nothing overlaps any more

A full pass over every screen at every window size, done on a Mac this time,
where a shorter window and a different system font had pushed things into each
other.

- **Settings**: the Theme buttons no longer sit on top of Interface size.
- **Arrange and Filters** menus stay inside the window instead of running off
  its right edge.
- **Edit studio**: the preview, the media list and the Look panel no longer
  overlap on a laptop screen. The side panels give way so the preview always
  has room. In a narrow window only one side panel shows at a time; use
  **Media** and **Look** to switch.
- **Edit studio**: clips' ratings and tags no longer run into the next clip's
  name, the format buttons stay above the preview, and Zoom no longer covers
  Snap.
- **RAW photos** show one RAW tag, not two stacked, and the filmstrip's stars
  are no longer drawn on top of it.
- No more white square in the corner of the Details list on dark themes.

## TV / large and Compact finally work

**Settings → Interface size** now really makes the whole app bigger (TV / large,
for a screen across the room) or smaller (Compact, for more room on a small
laptop). It had quietly stopped doing anything in August. If you had TV / large
selected, everything will look 22% bigger after this update. That's the setting
working, not a new bug.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet. On first launch macOS says it "could not
verify" FoxCull: open **System Settings → Privacy & Security** and click **Open
Anyway**. (Right-click → Open no longer works for this on macOS 15.)
