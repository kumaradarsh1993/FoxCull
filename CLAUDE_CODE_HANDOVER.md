# Claude Code Handover: FoxCull Codex

This file is intended to give Claude Code or any future coding agent enough
context to continue the Codex fork without accidentally touching the original
Claude-built FoxCull project.

## Project Lineage

- Original reference app: `D:\Claude Code Projects\fox-cull`
- Codex fork: `D:\Claude Code Projects\FoxCullCodex`
- GitHub repository: `https://github.com/kumaradarsh1993/FoxCullCodex`
- Visibility at creation time: private
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

## Release State

Published releases at handover time:

- `v0.1.0`: `https://github.com/kumaradarsh1993/FoxCullCodex/releases/tag/v0.1.0`
- `v0.2.0`: `https://github.com/kumaradarsh1993/FoxCullCodex/releases/tag/v0.2.0`
- `v0.3.0`: `https://github.com/kumaradarsh1993/FoxCullCodex/releases/tag/v0.3.0`
- `v0.4.0`: `https://github.com/kumaradarsh1993/FoxCullCodex/releases/tag/v0.4.0`

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
