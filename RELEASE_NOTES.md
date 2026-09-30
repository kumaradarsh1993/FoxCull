<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

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
