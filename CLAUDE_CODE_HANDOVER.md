# Agent Handover: FoxCull

This file is intended to give Claude Code or any future coding agent enough
context to continue FoxCull without accidentally touching the original
Claude-built `fox-cull` project.

> **2026-07-10 rename:** the working folder and GitHub repo were renamed from
> `FoxCullCodex` to **`FoxCull`** (the "Codex" origin distinction is no longer
> useful now that this is the sole active product). The old `fox-cull` reference
> folder/repo was renamed to `fox-cull_archive` / `FoxCull-Archive`. GitHub
> auto-redirects the old URLs. Every `FoxCullCodex` mention below this point is a
> **historical record** of names in effect at the time — left as-is for
> accuracy; don't "fix" them.

## 2026-07-21 (3): v1.1.0 STABLE — sprite unification + fullscreen docks; libmpv unparked

Owner tested nightly.7, confirmed the arm-then-hover fix ("scrub seems to be
working fine"), and called the release: **promote to stable as `v1.1.0`**, not
nightly.8. This is the deliberate checkpoint for the WebView2-player era — the
libmpv transplant starts immediately after and becomes the 1.2 line.

Second batch of fixes folded in before the tag (detail:
`docs/changes/2026-07-21-sprite-unification-fullscreen-docks.md`):

- **One sprite, not two.** Grid tiles and the Focus timeline were building
  *different* sheets from the same clip. That doubled extraction per video and
  produced the "build restarts at 10% when I open the clip" report. Both now use
  the dense `f` sprite, through the **shared loader queue** so a second request
  joins the in-flight promise. Two separate causes had stacked: (1) the Thumb
  teardown cancelled the armed tile's build on unmount, (2) Loupe's direct
  `invoke` took a fresh backend cancel-token, and `new_sprite_token` **cancels
  any in-flight build for the same key** — worth remembering, it is a trap.
- **Fullscreen honours the dock.** `.app.fs` was hiding `.lstrip`/`.rstrip` and
  both splitters outright, so a side-docked strip vanished on `F` and the bottom
  one lost its resize grip. Only the tree splitter (`.treeSplit`) hides now.
- Portrait timeline preview capped by height; grid filename tooltip removed;
  `reveal` uses `explorer.exe /select` so the file is actually selected.

**Measured, do not re-litigate from intuition** (`precache-policy.md` §5.1):
sprite building is **CPU/process bound, not disk bound** — the same 6 frames
cost 5.04 s cache-warm vs 5.92 s cold, and 0.80 s of each frame's 0.99 s is
ffmpeg startup + container-index parse. hwaccel is worth ~5% on a single
keyframe. It parallelises (12 frames: 6.16 s @2, 4.40 s @4, 3.61 s @6), so
`SPRITE_PARALLEL` became `(cores/3).clamp(2,4)`. The old comment claiming the 2
protected a USB SSD's read queue was wrong and has been replaced with the data.

**Owner's design call for libmpv, recorded because it scopes the work:** with a
native player, Focus view should need **no sprite pre-caching at all** — he
drags the bar and expects real frames. Sprites stay ONLY for grid-view skimming,
where a decoder per tile is impossible. So the transplant must *remove* the
Focus filmstrip path, not sit beside it. He also accepted the honest caveat that
the risk is Windows compositing, not mpv's seek performance.

## 2026-07-21 (2): Scrub module audit, delete diagnosis, undo-restore (nightly.7)

Base-machine session, Opus. Owner installed nightly.6 and came back with one
compound report — skimming "still broken", a delete failing on his `E:\Bali
trip` folder of 4K60 HEVC clips with an unreadable error, undo not reaching the
Trash, no left filmstrip, Prepare stuck at folder scope. Handled as one audit of
the video-preview subsystem, not four point fixes. Module-level detail:
`docs/changes/2026-07-21-scrub-audit-delete-undo.md`.

**The three scrub bugs, and why they hid each other:**

1. *Arming never triggered a build.* nightly.6 made hover-scrub require the tile
   to be armed (clicked/selected) — but you arm by clicking, and the pointer is
   already inside the tile then, so the `pointerenter` handler that scheduled
   builds had fired **before** arming and never fired again. Result: builds for
   every tile swept past, none for the tile actually selected. Fixed by driving
   the build from an `$effect` on `(armed && hovering)` as *state*, so the two
   orderings are the same thing.
2. *Pointer→time mapped to the letterboxed picture*, not the cell — a 9:16 clip
   paints ~30% of a landscape cell, so the timeline was crammed into that sliver.
   Now mapped across the full cell.
3. *Leaving an armed tile cancelled its build*, so drifting off and back
   restarted a 10 s extraction from zero. Only unarmed tiles cancel now.

Also: with Live Scrub ON the Focus filmstrip builds on clip **open** (was: first
pointer contact with the seek bar, silently) with a progress chip. This
deliberately reverses a nightly.5 decision and is safe **only** because it stays
gated on the Live Scrub setting — ungate it and the nightly.3 "a minute of
ffmpeg per clip on an HDD" bug returns.

**Delete on `E:` was never an app lock.** `move_into_recycle` reported every
rename failure as "file is in use"; the actual errno was `PermissionDenied`.
Explorer refuses those same files with "You'll need to provide administrator
permission" — an ACL leftover from the Windows reinstall, since the files carry
the old install's SID. The backend now separates sharing violations from
permission denials, clears a read-only attribute and retries once, names the
file, and the frontend shows the real reasons in a modal instead of a truncated
chip. **The OS-level fix (take ownership of the folder once) is the owner's to
run; the app can only report it accurately.**

**Undo now reaches into the Trash.** The undo stack became a discriminated union
(`marks` | `delete`); a dispose into the in-app Trash pushes a `delete` entry
carrying the new `TrashOutcome.trashed` keys. Undoing one confirms first ("N
files will be moved back"), and delete entries are deliberately **not** redoable
— Ctrl+Y re-trashing files mid-history is exactly the misfire the owner flagged.

**New: `docs/design/precache-policy.md`** — owner-requested authoritative record
of every cached artifact, its key, its build triggers, the concurrency doctrine,
and an honest gaps list, in both prose and a machine-readable YAML block. It is
now in the doc map and must be updated in the same commit as any caching change.
Read it before touching thumbs/posters/sprites; it is the thing this session
wished existed.

Smaller: filmstrip can dock **left**; Prepare is a split button (folder /
selection / videos / photos); opt-in `scrubPrefetch` builds strips for the ±3
clips around the one open in Focus.

Gates: `npm run check` 0/0, `cargo check` clean. libmpv work stays parked on
`feat/libmpv-video` (M2 unresolved — see `docs/design/libmpv-transplant.md`).

## 2026-07-20 (3): Video-focus UX pass — minimal transport, sharp poster, fullscreen filmstrip (nightly.5, being pushed)

Base-machine session, Opus. Owner lifted the push hold ("push all these changes,
send out a build so I can test"). This folds the earlier batched local commits
(HEIC/delete/cast + Live Scrub fix) plus a video-focus UX pass into
**v1.1.0-nightly.5** and pushes + tags. Full module-level detail:
`docs/changes/2026-07-20-video-transport-poster.md`.

- **Minimal video transport**: the bar overlays the stage and collapses to a 3px
  progress hairline; hovering the bottom band reveals it, scrubbing/Clip-tools
  keep it open. Setting `minimalVideoBar` (default ON) + Settings toggle; Off =
  pinned bar.
- **Sharp Focus poster**: new 1280px `w…` poster cache (`video_poster_hires`);
  grid keeps the 480px `v…` poster, so grid memory is unchanged.
- **Play-mode (F) filmstrip**: hidden and hover-revealed at the bottom edge
  (`.fsStripSensor`), instead of pinned.
- **Architecture Q answered for the owner**: the Chromium/WebView2 approach is
  fine for photos; the ONE real ceiling is `<video>` scrub smoothness. A future
  libmpv native-surface transplant would fix it *and keep every edit feature*
  (in/out, segments, EditStudio, export are backend/state, not tied to the
  `<video>` element). Recommendation: targeted transplant later, not a rewrite.
- **In/out persistence**: confirmed already working in current code
  (`get_trim`/`set_trim` restore on reopen) — the owner's report was an older
  installed build.

## 2026-07-20: HEIC fix, delete-freeze fix, cast follow-mode (local, unpushed — owner is batching)

Base-machine session, post-nightly.4 live testing by the owner. Committed
LOCALLY only — the owner wants further fixes batched before anything goes
online. Full module-level detail: `docs/changes/2026-07-20-heic-delete-cast.md`
(the first entry of the now-mandatory per-push change ledger — see CLAUDE.md).
New: `docs/DECISIONS.md` (ADR log; HEIC decode strategy + cast quality).

- **HEIC decode fixed**: tiled phone HEICs failed in `decode_still` (`-vf` vs
  the auto-inserted tile-stitch complex filtergraph) → `-filter_complex`, and
  ffmpeg stderr is no longer swallowed. Verified against the owner's real
  Samsung HEICs with the shipped ffmpeg binary.
- **Delete can't freeze the app**: no more copy-fallback for in-use files,
  `dispose_rejected` is async (off the main thread), background sprite/warm
  work is cancelled before disposing, failures surface in the activity chip.
- **Cast follows browsing**: one session, the TV mirrors the active
  photo/video (debounced); HEIC/RAW cast their 1920px loupe JPEG (the Default
  Media Receiver can't decode them raw).
- **Live Scrub regression FIXED (owner-approved plan, second commit)**: Focus
  open is cached-only; the dense build fires only on Live Scrub ON + first
  timeline intent; `-hwaccel auto` on frame extraction; strip cap 100→48.
  RCA that drove it: `.rca-live-scrub.md` (repo root, untracked); ledger:
  `docs/changes/2026-07-20-live-scrub-fix.md`. Owner still owes the SSD-vs-HDD
  A/B and live verification on the next installed build.

## 2026-07-19 (later session): UX/UI audit + Look-panel overhaul (v1.1.0-nightly.4, on main)

Same remote Fable session, second pass at the user's request: an
independent-auditor sweep of every UI surface, then the overhaul. Full
findings ledger (fixed vs deferred, with reasoning):
[`docs/UX-AUDIT-2026-07-19.md`](docs/UX-AUDIT-2026-07-19.md). Highlights:

- **Light dismiss everywhere**: all toolbar popovers + the Details Columns
  menu close on outside click and Escape (they previously only closed by
  re-clicking their toggle). Escape now closes overlays/popovers before any
  other Escape behavior.
- **`?` shortcut guide**: every keyboard shortcut in one grouped overlay;
  tooltips on view chips / Pick / Reject / stars teach their keys; welcome
  and filtered-empty states are actionable (Clear filters button).
- **Honest ETAs**: `activity.svelte.ts` computes sliding-window rate ETAs
  (`etaSeconds`/`fmtEta`; shown only after ~2.5 s of stable data, never on
  indeterminate jobs) and the chip shows `n / m · ~4m 30s`. **Prepare now
  runs photos first, then videos, with per-phase measured rates + priors**
  (photo ~0.3 s, video ~4 s) — the fix for blended-rate estimates lying on
  mixed folders.
- Details rows gained the media context menu; ★ glyph consistency; global
  `:focus-visible` ring; safer context-menu glyphs.
- **Look panel overhauled** (Opus subagent, verified + committed by the
  session): root cause of "presets do nothing" was timid coefficients —
  warmth strengthened 0.4→0.5 (±25% R/B at full range) and split-tone deltas
  widened ~40% in BOTH the preview math and the export filters (parity is
  the invariant; cross-referencing comments added on both sides).
  12 presets in 5 collapsible groups: Vlog & Portrait (Warm Portrait, Soft
  Skin, Golden Hour) · Drone & Landscape (Vivid Landscape, Orange & Teal) ·
  B&W (Mono, Noir) · Cinematic (Teal & Orange, The Batman, Moody Film) ·
  Clean & Correction (Osmo Clean, De-Log Boost — for future D-Log footage).
  Group open-state persists per app session; active preset's group carries a
  dot; "Reset look" in the header. Brightness/contrast/saturation verified
  algebraically fine and untouched. Known limitation, by design: **sharpen
  has no live preview** (export-only unsharp; SVG feConvolveMatrix is the
  candidate if it ever bites — noted in BACKLOG P3).

Deferred with reasoning (see the audit doc): responsive toolbar collapse
below ~900 px, Focus-view photo zoom (both in BACKLOG as P2), Trash per-item
context menu, unified SVG icon set.

## 2026-07-19: Video preview rework + controller culling (v1.1.0-nightly.3, on main)

A remote Fable session did the user-requested deep pass on the video preview
system (the "Live Scrub sometimes hangs forever / Focus seek stutters"
complaints), plus the first cut of PS5-controller culling. Everything landed
directly on `main` per the user's instruction; nightly tag `v1.1.0-nightly.3`
(nightly.2 turned out to already exist on the remote, pointing at old commit
`3a0e471` — apparently tagged but never documented/released; left untouched).

**The core fix — sprite generation was full-decode.** `ensure_sprite` ran one
ffmpeg pass with an `fps=` filter, which decodes EVERY frame of the clip to
keep ~40: a 5-minute 4K60 HEVC Osmo clip meant ~18k frames of software HEVC
decode per hover strip (minutes on the XPS 13), uncancellable once started, so
sweeping the cursor across a row of clips stacked several of those up and
"hung" the disk. Now (`video.rs`):

- Each sampled frame is an independent **keyframe seek** (`-ss` before `-i` +
  `-skip_frame nokey`, one frame decoded per sample; 2 extractions in parallel,
  builds serialized process-wide). A strip costs seconds regardless of clip
  length. Unseekable containers fall back to a single-pass scan that is itself
  now `-hwaccel auto` + keyframes-only.
- Builds are **cancellable between frames** and report per-frame progress via
  the activity events (grid tiles show a tiny "scrub N%" tag). `commands.rs`
  keeps a token registry: hover-leave cancels that clip's build
  (`cancel_sprite`), folder switch cancels everything (`cancel_all_sprites`,
  called from `resetThumbs`), and a re-request supersedes the previous build.
- **Prepare** (heavy warm) now also pre-builds hover scrub strips for videos,
  so a prepared folder skims with zero on-hover work. Strip sizes are trivial
  (hover ~40–120 KB, filmstrip ~0.3–1.2 MB per clip; see STORAGE.md).
- Poster extraction uses `-skip_frame nokey` (1 decode instead of ~60), and
  the H.264 **proxy build decodes via `-hwaccel auto`** (NVDEC on the 1070).
- Tile sizes bumped: filmstrip 160→240 px wide (it now doubles as a
  full-canvas drag overlay), hover strip 128→160 px.

**Focus playback** (`Loupe.svelte`): cached poster paints the stage instantly
(`<video poster>`); the seek-bar filmstrip is no longer gated on the Live
Scrub toggle (one clip at a time is cheap now) and shows the cached hover
strip as a coarse layer while the dense one builds; switching clips cancels
the previous build. While DRAG-scrubbing, the sprite frame under the cursor
paints the whole stage (decode-free, Final Cut-style) with the real decoder
chasing via throttled `fastSeek`, and release lands a frame-accurate seek.
`seekBy` (keys `,`/`.`, controller triggers) is optimistic from the last
commanded position with a trailing accurate seek, so held-repeat shuttles
instead of stalling per keypress.

**Controller culling** (new `gamepad.svelte.ts`, `ControllerPanel.svelte`):
Gamepad-API polling with edge detection, hold-to-repeat nav and analog
trigger shuttle (L2/R2 scale by pressure). Defaults are DualSense-shaped:
d-pad navigates, ✕ Pick, ○ Reject, △ clear marks, □ play/pause, L1/R1
grid/Focus, Options fullscreen, Create shows a button-guide overlay. Every
action is remappable via press-to-bind in Settings → Controller (pairing
guide included); bindings persist in `padBindings`. The mouse's Back/Forward
extra buttons route through the same action map (`mouseBack`/`mouseForward`
settings; defaults preserve the old Focus⇄grid toggle). Fullscreen (F) now
KEEPS the bottom filmstrip — that's the couch/TV "play mode" the user
described (Filmstrip Off still gives a bare photo).

**Release plumbing**: `release.yml` prepends `RELEASE_NOTES.md` (repo root)
to the generated release body when present, so the notes on the release page
say what actually changed — keep that file fresh per tag. It also gained a
`workflow_dispatch` `tag` input that CREATES the tag at the dispatched commit
and releases it: remote agent sessions (like this one) can push branches but
their git proxy 403s tag pushes, so dispatch-with-input is the agent release
path (this nightly was cut that way).

Validation: `npm run check` 0/0, `npm run build`, `cargo check`,
`cargo test --lib` (6 passing) — all green in the session container (Linux;
webkit deps + a stubbed ffmpeg sidecar like CI).

**Where to continue** — P0s from the audit remain (portable cache keys,
cache GC/size visibility); candidate follow-ups from this session: NVENC
*encode* for proxies/exports (PERF-5 second half), a controller-driven rating
chord (hold a shoulder + d-pad?), and letting the Focus drag overlay upgrade
to decoded frames when the machine keeps up.

## 2026-07-18: Holistic audit (branch `claude/fox-cull-audit-kc8iwu`, PR #1)

A separate Claude Code session (running remotely, not this machine) did a
full-codebase audit — architecture, performance (perf focus: Alienware 15 R4 /
GTX 1070 and XPS 13), memory, code quality, security, and macOS compatibility.
**If you're picking this repo up fresh, read this section before touching
performance, caching, or the warm/Prepare pipeline — some of it just changed.**

Full writeup: [`docs/AUDIT-2026-07.md`](docs/AUDIT-2026-07.md). Prioritized
worklist: [`BACKLOG.md`](BACKLOG.md) (P0 = do next, P1 = high value soon,
P2 = scheduled incl. all Mac items, P3 = nice-to-have).

**Landed** (on the audit branch — merge or cherry-pick before continuing local
work, so the two lines of history don't diverge on the same files):

- CI now stamps the release tag's version into `Cargo.toml` too, not just
  `tauri.conf.json` — tagged builds previously showed the wrong version in the
  window title (it reads `CARGO_PKG_VERSION` at compile time).
- `warm_thumbnails` gained an opt-in `heavy` flag. **Prepare was silently a
  no-op on RAW/video folders** — the backend filtered its work down to plain
  JPEGs regardless of what Prepare asked for. Automatic folder-open warming is
  unchanged (still images-only, same bounded pool).
- Edit-mode source-pane probing throttled to 4 concurrent ffmpeg processes
  (was unbounded — up to 80 sources meant up to 80 forked processes on Edit
  open, a real stall on 4-core machines).
- `buildRelatedIndex` (the stacks grouping) no longer re-spreads arrays per
  entry while filling buckets — it rebuilds on every mark keystroke, so this
  was quadratic on large related-groups.
- `move_media_files`'s cross-volume fallback no longer leaves a duplicate file
  at the destination if removing the source fails partway through.
- All three folder walkers (`collect`, `collect_edit_sources`, `count_media`)
  skip dotfiles now — macOS's `._*` AppleDouble sidecars on shared exFAT/NTFS
  drives were showing up as broken media items.
- New `.github/workflows/check.yml`: svelte-check + frontend build +
  `cargo check`/`cargo test` on every push/PR. Nothing compiled the code
  before a release tag until now, and the Rust unit tests had never run
  anywhere (they can't run on this project's local Windows/GNU toolchain).
- `@types/node` added as a dev dependency — the long-standing svelte-check
  Node-types warning is gone; checks are 0 errors / 0 warnings for the first
  time.

**Not done — top of the backlog** (see `BACKLOG.md` for the rest):

- **P0**: preview-cache keys hash the *absolute* path, so the cache does NOT
  actually travel between machines sharing a drive (Alienware `E:\` vs XPS
  maybe `D:\` vs a future Mac `/Volumes/...`) despite that being the documented
  design intent — needs a switch to library-relative-path keys + a stable
  hash, plus GC (the cache currently never shrinks).
- **P0**: filmstrip/scrub-strip generation fully software-decodes clips to
  sample ~40-100 frames — add `-hwaccel auto` + keyframe-only sampling; this
  is the single biggest perf win identified, especially on the XPS 13.
- **P1**: try `hevc_nvenc` for Keep-HDR exports on the Alienware's GTX 1070
  (currently CPU-only `libx265 medium`, the slowest path in the app).
- **P2 (Mac)**: `window.confirm()` gating the JPEG export isn't reliable in
  WKWebView — can silently no-op on macOS; needs an in-app dialog instead.
- Everything else (search, catalog backup, cast hardening, monolith
  splitting, etc.) is scoped in the backlog with reasoning.

If you (the agent reading this) are about to touch caching, Prepare, the edit
probe path, or the folder walkers, check whether the audit branch has already
merged first — redoing this work independently will conflict.

## What landed in v1.1.0-nightly.1 (2026-07-12, one big live-audit batch)

Driven by a long user audit session; stable base remains v1.0.1 (v1.0.0 re-cut
after a transient publish failure — code-identical). Newest additions:

- **Cast to TV (Chromecast)**: `src-tauri/src/cast.rs` — mDNS discovery
  (`mdns-sd`), hand-rolled CASTV2 protobuf over `native-tls` (schannel on
  Windows; deliberately no rust_cast — it drags in aws-lc-rs/C), local
  Range-enabled `tiny_http` media server with a per-cast token allowlist.
  Videos stream untranscoded (4K60 HEVC decodes on the TV); photos are capped
  ~720–1080p by the Default Media Receiver (custom receiver = later work).
  UI: cast button + device popover in the top-right toolbar (`+page.svelte`).
- **RAW → JPEG bulk export**: `src-tauri/src/raw.rs` — extracts the camera's
  full-res embedded JPEG from TIFF-based RAW (NEF/CR2/ARW/DNG) via a full
  IFD/SubIFD walk (largest FFD8..FFD9 candidate wins; brute-scan fallback);
  verbatim write when upright, rotate+re-encode(q95)+ICC otherwise; output is
  `<stem>.JPG` beside the source so it auto-stacks with its RAW. Context-menu
  "Export JPEG from RAW" + `raw-export-progress` event → ActivityBar.
- **Scalable stacks**: besides the suffix whitelist, any stem that extends
  another file's stem across a `[_-. ]` boundary joins that file's stack
  (`buildRelatedIndex` re-rooting pass). Per-item RAW/JPG corner tags.
- **Undo/redo for culling marks**: snapshot-based stack (Ctrl+Z / Ctrl+Y /
  Ctrl+Shift+Z), toast + logfile trail; file operations deliberately excluded.
- **Filters overhaul**: rating operator ≥/≤/=, Lightroom-style multi-select
  labels + a proper "None" control, "N of M" shown-count, popover no longer
  clips. X/P now toggle on multi-selections like the toolbar buttons.
- **Look presets rebuilt** (EditStudio): The Batman / Noir B&W / Orange&Teal /
  Vivid drone / Osmo clean / Warm travel, each scaled by an Intensity slider
  (0–150%). New `splitTone` adjustment: SVG `feComponentTransfer` preview ↔
  ffmpeg `curves` export from the same control points; brightness/warmth
  recalibrated (CSS-vs-eq mismatch folded algebraically into `eq`).
- **Export dialog**: aligned 3-column Property/Source/Output grid; stacked
  time-cost bar with legend; Quality labeled by CRF (16/18/20/23) with a
  "High is right for Instagram" note; filename hint about stack-friendly
  naming. Toolbar: Frame button folded into Export ▾; "Render required" pill
  → dot on Export; capped gap (no more wrap).
- **Chrome polish**: taskbar FOX icon enlarged (content zoomed 1.24× inside
  the tile from `icons/icon.png`, corners re-masked; the icon set is
  regenerated from the new canonical source `assets/icon-fox-1024.png` — the
  original fox master PNG was never committed, which is why regenerating from
  `assets/icon.svg` (geometric motif, favicon/docs only) replaces the fox by
  mistake. `npx tauri icon assets/icon-fox-1024.png` is the correct command),
  standard panel glyph for sidebar collapse, gear icon + grouped settings
  popover (Stacks/Live Scrub live ONLY there now), aligned popover labels,
  icons on Reject/Clear/Delete, drive list re-enumerates on refresh ↻.

**Local toolchain note (this machine)**: `cargo check` works via
`D:\dev-tools` (gnu toolchain + winlibs mingw64 for `dlltool`/`gcc`,
`CARGO_TARGET_DIR=D:\dev-tools\rust\target-shared` because windres chokes on
the space in "Claude Code Projects"). See `D:\dev-tools\README.md`.
`cargo test` does NOT run locally: linking the cdylib overflows the 65k DLL
export-ordinal limit under GNU ld, and even rlib-only test exes die with
STATUS_ENTRYPOINT_NOT_FOUND. Checks are the local gate; tests are CI/MSVC-only.

## Current State: Stable FoxCull v1.0.0

- Current main product name: **FoxCull**.
- Working folder: `D:\Claude Code Projects\FoxCull`.
- GitHub repository: `https://github.com/kumaradarsh1993/FoxCull`.
- Stable tag: **`v1.0.0`** (commit `e00749e`, tagged + pushed 2026-07-10;
  CI publishing — see release page for final status).
- Dev port: **1460** (`.claude/launch.json` config name `foxcull`).
- The earlier `fox-cull` folder is now `fox-cull_archive` — legacy/reference
  only, frozen.
- User-facing "Codex" branding has been removed from the product. Current
  builds use `_FoxCull` and `foxcull-data` only; old pre-stable library folders
  are not adopted by the runtime.
- App icon: orange low-poly fox on slate (`src-tauri/icons/`, regenerated via
  `npx tauri icon` from a squared/transparent source PNG).
- Window title shows the exact running version (`FoxCull v1.0.0`, or the exact
  nightly string) — set in `src-tauri/src/lib.rs` from `CARGO_PKG_VERSION` at
  startup, so the user can always tell which build is installed.

### Changelog: v0.6.3 → v1.0.0 (2026-07-09 to 2026-07-10)

A single long session took the app from v0.6.3 through five nightlies to
v1.0.0 stable. Highlights, newest first:

- **v1.0.0** — Instagram-pipeline audit (see `docs/INSTAGRAM_EXPORT_PLAYBOOK.md`
  2026-07-10 addendum, sourced from Meta's official Reels ingest spec + Meta
  engineering blog posts, not just SEO blogs): Keep-HDR now tags output with
  the SOURCE's transfer function (HLG stays HLG for the Osmo Pocket 3, PQ/HDR10+
  stays PQ for the S23 Ultra — blanket-HLG previously decoded PQ footage with
  wrong gamma); Instagram exports capped at 25 Mbps VBR (Meta's ingest ceiling);
  export dialog reminds about the "Upload at highest quality" IG app toggle;
  new icon; versioned window title; repo/folder rename to `FoxCull`.
- **v0.7.0-nightly.5** — unified export dialog for every entry point (source
  facts left / editable target settings right, pre-filled by preset but always
  overridable — resolution, frame rate, quality); Instagram exports **retain
  source fps capped at 60** (was previously force-set to 30 — Meta's ingest
  spec accepts 23–60 fps and forcing 30 made 60fps drone/gimbal footage look
  worse); live export progress bar + two-step cancel (kills ffmpeg, deletes
  the partial file); library auto-refreshes after export/snapshot; fixed a
  Look-panel z-index inversion that let it cover the export menu/buttons;
  timeline zoom-out now fits the whole program instead of stopping at a fixed
  floor.
- **v0.7.0-nightly.4** — Edit module rebuilt as a real mini-NLE: single
  timeline playhead/program engine that plays the whole sequence including
  gaps (not just the clicked clip), razor cut (C), Delete key, Ctrl+click
  multi-select, drag clips between tracks with a no-overwrite rule, live
  trim-drag preview. Aspect is decided ONLY in the edit screen (removed from
  export dialogs). Crop-aware export math (a 1080p→9:16 crop correctly reads
  "upscaled — soft" instead of "downscaled"). Mixed-resolution/mixed-codec
  composites (e.g. S23 HEVC + Mavic H.264) conform to a shared canvas instead
  of producing a broken stream-copy. Contiguous stack-line rendering across
  grid tile gaps.
- **v0.7.0-nightly.1–3** — stack-line grid/filmstrip toggle, Lightroom-style
  folder tree, Focus view compaction, Look panel Presets/Adjust split with
  per-slider double-click reset, first Instagram export dialog (current →
  optimised spec comparison + time estimate), smart backend export (HDR
  tone-map default with Keep-HDR opt-in, soft-crop auto-sharpen), thumbnail
  derivative badges (IG/trim/crop/composite), Loupe→Edit in/out segment
  carryover.

See `docs/ROADMAP.md` for deferred ideas (auto-expose clip tools when marks
exist, merging the Edit top bar into the global bar, etc.) and
`docs/INSTAGRAM_EXPORT_PLAYBOOK.md` for the full Instagram export research,
including the 2026-07-10 audit table of corrected assumptions.

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
