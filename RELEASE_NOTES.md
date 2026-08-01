<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## Interface recovery and smoother browsing

- FoxCull now uses one modern fox/aperture identity across the Windows title
  bar, executable, installer and in-app chrome. The native title is simply
  **FoxCull**, without build metadata competing with the product name.
- Arrange and Filters reliably float above the media grid again.
- Edit always exposes a **Media** recovery control after its picker is hidden,
  and Preview now has a visible **Back to edit** action instead of relying on
  Escape.
- The refresh action uses the same balanced SVG language as the rest of the
  command bar.
- Horizontal filmstrip input is direction-corrected for precision mouse wheels
  and eased to remove abrupt stepping.
- Grid thumbnail work is throttled so video poster extraction cannot saturate
  every loader slot while scrolling; virtualization and asynchronous loading
  remain intact.
- The responsive command bar switches to its two-row layout earlier, keeping
  Library, Edit and Settings reachable on laptop-width workspaces.

## A complete visual refit

FoxCull now looks and behaves like one coherent professional media workstation.
The Library, Focus player, Edit workspace, folder tree, details table, filmstrip,
menus, settings, dialogs, Trash and controller setup share the same surface
hierarchy, spacing, type, corners, shadows and interaction states.

The media stage stays spectrally neutral in every theme. Chrome is layered with
quiet luminance and depth instead of colour that could bias a photo or video
judgement.

## Four purpose-built themes

- **Studio** is the refined neutral graphite default.
- **Midnight** is a deeper, cooler black workspace.
- **Amber** reduces blue light for late-night sessions.
- **Daylight** is a crisp light workspace for bright rooms.

Each theme now carries the whole semantic palette: selection, focus, hover,
pick, reject, ratings, labels, stacks, borders, overlays and floating surfaces.

## Built for the XPS, Alienware and the TV

Settings now has three persistent interface sizes:

- **Compact** recovers canvas room on the XPS 13.
- **Standard** is tuned for normal laptop and monitor use.
- **TV / large** enlarges the entire interface for a distant display or a
  65-inch TV over HDMI.

The command bar no longer clips its right-hand controls. It compacts at laptop
widths and becomes a calm two-row bar in narrow windows or TV-large mode, keeping
Settings, casting and destructive actions reachable.

## Library polish

- The folder panel has a real FoxCull identity and shows the active folder.
- Grid, Details and Focus use one SVG icon language.
- Arrange and Filters are easier to scan; active filter counts are separate,
  high-contrast badges.
- Media tiles feel like real objects: cleaner focus/selection, better stack
  treatment, neutral image wells and less visual noise.
- The bottom culling bar has a stronger filename/metadata hierarchy, a contained
  rating control, quieter tags and explicit pick/reject totals.
- The first-run screen is now a purposeful launch surface with an Open Folder
  action and the four keys that matter first.

## Focus and Edit polish

- Focus keeps a true near-black reference surround. Video transport and clip
  tools use a legible glass overlay over any frame, with a cleaner scrub track
  and metadata card.
- Edit has clearer separation between Source, preview, Timeline and Look. Source
  cards, tracks, clips, preset groups, export menus, restore tabs and the export
  dialog now belong to the same design system as Library.
- Trash and controller setup use full-workspace modal surfaces with clearer
  headers, cards and selected states.

No culling, playback, export, cast or file-management behavior was intentionally
changed in this pass. The work is a visual and responsive refit over the proven
workflow model.

## Verification

- Visual QA: 1440×900, 1366×768 and 1024×768, plus TV-large scaling.
- `svelte-check`: 0 errors, 0 warnings.
- Production frontend build: passed.
- Rust `cargo check`: passed.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use “More info” → “Run anyway”.
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
