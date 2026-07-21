# 2026-07-22 — Stage-freeze RCA, scrub readouts, thumbnail progress, filmstrip toggle

## Intent

Owner feedback on v1.2.0-nightly.2, in his order of severity: (1) clicking the
timeline during playback intermittently freezes the picture while the audio keeps
running, and once it starts on a clip it keeps happening; (2) a timestamp pill
floats over the middle of the frame while dragging and the transport's own clock
is too faint; (3) no way to hide the bottom filmstrip; (4) a cold folder of
videos rendered thumbnails for 10–15 s with no progress shown anywhere.

## RCA — the freeze (the one that matters)

**Mechanism.** `paintStage()` raises `canvasHold` inside its decode callback, and
`onSeeked()` lowered it *and* cleared the 1500 ms `holdTimer` safety net.
Clicking the timeline during playback starts two async things at once: the
exact-frame decode (~150 ms) and the element's own precise seek. The order they
finish in was never controlled.

- decode finishes first → paint, then `seeked` clears the hold. Correct.
- `seeked` finishes first → hold cleared **and the safety timer cancelled**, then
  the late frame raises `canvasHold` again. Nothing is left alive that can ever
  lower it: no further `seeked` is coming, the timer is gone, and `scrubbing` is
  already false. The still covers a playing video **permanently**.

That explains every part of the report. *Intermittent*, because it depends on a
race. *Sticky per clip*, because which side wins is a property of that file's
seek latency, so a clip that loses once loses every time. *Audio continues*,
because only the picture was ever covered — `<video>` was playing correctly the
whole time, underneath an opaque canvas.

**Fix.** A hold generation (`holdGen`), bumped by the single new `releaseHold()`
that every hand-back now goes through. `paintStage()` captures the generation
when it requests a frame and discards the frame if the generation moved while it
was decoding. Dropping it costs nothing: the element has already landed on that
same frame, which is exactly what the still would have shown.

**Second line of defence.** `onTime()` (i.e. `timeupdate`, ~4 Hz) now asserts the
invariant directly: if the video is playing and no gesture is in progress, a
raised hold is stale and is released. Any future path that leaks a hold
self-heals in ~250 ms instead of freezing until the clip is changed. The
generation fix alone is sufficient for the known bug; this is here because the
failure mode is bad enough to deserve belt and braces.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src/lib/components/Loupe.svelte` | logic | NEW `holdGen` + `releaseHold()`; `paintStage()` drops stale frames; `onSeeked`, the 1500 ms timer, item-change reset and video cleanup all route through `releaseHold()`; `onTime()` enforces "a playing video is never covered". |
| `src/lib/components/Loupe.svelte` | UX | `.stageTs` pill removed from both the live and sprite scrub paths (it floated mid-frame and read as a glitch). Transport clock: current position 14 px/650 in white with a drop shadow, duration dimmed — it is now the only time readout on the stage. |
| `src/lib/thumbnail-loader.ts` | logic | NEW local activity job for the on-demand queue: `jobReport()` / `jobFinished()` / `jobReset()`, announced only after 700 ms so warm-cache loads don't flash a chip. Total counts queued + in-flight, so the denominator stays honest while scrolling adds work. |
| `src/lib/components/ActivityBar.svelte` | UX | Border moves to the top — it docks at the bottom of the sidebar now. |
| `src/routes/+page.svelte` | UX | `ActivityBar` moved from under the sidebar header to the sidebar footer, plus a floating `.actFloat` copy when the tree is collapsed. NEW `toggleFilmstrip()` + `B` shortcut + shortcuts-panel row; the bottom `.hsplit` rail now carries a chevron toggle and survives (at 14 px) while the strip is hidden. |

## Behavior changes

- Clicking or dragging the timeline during playback no longer freezes the picture.
- No timestamp floats over the frame while scrubbing; the transport clock is legible.
- **`B`** hides/shows the filmstrip, as does the chevron on the rail above it.
  Hiding remembers the dock, so unhiding restores left/right/bottom as it was.
- A cold folder now shows "Loading thumbnails N / M" while its tiles render.
- The activity chip lives at the bottom-left, and survives a collapsed sidebar.

## Risks / compat

- The `timeupdate` guard could, in principle, retire the hold a few frames before
  `seeked` on a resumed drag, showing live video slightly early. Chrome does not
  fire `timeupdate` while seeking, so this is theoretical; a one-frame early swap
  is in any case strictly better than the freeze it protects against.
- `.hsplit` is now rendered when `filmstripPos === "hidden"`, which is a state
  that previously drew nothing at the bottom of the window.
- Thumbnail progress counts filmstrip requests too (they share the queue), so a
  sprite fallback build is counted both here and in its own backend job. Honest,
  slightly redundant.

## Verification actually run

- `npm run check` 0 errors / 0 warnings. `npm run build` clean. No Rust changed.
- **The hold state machine was transcribed out of the patched component and
  driven through both orderings of the race** (`scratchpad/hold-race.mjs`). It
  reproduces the freeze on the pre-fix logic (`seeked` wins → hold stuck true),
  shows the fix releasing in both orderings, and confirms the fix does **not**
  break what the hold is for: during a paused drag the still stays up until the
  element lands, and the never-fires-`seeked` case is still caught by the timer.
  This tests the diagnosis, not just the patch — the pre-fix branch failing in
  exactly one ordering is the evidence that the RCA is right.
- **Not verified on device:** all four items need a hand on the mouse. In
  particular the freeze fix is proven at the logic level only; the real
  `<video>`/WebCodecs timing on a 4K60 clip is the owner's confirmation to give.
