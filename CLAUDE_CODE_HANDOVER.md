# Agent Handover: FoxCull

This file is intended to give Claude Code or any future coding agent enough
context to continue FoxCull without accidentally touching the original
Claude-built `fox-cull` project.

## Current State: Stable FoxCull v0.6.0

- Current main product name: **FoxCull**.
- Working folder: `D:\Claude Code Projects\FoxCullCodex`.
- GitHub repository: `https://github.com/kumaradarsh1993/FoxCullCodex`.
- Stable tag prepared: `v0.6.0`.
- Stable commit: `838a19f` (`Prepare FoxCull stable v0.6.0`).
- The earlier `fox-cull` folder is now legacy/reference only.
- User-facing "Codex" branding has been removed from the product. Remaining
  `_FoxCullCodex` / `foxcull-codex-*` references are compatibility fallbacks for
  existing libraries, stores, and portable folders.

Latest v0.6.0 changes:

- Product rename to FoxCull (`productName`, app title, package metadata, docs).
- Modern abstract icon applied across Tauri icons and favicon; four SVG options
  live in `assets/icon-options/` with a preview page at `docs/icon-options.html`.
- Library toolbar cleanup: Arrange menu owns sort/group/subgroup/stacks; top
  All and Trash duplicates removed; Prepare icon restored; Library/Edit switch
  made more prominent.
- Selection fixes: Shift-click and Shift-arrow range selection, Ctrl/Cmd+A
  select all, Grid up/down moves by row, Details remains row-by-row.
- Grouping fixes: Group + Subgroup now render as nested section headers.
- Warm theme restored for late-night work.
- Edit mode trim memory persists in-session per source path, so in/out points
  survive panel remounts and re-adding a clip.
- Prepare/pre-cache behavior formalized in `README.md` and `STORAGE.md`.
- New-drive library folder is `_FoxCull`; if a drive only has legacy
  `_FoxCullCodex`, FoxCull keeps using it to avoid hiding old catalog/cache work.

## Project Lineage

- Original reference app: `D:\Claude Code Projects\fox-cull`
- Current FoxCull app: `D:\Claude Code Projects\FoxCullCodex`
- GitHub repository: `https://github.com/kumaradarsh1993/FoxCullCodex`
- Visibility at creation time: private. A later request asked to make it public,
  but the tool policy blocked changing repository visibility because that would
  expose the full private code/history. The user may change visibility manually
  on GitHub if they decide the history is safe to publish.
- Default branch: `main`
- Codex-origin first commit: `b7a256f` / tag `v0.1.0`
- Codex releases documented here: `v0.1.0`, `v0.2.0`, `v0.3.0`, `v0.4.0`

The original `fox-cull` folder was treated as read-only reference material.
The fork was created by copying `fox-cull` into `FoxCullCodex` while excluding
generated/heavy folders such as `.git`, `node_modules`, `.svelte-kit`, `build`,
and Tauri/Rust `target` output.

## User Intent Dump

The user described FoxCull as a Claude-built Lightroom-style photo/video culling
app for Windows and macOS. They wanted Codex to create a separate fork, clearly
marked as Codex-origin work, and continue development there instead of editing
the original project.

The user's main product direction:

- Primary use case remains fast culling and organization of photos/videos.
- Secondary use case is a lightweight, Premiere-like editor inside the app.
- The editor should be optimized for real personal workflows, not a generic
  heavy editing suite.
- Typical source devices:
  - DJI Osmo Pocket 3, mostly 4K 60 fps, sometimes 4K 30, rarely 1080p.
  - DJI Mavic Mini, mostly 1080p 60 fps, sometimes 2K 30 fps.
  - Samsung S23 Ultra, usually Full HD 60 fps, sometimes Full HD 30 or 4K 60.
  - iPhone and other HEVC-capable devices may appear in trip footage.
- Important output use case: crop/trim landscape footage into portrait-friendly
  Instagram/Reels-ready clips.
- Output should be non-destructive and saved beside the original when possible.
- Re-encoding should be avoided when a stream-copy trim/concat can do the job.
- Re-encoding is acceptable when crop, resize, color tweaks, or audio/music
  changes require it.
- GPU acceleration is welcome on the user's Alienware 15R4 with NVIDIA GTX 1070.
- The user prefers local file workflows. Do not imply "upload" to cloud. Use
  local terms such as export, choose audio, reveal in folder.
