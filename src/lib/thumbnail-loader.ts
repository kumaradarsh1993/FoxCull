// Viewport-prioritized, cancellable, MEMORY-DISCIPLINED thumbnail loader.
//
// Design notes (after resolving the "progressively worse / not responding" bug):
//
// The freeze was main-thread backpressure: reactive activity writes on every
// queue mutation plus an unpaced settle burst of cached promise resolutions and
// <img src> assignments. The scheduler below bounds that work per paint.
// Separately, an older implementation held a large LRU of decoded <img> bitmaps
// warm. We retain the memory discipline that removed that pressure: JS memoizes
// asset URLs, not decoded grid pixels, and lets the browser manage image-cache
// eviction.
//
// The disciplined approach:
//  - hold (almost) NO decoded grid bitmaps. Virtualization keeps only the visible
//    ~2 screens of <img> alive; the browser decodes/evicts them as it sees fit.
//    Scroll-back is still fast because the asset URL stays in the WebView's own
//    resource cache (a memory-cache hit, no IPC, quick re-decode of a small thumb).
//  - memoize resolved URLs (bounded LRU) so we never re-invoke Rust for a thumb we
//    already resolved, but DON'T pin its pixels.
//  - cap concurrent decodes; serve the CURRENT viewport first (LIFO); a cell that
//    scrolls away before its decode starts CANCELS its request.
//  - keep ONLY a tiny bounded set of decoded Focus previews warm (those are big,
//    1920px, and re-decoding them IS visible as a blur — the grid is not).
//  - generation token abandons queued work for the old folder on a switch.

import { api } from "./api";
import { activity } from "./activity.svelte";
import type { FilmstripInfo } from "./types";

const MAX_INFLIGHT = 6; // parallel decodes — enough to fill a viewport, gentle on the USB SSD
// Video posters/sprites launch ffmpeg and read far more source data than an
// image thumbnail. Six concurrent ffmpeg processes made video-grid scrolling
// hitch even though every command was correctly off the UI thread.
const MAX_HEAVY_INFLIGHT = 2;
const MEMO_CAP = 4000; // bound the URL cache so a long session can't grow it unbounded

const memo = new Map<string, string>(); // key -> asset url (LRU, bounded)
const pending = new Map<string, Promise<string | null>>(); // key -> in-flight promise
const stripMemo = new Map<string, FilmstripInfo>(); // key -> filmstrip geometry + asset url
const stripPending = new Map<string, Promise<FilmstripInfo | null>>();
type QItem = {
  key: string;
  heavy: boolean;
  run: () => void;
  drop: () => void;
  revision: number;
};
type QEntry = { item: QItem; revision: number };
let queue: QEntry[] = []; // priority log; `queued` is the authoritative live set
const queued = new Map<string, QItem>();
// Hard ceiling on deferred work. Holding ↓ through a 1000-clip folder repoints
// ~9 tiles per row at key-repeat rate; with dispatch deferred during the fling
// nothing drains, so the queue (and its closures + `pending` entries) grew
// without limit, and every per-tile cancel does an O(n) scan over it. Since the
// queue is served LIFO — newest first, because that IS the current viewport —
// the OLDEST entries are the stalest and are the right ones to shed. Dropping
// resolves the waiter with null rather than orphaning its promise.
const MAX_QUEUE = 240;
const MAX_QUEUE_STORAGE = MAX_QUEUE * 2;

function compactQueue() {
  queue = queue.filter(
    ({ item, revision }) => queued.get(item.key) === item && item.revision === revision,
  );
}

function trimQueue() {
  if (queue.length > MAX_QUEUE_STORAGE) compactQueue();
  if (queued.size <= MAX_QUEUE) return;
  for (let i = 0; i < queue.length && queued.size > MAX_QUEUE; i++) {
    const { item, revision } = queue[i];
    if (queued.get(item.key) !== item || item.revision !== revision) continue;
    queued.delete(item.key);
    item.revision++;
    item.drop();
  }
  if (queue.length > MAX_QUEUE_STORAGE) compactQueue();
}
let inflight = 0;
let heavyInflight = 0;
let generation = 0;

