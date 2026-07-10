# Roadmap

A running note of things we've deliberately put off, and why — so they don't
get re-litigated from scratch every time they come up.

## Queued (evaluating)

**Auto-expose Clip tools when in/out marks already exist.** Right now Clip
tools is a manual toggle in Focus/Loupe even when a clip already has saved
in/out points from a previous session. Auto-opening it in that case would
save a click, but might also just be noise for people who trimmed once and
don't care to look at it again. Owner wants to live with the manual toggle a
while longer before deciding.

**Merge the Edit top bar into the global app bar.** EditStudio currently has
its own top bar sitting under the app's main bar — two stacked bars. Folding
them into one would tighten the layout, but it's a nesting/layout change more
than a functional one, and lower priority than it looks.

**Live export progress %.** Exports (Instagram/lossless/custom) currently
show a spinner-style "Saving…" state, not a live percentage. The activity
system already carries `done`/`total` for other long jobs (proxies, thumb
warming) — wiring exports through the same path is plausible but not done.

**Smooth expand/collapse animation for stacks.** Toggling a stack open/closed
reflows the virtualized grid instantly (cells jump to new positions). An
animated reflow would feel nicer but the grid's virtualization (position via
transform, cells mounted/unmounted on scroll) makes a naive CSS transition
unreliable — needs real thought, not a quick style tweak.

**Per-clip source-audio mute on the edit timeline.** No per-clip audio
control in the timeline today — audio comes along with whatever video is
placed. Useful once multi-clip sequences with mismatched audio become common.

**Ripple/insert timeline editing.** Today's timeline edits (cut, trim) don't
ripple later clips to close gaps or shift insertion points. Fine for the
single-clip-at-a-time workflows FoxCull is built around now; would matter
more with a proper multi-clip sequence editor.

## Deferred by design

**AI upscaling for vertical crops — rejected.** Considered adding AI upscale
to soft-crop exports so a tight vertical crop doesn't lose resolution.
Rejected: Instagram recompresses everything down to ~1080p on upload, which
wipes out most of the upscaling gain — you'd pay the compute cost for a
result IG throws away anyway. Shipped a cheap auto-sharpen on soft-crop
instead, which is most of the perceptual benefit for near-zero cost.