- The user also wants Lightroom-style physical organization:
  - select files in a folder;
  - drag them to a folder in the left tree;
  - or use Cut/Paste to physically move them on disk;
  - preserve ratings/tags/flags/trims after the move.

The user asked for GitHub Actions to build native installers in the cloud so the
local machine does not have to run heavy Tauri builds.

## Version 0.1.0: Codex-Origin Fork And Edit Mode

Tag: `v0.1.0`

Major fork identity changes:

- `package.json` / `package-lock.json`: app name `foxcull-codex`
- Tauri product name: `FoxCull Codex`
- Tauri identifier: `com.foxcull.codex`
- Dev port: `1460`, HMR `1461`
- Rust package: `foxcull-codex`
- Rust library: `foxcull_codex_lib`
- Portable data folder: `foxcull-codex-data`
- Log file: `foxcull-codex.log`
- Settings/store files use Codex-specific names
- Per-drive library folder changed from `_FoxCull` to `_FoxCullCodex`
- UI branding changed to `FoxCull Codex`
- README and STORAGE notes updated for Codex fork separation

Edit mode implementation:

- New UI component: `src/lib/components/EditStudio.svelte`
- Integrated from main page: `src/routes/+page.svelte`
- Backend command: `edit_export` in `src-tauri/src/commands.rs`
- API/types added in:
  - `src/lib/api.ts`
  - `src/lib/types.ts`
- Tauri command registered in `src-tauri/src/lib.rs`

Edit mode capabilities:

- Add active video or selected videos into an edit timeline.
- Duplicate segments from the same clip.
- Reorder segments.
- Set in/out trim points.
- Use output presets:
  - Instagram/Reels 9:16, 1080x1920
  - Square 1:1, 1080x1080
  - Landscape 16:9, 1920x1080
  - Original stream-copy mode
- Drag/adjust crop position and zoom per segment.
- Basic look controls:
  - brightness
  - contrast
  - saturation
  - warmth
  - sharpen
- Optional local music/audio track selection.
- Export behavior:
  - stream-copy for simple original-resolution trim/concat when no pixel/audio
    changes are requested;
  - re-encode for crop, resize, color tweak, music, or filtered multi-clip output;
  - `auto` encoder tries NVIDIA NVENC first, then falls back to x264.

Release workflow:

- `.github/workflows/release.yml` was adapted for FoxCull Codex branding.
- GitHub Actions builds Windows, macOS Apple Silicon, and Linux.
- Windows portable zip is also packaged.

Validation run for v0.1:

- `cargo check` passed.
- `npm run check` passed with 0 errors and one existing Node type warning.
- `npm run build` passed.

## Version 0.2.0: Safe Organization And Path Guardrails

Tag: `v0.2.0`

User asked to fix earlier audit concerns around destructive backend operations
trusting frontend paths, plus add Lightroom-style physical organization.

Backend path-safety changes:

- Added canonicalization and validation helpers in `src-tauri/src/commands.rs`.
- Destructive/media operations now verify paths are:
  - absolute and canonicalizable;
  - inside the currently opened library/drive root;
  - outside `_FoxCullCodex` internal app folders;
  - real media files where a media operation is expected;
  - free of path traversal tricks such as absolute nested trash paths or `..`.
- Applied guardrails to:
  - trim/export source validation;
  - edit export source validation;
  - JPEG export source validation;
  - delete/dispose rejected files;
  - folder writability probe.

Safer in-app Trash validation:

- Trash restore/purge no longer trusts arbitrary UI-supplied paths.
- Restore and purge use catalog-tracked trash entries.
- Stored trash paths must remain inside `_FoxCullCodex/recycle`.
- Restore targets must remain inside the active drive/library.
- Malformed or stale trash rows are ignored/pruned instead of acted on.

Physical media organization:

- Backend command added: `move_media_files`
- Types/API added:
  - `MoveRecord`
  - `MoveOutcome`
  - `api.moveMediaFiles`
- UI features added:
  - drag media from Grid onto a folder in the left tree;
  - drag media from Details view onto a folder in the left tree;
  - keyboard `Ctrl/Cmd+X` then `Ctrl/Cmd+V` to move selected files into the
    currently open folder.
