# CLAUDE.md — FoxCull

Orientation file for Claude Code / any coding agent. Read this first, then
`CLAUDE_CODE_HANDOVER.md` (the running project narrative — its top dated
section is always the latest state of the world).

## What this is

FoxCull — a fast photo/video **culling + lightweight editing** desktop app.
Tauri 2 + SvelteKit 2 + Svelte 5 (runes) + SQLite (bundled rusqlite). Built for
one user's real workflow: cull huge Nikon D5200 RAW/JPEG and drone/phone video
shoots off external SSDs, then export social-ready clips. Private repo:
`github.com/kumaradarsh1993/FoxCull`. The sibling `fox-cull_archive/` repo is
frozen history — never edit it.

- **Dev port:** 1460 (workspace convention; `tax-fox` also claims 1460 — never run both).
- **App identifier:** `com.foxcull.app`; product name `FoxCull`.
- **Per-drive data:** a `_FoxCull/` folder at each drive root (catalog SQLite,
  thumbnail/preview cache, in-app Trash). See `STORAGE.md`.

## Doc map (all in-repo — keep it that way)

| Doc | What it is |
|---|---|
| `CLAUDE_CODE_HANDOVER.md` | **Authoritative running narrative.** Newest dated section first. |
| `BACKLOG.md` | Prioritized P0–P3 worklist (from the 2026-07 audit). Do P0s next. |
| `docs/AUDIT-2026-07.md` | Full independent audit writeup (perf/memory/security/Mac). |
| `docs/HANDOFF-FROM-FABLE-AUDIT.md` | Short brief that accompanied audit PR #1. |
| `docs/ROADMAP.md` | Product direction. |
| `docs/INSTAGRAM_EXPORT_PLAYBOOK.md` | The IG export pipeline's reasoning. |
| `STORAGE.md` | On-disk layout of `_FoxCull/` per-drive data. |

**Convention: everything about FoxCull lives in this repo** — design notes,
internal discussion, handover docs, roadmap, playbooks. Only secrets stay out
(`.env*` is gitignored; there are currently no secrets in this project).

## Commands

Frontend (Node 20):

```
npm ci                 # install
npm run check          # svelte-check — 0 errors / 0 warnings is the bar
npm run build          # vite build (static adapter)
npm run tauri dev      # full app, port 1460 — prefer the workspace launch config
```

Backend (from `src-tauri/`):

```
cargo check            # the local gate before any push
cargo test --lib       # unit tests (raw.rs etc.) — runs in CI on Linux
```

**CI is the real build.** `.github/workflows/check.yml` runs svelte-check +
frontend build + `cargo check`/`cargo test` on every push/PR.
`release.yml` builds installers for Windows/macOS/Linux on every `v*` tag and
stamps the tag's version into **both** `tauri.conf.json` and `Cargo.toml`
(the window title reads `CARGO_PKG_VERSION` at compile time).

### Main dev machine quirks (Windows 11 — see the workspace `CLAUDE.md` one level up)

- **No heavy local builds** — never `npm run tauri build` (rustc OOMs on LTO);
  installers come from CI on tag push.
- Rust toolchain is **windows-gnu** + winlibs MinGW (`D:\dev-tools\mingw64\bin`
  on PATH provides `dlltool`/`gcc`); `CARGO_TARGET_DIR` must stay the
  space-free `D:\dev-tools\rust\target-shared` (windres chokes on the space in
  "Claude Code Projects").
- `cargo test` does **not** link on the local GNU toolchain (65k DLL
  export-ordinal limit) — tests are CI-only from this machine.
- The ffmpeg sidecar `src-tauri/binaries/ffmpeg-<target-triple>.exe` is
  gitignored; a stub file satisfies `cargo check` (CI stubs it too).

## Architecture in one screen

**Frontend** (`src/`):

