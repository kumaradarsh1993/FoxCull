# FoxCull v1.1.0-nightly.4 — the UX & Looks pass

A full independent audit of every button, menu, shortcut, and panel — then
the overhaul. (The video preview rework from nightly.3 is included too if
you're jumping straight here.)

## Feels-native fixes

- **Menus finally behave**: every toolbar popover (Settings, Filters,
  Arrange, Clear, Cast) and the Details Columns menu now close when you
  click anywhere else or press Esc — no more re-clicking the button to make
  a menu go away.
- **Press `?` for the shortcut guide** — every key, grouped (navigate,
  views, culling, video, files, mouse & controller). Tooltips on the main
  buttons now show their key too.
- Right-click works in the **Details list** now (same menu as the grid),
  keyboard focus is visible when tabbing, empty states offer a one-click
  **Clear filters**, and a handful of glyph/consistency papercuts are gone.

## Progress bars you can trust

- Background jobs in the activity chip now show a **time estimate**
  (`124 / 800 · ~4m 30s`) computed from the actual observed rate — it only
  appears once there's enough data to be meaningful, and never on jobs whose
  length genuinely isn't knowable.
- **Prepare** now processes photos first, then videos, and estimates each
  phase separately — so preparing a mixed folder says ~20 minutes when it
  will take ~20 minutes, instead of extrapolating a photo-speed fantasy.
  Its tooltip also finally says what it does now (full previews + video
  posters + hover scrub strips).

## Looks that actually look like something

The Edit studio's Look panel was overhauled end to end:

- **Sliders have real impact now.** Warmth and split-tone were
  mathematically too timid — a full-range warmth drag only shifted color
  ~20%; presets built on tiny values barely registered. Both were
  strengthened in the live preview *and* the export filters together, so
  what you see is exactly what renders.
- **12 researched presets in collapsible groups**: Vlog & Portrait (Warm
  Portrait, Soft Skin, Golden Hour) · Drone & Landscape (Vivid Landscape,
  Orange & Teal) · Black & White (Mono, Noir) · Cinematic (Teal & Orange,
  The Batman, Moody Film) · Clean & Correction (Osmo Clean, and a new
  **De-Log Boost** for flat/log footage). Every preset is clearly visible at
  default intensity; the Intensity slider (0–150%) scales them all.
- Clear active-preset highlight, a "Reset look" button, and per-slider
  double-click reset as before.
- Heads-up: **Sharpen** applies on export only (no live preview yet) — it's
  the one slider without instant feedback.
