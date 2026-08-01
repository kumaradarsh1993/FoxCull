# FoxCull visual and interaction audit — August 2026

This is the design brief and coverage map for the August 2026 visual refit. The
owner's instruction was deliberately broad: inspect every visible surface and
workflow, preserve the functionality that already works, and make the product
feel professionally designed on the Alienware 15 R4, XPS 13 and a large HDMI
display viewed from across a room.

## Product posture

FoxCull is not a gallery and it is not a full NLE. It is a high-throughput media
decision workstation: find a folder, judge quickly, mark decisively, inspect a
keeper, cut a useful range, and move or export without an import ceremony.

The design system therefore optimizes for five things:

1. **Media authority.** Photos and video own the darkest, most neutral surface.
2. **Calm density.** Many controls can be present without becoming equal-weight
   visual noise.
3. **State legibility.** Active, selected, picked, rejected, rated, labeled,
   stacked, casting and disabled states must not be confused with one another.
4. **Progressive disclosure.** Frequent view/cull controls remain immediate;
   arrangement, filters, settings and destructive variants live in menus.
5. **Scale without a second app.** One layout adapts from XPS compact use to
   couch/TV use with persistent interface scaling and responsive toolbars.

## Workflow map

| Flow | Entry | Core surface | Decisions/actions | Exit or continuation |
|---|---|---|---|---|
| Start / resume | Launch, recent folder, Open | first-run or restored Library | choose folder, scan | Grid by default |
| Browse and cull | folder tree | Grid / grouped Grid | navigate, multi-select, pick, reject, rate, label, tag | next item, Focus, move, delete sweep |
| Compare metadata | View → Details | virtual details table | resize columns, choose columns, sort contextually, mark | Focus or Grid |
| Inspect a still/RAW | Enter / Focus | neutral Focus stage + filmstrip | next/previous, info, mark, cast, fullscreen/dim | return to prior view |
| Inspect a video | Focus on video | video stage + overlay transport | play, seek, Glimpse, in/out, marked segments, save range(s) | next clip, Edit, cast |
| Quick edit | Edit mode | Source + Preview + Timeline + Look | assemble, trim, crop, apply look, add audio, preview | export dialog |
| Export | Export split button / context action | preset menu + export dialog | destination intent, format, HDR, quality, name, time cost | output returns to Library |
| Organize | drag to tree or cut/paste | Library + folder tree | physical move, reveal, copy path | destination folder or current view refresh |
| Reject cleanup | hold Delete | confirmation + in-app Trash/system bin | remove rejects safely | Trash restore/purge or continue |
| Cast | Cast control | discovery/status popover + Focus/Grid | connect, follow active media, control TV transport | stop cast |
| Couch cull | controller enabled | Grid/Focus + controller guide | navigate, mark, rate, label, transport, filmstrip | entire session without keyboard |
| Background prep | Prepare / viewport demand | toolbar progress + ActivityBar | watch phase, ETA and failures | keep working while it runs |

## Permutation coverage

The UI has more states than its route count suggests. The refit treats the
following dimensions as composable rather than one-off screens:

- media: image, RAW, video, derived export, related-family representative;
- view: Grid, grouped/subgrouped Grid, Details, Focus, Edit;
- selection: none, active only, multi-selection, range selection;
- culling: unmarked, pick, reject, rating 1–5, five colour labels, tags;
- family: unstacked, expanded stack, folded stack, orphaned derivative;
- filter: none, type/status/rating/label/tag/scope combinations, zero results;
- filmstrip: bottom, left, right, hidden, full-screen dimmed, bare full-screen;
- storage: writable, read-only, per-drive library available/unavailable;
- playback: still, paused, playing, scrubbing, Glimpse, proxy/fallback, failed;
- cast: idle, searching, connecting, loading, live, paused, disconnected;
- background: idle, determinate progress, indeterminate work, error;
- appearance: Studio, Midnight, Amber, Daylight × Compact, Standard, TV-large;
- window: wide single-row toolbar, laptop-compacted bar, narrow two-row bar.

## Surface audit and result

| Surface | Previous visual problem | August 2026 treatment |
|---|---|---|
| Global theme | broad graphite slabs with weak hierarchy | semantic neutral palettes, layered luminance, soft/strong borders, two shadow levels, unified radii and type |
| Folder tree | generic “Folders” header; weak active row | FoxCull lockup, active-folder subtitle, iconized Open, accent rail + contained active row |
| Command bar | mixed glyphs; right actions clipped | SVG view/arrange/filter icons, explicit filter badge, responsive compaction, two-row fallback |
| First run / empty | helpful but visually incidental | product proposition, strong Open action, minimal starter shortcuts, controlled radial stage light |
| Grid / filmstrip | boxy tiles and hard selection | neutral image wells, rounded clipping, separate hover/selected/active layers, quieter elevation |
| Details | table felt detached from new chrome | consistent headers, floating Columns menu, stronger thumbnail wells and row states |
| Culling info bar | filename, marks and counts at one level | two-line identity, contained rating group, restrained tags, semantic totals |
| Focus photo/video | nearly neutral but warm-black; utility controls looked pasted on | true near-black stage, glass transport/clip panel, pill scrub rail, legible metadata glass |
| Edit | dense but visually fragmented mini-app | common pane chrome, source cards, stage, tracks, clips, inspector cards and export surfaces |
| Popovers/context menus | correct behavior but small flat boxes | larger targets, strong floating borders, blur, depth, consistent item radii |
| Settings | long list with no visual theme preview | theme swatches/names, persistent interface size, improved section rhythm and scrolling |
| Trash/controller/dialogs | functional overlays with generic panels | dim+blur backdrops, full studio cards, larger headers, deliberate selection/elevation |
| Activity | tiny utility footer | clearer progress rail, gradient fill/glow and better dock separation |

## Responsive contract

- **Above 1600 px:** full labels and one-row command bar where space permits.
- **1366–1600 px:** compact toolbar spacing and labels without losing actions.
- **1250 px and below:** two-row command bar; the action cluster stays complete.
- **760 px and below:** icon-first toolbar, narrower folder pane, tags suppressed in
  the culling footer; essential actions remain accessible.
- **TV / large:** the whole app scales to 122%, and explicitly uses the two-row
  command bar because CSS media queries cannot see transformed layout width.
- **Compact:** the whole app scales to 90%, preserving component proportions and
  increasing media canvas space on the XPS.

## Accessibility and fluency guardrails

- Existing keyboard focus rings and shortcut model remain intact.
- `prefers-reduced-motion` collapses non-essential animation and transition time.
- Semantic pick/reject/rating/label colours remain distinct across all themes.
- Disabled destructive actions stay visible in place, so the command layout does
  not jump as selection changes.
- All new icon-only controls retain accessible names/tooltips.
- The black media stage is theme-independent to avoid perceptual colour bias.

## Verification performed

- Browser visual inspection at 1440×900, 1366×768 and 1024×768.
- Standard and TV-large modes checked for exact viewport fit and reachable
  Settings/action clusters.
- Settings, first-run, single-row and two-row command states visually inspected.
- Native Tauri dev window inspected at 1280×850 with real drive roots, restored
  folder context, persisted Amber theme and the complete Settings panel.
- `npm run check`: 0 errors, 0 warnings.
- `npm run build`: passed.
- `cargo check`: passed.
- `git diff --check`: passed (normal Windows line-ending notices only).

Native media-heavy surfaces still deserve owner QA against real RAW/video folders
on both target laptops. The refit intentionally avoids changing media, catalog,
export, cast or filesystem logic.