- Moves physically move files on disk and uniquify target names on collision.
- Cache cleanup runs for moved files so stale previews/posters/proxies do not
  linger.
- Catalog metadata follows moved files:
  - ratings
  - labels
  - flags
  - tags
  - video trims
  - cached capture dates
- Folder count cache is cleared after moves.

Validation run for v0.2:

- `cargo check` passed.
- `npm run check` passed with 0 errors and one existing Node type warning:
  `Cannot find type definition file for 'node'`.
- `npm run build` passed.

## Version 0.3.0: Editor Entry Flow And UI Repair

Tag: `v0.3.0`

User feedback that triggered this release:

- Clicking Edit after selecting a video opened an empty editor with no obvious
  way to add the video.
- The left folder tree must remain folder-only, but needed a collapse control.
- The top toolbar was cluttered, wrapped to a second line, and made Edit look
  like another view button instead of a dedicated mode.
- Type/status/rating/tag controls needed clearer information grouping.
- Visible Cut/Paste controls were unwanted; the user wanted keyboard move
  semantics and drag-to-folder organization.
- Reject should become Unreject when the active item or selection is already
  rejected.

UI/editor changes:

- Edit is now a dedicated Browse/Edit mode toggle on the top-right.
- Grid, Details, and Focus remain grouped under View.
- Sort and Group have explicit labels and shorter option names.
- Type, Status, Rating, Label, Tag, and Scope moved into one Filters popover;
  the Filters button shows a count when any of those filters is active.
- The left folder pane can collapse to a narrow rail and expand again without
  exposing files in the tree.
- Visible toolbar/context-menu Cut/Paste entries were removed. `Ctrl/Cmd+X`
  and `Ctrl/Cmd+V` still move selected files into the current folder, and
  drag-to-folder still works.
- Reject now toggles to Unreject when all active targets are rejected.
- The bottom active-item Reject button also shows Unreject for rejected media.

Edit workspace changes:

- `EditStudio.svelte` now receives the current media view as `sourceItems`.
- Opening Edit with selected videos preloads those videos into the edit
  timeline; if no selected video exists, the active video is used.
- The editor viewport is split into:
  - source video tray;
  - preview/transport/timeline work area;
  - segment/look/audio/export inspector.
- Current view videos appear in the source tray and can be clicked or dragged
  into the timeline.
- Timeline drop target is visible even when empty.
- Preview brightness/contrast/saturation now reflect the current look sliders.
- The source item double-click duplicate-add path was removed so clip adding has
  one predictable click/drag path.

Validation run for v0.3:

- `cargo check` passed.
- `npm run check` passed with 0 errors and one existing Node type warning:
  `Cannot find type definition file for 'node'`.
- `npm run build` passed.
- In-app browser smoke test passed for:
  - one-line toolbar;
  - Filters popover with Type/Status controls;
  - collapsed folder rail.

## Version 0.4.0: Distinct Identity And Edit Workflow Hardening

Tag: `v0.4.0`

User feedback that triggered this release:

- The Codex fork needed a clearly different desktop identity from the
  Claude-built original.
- The old icon was not visually useful; the user wanted a new noticeable app
  icon across all touchpoints.
- Edit mode still felt difficult to start because there was no visible way to
  bring videos into the editor.
- Edit mode needed a more dedicated workflow, not the full crowded culling
  toolbar competing with the editor.

Identity/theme changes:

- `assets/icon.svg` was replaced with a cyan/violet fox-skull/camera mark.
- `scripts/make-icon.mjs` now regenerates:
  - `assets/icon-1024.png`;
  - `static/favicon.png`;
  - `docs/images/foxcull-codex-icon.png`.
- `npm run tauri -- icon assets/icon-1024.png` regenerated all Tauri icon
  touchpoints: Windows ICO/store logos, macOS ICNS, Linux PNGs, and mobile
  generated icons.
- App theme tokens moved away from orange/brown into:
  - light cool neutral with teal accent;
  - dark graphite with cyan accent;
  - warm low-blue plum accent.
- The docs/presentation page theme and v0.4.0 download metadata were updated.

Edit workflow changes:

- Edit mode now receives all media from the open folder, not only the filtered
  Browse view, so active filters no longer hide usable source videos.