- `routes/+page.svelte` — the library: folder tree, virtual grid/filmstrip,
  culling marks (rating/label/pick/reject), stacks ("related" grouping:
  suffix whitelist + stem-prefix re-rooting), filters (rating ≥/≤/=,
  multi-select labels), undo/redo (snapshot stack, marks only), selection +
  keyboard model, cast button, settings popover. The big one (~2.7k lines).
- `lib/components/EditStudio.svelte` — edit/export studio: Look presets
  (CSS/SVG filter preview ↔ ffmpeg filter export, algebraically matched),
  trim, export dialog with CRF-labeled quality + time-cost bar.
- `lib/components/` — Loupe (zoomable viewer), VirtualGrid/VirtualStrip
  (windowed rendering), SectionedGrid, DetailsView, TrashPanel, ActivityBar,
  ContextMenu, Thumb, TreeNode.
- `lib/api.ts` — every Tauri `invoke` goes through here (typed wrappers).
  `lib/cast.ts` — cast commands. `lib/types.ts` — shared types.
  `lib/settings.svelte.ts` / `persist.ts` — settings via plugin-store.
  `lib/thumbnail-loader.ts` — viewport-bounded thumb fetch queue.

**Backend** (`src-tauri/src/`), all commands registered in `lib.rs`:

- `commands.rs` — the bulk: folder walking (skips dotfiles/AppleDouble),
  thumbnails + bounded background warming (`warm_thumbnails`; `heavy` flag =
  explicit Prepare pre-builds RAW previews/video posters), loupe sources,
  file moves (safe cross-volume fallback), delete-to-trash, export pipeline
  (ffmpeg filtergraphs mirroring the preview math).
- `media.rs` — classification (Image/Raw/Video), EXIF orientation, ICC.
- `thumbs.rs` — decode/resize/cache (DCT-scaled JPEG decode is the perf core).
- `video.rs` — poster/scrub-strip/proxy generation via the ffmpeg sidecar.
- `raw.rs` — embedded-JPEG extraction from TIFF-based RAW (NEF/CR2/ARW/DNG)
  + bulk RAW→JPEG export (unit-tested).
- `cast.rs` — Chromecast: mDNS discovery, hand-rolled CASTV2 protobuf over
  `native-tls`, token-gated `tiny_http` Range server. Deliberately
  C-toolchain-free deps — don't swap in rustls/rust_cast.
- `catalog.rs` — SQLite catalog (marks) per drive. `config.rs`, `log.rs`.

**Performance doctrine** (hard-won — read the comments before "optimizing"):
background work must never starve the foreground. Warm pool is tiny
(1–2 threads), USB-SSD read queues stay shallow, viewport drives on-demand
loads, ffmpeg fan-out is throttled (probe queue = 4). The audit
(`docs/AUDIT-2026-07.md`) tunes for the two real machines: Alienware 15 R4
(GTX 1070) and XPS 13.

## Working conventions

- **Single-writer per repo** (workspace rule): one agent/machine on this repo
  at a time. `git fetch` + reconcile with `origin/main` before any work;
  commit code AND docs and push at session end.
- Work directly on `main` — pushing ships nothing (releases are tag-driven).
  Branches/PRs only for isolated independent lines (e.g. audit PR #1).
- **Release discipline:** nightly = tag `v*-nightly.N` → CI → prerelease
  draft; stable promotion ONLY on the user's explicit "ship it". Windows and
  macOS artifacts must come from the same tagged commit. Release notes are
  user-friendly prose, not commit logs.
- Commit author must be `kumar.adarsh.cse12@itbhu.ac.in` (the GitHub-linked
  email — global git config is set; don't override).
- Tauri security baseline (workspace rule): strict CSP (never `"csp": null`),
  no unscoped `fs:` capabilities, secrets keyring-first (n/a here so far).
- Icons: the canonical app-icon source is **`assets/icon-fox-1024.png`** (the
  fox). `npx tauri icon assets/icon-fox-1024.png` regenerates the set.
  `assets/icon.svg` is the geometric motif used for favicon/docs art ONLY —
  regenerating app icons from it replaces the fox by mistake (this happened
  once).
