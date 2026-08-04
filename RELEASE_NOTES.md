<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

**The scroll-freeze release.** Everything proven across the 1.4.0 nightlies is
now the stable build.

## Scrolling a big folder no longer freezes FoxCull

This was the headline problem, and it is fixed at the source. Your key
observation is what cracked it: during a freeze the window could still scroll
and highlight controls, but FoxCull could not click, move the selection or fill
in tiles. The app's main worker was jammed — the display and the GPU were
healthy the whole time.

The thumbnail loader had been doing two kinds of work in one burst. It updated
the progress readout for every single tile entering or leaving the queue, and
then released as many as 240 cached thumbnails together the moment scrolling
stopped. One settle could become seconds of progress updates, promise callbacks
and image swaps with no chance to paint in between.

That work now has hard limits:

- Cached thumbnails are released about one row per screen refresh, never all at
  once.
- Scrolled-past requests are cancelled immediately instead of being hunted
  through a growing backlog.
- Loading progress is an honest spinner. A moving viewport has no fixed total,
  so it no longer counts backwards or redraws for every tile.
- The queue stays capped and can no longer get stuck paused.

Verified in the real app on the 6,825-item library with 800 rapid three-row
jumps: no paint stall, the queue stayed bounded and drained to zero, every tile
filled, the loading indicator cleared, and keyboard selection responded straight
afterwards. Grid and Focus live scrubbing are unchanged.

## One selector, following both mouse and keyboard

Clicking a photo and then pressing an arrow key could look like two photos were
selected: FoxCull moved its real selector correctly, but the window drew a
keyboard-focus outline around the old clicked tile as well. Media cells now hand
off that stale outline before arrow navigation, so exactly one selector is ever
visible. Intentional Ctrl/Shift multi-selection is unchanged.

## Also in this release

- The grid recycles its tiles instead of rebuilding them, so a long fling costs
  a fraction of the work it used to.
- A Windows display quirk that could leave the app frozen while completely idle
  is worked around at startup.
- Trims report real errors instead of failing quietly, exports no longer collide
  on temporary filenames, and Restore can't duplicate a file out of the Trash.
- Logging is append-based and stamped per launch, so a relaunch can never wipe
  the log of the session you were trying to diagnose.

Full history, everything ruled out along the way and the remaining leads:
`docs/design/FREEZE-HANDOVER-2026-08-03.md`.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