// A cached hit still resolves a promise and changes an <img src>, so it counts
// as main-thread work. The old recursive pump released every deferred memo hit
// in one turn (up to 240 after a fling). Pace dispatch to at most one grid row
// per paint; backend work remains bounded by the inflight caps below.
const MAX_DISPATCH_PER_FRAME = 12;
let pumpScheduled = false;

function scheduleFrame(fn: () => void) {
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(() => fn());
  else setTimeout(fn, 0);
}

function takeRunnable(): QItem | null {
  for (let i = queue.length - 1; i >= 0; i--) {
    const { item, revision } = queue[i];
    if (queued.get(item.key) !== item || item.revision !== revision) {
      queue.splice(i, 1);
      continue;
    }
    if (item.heavy && heavyInflight >= MAX_HEAVY_INFLIGHT) continue;
    queue.splice(i, 1);
    queued.delete(item.key);
    return item;
  }
  return null;
}

function schedulePump() {
  if (scrolling || pumpScheduled || inflight >= MAX_INFLIGHT || queued.size === 0) return;
  pumpScheduled = true;
  scheduleFrame(() => {
    pumpScheduled = false;
    if (scrolling) return;
    let dispatched = 0;
    while (dispatched < MAX_DISPATCH_PER_FRAME && inflight < MAX_INFLIGHT && queued.size > 0) {
      const next = takeRunnable();
      if (!next) {
        // The remaining work is heavy and already at its own concurrency cap.
        // Its completion callback will wake the pump; do not scan again every
        // animation frame while video posters or filmstrips are still decoding.
        return;
      }
      dispatched++;
      next.run();
    }
    if (queued.size > 0 && inflight < MAX_INFLIGHT) schedulePump();
  });
}

// ── Fast-scroll fetch suppression ────────────────────────────────────────────
// Corrected RCA (2026-08-03): native scrolling and CSS hover continued during a
// freeze while clicks, selection, rAF and tile population stopped. The JS main
// thread was blocked; the compositor was healthy. A fast fling creates loader
// churn, and the old settle path amplified it by resolving every deferred memo
// hit recursively in one turn. The +16k handles / +1 GB were fallout from that
// unpaced resource burst, not evidence of a GPU stall.
//
// While a fast scroll is in flight we hold ALL dispatch — including memoized
// (already-resolved) URLs, which are exactly what churn fastest through a
// previously-cached folder — so no new image work reaches the webview. Tiles that
// keep their item keep their pixels (their Thumb effect never re-ran); tiles that
// recycled to a new item stay neutral until motion settles, then resolve (memo
// hits resolve instantly, so the grid fills the moment you stop). This is the
// standard windowed-list "isScrolling → defer loads" contract. On settle, the
// frame-paced scheduler releases at most one row per paint.
let scrolling = false;
// ── the gate must NOT be able to latch ───────────────────────────────────────
// `scrolling` is raised here and lowered by VirtualGrid's 160 ms settle timer.
// That timer is a `setTimeout` on the main thread, so it cannot interrupt a long
// task. The frame-paced scheduler prevents that task now; this ceiling is the
// secondary guarantee that a missed caller-side settle cannot leave the gate on
// after the event loop is available again. Deferral is an optimization, never a
// correctness requirement.
const MAX_DEFER_MS = 700;
let deferGuard: ReturnType<typeof setTimeout> | undefined;
export function setScrolling(v: boolean) {
  const was = scrolling;
  scrolling = v;
  clearTimeout(deferGuard);
  deferGuard = undefined;
  if (v) {
    // Re-armed on every fast-scroll event, so a continuous fling keeps deferring;
    // it only fires if nothing lowers the gate within the ceiling.
    deferGuard = setTimeout(() => {
      deferGuard = undefined;
      if (!scrolling) return;
      scrolling = false;
      schedulePump();
      jobStateChanged();
    }, MAX_DEFER_MS);
  } else if (was) {
    schedulePump(); // settled: drain over multiple frames, never one long task
    jobStateChanged();
  }
}
/** Whether a fast scroll is currently in flight. Read by grid tiles so heavy
 *  per-tile work (the WebCodecs skim decoder) never spins up mid-fling. */
export function isScrolling(): boolean {
  return scrolling;
}

