<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## We found what the freeze actually is

Your description of what still *works* during a freeze cracked it open. You can
still scroll, buttons still highlight, tooltips still appear — but nothing can be
clicked, the selection won't move, and tiles never fill in.

That split is decisive: scrolling, hover highlights and tooltips are done by the
browser engine **without** the app's code running. Clicking, moving the selection
and loading tiles **need** the app's code. So the app's main worker is what's
jammed — **the display was never the problem.** That reverses what we'd assumed
for days, and it means earlier fixes were aimed at the wrong layer. The "12 rows
by 9 columns frozen in the middle, blank above and below" is simply the last
screenful the app managed to draw before it jammed — the dead zone and the freeze
are one and the same event, exactly as you suspected.

**This build fixes one thing that was making it far worse — and it's mine, not
1.2's.** The fast-scroll optimization I added pauses thumbnail loading while you
fling and resumes when you stop. But "when you stop" was decided by the same
jammed worker — so once it froze, loading was paused **permanently**. That's why
tiles never came back even after scrolling around, and why the loading bar stuck.
It now has its own independent stop-watch and always resumes, so a stall can no
longer turn into a permanently dead grid.

Also: the backlog of pending thumbnail requests is now capped. Holding ↓ through a
1000-clip folder was piling up thousands of queued requests that all landed at
once — the stalest are now dropped instead of hoarded.

**Honest status: the underlying jam is still not fixed.** These two changes should
make it recoverable rather than terminal, and should reduce how often it trips.
Grid scrubbing (select a clip, hover, seek) and Focus scrubbing are both unchanged
and working.

Full history, everything ruled out and the next leads: `docs/design/FREEZE-HANDOVER-2026-08-03.md`.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
