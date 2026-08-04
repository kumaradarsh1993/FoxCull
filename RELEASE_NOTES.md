<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

Two new features on top of stable 1.4.0: your photos' ratings now survive being
moved around, and Events give a trip its own block in the grid.

## Moving photos no longer loses their ratings

FoxCull remembers every star, label, flag, tag and trim by where the file lives
on the drive. That is what lets a drive carry its own catalog between machines —
and it is also why moving a photo in Explorer used to quietly orphan everything
you had marked on it.

That is now handled the way Lightroom handles it.

**FoxCull checks itself when it opens.** After your folder is on screen it
verifies that every marked file is still where it expects. This costs nothing
when nothing has moved — it only searches the drive if something is actually
missing, so a normal launch is not slower.

**It works out what moved.** If you moved a whole folder, FoxCull recognises it
as one move rather than hunting file by file, and reconnects the lot. Single
files that were renamed or moved somewhere unambiguous are reconnected too. It
will never adopt a file that already has its own ratings — reconnecting one photo
must not overwrite another's.

**Nothing is ever deleted behind your back.** Anything it cannot place shows up
in the grid as a "?" tile that still carries all of its ratings, labels, tags and
events. Right-click it to:

- **Locate this file…** — point FoxCull straight at it
- **Locate the folder it moved to…** — reconnect everything from that folder at once
- **Forget** — drop the marks, once you know the file is genuinely gone

Forget is the only thing in the app that deletes marks for a missing file. A
check never does.

**And the easy way to avoid all of it:** drag a selection from the grid onto any
folder in the left pane. That moves the files *and* their marks together, which
has always been the safe path. What was missing was somewhere to drop them — the
folder pane can now create subfolders, from the ＋ button at the top or by
right-clicking any folder. Create one while files are staged and they move
straight in.

You can turn the launch check off, or run it on demand, in Settings.

## Events

An event is a trip or an occasion — "Monar trip", "Rashi's birthday". It sits
alongside tags rather than being a folder, so your photos stay filed exactly
where they are and the event still holds them together.

Select some photos, right-click, **New event…**. Add more from the same menu at
any time, from any folder.

Then **Arrange ▸ Group ▸ Event** turns the grid into event blocks. Because it is
metadata and not a folder, a trip spread across ten different subfolders still
comes out as one block. Each one is fronted by a cover band showing the photo,
the event name, its dates and how many shots are in it — right-click any member
and choose **Use as cover** to pick the frame.

- **Order by date** ranks blocks by each event's earliest photo instead of
  alphabetically. The existing ↑↓ button flips ascending and descending either
  way. Both controls live under Arrange and switch on when you group by event.
- **Cover art** can be turned off for plain headers.
- **Filters ▸ Event** isolates a single event across everything in view.
- Rename, delete or jump to an event from the list under Arrange. Deleting an
  event only removes the grouping — it never touches a photo.

Events travel with a file when you move it in FoxCull, and when a moved file is
reconnected.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