function memoGet(key: string): string | undefined {
  const v = memo.get(key);
  if (v !== undefined) {
    memo.delete(key);
    memo.set(key, v); // refresh recency
  }
  return v;
}
function memoSet(key: string, url: string) {
  memo.set(key, url);
  if (memo.size > MEMO_CAP) {
    const oldest = memo.keys().next().value as string;
    memo.delete(oldest);
  }
}

function stripMemoGet(key: string): FilmstripInfo | undefined {
  const v = stripMemo.get(key);
  if (v) {
    stripMemo.delete(key);
    stripMemo.set(key, v);
  }
  return v;
}
function stripMemoSet(key: string, info: FilmstripInfo) {
  stripMemo.set(key, info);
  if (stripMemo.size > MEMO_CAP) {
    const oldest = stripMemo.keys().next().value as string;
    stripMemo.delete(oldest);
  }
}

// ── progress reporting for on-demand loads ──────────────────────────────────
//
// This queue used to be completely silent. The backend announces `warm_thumbnails`
// and sprite builds, but neither covers what actually happens when you open a
// folder: tiles ask for their own thumbnails one at a time through here, and
// warming SKIPS video entirely unless it was asked to be heavy (Prepare). So a
// folder of clips whose posters aren't cached yet renders for ten or fifteen
// seconds behind a grid of blank tiles with nothing anywhere saying why. That
// was invisible while old caches existed and became obvious the moment they
// were wiped.
//
// A batch is only announced once it has been running for a beat: warm caches
// resolve in single-digit milliseconds and a chip flashing on every folder step
// would be worse than silence.
const JOB_ID = "thumbs";
const JOB_LABEL = "Loading thumbnails";
const ANNOUNCE_AFTER_MS = 700;
let jobShown = false;
let jobTimer: ReturnType<typeof setTimeout> | undefined;

/** On-demand work has no stable denominator: scrolling discovers new tiles and
 * cancels old ones. Report state transitions only, never every queue mutation. */
function jobStateChanged() {
  if (scrolling) return;
  const outstanding = queued.size + inflight;
  if (outstanding === 0) {
    clearTimeout(jobTimer);
    jobTimer = undefined;
    if (jobShown) activity.end(JOB_ID);
    jobShown = false;
    return;
  }
  if (!jobShown && !jobTimer) {
    jobTimer = setTimeout(() => {
      jobTimer = undefined;
      if (!scrolling && queued.size + inflight > 0) {
        jobShown = true;
        activity.local(JOB_ID, JOB_LABEL, 0, 0);
      }
    }, ANNOUNCE_AFTER_MS);
  }
}

function jobFinished() {
  jobStateChanged();
}

/** Folder switch: whatever was queued is abandoned, so the batch is over. */
function jobReset() {
  clearTimeout(jobTimer);
  jobTimer = undefined;
  if (jobShown) activity.end(JOB_ID);
  jobShown = false;
}

/** Abandon queued (not-yet-started) work — call when the folder changes. */
export function resetThumbs() {
  generation++;
  scrolling = false; // a folder switch is never a "still flinging" state
  clearTimeout(deferGuard);
  deferGuard = undefined;
  jobReset();
  for (const item of queued.values()) item.drop();
  queued.clear();
  queue = [];
  pending.clear();
  stripPending.clear();
  stripMemo.clear();
  // Release the warm Focus previews from the folder we're leaving.
  loupeDecoded.clear();
  loupeInflight.clear();
  // And tell the backend to abandon any sprite build already running — without
  // this, hover-scrub work for the folder you just LEFT kept the disk busy
  // while the new folder tried to load its thumbnails.
  api.cancelAllSprites();
}

/** Drop a single not-yet-started request (a grid/strip cell scrolled out of
 *  view before its decode began). In-flight requests are cheap to let finish. */
function cancel(key: string) {
  const item = queued.get(key);
  if (!item) return;
  queued.delete(key);
  item.revision++;
  item.drop();
  jobStateChanged();
}

/** Lightweight stats for the diagnostic memory log. */
export function loaderStats() {
  return {
    memo: memo.size,
    loupe: loupeDecoded.size,
    pending: pending.size,
    stripPending: stripPending.size,
    queue: queued.size,
    queueStorage: queue.length,
    inflight,
    heavyInflight,
  };
}