- The editor source panel has explicit `Choose videos`, `Add source`, and
  `Add selected` actions.
- Source rows now select/highlight first; adding is visible via the row Add
  button, double-click, drag-to-timeline, or the header Add button.
- The empty timeline/preview states now point toward adding video instead of
  looking like a dead panel.
- `api.pickVideos()` was added for multi-video selection inside Edit mode.
- Timeline duration lookup first tries WebView metadata, then falls back to the
  existing FFmpeg-backed filmstrip metadata for camera-native/HEVC clips.
- Preview playback uses cached H.264 proxies when available and can generate a
  proxy on preview decode failure.
- Multi-clip exports no longer ask the backend to preserve a single source audio
  track.
- Browse-mode sort/group/filter/culling actions are hidden while Edit mode is
  active; the top bar becomes a compact Quick Edit mode header plus the
  Browse/Edit toggle and Settings.

Validation run for v0.4:

- `npm run check` passed with 0 errors and the existing Node type warning:
  `Cannot find type definition file for 'node'`.
- `npm run build` passed.
- `cargo check` passed.
- `git diff --check` passed, reporting only normal CRLF line-ending warnings.
- Per latest user instruction, do not use localhost/browser rendering as release
  validation; push the tag, wait for GitHub Actions, and provide the release
  page after native artifacts are built.

## Version 0.5.2-nightly.3: Culling Subclips, Related Stacks, And Edit Polish

User goals in this round:

- Keep culling as the primary workflow, but add a clean way to extract multiple
  useful video subclips from long Osmo Pocket / DJI / phone footage without
  entering a heavy Premiere-style edit session.
- Show relationships between original files and derived files: RAW+JPEG pairs,
  motion-photo sidecars, burst groups, subclips, and crop/edit outputs.
- Add a Lightroom-style `I` overlay in Focus for important file/video metadata.
- Make Edit mode panels resizable/collapsible, fix the Look panel collapse,
  improve timeline zoom, and support cropped-output preview in fullscreen.
- Keep Live Scrub behind a toggle and reduce thumbnail/scrub aspect jumps.

Implemented in this nightly:

- Focus video subclips:
  - Users can mark multiple in/out ranges on the Focus video timeline.
  - Ranges are persisted in the per-drive catalog table `video_segments`.
  - Batch export creates separate stream-copy subclips beside the source video
    as `_sub01`, `_sub02`, etc., uniquifying names if needed.
  - Export refreshes the active folder so newly-created subclips appear in the
    Library/Focus workflow.
  - Partial export failures are surfaced in the Focus note instead of being
    hidden behind a generic success message.
- Related stack UI:
  - Library derives related groups for RAW+JPEG, `_subNN`, crop/edit suffixes,
    motion-photo style image/video pairs, and conservative burst-name runs.
  - Grid and filmstrip show subtle stack treatment, badges, role labels, and
    folded counts.
  - Context menus can expand/collapse a related group, and the top toolbar /
    settings can open or fold all related groups.
  - Focus view can show a compact related-family strip for the active item.
- Focus metadata overlay:
  - Press `I` in Library/Focus to toggle an overlay with filename, kind/format,
    size, video duration/resolution/FPS/codec/camera when probed, and modified
    date.
- RAW/JPEG export:
  - Library export now confirms RAW count vs photo count before exporting.
  - If exported into the active folder/subfolder, the current folder refreshes.
- Edit mode:
  - Source, Look/right inspector, and timeline panels have resize/collapse
    behavior that reclaims the underlying space.
  - Ctrl+mouse-wheel over the edit timeline zooms horizontally around the cursor.
  - Video and audio tracks are visually separated.
  - The `Preview` toggle shows cropped-output preview, and pressing `F` from
    Edit mode now enters that cropped preview before toggling app fullscreen.
  - The small-screen CSS no longer makes the Look panel impossible to reopen.
- Live Scrub:
  - Thumbnail scrub frames preserve the static thumbnail's visible aspect area,
    and cursor math ignores letterboxed padding.
  - Scrub sprite cache cleanup is included in per-file cache cleanup.

Validation run for v0.5.2-nightly.3:

- `npm run check` passed with 0 errors and the existing Node type warning.
- `npm run build` passed.
- `cargo check` passed after version bump to `0.5.2-nightly.3`.
- `git diff --check` passed, reporting only normal CRLF line-ending warnings.

