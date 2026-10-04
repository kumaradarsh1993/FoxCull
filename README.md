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
[FoxCull releases](https://github.com/kumaradarsh1993/FoxCull/releases/latest)

Current stable `v0.6.3` assets:

- Windows installer: `FoxCull_0.6.3_x64-setup.exe`
- Windows portable: `foxcull_0.6.3_x64_portable.zip`
- macOS Apple Silicon: `FoxCull_0.6.3_aarch64.dmg`
- Linux: `FoxCull_0.6.3_amd64.AppImage` or `.deb`

Builds are not notarized, so each OS warns once on first launch:

- **Windows** — SmartScreen: **More info → Run anyway**.
- **macOS** — *"Apple could not verify FoxCull is free of malware"*: click **Done**, then open
  **System Settings → Privacy & Security** and click **Open Anyway** next to FoxCull.
  Terminal equivalent: `xattr -dr com.apple.quarantine "/Applications/FoxCull.app"`

  > Right-click → Open stopped bypassing Gatekeeper in macOS 15 (Sequoia). Use **Open Anyway**.

## Prepare And Pre-Caching

FoxCull has three separate caching layers:

1. Folder open warms grid thumbnails automatically in the background. This is
   small-preview work for scrolling and poster frames.
2. Focus view prefetch keeps a few nearby full previews warm around the active
   item, biased in the direction you are moving.
3. **Folder right-click → Build previews for this folder** builds every
   full-size Focus preview and video poster up front.

You rarely need the third. Focus already prepares the next few shots as you
move, so on an internal or USB SSD there is no wait to remove. It helps on slow
cards and spinning disks: start it, do something else, come back to a folder
with no loading at all. It shows progress, time left and a Stop button in the
progress panel (bottom left), and stops by itself if you switch folders.

Live Scrub is separate from this. When Live Scrub is off, videos keep static
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

Stable releases are produced by pushing a tag like `v0.6.3`.
