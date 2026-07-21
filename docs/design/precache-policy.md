# Cache & pre-cache policy

**Status:** authoritative · **Last verified against code:** 2026-07-21
(nightly.7 work) · **Owner ask:** keep this in sync with the code, and keep it
readable by both a human and a model, so either can audit the two apart.

This is the single record of *what FoxCull caches, where, keyed by what, and
when it decides to build it*. It exists because the caching rules were spread
across five files and three reworks, and the only way to answer "why did this
clip take ten seconds to skim?" was to read all of them. If you change a
constant or a trigger below, change it **here in the same commit**.

---

## 1. The shape of the problem

FoxCull culls off external SSDs and internal HDD partitions holding 4K60 HEVC
(Osmo Pocket 3) and 24 MP RAW. Two facts drive every decision here:

1. **Decoding is expensive, re-decoding is free.** Every derived artifact is
   written to disk beside the catalog, so it is generated **once per file, ever**
   — not once per session, and not once per machine (the cache follows the
   catalog onto the drive, so a second computer reading that drive inherits it).
2. **Background work must never starve the foreground.** A pre-cache pass that
   makes the grid you are looking at stutter is a net loss, however much it
   speeds up the folder you haven't reached. Hence bounded pools, shallow read
   queues, LIFO viewport priority, and cancel-on-navigate everywhere.

Everything below is a consequence of those two.

---

## 2. Artifacts (what lands on disk)

All live in `<library>/cache/` — see `STORAGE.md` for where `<library>` is
(normally `<drive-root>/_FoxCull/`). Every key is
`hash(absolute path, mtime, size)`, so **editing or replacing a file
invalidates its whole derived set automatically** and nothing has to be purged
by hand.

| Artifact | File | Built by | Size | Purpose |
|---|---|---|---|---|
| Image/RAW thumbnail | `<hash>.jpg` (one per requested tier) | `thumbs::ensure` | 192 / 320 / 480 px long edge | grid + filmstrip tiles |
| Image/RAW Focus preview | `<hash>.jpg` at tier 1920 | `thumbs::ensure` | `LOUPE_MAX` = 1920 px | Focus/full-screen picture |
| Video poster (grid) | `v<hash>.jpg` | `video::ensure_poster` | 480 px box | grid + filmstrip video tiles |
| Video poster (Focus) | `w<hash>.jpg` | `video::ensure_poster_hires` | 1280 px box | Focus first frame before playback |
| Hover scrub strip | `s<hash>.jpg` + `s<hash>.json` | `video::ensure_scrubstrip` | 8 cols × 160 px tiles, 12–40 frames | grid-tile skimming |
| Focus filmstrip | `f<hash>.jpg` + `f<hash>.json` | `video::ensure_filmstrip` | 10 cols × 240 px tiles, 16–48 frames | timeline hover + drag-scrub |
| H.264 proxy | `p<hash>.mp4` | `video::ensure_proxy` | ≤1920 long edge, CRF 22 | clips the webview cannot decode |

The two sprite sheets carry a `.json` sidecar (`Filmstrip`: cols, rows, count,
tile_w, tile_h, duration) so the frontend can map cursor → frame without
re-probing the clip.

**Why two posters and two sprite sheets, not one of each.** A 480 px poster
blown up to a 4K stage is visibly pixelated; a 1280 px poster in a 176 px grid
cell is wasted decode and wasted RAM. Same logic for the sprites: the hover
strip only ever paints inside a grid cell, the Focus filmstrip fills the stage
during a drag. Separate keys mean opening one clip in Focus never bloats the
grid's working set.

---

## 3. When each artifact is built

This is the part that has churned most, and where every past bug lived. Read
the "trigger" column as the *complete* list — nothing else builds these.

| Artifact | Trigger | Notes |
|---|---|---|
| Image thumbnail | a grid/strip cell becomes visible | on-demand, viewport-bounded |
| Image thumbnail | folder open → `warm_thumbnails(heavy=false)` | images only, first `WARM_CAP` = 600 |
| Focus preview | entering Focus on that item | on-demand |
| Focus preview | Focus prefetch: 3 ahead / 2 behind, biased by travel direction | images + RAW only |
| Focus preview, RAW thumbnail, video poster, hover scrub strip | **Prepare** (`heavy=true`) | the only unprompted bulk pass |
| Video poster (grid) | a video cell becomes visible | on-demand |
| Video poster (Focus) | opening a video in Focus | on-demand |
| Hover scrub strip | grid tile is **armed** (clicked/selected) **and** hovered, Live Scrub ON, 140 ms settle | see §4 |
| Hover scrub strip | neighbouring clips ±3 while a video is open in Focus — **only if** Live Scrub ON **and** "Pre-build nearby clips" ON, after a 900 ms settle | opt-in, default off |
| Focus filmstrip | opening a video in Focus with Live Scrub ON | since nightly.7; previously waited for the pointer to reach the seek bar |
| Focus filmstrip | cached-only paint on open with Live Scrub OFF | never builds |
| H.264 proxy | the `<video>` element fails to decode the original | one at a time, process-wide lock |