Known caveats after v0.5.2-nightly.3:

- Burst grouping is heuristic and intentionally conservative.
- Related grouping is frontend-derived from filenames/kinds/timestamps; it does
  not yet store explicit parent-child relationships in the catalog.
- The quick editor is still intentionally lightweight. It supports timeline
  clips, crop presets, look presets, audio choice, snapshots, and export, but
  not crop keyframes or full Premiere/Final Cut feature depth.
- Mobile rotation/display-aspect edge cases should be tested on real S23/iPhone
  vertical clips before treating the export path as final.

## Version 0.5.2-nightly.4: Edit/Library Polish After User QA

Tag: `v0.5.2-nightly.4`

This pass addressed the user's screenshot-based QA notes after trying the edit
workspace and related stack UI:

- Edit mode:
  - Collapsing the Look/right panel or bottom timeline now leaves visible restore
    tabs over the preview, so the panels can be brought back.
  - The export Options menu is no longer clipped behind the video preview.
  - Frame capture shows a top-bar confirmation toast after saving.
  - The Source pane now renders media as compact cards with thumbnail, name,
    duration, resolution, FPS, codec, capture date/camera when probed, and size
    chips instead of cramped overlapping columns.
  - `[` and `]` shortcuts set the current clip's in/out points in Edit mode.
- Focus/video playback:
  - Videos default to paused; autoplay is now an explicit visible setting.
  - The Focus video controls expose Auto and Info buttons.
  - `[` and `]` shortcuts mark in/out points in Focus video trim mode.
  - Drag-seeking updates the playhead optimistically, uses `fastSeek` when the
    webview supports it, and throttles real seeks to animation frames.
  - The playhead target is larger and easier to grab.
- Library grouping and stacks:
  - Related groups choose a stable mother/original first, with RAW preferred for
    RAW+JPEG and the still image preferred for motion-photo pairs.
  - Collapsed related groups show the mother item as the representative.
  - Stack backgrounds, mother borders, collapsed counts, and role badges are more
    visible in grid/filmstrip views.
  - Section headers are stronger and show item counts.
  - Grid grouping supports an optional second grouping level, for example type
    then month.
- Details view:
  - Details mode now has resizable columns and a Columns menu.
  - Added richer media columns including resolution, FPS, duration, codec,
    camera, folder, and tags.
- Theme:
  - Removed the old plum/purple Warm theme.
  - Added a neutral graphite Lightroom-like theme and made it the default.
  - Dark mode also uses neutral graphite tones with a muted blue-gray accent.

Validation run for v0.5.2-nightly.4:

- `npm run check` passed with 0 errors and the existing Node type warning.
- `npm run build` passed.
- `cargo check` passed after the version bump to `0.5.2-nightly.4`.

## Version 0.5.2-nightly.5: Edit Regression Fixes And Playback Controls

Tag: `v0.5.2-nightly.5`

This pass addressed the user's follow-up screenshots and notes after
`v0.5.2-nightly.4`:

- Edit Source pane:
  - Removed duplicate duration/resolution/FPS/codec presentation.
  - Reworked each source item into a card hierarchy: thumbnail, filename,
    duration, technical subline, file/date chips, and culling-state chips from
    the Library metadata when available.
  - Source cards now surface Pick/Reject/rating/label/tags when the file is
    also present in the current library listing.
- Edit workspace:
  - Space now toggles play/pause in Edit mode without first clicking the video.
  - Shift+Left/Right seek the edit preview by five seconds.
  - Edit preview seeking is optimistic and animation-frame throttled, with
    `fastSeek` used where the WebView supports it.
  - Preview videos now request `preload="auto"`.
  - The edit grid no longer forces a 420px center column, reducing the issue
    where the right Look panel appeared to float over the video on constrained
    window widths.
  - Toolbar/menu stacking was tightened so the Options menu stays above preview
    and timeline content.
- Focus/video playback:
  - Dragging the timeline now pauses playback, throttles decoder seeks, updates
    the playhead immediately, final-seeks on release, and resumes playback if it
    was playing before the drag.
  - Focus videos now request `preload="auto"`.
  - The visible Auto toggle was removed from the Focus video strip; autoplay
    remains in the app settings menu.
  - In/out/export controls are tucked behind a `Clip tools` toggle with clearer
    labels: save current range vs save marked ranges.
  - The timeline playhead was simplified back to a clean vertical bar, and the
    information overlay was made more transparent.