// ── loupe (Focus-view) preview prefetch — the ONLY place we pin bitmaps ──────
//
// Focus previews are large (1920px ≈ 11 MB decoded each) and re-decoding one IS
// visible as a blur, so we keep a SMALL bounded set warm: the shots just
// ahead/behind the one you're on. Kept tiny (6) so the held memory is bounded
// (~66 MB max) and released entirely on a folder switch.
const LOUPE_RETAIN = 6;
const loupeDecoded = new Map<string, HTMLImageElement>(); // path -> decoded image (LRU)
const loupeInflight = new Set<string>(); // paths currently being prefetched

/** Pre-generate + pre-decode the large Focus preview for `path`, and keep it
 *  warm. Cheap to call repeatedly (deduped + memoized). Images/RAW only. */
export function prefetchLoupe(path: string): void {
  const have = loupeDecoded.get(path);
  if (have) {
    loupeDecoded.delete(path);
    loupeDecoded.set(path, have); // mark most-recently-used
    return;
  }
  if (loupeInflight.has(path)) return;
  loupeInflight.add(path);
  enqueue(`loupe:${path}`, () => api.loupeSrc(path))
    .then((url) => {
      if (!url) return;
      const img = new Image();
      img.decoding = "async";
      img.src = url;
      img.decode?.().catch(() => {});
      loupeDecoded.set(path, img);
      while (loupeDecoded.size > LOUPE_RETAIN) {
        const oldest = loupeDecoded.keys().next().value as string;
        loupeDecoded.delete(oldest);
      }
    })
    .finally(() => loupeInflight.delete(path));
}

/** Shared queue/dedup/cap machinery. `fetchFsPath` resolves to a filesystem path
 *  the backend produced; we convert it to an asset URL and memoize it. */
function enqueue(key: string, fetchFsPath: () => Promise<string>, heavy = false): Promise<string | null> {
  // A memo hit normally resolves instantly — but NOT during a fast scroll, where
  // instant resolution can still create an unbounded main-thread src/decode
  // burst. When scrolling, fall through to the queue so it is held until settle;
  // its run() resolves the memo without a fetch.
  const cached = memoGet(key);
  if (cached && !scrolling) return Promise.resolve(cached);

  const existing = pending.get(key);
  if (existing) {
    const item = queued.get(key);
    if (item) {
      item.revision++;
      queue.push({ item, revision: item.revision });
      if (queue.length > MAX_QUEUE_STORAGE) compactQueue();
    }
    return existing;
  }

  const myGen = generation;
  let resolvePromise!: (value: string | null) => void;
  const promise = new Promise<string | null>((resolve) => (resolvePromise = resolve));
  pending.set(key, promise);
  const item: QItem = {
    key,
    heavy,
    revision: 0,
    run: () => {
      if (myGen !== generation) {
        if (pending.get(key) === promise) pending.delete(key);
        resolvePromise(null);
        jobFinished();
        schedulePump();
        return;
      }
      const c = memoGet(key);
      if (c) {
        if (pending.get(key) === promise) pending.delete(key);
        resolvePromise(c);
        jobFinished();
        schedulePump();
        return;
      }
      inflight++;
      if (heavy) heavyInflight++;
      fetchFsPath()
        .then((fsPath) => {
          const url = api.fileSrc(fsPath);
          memoSet(key, url);
          resolvePromise(myGen === generation ? url : null);
        })
        .catch(() => resolvePromise(null))
        .finally(() => {
          inflight--;
          if (heavy) heavyInflight--;
          if (pending.get(key) === promise) pending.delete(key);
          jobFinished();
          schedulePump();
        });
    },
    drop: () => {
      if (pending.get(key) === promise) pending.delete(key);
      resolvePromise(null);
    },
  };
  queued.set(key, item);
  queue.push({ item, revision: item.revision });
  trimQueue();
  schedulePump();
  jobStateChanged();
  return promise;
}