### The Live Scrub contract

**Live Scrub OFF means no video preview work ever happens.** Opening a clip
paints whatever sprite sheets already exist and nothing more; the timeline is a
plain seek bar. (nightly.3 built a filmstrip on every Focus open regardless of
the toggle — a minute-plus of ffmpeg per clip on an HDD library. That is the
bug this sentence exists to prevent recurring.)

**Live Scrub ON means the work happens as early as it is useful.** The Focus
filmstrip starts on clip open, not on first pointer contact with the seek bar:
the toggle already answered "do you want this", and making the user discover a
10-second build *after* reaching for the timeline is the exact friction the
feature exists to remove.

---

## 4. Grid skimming: arm, then hover

The grid-tile scrub has one rule: **a tile skims only when it is armed** —
armed = it is the active/selected item. Click a clip to arm it, then hover it
to skim.

Why arming exists: sweeping the pointer across a wall of video tiles used to
start (and immediately cancel) a strip build per tile, which on a folder of 4K60
clips meant the disk was permanently busy building things nobody would look at.

Two implementation details that are load-bearing, both learned the hard way:

- **The build is driven by an `$effect` on `(armed && hovering)`, not by the
  `pointerenter` handler.** You arm a tile by clicking it, and the pointer is
  already inside by then — `pointerenter` fired before the click and never fires
  again. A handler-only trigger therefore built strips for every tile you swept
  past and for *none* of the tile you actually selected. Symptom: the scrub rail
  appears on the selected clip but the frames never change.
- **Leaving an armed tile does not cancel its build.** Only unarmed tiles cancel
  on pointer-leave. Cancelling a 10-second extraction because the pointer
  drifted, then restarting from zero on the way back, is indistinguishable from
  "it doesn't work". Disarming (selection moves elsewhere) is what cancels.

**Pointer → time mapping is across the whole cell, not the letterboxed picture.**
A 9:16 clip paints ~30% of a landscape cell's width; mapping the timeline onto
just that sliver made portrait clips hypersensitive while the pillarboxed
remainder was dead travel. The cell is what the hand aims at.

---

## 5. Extraction strategy (why builds are seconds, not minutes)

The original filmstrip did one ffmpeg pass with an `fps=` filter, which **decodes
every frame** of the clip to keep ~40. On a 5-minute 4K60 HEVC clip that is
~18,000 frames of software HEVC decode for one hover strip.

Now each sampled frame is its own keyframe seek: `-ss T` before `-i` jumps
through the container index and `-skip_frame nokey` makes the decoder emit only
the keyframe there. ~40 frames costs ~40 keyframe decodes **regardless of clip
length**, `-hwaccel auto` puts those on the GPU (NVDEC on the GTX 1070), and the
build can be cancelled *between* frames.

Fallbacks, in order: keyframe seek → exact seek at that timestamp → whole-clip
`fps=` scan (only for containers whose index can't be seeked at all — broken
AVIs, raw streams).

---

## 6. Concurrency & priority doctrine

| Knob | Value | Where | Why |
|---|---|---|---|
| Warm pool threads | `(cores / 4).clamp(1, 2)` | `commands.rs::warm_threads` | shallow read queue on HDD, still uses both heads of an SSD; never monopolizes cores the foreground needs |
| Warm batch cap | `WARM_CAP` = 600 | `commands.rs` | a folder-open warm can't become an unbounded job |
| Frontend decode slots | `MAX_INFLIGHT` = 6 | `thumbnail-loader.ts` | enough to fill a viewport, gentle on a USB SSD |
| Frontend queue order | **LIFO** | `thumbnail-loader.ts::pump` | the most recently requested cell is the one on screen now |
| Sprite builds | 1 at a time process-wide | `video.rs::SPRITE_BUILD_LOCK` | a second hover queues instead of racing for the disk |
| Frames per sprite build | `SPRITE_PARALLEL` = 2 | `video.rs` | halves wall time without deepening the read queue |
| Proxy transcodes | 1 at a time | `video.rs::PROXY_LOCK` | two would thrash the disk and halve both |
| Warm Focus previews held in JS | `LOUPE_RETAIN` = 6 | `thumbnail-loader.ts` | ~66 MB ceiling; grid bitmaps are deliberately **not** pinned |
| Resolved-URL memo | `MEMO_CAP` = 4000 | `thumbnail-loader.ts` | bounds a long session |

