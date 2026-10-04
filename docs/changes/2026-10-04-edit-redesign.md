# Edit window: design review and redesign

Nightly `v1.5.2-nightly.9`. Owner, 2026-10-04: "can we do a design review and
modernisation / overhaul if required of the edit window — to make it more
modern and better looking".

## Review (what was there)

- **Top bar:** five aspect buttons each with a second line of small print
  ("Stream copy", "1920x1080"…), text toggles "Timeline" / "Look" / "Side by
  side", a text "Preview", Export with a "▾" glyph. No sense of which window
  this is or what is on the timeline.
- **Transport:** a text "Play" button, "0:00 / 2:36" in whole seconds, a bare
  range slider. No frame stepping, no start/end, and stepping a frame showed
  no change in the time.
- **Timeline:** a "Zoom" slider and a "Snap" label; clips as flat blue bars
  with the file name; track names (V1…A3) scrolled away with the clips; a
  ruler with a label every 5 s at every zoom (crowded zoomed out, sparse
  zoomed in); the playhead could run off screen and stay there.
- **Inspector:** collapsible preset groups with counts and gradient swatches
  that didn't show the footage; the clip's trim, framing (crop/zoom) and info
  were hidden (`display: none`), so they could only be reached by dragging.
  Music could only be added by dropping a file.
- **Styles:** a base layer plus a "2026 studio finish" override layer on top
  of it, so most rules were defined twice; hard-coded dark colours in places,
  so the Daylight theme half-applied.

## Decisions

| Area | Now |
|---|---|
| Top bar | Window identity ("Edit · 5 clips · 2:40"), one segmented control for the output shape with a small glyph of each shape (details in the tooltip), icon toggles for the timeline / Look panel / side by side, Preview, and Export as the one accent button. Fits one line at 1024 px; drops the identity and labels first as it narrows. |
| Transport | Time to the hundredth (frame steps show), go to start / back a frame / play-pause (round) / forward a frame / go to end, and a program bar that shows where the clips are, fills to the playhead and scrubs on drag. New keys: ← → one frame, Home / End. |
| Timeline toolbar | Split, Snap (toggle), Music (pick a song or sound), zoom − slider + Fit, Clear, hide. |
| Tracks | Clips show their frames (the clip's poster at once, then its Focus filmstrip, built one clip at a time and never during playback); name and length over a shade at the top; trim grips appear on hover/selection. Track names in a column pinned to the left. Audio clips have a waveform-like stripe and a note icon. |
| Ruler | Labels every 1/2/5/10/15/30/60… s so they're ≥ 64 px apart, minor ticks between. |
| Playhead | Rounded head; the view pages to keep it on screen when it runs (or is sent) off either side. |
| Inspector | Two tabs. **Look**: group chips (All, Vlog & Portrait…), preset tiles that preview each look on the selected clip's picture, intensity, then the Adjust sliders. **Clip**: the clip (thumbnail, track, length), In/Out fields with Set in/out, framing sliders (when cropping), source → output facts, remove. Audio clips: name, keep the clips' sound, remove. |
| Styles | One stylesheet on the app's tokens (follows all four themes); the picture stage stays dark. The export dialog's styles are kept as they were. |

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/components/EditStudio.svelte` | UX | Markup of the top bar, transport, timeline (toolbar, ruler, track-name column, clip thumbnails, playhead follow) and inspector (tabs); stylesheet rewritten (dialog block kept). New helpers: posters/filmstrips per clip (`clipTiles`, `spriteCss`), `fmtTC`, `stepFrame`, `goToEdge`, `startProgramScrub`, `rulerStep`/`rulerMinor`, `zoomBy`, `fitTimeline`, `aspectGlyph`, `lookThumb`. Track height 46 px (`TRACK_HEIGHT`), timeline default 300 px. |
| `src/lib/components/EditWindow.svelte` | UX | ← → step a frame, Home/End; notices sit under the new top bar. |

## Behavior changes

- Clip trim (In/Out numbers), framing sliders and clip facts are back, in
  the Clip tab.
- "Music" in the timeline toolbar adds a song from a file picker.
- The timeline scrolls to follow the playhead.
- Building filmstrips for clips on the timeline is new work in the
  background (the same cache Focus uses), paused while playing.

## Risks / compat

- Nothing in the edit model, playback engine or export changed; the saved
  timeline format is the same.
- Filmstrip builds for long 4K clips cost some CPU after a timeline opens;
  they run one at a time and wait while playing.
- Built and checked in the Chromium harness only.

## Verification actually run

- `npm run check` 0/0, `npm run build`.
- Harness at 1440×900 and 1024×700 (one-line top bar), Studio and Daylight:
  timeline restored with 5 clips on V1/V2/V3 and a song on A1; clip frames
  drawn; look tiles show the clip's picture through each look; Clip tab with
  trim/framing/facts; program bar scrub to 0:48 shows the V2 clip's 10 s
  frame; ← → step 1/60 s; Play from 0:36 crosses into the V2 clip and the
  timeline scrolls to follow; export dialog renders its Source → Output table
  (a dropped flex-shrink rule collapsed it, restored).
