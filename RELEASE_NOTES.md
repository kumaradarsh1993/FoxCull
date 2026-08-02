<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## The fast-scroll freeze is fixed at its source

Your key observation was right: during the freeze the browser could still scroll
and highlight controls, but FoxCull could not click, move selection or populate
tiles. The app's main worker was jammed; the display and GPU were healthy.

The thumbnail loader was doing two kinds of work in one burst. It updated the
progress display for every tile entering or leaving the queue, then released as
many as 240 cached thumbnails together when scrolling stopped. That could turn
one settle into seconds of progress updates, promise callbacks and image changes
without giving the interface a chance to paint.

This build gives that work hard limits:

- Cached thumbnails are released one row per screen refresh, never all at once.
- Scrolled-past requests are cancelled immediately instead of searched and
  spliced through a growing backlog.
- Loading progress is now an honest spinner. A moving viewport has no fixed
  total, so it no longer counts backwards or re-renders for every tile.
- The queue is still capped and can no longer remain paused permanently.

We stress-tested the native app on the real 6,825-item library with 800 rapid
three-row jumps. There was no paint stall, the queue stayed bounded and drained
to zero, every tile filled, the loading indicator cleared, and keyboard selection
responded immediately afterward. Grid and Focus live scrubbing are unchanged.

Full history, everything ruled out and the next leads: `docs/design/FREEZE-HANDOVER-2026-08-03.md`.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