**Cancellation points** (all of these abandon in-flight work):
folder switch (`resetThumbs` → `cancelAllSprites`), entering Focus
(`cancelWarm` — playback gets the read bandwidth), starting a timeline drag
(`cancelWarm`), disposing files (`cancelAllSprites` + `cancelWarm`, then a
350 ms drain so no ffmpeg still holds a handle), and a newer warm generation
superseding an older one.

**Memory doctrine, stated once:** decoded grid bitmaps are *not* held in JS.
Virtualization keeps ~2 screens of `<img>` alive and the browser evicts as it
sees fit; scroll-back is fast because the asset URL is still in the WebView's
resource cache. An earlier build pinned a large LRU of decoded bitmaps (~350 MB)
and that is what produced the "progressively worse, then not responding" bug.
Only Focus previews are pinned, and only 6 of them.

---

## 7. Prepare

**Prepare** is the one place FoxCull does bulk work without being asked
per-item. It runs `warm_thumbnails(heavy=true)` in chunks of 16 on the same
bounded pool, so it is safe to keep culling while it runs.

- Photos & RAW first (fast, and the common reason to press it), then videos —
  kept as separate phases so the ETA is honest instead of blending a
  0.3 s/photo rate with a 4 s/clip rate into a meaningless average.
- Videos get poster **and** hover scrub strip. A prepared folder skims with zero
  on-hover work.
- Scope (the ▾ next to the button): everything in the folder (default),
  selection only, videos only, photos & RAW only.
- Abandons itself if the folder changes mid-run.

---

## 8. Machine-readable summary

Keep this block in sync with §2/§3/§6; it is the part a model should diff
against the code.

```yaml
cache_root: "<library>/cache"          # STORAGE.md; follows the catalog onto the drive
key: "hash(abs_path, mtime, size)"     # invalidation is automatic on edit/replace
key_note: "thumb keys also hash the tier (max), so each size is its own entry"
artifacts:
  - id: thumb
    file: "<hash>.jpg"
    tiers: [192, 320, 480, 1920]       # 1920 == LOUPE_MAX (Focus preview)
    builder: "thumbs::ensure"
    triggers: [viewport_visible, folder_warm, focus_open, focus_prefetch, prepare]
  - id: poster_grid
    file: "v<hash>.jpg"
    box_px: 480
    builder: "video::ensure_poster"
    triggers: [viewport_visible, prepare]
  - id: poster_focus
    file: "w<hash>.jpg"
    box_px: 1280
    builder: "video::ensure_poster_hires"
    triggers: [focus_open]
  - id: scrubstrip
    file: "s<hash>.jpg + s<hash>.json"
    cols: 8
    tile_w: 160
    frames: {min: 12, max: 40, rate: "~1/sec clamped"}
    builder: "video::ensure_scrubstrip"
    triggers: [grid_tile_armed_and_hovered, neighbour_prefetch, prepare]
    gated_by: [settings.liveScrub]
  - id: filmstrip
    file: "f<hash>.jpg + f<hash>.json"
    cols: 10
    tile_w: 240
    frames: {min: 16, max: 48, rate: "~1/sec clamped"}
    builder: "video::ensure_filmstrip"
    triggers: [focus_open_video]
    gated_by: [settings.liveScrub]
  - id: proxy
    file: "p<hash>.mp4"
    max_long_edge: 1920
    codec: "h264 crf22 + aac 128k"
    builder: "video::ensure_proxy"
    triggers: [webview_decode_failed]
settings:
  liveScrub: {default: false, gates: [scrubstrip, filmstrip]}
  scrubPrefetch: {default: false, span: 3, settle_ms: 900, requires: liveScrub}
limits:
  warm_threads: "(cores/4).clamp(1,2)"
  warm_cap: 600
  frontend_inflight: 6
  frontend_queue: LIFO
  sprite_builds_concurrent: 1
  sprite_frames_concurrent: 2
  proxy_concurrent: 1
  loupe_bitmaps_pinned: 6
  grid_bitmaps_pinned: 0
cancel_on:
  [folder_switch, enter_focus, timeline_drag_start, dispose_files, newer_warm_generation]
```

---

## 9. Known gaps (honest list)

- **Prepare has no resume.** Re-running it re-walks the folder; every already
  cached item is a cheap `exists()` check, so it's fast, but there is no
  progress persistence across a restart.
- **No cache eviction.** Nothing prunes `cache/` by age or size; purging a file
  from the in-app Trash removes its derived set, and that is the only automatic
  cleanup. A long-lived library grows monotonically.
- **The neighbour prefetch ignores travel direction.** The photo prefetch biases
  ahead/behind by which way you're moving; the video one is symmetric ±3.
- **Scrub strips are not built for clips reached by keyboard alone in the grid.**
  Arming happens on selection, which the arrow keys do — but the build needs a
  hover, so a keyboard-only pass never warms them. Prepare or the neighbour
  prefetch covers that case.
