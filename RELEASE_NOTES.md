<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## The scroll freeze — traced to the video overhaul and removed at the source

Thank you for bisecting this against the old stables — that was the key. It
confirmed the freeze **did not exist in v1.0.1 or v1.1.0** and **began in v1.2.0**,
the video-playback overhaul. That pointed straight at what changed.

In v1.2.0 the new live-frame scrubbing engine — which is excellent in Focus view —
was also wired into the **grid tiles**, so a selected clip you hovered decoded
video frames into its own little canvas right there in the grid. That was never
part of the original plan (the design note is explicit: the live decoder belongs
to Focus view, *not* to grid tiles), and it shipped without a hard fast-scroll
test. It is the freeze:

- Each of those grid canvases is a separate GPU layer, and each decoder holds
  real graphics-card resources.
- Fast scrolling, and especially holding the ↓ arrow (every step moves the
  selection to a new tile), spun those decoders and canvases up and down faster
  than Windows could release them — until the display compositor ran out of room
  and blacked out the whole window. That exactly matches what the monitoring
  showed: graphics card **idle**, handle count spiking, picture frozen.

**This build removes the live decoder from grid tiles entirely.** The grid goes
back to the way v1.1.0 worked when fast scrolling was smooth: a poster frame per
clip, and optional hover-skim only if you've turned on **Live Scrub** (the
pre-built sprite option).

**Focus-view scrubbing is completely unchanged** — dragging the timeline on a 4K60
clip still decodes real frames live, full resolution. That is the part of the
overhaul that worked, and none of it was touched.

Also in this build: the "Loading thumbnails" chip no longer flickers or counts
backwards while you fling — it stays quiet during a fast scroll and updates the
moment you stop.

### Please test this way

It would help to confirm the fix cleanly:

1. A **video** folder — fast wheel-fling the grid, then hold the ↓ arrow through it.
2. A **photo** folder — same two motions.

If either still freezes, `foxcull.log` now captures the detail; the video-vs-photo
result tells us exactly where to look next.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
