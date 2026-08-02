<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## Grid scrubbing is back — and the freeze hunt continues honestly

The previous nightly removed the grid's live-frame skimming on a theory that it
was causing the freeze. Testing proved that theory **wrong** — the freeze still
happened with it gone — so this build **restores grid scrubbing in full**: select
a clip, hover it, and seek through it live at full resolution, no sprite
pre-building, exactly as you had it.

Two small guards were added around it, per your suggestion, so it can never be a
problem: the skim decoder now only spins up after a short deliberate **dwell** on
a clip, and **never while you're actively scrolling or holding an arrow**. Landing
on a clip to skim it works as before; fast navigation simply never starts it.

**The freeze itself is not yet fixed** — this is an honest checkpoint, not a
declared win. What we now know for certain: it is **not** the grid decoder, it is
**not** the GPU (it sits idle), and DOM recycling alone didn't cure it. The
remaining lead is the virtualized scroll/loading path (the "scroll into a blank
zone where tiles vanish and the loader sticks" behavior). That is where the next
session picks up — see `docs/design/FREEZE-HANDOVER-2026-08-03.md` for the full
history of what was tried.

Focus-view scrubbing (dragging the timeline on a 4K clip) is unchanged throughout.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
