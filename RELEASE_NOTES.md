<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## Video stills are no longer washed out

Your read was right that something was off before playback started, and right
that it wasn't a paused state — colour came back on play and never went washed
out again on pause.

That is the fingerprint of the still image FoxCull paints while the video is
still opening. Your footage is BT.709, but JPEG means BT.601, and the still was
being *labelled* BT.601 without actually being converted. FoxCull was showing
BT.709 colours decoded with the wrong maths; the video player then decoded the
same clip correctly, which is why it snapped.

Measured on your own Note 10+ footage against a reference decode, the fix is
worth ~2.8 dB and a visible lift in saturation. It applies to grid thumbnails
too — they were wrong as well, just too small for it to read.

One caveat: **stills already in the cache keep the old colour.** The cache is
keyed on the file, and the file hasn't changed. New clips are correct
immediately; to rebuild everything, delete `<drive>\_FoxCull\thumbs` and it
regenerates.

## A paused video now looks paused

A play button sits over the frame whenever a clip is stopped, the way YouTube
does it. It gets out of the way while you scrub or Glimpse.

## New: a written guide to Events, moving files, and relinking

`docs/EVENTS-MOVES-AND-RELINK.md` in the repo covers all three features
end-to-end — what to do, what happens in the awkward cases, and a blunt list of
where they don't work. Three things in it are worth knowing before you plan a
workflow:

- **Moving and renaming a file in the same pass cannot be recovered** by the
  relink pass. Both stages of matching key on the filename.
- **Marks don't cross drives.** Catalogs are per-drive.
- **Don't use "Forget" while a drive is disconnected** — every file on it looks
  missing, and Forget is the one action that deletes marks.

It also proposes a command-line adapter so an external agent can tell FoxCull
what it moved or tagged, and a rework of the Trash into a visible folder you can
browse and play from. Both are waiting on your go-ahead.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
