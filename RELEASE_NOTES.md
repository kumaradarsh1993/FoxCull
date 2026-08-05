<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

Both things you called out on nightly.4, fixed.

## The Trash is a real folder now

`FoxCull Trash` sits at the root of each drive, visible in the folder tree like
anything else. Click it and you get the **normal grid** — thumbnails, file sizes,
sorting, Focus view, full playback. So you can actually watch the clip before
deciding whether it deserved deleting.

- **The popup window is gone.** Restore and Delete permanently are right-click
  actions inside the folder.
- Where each file came from shows in the info bar (`🗑 from …`) and in the
  right-click menu.
- It's flat — no twenty-deep folder tree to click through for three rejects.
- Trashed files never leak back into the library: the folder is skipped when
  scanning a parent, and only listed when you open it directly.

**Your existing trash moves itself on first launch, per drive.** Same volume, so
it's an instant rename even for the 18 GB clip — nothing is copied and nothing is
deleted. The orphaned files that had no Trash entry get one on the way, so the
~19 GB on E: and P: becomes visible and restorable.

There's also a small `_trash-index.json` in the folder recording where everything
came from. That's the belt-and-braces fix for how those orphans happened: even if
the catalog is lost again, the files still know where they belong.

## The event banner — why you never saw it

Not a rendering bug. Your saved settings had `Group by: Event` and `Sort by:
Name`. The first kept you on the old cover-art blocks; the second suppressed the
banner, which needs a time order to mean anything. Nothing on screen told you.

That's my fault for keeping both modes. **The event grouping is now removed** —
events are only the banner. Your settings migrate automatically: the grouping is
cleared and you're switched to capture-date order, which is where the banner
actually draws.

- If a sort ever makes it impossible again, the Arrange panel now says so with a
  one-click fix.
- Creating a new event turns the banner on and switches sort if needed, so you
  see the result immediately.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
