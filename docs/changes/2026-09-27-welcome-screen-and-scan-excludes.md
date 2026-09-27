# Welcome screen, Excluded folders, and the whole-drive scan

## Intent

The owner asked for two things after installing `v1.5.0-nightly.7` on a Mac:

1. On first open, the right-hand panel should be a proper welcome screen that
   says where to start (pick a folder on the left) and what the app does,
   instead of anything that points at a drive root.
2. Clicking a system drive (`C:\`, Macintosh HD) must not scan and cache system
   folders. Exclusions should be a setting, a full "exclude module" with system
   folders pre-selected, the user's own additions, and local persistence.

What the Mac's own log showed, before any code changed:

- `SCAN dir="Macintosh HD" recursive=true files=241138 walk=71237ms`, on 2026-08-27.
  It left a 234 MB `libraries/root` library in app data: 95 MB catalog, 96 MB WAL,
  43 MB thumbnails, **241,138 capture-cache rows and zero decisions**.
- On 2026-09-27 the app relaunched with `lastDir = /Volumes/Macintosh HD` and
  started the same walk again (cancelled after 25 s when the user clicked away).
- nightly.7 is tagged at `0ed1bf2` (the signing fix), one commit **before** the
  macOS skip-list fix `d7e6995`, so the installed build had no Mac exclusions.
- Neither external drive (`SD_Card`, `MAHINDRA`) had a `_FoxCull` folder. Their
  caches had gone to the Mac's `root` library instead, a separate bug (below).

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/commands.rs` | logic / architecture | `is_skippable_dir` replaced by one `skip_dir(parent, name)` used by `list_tree`, `collect_cancellable` (scan + relink + edit sources) and `count_media` (badges, which previously skipped only dotfolders). Rules are location-anchored: machine-owned names anywhere; Windows OS names only directly under a drive root; macOS OS names only directly under the boot volume; `Library`/`Applications` only directly inside a home folder. Five built-in groups plus user paths and `*` name patterns, held in a `RwLock<ScanExcludes>` pushed by the frontend (`set_scan_excludes`). Always skipped regardless: dotfolders, `_FoxCull`, Trash, `$RECYCLE.BIN`, `System Volume Information`, and `/Volumes` + `/System/Volumes` when walking a Mac's startup disk. New `is_system_root` and `suggested_folders` commands. `list_drives` on macOS drops the `/Volumes/Macintosh HD` symlink (a second copy of `/`) and names the `/` entry after it. |
| `src-tauri/src/lib.rs` | process | Registers the three new commands. |
| `src/lib/components/Welcome.svelte` | UX | New. First-open placeholder, deliberately quiet (a first draft with six feature cards and a bouncing sidebar cue was judged cluttered by the owner and cut): icon, "Welcome to FoxCull", two short lines pointing at the sidebar, one Open folder… button, a "Start from" list (camera cards first, then Pictures/Movies/Desktop/Downloads, max 5, with short paths), a one-line note when a system drive was not reopened, and a faint footer (features in one line, `?` for shortcuts, Excluded folders link). One soft accent glow is the only decoration. Scrolls from the top on short windows. |
| `src/lib/components/ExcludePanel.svelte` | UX | New. Settings → Excluded folders: the five groups as ticked checkboxes listing exactly what each hides, custom rules (Add folder…, or a typed name/`*` pattern/full path), remove per rule, Reset to defaults. Writes through to the settings store. |
| `src/lib/settings.svelte.ts` | logic | `scanExcludes` setting + `defaultScanExcludes()`; load merges stored value over defaults so future groups arrive ON. |
| `src/lib/types.ts`, `src/lib/api.ts` | logic | `ScanExcludes`, `SuggestedFolder` types; `setScanExcludes`, `isSystemRoot`, `suggestedFolders` wrappers. |
| `src/routes/+page.svelte` | logic / UX | Pushes exclude rules before any listing on mount. Launch no longer reopens `lastDir` when it is a system drive root; the welcome screen offers it back. `rootForDir` picks the most specific drive (see below). Welcome markup + its CSS replaced by `<Welcome>`. Settings row "Excluded folders", folder right-click "Exclude from scans" (disabled on drive roots). Rule changes re-list the tree (`{#key treeGen}`), clear cached badge counts and re-scan the open folder. |
| `STORAGE.md`, `docs/design/precache-policy.md` | process | Record the external-drive library bug and how exclusions affect what gets cached. |

## Behavior changes

- **First launch** shows the new welcome screen (it already showed a simpler one;
  nothing auto-opened on a clean install).
- **Relaunch after leaving a system drive open** no longer re-scans it. Other
  drive roots (an SSD, a camera card) and ordinary folders still reopen.
- **Opening a system drive** skips OS and app folders by default. Measured on this
  Mac, `collect("/")`: 133,965 files / 37.2 s with every group off (and `/Volumes`
  already excluded; the original 241k/71 s run also crawled both external drives),
  vs **89,726 files / 9.3 s** with defaults. Of the remaining 89,726, 89,484 are
  in `~/Documents` (real image datasets from coursework). That is user content, which
  is exactly what the custom exclude list is for.
- **Relaxed**: ambiguous names are no longer skipped everywhere. `Library`,
  `System`, `Private`, `Applications` deeper in a tree or at an external drive's
  root are now scanned; so are `Windows`, `Recovery`, `Program Files` below a
  drive root. Tests pin both directions.
- **Folder badges** now count what the scan would find (they used to include
  everything the scan skipped). Cached counts are cleared on any rule change;
  counts cached before this build stay until ↻.
- **macOS external drives now get their own `_FoxCull` library.** `rootForDir`
  took the first drive whose path prefixed the folder, and `/` prefixes
  everything, so every `/Volumes/*` folder activated the Mac's app-data `root`
  library. Now the longest (path-boundary-aware) match wins. Consequence: on the
  next open of an external drive, FoxCull creates `<drive>/_FoxCull` and starts
  a fresh catalog there. Anything rated on a Mac external drive before this fix
  lives in `libraries/root/catalog.sqlite` under keys like
  `Volumes/SD_Card/...` and will not follow. On the owner's Mac that catalog has
  0 decisions, so nothing is lost there.
- The tree shows the Mac startup disk once, named "Macintosh HD" (was `/` plus a
  duplicate "Macintosh HD" entry).

## Risks / compat

- The exclude rules live in the frontend store and are pushed on mount. The
  backend defaults to all groups ON, so a walk that somehow ran before the push
  errs toward skipping.
- Windows root rules key off `Path::parent().is_none()`; verified by a
  `#[cfg(windows)]` test that only runs on Windows (CI runs Linux, so it is not
  exercised there). The Windows root names are unchanged in spirit from the
  previous list; only their scope narrowed.
- Custom path rules compare case-insensitively on every OS (both default
  filesystems are case-insensitive; Linux is not, a negligible gap here).
- The group name lists appear twice: in the Rust matchers and in
  `ExcludePanel.svelte` for display. A comment in the panel says to keep them in step.

## Verification actually run (macOS, Apple Silicon)

- `cargo check`: clean, no warnings. `cargo test --lib`: 32/32, including 10
  new `scan_filter_tests` (boot-root/home anchoring, relaxed names, groups
  independently off, internals always skipped, custom names/paths, wildcards).
- `npm run check`: 0 errors, 0 warnings.
- Temporary ignored test walking `/` on the real Mac (numbers above), then removed.
- Debug build run with an isolated `HOME`: first launch opens to the welcome
  screen and scans nothing; `lastDir` = `/Volumes/Macintosh HD` or `/` → no
  scan at launch; `lastDir` = an ordinary folder → reopened as before.
- Welcome screen (dark and Daylight themes, with and without the resume line)
  and Excluded folders panel exercised in the browser pane via the vite dev
  server at the real window size (1280×820): layout, suggestions (IPC stubbed), adding a
  name pattern and a path, the invalid-name error, untick a group, Reset.
- `rootForDir` checked with Mac and Windows drive lists in node.
- **Not verified:** Windows. No Windows machine this session; CI covers the
  build and Linux tests only.
