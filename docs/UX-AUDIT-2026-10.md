# FoxCull UI and UX audit, October 2026

Owner, 2026-10-04: "a holistic UI and UX audit for modern 2026–27 standards
across all screens, especially the Library, and themes and colour
combinations that may look good" — then "revisit everything: font size,
colour, shape, element sizes, card dimensions… make it professional grade".

The interactive version (live theme previews, contrast readouts) is a
claude.ai artifact: https://claude.ai/artifact/9PCFyNNGiyZS9rbWS5S1gW
(private to the owner). This file is the in-repo record.

Reviewed on the dev build of nightly.9 in the browser harness at 1440×900 and
1024×700, in all four themes of the time, with contrast measured against WCAG
2.2 AA.

## Measured

- 10 font sizes on the library screen (9 to 26 px), 7 weights (400–800).
- 10 corner radii (3–11 px, 50 %, 999 px).
- Faint text on raised panels: 4.2:1 Studio, 4.0:1 Midnight, 3.9:1 Amber,
  3.6:1 Daylight (AA needs 4.5). Daylight's accent 4.0:1; white on it 4.3:1.
- ~360 literal colours in component styles (app.css 127, Edit 72, Library
  53, Focus 53, Settings 38).

## Themes (implemented in nightly.10)

Every text role passes AA on both panels and raised cards (the faintest is
5.2:1). Pick green, reject red and star amber are fixed per theme and never
offered as accents.

| Theme | For | Panel / raised / text / faint | Accent |
|---|---|---|---|
| **Graphite** (default) | True neutral dark; nothing tints the photos | `#17181b` / `#1f2024` / `#ececef` / `#9a9da4` | `#82abff` |
| **Studio Grey** | Lightroom-like mid grey for judging exposure | `#333438` / `#3d3e43` / `#f4f4f5` / `#b9bbc0` | `#9dbdff` |
| **Midnight** | Near black, cool, for a dark room | `#0d0f14` / `#151821` / `#eef2f8` / `#8d98ab` | `#5aaeff` |
| **Amber Night** | Warm, low in blue, late sessions | `#1e1914` / `#28221b` / `#f4ece0` / `#ab9c87` | `#e2ab5c` |
| **Daylight** | Bright rooms (contrast fixed) | `#f6f7f9` / `#ffffff` / `#111418` / `#596270` | `#1b63c0` |
| **Paper** | Warm light | `#f7f4ee` / `#fffdf9` / `#1d1a16` / `#625a4f` | `#a84a22` |
| **Match system** | Graphite at night, Daylight by day | | |

- **Accent setting:** the theme's own, Blue, Indigo, Teal, Fox orange, Rose,
  Graphite (a dark and a light shade of each).
- **Behind the pictures:** the theme's dark, black, 18 % grey (`#767676`),
  light.
- The old themes migrate: Studio → Graphite, Midnight → Midnight, Amber →
  Amber Night, Daylight → Daylight.

## Design system (the numbers)

- **Type:** 11 (counts, times, badges) · 12 (buttons, labels) · 13 (body,
  menus, names; the default) · 15 (section heads) · 20 (window titles);
  weights 400 / 560 / 650; the platform's own face (SF Pro / Segoe UI
  Variable); tabular figures where numbers line up. Tokens `--fs-*`,
  `--fw-*`.
- **Radii:** 6 chips and badges · 8 buttons, fields, tiles · 12 cards and
  menus · 16 sheets · 999 pills. Tokens `--radius-*`.
- **Controls:** 24 chip · 28 toolbar · 32 button · 40 hero (play). Icons
  16 / 18 px at a 1.75 px stroke. Focus ring 2 px accent, 2 px out.
- **Sizes:** toolbar 52–54 px with the traffic lights inside; info bar 36;
  sidebar 240 (200–360); rows 28; grid tile 180 (120–320), gap 12 / 6;
  filmstrip cell 96; menus 240–320 wide; sheets ≤ 940 × 660.
- **Motion:** 120 ms hover, 160 ms menus (ease-out), 220 ms sheets; nothing
  moves under Reduce Motion.
