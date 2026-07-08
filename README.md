# FoxCull

FoxCull is a fast desktop photo and video culling app with a lightweight edit
lane for practical social/video exports. It is built for browsing media in
place, marking what matters, moving files between folders, and trimming/cropping
clips without opening a full NLE unless the job truly needs one.

This repository is now the main FoxCull product line. The older `fox-cull`
project is treated as the legacy Claude-built variant.

## What It Does

- Browse folders and drives in place without importing originals.
- Cull with Grid, Details, and Focus views.
- Rate, color-label, pick/reject, tag, filter, sort, group, and subgroup media.
- Detect related stacks such as RAW+JPEG, edited derivatives, crop/export
  outputs, burst-like shots, and motion-photo-style companions.
- Move selected files physically by dragging onto the folder tree or by
  `Ctrl/Cmd+X` then `Ctrl/Cmd+V`.
- Use Live Scrub for hover previews when you want it; leave it off to avoid
  video scrub-strip work.
- Use Edit mode for timeline trims, crop presets, look presets, audio lanes,
  screenshots, preview/fullscreen review, and exports.
- Stream-copy where possible; re-encode only when crop, color, audio, or format
  conversion requires new pixels/audio.

## Download

Latest stable release:
[FoxCull releases](https://github.com/kumaradarsh1993/FoxCullCodex/releases/latest)

Current stable `v0.6.2` assets:

- Windows installer: `FoxCull_0.6.2_x64-setup.exe`
- Windows portable: `foxcull_0.6.2_x64_portable.zip`
- macOS Apple Silicon: `FoxCull_0.6.2_aarch64.dmg`
- Linux: `FoxCull_0.6.2_amd64.AppImage` or `.deb`

Windows and macOS builds are not code-signed/notarized yet, so first launch may
show SmartScreen or Gatekeeper warnings.

## Prepare And Pre-Caching

FoxCull has three separate caching layers:

1. Folder open warms grid thumbnails automatically in the background. This is
   small-preview work for scrolling and poster frames.
2. Focus view prefetch keeps a few nearby full previews warm around the active
   item, biased in the direction you are moving.
3. The **Prepare** button explicitly builds full-size Focus previews and video
   posters for the current folder/filter set up front.

Prepare is optional. It is useful before a serious culling pass because moving
through Focus view should then avoid blur/loading waits. It runs in chunks on the
backend warmer, shows progress/ETA, and abandons itself if you switch folders.

Live Scrub is separate from Prepare. When Live Scrub is off, videos keep static
posters and do not generate hover scrub strips. When it is on, scrub previews are
generated on demand and cached at preview scale.

All generated cache files live in the active drive library (`_FoxCull/thumbs`) or
the app-data fallback for read-only drives. Originals are not modified.

## Storage

Each writable drive gets a self-contained `_FoxCull` folder with:

- `catalog.sqlite` for ratings, labels, flags, tags, trims, and capture dates.
- `thumbs/` for thumbnails, Focus previews, posters, and scrub assets.
- `recycle/` for the in-app Trash.

`_FoxCull` is the only per-drive library folder used by current builds. Old
preview/cache folders from pre-stable builds can be deleted after migration.

Full details are in [STORAGE.md](STORAGE.md).

## Useful Shortcuts

| Key | Action |
|---|---|
| Arrow keys | Move selection; Grid up/down moves by row |
| Shift + click / Shift + arrows | Select a range |
| Ctrl/Cmd + A | Select all visible items |
| Enter | Toggle Focus view |
| G / D | Grid / Details |
| F | Full screen |
| L | Dim / lights-out |
| Space | Play/pause active video |
| [ / ] | Set video in/out |
| 1-5 | Star rating |
| 6 / 7 / 8 / 9 / 0 | Blue / purple / red / green / yellow label |
| P / X | Pick / Reject |
| U | Clear stars, color, and pick/reject |

## Build Notes

FoxCull is Tauri 2 + SvelteKit + Rust. Heavy native builds should run through
GitHub Actions release tags, not on the local Windows machine. For local sanity:

```powershell
npm.cmd run check
cd src-tauri
cargo check
```

Stable releases are produced by pushing a tag like `v0.6.2`.
