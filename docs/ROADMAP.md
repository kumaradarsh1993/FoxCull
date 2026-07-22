# Roadmap

A running note of things we've deliberately put off, and why — so they don't
get re-litigated from scratch every time they come up.

## Open after v1.2.0 (2026-07-22)

**Awaiting the owner's hands — nothing to build until he reports:**

- **Grid skimming.** Two bugs kept the grid decoder from ever opening (see
  `docs/PROJECT-LOG.md`, 2026-07-22). Fixed but **never seen working**. Click a
  clip to select it, then hover across its tile.
- **Cast on the Sony TV.** Three fixes shipped; the wire exchange with a real
  Bravia after its receiver idles out cannot be simulated here.
- **XPS 13 decoder support.** Open a few clips there and read the
  `UI scrub-engine OK|FALLBACK` lines in `%APPDATA%\com.foxcull.app\foxcull.log`.
  Chrome has no software HEVC decoder, so it depends on that machine's iGPU
  generation (Intel gained 10-bit HEVC decode at Kaby Lake / 7th gen) — which no
  document records. **Do not guess this; the log answers it.**
- **H.264 phone clips.** The `avcC` branch of the decoder's config path is
  written but has never met a real file.

**Queued work, roughly in priority order:**

- **Disk usage + cleanup module** (owner-requested 2026-07-22). Show how much
  space FoxCull's `_FoxCull/` folders take per drive and let redundant files be
  removed. Now more tractable than it was: video pre-caching is gone, so the
  only sizeable derived artifacts left are posters, image thumbnails, RAW
  previews, and any legacy sprite sheets from before 1.2 — those last are pure
  dead weight on drives prepared under older builds and are the obvious first
  thing to offer to delete. The in-app Trash is *not* derived data; never sweep
  it without asking.
- **Virtualize EditStudio's source pane.** The visibility gate bounds the fetch
  rate, not the ceiling — scroll a 229-clip folder end to end and 229 tiles are
  loaded. Fine at a few hundred; the real fix is `VirtualGrid`, which the
  library grid already uses.
- **Attribute the remaining Edit-mode memory.** 7,940 MB → ~519 MB is measured,
  but the split between the three fixes never was, and JS heap was only ever
  ~20 MB — so the renderer gigabytes were DOM/image-decode, not JS. Only worth
  revisiting if it regresses.
- **Cast relaunch attempt cap.** Relaunch is time-bounded (once per 3 s) but not
  attempt-bounded; a TV that is reachable yet refuses to launch would be re-asked
  indefinitely. Not observed; cheap insurance if it ever is.

**Ideas the owner raised, not scheduled:**

- **A phone app that casts photos/video to the TV at full quality.** His
  observation: FoxCull's cast quality beats Google Photos noticeably. The reason
  is architectural and would port — FoxCull sends the **original file untouched**
  and lets the TV decode it, where Photos re-encodes for a generic pipe. A
  separate product, not a FoxCull feature.

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