- **Spacing:** a 4 px grid: 4 8 12 16 20 24 32.

## Findings

P1 = makes FoxCull look or feel a generation behind, or gets in the way while
culling. P2 = polish. "Done in" names the nightly that addressed it.

### Foundations

| ID | P | Finding | Change | Done in |
|---|---|---|---|---|
| F1 | P1 | Native title bar above FoxCull's own toolbar (~28 px of empty chrome) | macOS title bar folded into the top bars (Overlay, traffic lights inset); Windows keeps its own | .10 |
| F2 | P1 | No materials (macOS 26 Liquid Glass, Windows 11 Mica) | Sidebar/menu materials; the picture area stays opaque | later (needs a transparent window; risky) |
| F3 | P1 | 10 font sizes, 7 weights; 9–10.5 px labels | Five sizes, three weights, nothing under 11 px | .10 |
| F4 | P1 | Faint text fails AA in every theme | New palettes, all roles ≥ 4.5:1 | .10 |
| F5 | P1 | No "match system", no accent choice | Match system + accent setting | .10 |
| F6 | P2 | 10 radii | Four steps | .10 |
| F7 | P2 | Emoji and symbol glyphs as icons | One SVG line-icon set (`src/lib/icons.ts`) | .10 |
| F8 | P2 | ~360 literal colours | Tokens for scrims, glass, on-image text | .11 / .12 |

### Library

| ID | P | Finding | Change | Done in |
|---|---|---|---|---|
| L1 | P1 | Reject / Clear / red "Delete 24" always on the toolbar | A selection bar when something is selected; "Delete rejected" with the Rejected collection | .11 |
| L2 | P1 | Arrange and Filters can be open at once, overlapping | Opening one closes the other | .11 |
| L3 | P1 | Filters is a long form; what's filtered is invisible once closed | Filter chips under the toolbar | .11 |
| L4 | P1 | Up to eight badges per tile at 9–10.5 px | Two zones, larger badges, Minimal/Standard/Everything | .11 |
| L5 | P1 | Sidebar is only a tree | Sections: Drives, Pinned, Events, Collections | .11 |
| L6 | P1 | Shortcut guide shows Ctrl on the Mac | ⌘ ⌥ ⇧ ⌃ on macOS | .10 |
| L7 | P2 | Rejected = red tab + ✕ + heavy dimming | Lighter dimming | .11 |
| L8 | P2 | Unloaded video tiles say "MP4" | Shimmer while loading | .11 |
| L9 | P2 | Bottom bar holds everything | Name, marks, counts; the rest on demand | .11 |
| L10 | P2 | Details squeezes the name; Size off screen | Columns sized to content | .12 |
| L11 | P2 | Focus keeps all panels; very large play button | Tab hides panels; calmer play button | .12 |
| L12 | P2 | Right-click opens with Previous / Next | Most-used actions first | .12 |
| L13 | P2 | "Loading thumbnails" text | Progress ring only past a second | .12 |

### Tool windows

| ID | P | Finding | Change | Done in |
|---|---|---|---|---|
| T1 | P1 | Two generations of controls (Settings/Edit new; library menus, Merge, Reel old) | Shared components everywhere | .11 / .12 |
| T2 | P2 | Merge: dense table, long side column | Shared type scale, grouped side column | .12 |
| T3 | P2 | Reel step 1 half empty | Larger waveform, clearer steps | .12 |
| T4 | P2 | Edit has no visible undo | Undo/redo in its toolbar | later |
| T5 | P2 | Settings is the new standard | The reference for the rest | — |

### Accessibility and platform

| ID | P | Finding | Change | Done in |
|---|---|---|---|---|
| A1 | P1 | Colour labels are colour only | Names on hover and in menus | .11 |
| A2 | P2 | 22–26 px targets | 28 px minimum on Mac | .11 |
| A3 | P2 | Uneven animation | One set of durations | .10 (tokens) |
| A4 | P2 | Windows fonts first in the stack | Platform face first | .10 |
| A5 | P2 | Daylight half-applied in places | Token pass | .10 / .11 |
| A6 | P2 | No high-contrast option | Follow Increase Contrast | later |