Validation run for v0.5.2-nightly.5:

- `npm run check` passed with 0 errors and the existing Node type warning.
- `npm run build` passed.
- `cargo check` passed after the version bump to `0.5.2-nightly.5`.

## Release State

Published releases at handover time:

- `v0.1.0`: `https://github.com/kumaradarsh1993/FoxCullCodex/releases/tag/v0.1.0`
- `v0.2.0`: `https://github.com/kumaradarsh1993/FoxCullCodex/releases/tag/v0.2.0`
- `v0.3.0`: `https://github.com/kumaradarsh1993/FoxCullCodex/releases/tag/v0.3.0`
- `v0.4.0`: `https://github.com/kumaradarsh1993/FoxCullCodex/releases/tag/v0.4.0`
- `v0.5.2-nightly.3`: `https://github.com/kumaradarsh1993/FoxCullCodex/releases/tag/v0.5.2-nightly.3`

Important v0.4.0 assets:

- Windows installer:
  `FoxCull.Codex_0.4.0_x64-setup.exe`
- Windows portable:
  `foxcull-codex_0.4.0_x64_portable.zip`
- macOS Apple Silicon:
  `FoxCull.Codex_0.4.0_aarch64.dmg`
- Linux:
  `FoxCull.Codex_0.4.0_amd64.AppImage`
  and `FoxCull.Codex_0.4.0_amd64.deb`

Because the repository is private, release pages and assets are visible only to
accounts with access to the repository.

## Important Files

- `src-tauri/src/commands.rs`
  - Core Tauri commands.
  - Edit export implementation.
  - Path-safety helpers.
  - Physical file move command.
  - Trash validation.
- `src-tauri/src/catalog.rs`
  - SQLite catalog.
  - Added `move_media_entries` to move metadata rows after physical file moves.
- `src-tauri/src/lib.rs`
  - Tauri app setup and command registration.
- `src/lib/components/EditStudio.svelte`
  - Quick editor UI.
- `src/routes/+page.svelte`
  - Main app integration.
  - Edit mode entry point.
  - File organization keyboard/drag/drop wiring.
- `src/lib/components/TreeNode.svelte`
  - Folder tree drop targets.
- `src/lib/components/DetailsView.svelte`
  - Details-row drag support.
- `src/lib/api.ts`
  - Frontend Tauri API wrapper.
- `src/lib/types.ts`
  - Shared TypeScript types.
- `.github/workflows/release.yml`
  - Native release builds and portable packaging.

## Local Development Notes

- Do not run heavy local `npm run tauri build` unless the user explicitly asks.
  This machine is resource constrained; GitHub Actions should build installers.
- Local sanity checks that were safe and already used:
  - `npm run check`
  - `npm run build`
  - `cargo check` inside `src-tauri`
- The dev server uses port `1460`, but it is only a frontend/dev preview. The
  native app artifacts come from GitHub Releases.

## Known Caveats / Future Work

- The editor is a lightweight quick editor, not a full Premiere replacement.
  It has timeline segments, crop, color basics, audio choice, and export, but
  not advanced transitions, nested tracks, keyframed crop motion, or full
  multitrack audio mixing.
- The user's "moving portrait window following a drifting subject" idea is not
  implemented yet. A future version could add simple crop keyframes.
- Drag/drop and cut/paste organization passed compile/build checks, but should
  be manually exercised on disposable test media before using on an important
  folder.
- App binaries are not code-signed/notarized yet, so Windows SmartScreen and
  macOS Gatekeeper warnings are expected.
- Existing `svelte-check` warning remains: missing Node type definition file.

## User Preferences To Preserve

- Keep original `fox-cull` untouched unless the user explicitly asks.
- Keep Codex-origin work in `FoxCullCodex`.
- Prefer local, private, file-based workflows.
- Avoid wording like "upload" unless the feature genuinely uploads something.
- Keep exports non-destructive.
- Prefer stream-copy/lightweight ffmpeg routes when technically safe.
- Make release notes user-friendly, not commit-log style.
- Build native installers through GitHub Actions.
