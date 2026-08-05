<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

Fixes for everything you hit on the last nightly — plus something the audit
turned up that you should look at before deleting anything.

## ⚠️ There are ~19 GB of your files in FoxCull's Trash that it wasn't showing you

While checking the `_FoxCull` folders for the fresh start, two drives turned out
to be holding real media in their recycle folders with no Trash entry, so the
Trash panel showed nothing and Restore couldn't reach them:

- **E:** an 18 GB merged Dubai-trip clip
- **P:** 22 files — 14 photos/videos from Jan 2022, plus 8 DJI Mavic Mini clips
  (~1 GB) that had no Trash entry at all

None of the originals are back on disk. This build makes them visible: opening
the Trash now adopts anything sitting in the recycle folder without an entry,
reconstructed to restore exactly where it came from. Nothing is deleted — open
the Trash on each drive and decide.

## Clicking a whole drive no longer freezes the app

Opening `D:\` locked the window for minutes. The scan was running on the same
thread that keeps the window alive, so a folder large enough to take minutes took
the whole app with it.

Scanning now happens off that thread, and:

- **You can leave.** Click any other folder mid-scan and FoxCull drops the old
  one and goes where you asked.
- **It tells you what it's doing.** A live file count appears while it works, the
  activity bar at the bottom-left lifts and highlights itself while anything is
  running and settles back when it finishes, and the scanning screen says you're
  free to click elsewhere.
- **It skips what can't hold photos.** `node_modules`, Windows system folders,
  Steam libraries and the like are no longer walked — they were most of the wait.

The same treatment went to moving files, browsing Edit sources and opening the
Trash, so none of those can hang the window either.

## The filmstrip no longer rewinds to the beginning

Pressing `B`, going from Grid to Focus, or leaving full screen made the bottom
strip appear at the first photo and race across to your current one, reloading
thumbnails the whole way. It now lands directly on your photo. Arrow-key stepping
keeps its smooth glide, which is the only place that animation was useful.

## "Show in Explorer" works again

It was opening `OneDrive\Documents` instead of the photo's folder. Not the
external drive's fault — Explorer splits its own command line on spaces, so any
path with a space in it (like `All media MASTER`) lost everything after the
space. Fixed and verified on that exact file.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