function enqueueStrip(key: string, fetchInfo: () => Promise<FilmstripInfo>): Promise<FilmstripInfo | null> {
  const cached = stripMemoGet(key);
  if (cached) return Promise.resolve(cached);

  const existing = stripPending.get(key);
  if (existing) {
    const item = queued.get(key);
    if (item) {
      item.revision++;
      queue.push({ item, revision: item.revision });
      if (queue.length > MAX_QUEUE_STORAGE) compactQueue();
    }
    return existing;
  }

  const myGen = generation;
  let resolvePromise!: (value: FilmstripInfo | null) => void;
  const promise = new Promise<FilmstripInfo | null>((resolve) => (resolvePromise = resolve));
  stripPending.set(key, promise);
  const item: QItem = {
    key,
    heavy: true,
    revision: 0,
    run: () => {
      if (myGen !== generation) {
        if (stripPending.get(key) === promise) stripPending.delete(key);
        resolvePromise(null);
        jobFinished();
        schedulePump();
        return;
      }
      inflight++;
      heavyInflight++;
      fetchInfo()
        .then((info) => {
          const hydrated = { ...info, src: api.fileSrc(info.src) };
          stripMemoSet(key, hydrated);
          resolvePromise(myGen === generation ? hydrated : null);
        })
        .catch(() => resolvePromise(null))
        .finally(() => {
          inflight--;
          heavyInflight--;
          if (stripPending.get(key) === promise) stripPending.delete(key);
          jobFinished();
          schedulePump();
        });
    },
    drop: () => {
      if (stripPending.get(key) === promise) stripPending.delete(key);
      resolvePromise(null);
    },
  };
  queued.set(key, item);
  queue.push({ item, revision: item.revision });
  trimQueue();
  schedulePump();
  jobStateChanged();
  return promise;
}

export function loadThumb(path: string, size: number): Promise<string | null> {
  return enqueue(`${path}@${size}`, () => api.thumbnail(path, size));
}
export function cancelThumb(path: string, size: number): void {
  cancel(`${path}@${size}`);
}

/** Cached video poster frame (bundled ffmpeg), through the same capped queue. */
export function loadVideoPoster(path: string): Promise<string | null> {
  return enqueue(`vid:${path}`, () => api.videoPoster(path), true);
}
/** Sharp ~1280px poster for Focus view — same queue, separate cache key. */
export function loadVideoPosterHires(path: string): Promise<string | null> {
  return enqueue(`vidhi:${path}`, () => api.videoPosterHires(path), true);
}
export function cancelVideoPoster(path: string): void {
  cancel(`vid:${path}`);
}

/** The scrub sprite — ONE artifact shared by grid skimming and the Focus
 *  timeline. It used to be two (a light `s` strip for tiles, a dense `f` strip
 *  for Focus), which meant double the extraction for every clip and, worse, a
 *  visible "restart": arming a tile built the light strip, then double-clicking
 *  into Focus began the dense one from zero. Same sprite for both now, and
 *  because both go through THIS queue, a second request for a clip already
 *  building joins the in-flight promise instead of starting a rival build. */
export function loadVideoFilmstrip(path: string): Promise<FilmstripInfo | null> {
  return enqueueStrip(`film:${path}`, () => api.videoFilmstrip(path));
}
/** Cancel a Focus/grid filmstrip request (see `cancelVideoScrubstrip`). */
export function cancelVideoFilmstrip(path: string): void {
  const key = `film:${path}`;
  const wasQueued = queued.has(key);
  cancel(key);
  if (!wasQueued && stripPending.has(key)) {
    stripPending.delete(key);
    api.cancelSprite(path, "film");
  }
}
/** Legacy light hover sprite. Kept only so folders Prepared before the sprites
 *  were unified still skim instantly from their cached `s` strip. */
export function loadVideoScrubstrip(path: string): Promise<FilmstripInfo | null> {
  return enqueueStrip(`scrub:${path}`, () => api.videoScrubstrip(path));
}
/** Cancel a hover-strip request. Queued-not-started requests are dropped
 *  locally; a build already RUNNING on the backend is told to stop (it aborts
 *  between frame extractions), and its doomed promise is forgotten so a
 *  re-hover starts a fresh request instead of latching onto the cancelled one. */
export function cancelVideoScrubstrip(path: string): void {
  const key = `scrub:${path}`;
  const wasQueued = queued.has(key);
  cancel(key);
  if (!wasQueued && stripPending.has(key)) {
    stripPending.delete(key);
    api.cancelSprite(path, "scrub");
  }
}
