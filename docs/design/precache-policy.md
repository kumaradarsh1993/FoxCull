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
| **Scrub sprite** | `f<hash>.jpg` + `f<hash>.json` | `video::ensure_filmstrip` | 10 cols × 240 px tiles, 16–48 frames | **both** grid-tile skimming and the Focus timeline |
| ~~Hover scrub strip~~ (legacy) | `s<hash>.jpg` + `s<hash>.json` | `video::ensure_scrubstrip` | 8 cols × 160 px tiles, 12–40 frames | read-only: still painted if cached, never built |
| H.264 proxy | `p<hash>.mp4` | `video::ensure_proxy` | ≤1920 long edge, CRF 22 | clips the webview cannot decode |

**One sprite, not two (changed 2026-07-21).** The grid tile and the Focus
timeline used to build *different* sprite sheets from the same clip. That was
double the extraction for every video, and it surfaced as a bug: arming a tile
started the light `s` strip, then double-clicking into Focus started the dense
`f` strip from zero — which looks exactly like the build "restarting at 10%".
They now share the dense sprite. The `s` builder is retained only so folders
Prepared before this change still skim from their existing cache; nothing
creates a new `s` strip.

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
| Scrub sprite | grid tile is **armed** (clicked/selected) **and** hovered, Live Scrub ON, 140 ms settle | see §4 |
| Scrub sprite | opening a video in Focus with Live Scrub ON | previously waited for the pointer to reach the seek bar |
| Scrub sprite | neighbouring clips ±3 while a video is open in Focus — **only if** Live Scrub ON **and** "Pre-build nearby clips" ON, after a 900 ms settle | opt-in, default off |
| Scrub sprite | cached-only paint on open with Live Scrub OFF | never builds |
| H.264 proxy | the `<video>` element fails to decode the original | one at a time, process-wide lock |

### Video pre-caching is retired everywhere (2026-07-22)

The grid moved onto the same live decoder as Focus, which removes the last
consumer of scrub sprites. The objection that had kept sprites in the grid — "a
video decoder per tile is not viable" — turned out not to apply: **the armed
rule already guarantees exactly one skimming tile at a time**, so there is
exactly one decoder, the same as Focus. Consequences:

- **`Prepare` no longer builds sprites.** For video folders it now builds
  posters only, which was the smaller half of its work. `warm_thumbnails`
  (heavy) dropped the `ensure_filmstrip` call.
- **Neighbour scrub prefetch is gone**, setting and all. It existed to have a
  neighbouring clip's sprite ready before you stepped onto it; nothing reads
  sprites on that path any more, so it was pure disk and CPU spent on artifacts
  no one would open. `scrubPrefetch` remains in the settings type, deprecated,
  only so an older stored value still loads.
- **Sprites are now strictly a per-clip fallback**, built on demand when
  `ScrubEngine.open()` rejects a clip. Nothing pre-builds them, anywhere.
- Everything else Prepare does — image previews, RAW previews, video posters —
  is unchanged and still worth doing.

### Focus view no longer pre-caches anything (2026-07-21)

**The single biggest change to this policy since it was written.** Focus-view
scrubbing now decodes the real frame under the cursor on demand (WebCodecs —
`src/lib/scrub-engine.ts`, setting `liveDecodeScrub`, default ON). It needs no
sprite, no build, no cache entry and no progress UI: indexing a clip reads a
few MB of MP4 metadata (~300 ms even on a 546-second 4K60 file, because the
moov hunt skips the mdat) and every frame after that is decoded live.

Consequences for everything below:

- **The Focus timeline builds no sprite.** `ensureFilmstrip()` returns early
  while the engine is opening (`enginePending`) or open (`engineReady`), so the
  two paths never both run.
- **The sprite survives for the GRID only** — a decoder per grid tile is not
  viable, so tile skimming still uses `f<hash>.jpg`. "Live Scrub" in Settings
  now means *grid tiles*, and is labelled so.
- **Fallback is per clip, automatic and silent.** If a clip can't be indexed or
  its codec can't be decoded this way, `ScrubEngine.open()` rejects and the
  sprite path resumes exactly as documented below — including the build that
  was being held back.
- **The "Focus preview" row of the Prepare table is now grid-only value** for
  video: preparing a folder still builds posters + sprites, which help the grid,
  but Focus no longer depends on them.

Measured against the sprite path it replaces in Focus: ~1 s per frame to build
40 low-resolution frames, versus ~40 ms to decode one full-resolution frame on
demand. Detail and method: `docs/design/video-player-migration.md` §10-11.

### The Live Scrub contract (grid tiles; also the Focus fallback)

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

### 5.1 Where the remaining time actually goes (measured 2026-07-21)

Benchmarked on the Alienware (12 cores), 4K60 HEVC Main10 `.mov` from the Osmo,
on the **internal HDD**, using the bundled ffmpeg — the same command line the
sprite builder issues.

