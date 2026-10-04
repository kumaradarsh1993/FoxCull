<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## Edit and Merge in their own windows

- **Edit opens beside the library** instead of replacing it: just the
  timeline, the preview, Look and export. Click **Side by side** to put the
  library on the left and Edit on the right.
- **Bring clips from the library**: drag them across, press **E** with them
  selected, or copy them with **⌘C** (Ctrl+C) and paste in Edit with **⌘V**.
  Drop on a track to place them at that time, anywhere else to add them after
  the last clip.
- **In/out ranges you marked in Focus come along**: a clip with two marked
  ranges arrives as two segments. Clips with ranges show a ✂ in the grid.
- **Clips that don't match** the rest of the timeline (vertical in a
  landscape edit, a different size or frame rate, HDR among SDR) get a ≠ mark
  and a note saying what the export will do about it. Photos and missing files
  are left out with a message, never silently.
- **Your timeline is kept**: close the Edit window and it's there next time.

## Merge: pause it, stop it, or close it and carry on

- Merge opens in its own window. **Once it's running you can close the
  window**: the merge carries on, and the progress panel (bottom left) shows
  it, with **Pause / Resume** and **Show merge window** to come back to it.
- While it runs the window shows just the progress: no clip previews eating
  the disk. **Pause** freezes it in place; **Stop** deletes the partial file.
- When something goes wrong it says what: the drive filled up, a card or
  drive was removed, a clip is damaged, or FoxCull can't write there.
- **Quitting while something is running asks first**, then stops it cleanly
  and deletes anything half-written.

## A progress panel that shows everything going on

The strip at the bottom of the folder list grew into a proper progress panel.

- **What's running, how far, and how long is left**, for every task at once:
  moves, copies, merges, exports, preview building. Copies show the size done,
  the speed and the time left.
- **Several at once:** "3 tasks running" with one combined bar. Click it to see
  each task on its own, with a **Stop** button.
- **Finished work shows the result** for a few seconds ("Moved 24 items to
  Seattle", with Open folder or Show in folder), then waits under
  "Recent tasks" until you clear it. A failure stays until you dismiss it.
- Routine background work (loading thumbnails, reading dates) shows smaller and
  greyer, and disappears when done.

## Moving files by dragging

- **Dragging shows a small stack with a count** instead of the whole
  selection. The folder under the pointer says "Move 24" (hold **Option**,
  or **Ctrl** on Windows, for "Copy 24"). Hover a closed folder for a moment
  and it opens, so you can drop into a subfolder.
- **Moves to another drive now work**, with size, speed, time left and Stop
  in the progress panel. Each file is copied, checked and only then removed
  from where it was, and its stars, labels, tags and events go with it.
  Before, a move from an external drive to the Mac was refused, and a move
  the other way left the marks behind.
- Drop several batches in a row and they queue up instead of being ignored.

## Merge in the background

While a merge runs, click **Run in background** (or the – at the top). The
merge keeps going, and the progress panel shows it with Stop and **Show merge
window**. When it's done you get **Show in folder** there too. Starting an
Edit export no longer cancels a merge.

**Why merging takes a minute or two:** it's the drives, not the app. Measured
on Osmo clips from an external SSD: FoxCull joined 6.4 GB in 15 seconds;
LosslessCut took 24 seconds for 7.1 GB, because by default it rewrites the
whole file a second time. A 40 GB day of footage takes about a minute and a
half from a fast SSD, and longer from an SD card. The progress panel now shows
the speed, so you can see which drive is the slow one.

## Missing files, cleared in bulk

- **Right-click a folder → Remove N missing items** clears every missing ("?")
  entry in that folder and the folders inside it, in one go.
- In the grid, **select any mix** (Shift or ⌘/Ctrl-click) and right-click: it
  offers to remove just the missing ones, to select every missing item in the
  folder, or to select only the files that exist.
- **Find one, find them all:** pointing FoxCull at one missing file also
  reconnects the other missing files from its old folder, if they're in the
  same new place (like Lightroom).

## Smaller things

- **Group by Day** (Arrange → Group or Subgroup), for trips. Videos use the
  time in the camera's file name, so evening clips stay on the right day.
- **Picked and rejected at a glance:** a short green tab on top of picked
  tiles, a red one under rejected tiles.
- **Prepare left the toolbar.** Photos get their full-size preview ready as you
  step through them, so it rarely saved anything. For a slow SD card or disk:
  right-click a folder → **Build previews for this folder**.

## Merge clips that aren't all the same size

Some cameras don't record every clip at the same size. Meta's glasses crop each
clip slightly differently (1376×1840 in one, 1392×1856 or 1488×1984 in the
next). Joining clips like that without re-encoding breaks the picture after the
join, so the merge window used to refuse them. It also flagged some clips for no
real reason.

- **"Convert to match"** in the merge window brings those clips in. Every clip is
  re-encoded to one size (your largest clip's), one frame rate and the same HDR
  format, at about twice the camera's own bitrate, so it looks the same as the
  originals. It uses the Mac's hardware encoder, so 20 minutes of clips takes a
  few minutes. **Lossless** stays the default whenever the clips already match.
- **The Frame column shows the actual size** (1376×1840), not just "Vertical",
  so you can see what differs.
- **Frame rates are compared the way you'd expect.** Phones and glasses record a
  variable frame rate, so a 30 fps clip can read 29.73 or 29.94. Those join
  cleanly and are no longer flagged. Only a real difference (30 vs 60) is.
- HDR and SDR clips are now told apart. They can't share one file, so the window
  flags them instead of producing a file with wrong colours.
- If a merge fails partway, the half-written file is removed so it can't be
  uploaded by mistake.

## A proper Trash

The Trash used to be just another folder: culling buttons that did nothing,
star ratings for deleted files, and no way back. It now behaves like the Trash
in Photos or Finder.

- **Find it at the bottom of the sidebar**, with a count of what's in it for the
  drive you're on.
- **Back takes you to the folder and photo you came from.** The ‹ button, Esc,
  or ⌘[.
- **Only what makes sense here:** Restore, Delete permanently, Restore all and
  Empty Trash. Everything for culling and editing is hidden.
- **Each file shows where it came from and when you deleted it**, newest first.
  Right-click → Show original folder takes you there.
- You can still **preview and play** anything in Grid, Details or Focus before
  you decide.
- Keys: **R** restores, **Delete** deletes permanently (it asks first).

## A tidier folder sidebar

Denser rows with folder and drive icons and faint guide lines, so more of your
folders fit on screen and the hierarchy is easier to follow.

## Fixed

- **A folder you create in FoxCull now shows up straight away**, with the
  sidebar opened to it. It used to stay invisible until you restarted the app.
  The ↻ button also picks up folders made in Finder or Explorer now.
- Pressing Esc to close a right-click menu no longer also closes the view behind it.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet. On first launch macOS says it "could not
verify" FoxCull: open **System Settings → Privacy & Security** and click **Open
Anyway**. (Right-click → Open no longer works for this on macOS 15.) macOS may
also ask again for access to your Downloads or other folders after an update;
click Allow.