| Test | Result | What it tells us |
|---|---|---|
| 6 cold frames, `-hwaccel auto` | 5.92 s | ~0.99 s per frame |
| 6 cold frames, software decode | 6.22 s | **hwaccel is worth ~5%** here, not the 5–10× it gives on a full decode — a single keyframe doesn't amortise device setup |
| the SAME 6 frames, OS cache warm | 5.04 s | **only ~15% of the time is disk I/O** |
| 1 frame, `-f null` (no scale, no JPEG) | 0.80 s of 0.99 s | the cost is ffmpeg startup + container index parse + one keyframe decode; scaling and encoding are noise |
| 12 cold frames at parallel 2 / 4 / 6 | 6.16 / 4.40 / 3.61 s | it parallelises: 4 is 1.40×, 6 is 1.71× |

**The headline: sprite building is CPU/process bound, not disk bound.** Moving a
library from HDD to SSD barely helps, which matches what the owner observed.
The fixed cost is paid once per frame because each frame is its own ffmpeg
process re-opening a multi-gigabyte container.

Consequences for anyone optimising this next:

- Raising the per-build parallelism is the cheap win, and it was taken (2 → up
  to 4, core-scaled).
- Dropping `-hwaccel auto` would be a wash; don't bother.
- The real remaining lever is **not spawning a process per frame** — one ffmpeg
  invocation emitting N frames via a `select=` filtergraph would pay the
  container-open cost once. It was not attempted because `select` on
  arbitrary timestamps forces a full decode walk, which is the very thing the
  keyframe-seek design exists to avoid; a segmented approach (one process per
  chunk of the timeline, each emitting several frames) is the untried middle
  ground.
- Lowering the frame count is the other lever (48 max today). Untouched so far
  because it trades directly against scrub smoothness.

---

## 6. Concurrency & priority doctrine

| Knob | Value | Where | Why |
|---|---|---|---|
| Warm pool threads | `(cores / 4).clamp(1, 2)` | `commands.rs::warm_threads` | shallow read queue on HDD, still uses both heads of an SSD; never monopolizes cores the foreground needs |
| Warm batch cap | `WARM_CAP` = 600 | `commands.rs` | a folder-open warm can't become an unbounded job |
| Frontend decode slots | `MAX_INFLIGHT` = 6 | `thumbnail-loader.ts` | enough to fill a viewport, gentle on a USB SSD |
| Frontend queue order | **LIFO** | `thumbnail-loader.ts::pump` | the most recently requested cell is the one on screen now |
| Sprite builds | 1 at a time process-wide | `video.rs::SPRITE_BUILD_LOCK` | a second hover queues instead of racing for the disk |
| Frames per sprite build | `sprite_parallel()` = `(cores/3).clamp(2,4)` | `video.rs` | measured, see §5.1 — this work is CPU-bound, not I/O-bound |
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
  - id: filmstrip                      # GRID skimming (+ Focus fallback only)
    file: "f<hash>.jpg + f<hash>.json"
    cols: 10
    tile_w: 240
    frames: {min: 16, max: 48, rate: "~1/sec clamped"}
    builder: "video::ensure_filmstrip"
    triggers: [grid_tile_armed_and_hovered, focus_open_video]
    gated_by: [settings.liveScrub, "not (engineReady or enginePending)"]
    note: "FALLBACK ONLY since 2026-07-22. Focus and armed grid tiles both
           decode frames live; these triggers fire only when live-decode scrub
           is off or ScrubEngine.open() rejected the clip. Nothing pre-builds
           sprites any more — `prepare` and `neighbour_prefetch` were removed as
           triggers."
  - id: scrubstrip                     # legacy; read-only, never built
    file: "s<hash>.jpg + s<hash>.json"
    cols: 8
    tile_w: 160
    frames: {min: 12, max: 40, rate: "~1/sec clamped"}
    builder: "video::ensure_scrubstrip"
    triggers: []
    note: "painted if already cached so pre-2026-07-21 Prepared folders still skim"
  - id: proxy
    file: "p<hash>.mp4"
    max_long_edge: 1920
    codec: "h264 crf22 + aac 128k"
    builder: "video::ensure_proxy"
    triggers: [webview_decode_failed]
settings:
  liveScrub: {default: false, gates: [scrubstrip, filmstrip], scope: "grid tiles"}
  scrubPrefetch: {status: removed, on: 2026-07-22, reason: "nothing reads pre-built sprites"}
  glimpseSpeed:
    default: 40
    unit: "x realtime"
    range: [10, 100]
    note: "Glimpse (Ctrl+Space) sweeps a clip's keyframes on the live decoder;
           floored at a 4 s sweep so short clips stay readable. Caches nothing."
  liveDecodeScrub:
    default: true
    scope: "Focus view AND armed grid tiles"
    builds: []                         # decodes on demand; caches NOTHING
    impl: "src/lib/scrub-engine.ts (WebCodecs VideoDecoder + mp4box moov index)"
    index_cost: "~300 ms, ~5 MB read, regardless of file size"
    frame_cost_ms: {coarse: "~40 (read+decode+paint, 4K60 HEVC off HDD)", exact: "~150 (one GOP)"}
    fallback: "per clip, silent → filmstrip sprite path"
limits:
  warm_threads: "(cores/4).clamp(1,2)"
  warm_cap: 600
  frontend_inflight: 6
  frontend_queue: LIFO
  sprite_builds_concurrent: 1
  sprite_frames_concurrent: "(cores/3).clamp(2,4)"
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
