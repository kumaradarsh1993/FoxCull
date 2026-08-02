// Viewport-prioritized, cancellable, MEMORY-DISCIPLINED thumbnail loader.
//
// Design notes (after auditing the "progressively worse / not responding" bug):
//
// The freeze was NOT decode speed — folder scans are 0-3 ms and thumbnail files
// are generated once and disk-cached. It was MEMORY: an earlier version held a
// large LRU of decoded <img> bitmaps "warm" (up to 700 grid thumbs + a dozen
// 1920px previews ≈ 350 MB). Scrolling a big folder accumulated that fast and the
// WebView process ballooned until it thrashed. Holding decoded bitmaps in JS
// fights the browser's own image-cache eviction — exactly the wrong move for a
// virtualized grid of hundreds of images.
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
type QItem = { key: string; heavy: boolean; run: () => void; drop: () => void };
let queue: QItem[] = []; // served LIFO (newest request = current viewport first)
// Hard ceiling on deferred work. Holding ↓ through a 1000-clip folder repoints
// ~9 tiles per row at key-repeat rate; with dispatch deferred during the fling
// nothing drains, so the queue (and its closures + `pending` entries) grew
// without limit, and every per-tile cancel does an O(n) scan over it. Since the
// queue is served LIFO — newest first, because that IS the current viewport —
// the OLDEST entries are the stalest and are the right ones to shed. Dropping
// resolves the waiter with null rather than orphaning its promise.
const MAX_QUEUE = 240;
function trimQueue() {
  while (queue.length > MAX_QUEUE) {
    const stale = queue.shift();
    stale?.drop();
  }
}
let inflight = 0;
let heavyInflight = 0;
let generation = 0;

// ── Fast-scroll fetch suppression ────────────────────────────────────────────
// Measured root cause of the scroll freeze (2026-08-02): a fast fling makes the
// recycled tiles repoint their <img src> in quick succession; each src change is
// an asset-protocol fetch + Chromium decode, and the burst spikes WebView2's
// handle count ~+16k and RAM ~+1 GB until the compositor stalls for 8-12 s (GPU
// idle throughout — a pipeline stall, not a GPU one). DOM recycling removed the
// node churn but not this FETCH churn.
//
// While a fast scroll is in flight we hold ALL dispatch — including memoized
// (already-resolved) URLs, which are exactly what churn fastest through a
// previously-cached folder — so no new image work reaches the webview. Tiles that
// keep their item keep their pixels (their Thumb effect never re-ran); tiles that
// recycled to a new item stay neutral until motion settles, then resolve (memo
// hits resolve instantly, so the grid fills the moment you stop). This is the
// standard windowed-list "isScrolling → defer loads" contract, applied at the
// loader so it needs no Thumb changes and cannot touch live-scrub.
let scrolling = false;
// ── the gate must NOT be able to latch ───────────────────────────────────────
// `scrolling` is raised here and lowered by VirtualGrid's 160 ms settle timer.
// That timer is a `setTimeout` on the MAIN THREAD — and the freeze we are
// chasing IS a blocked main thread (see docs/design/FREEZE-HANDOVER-2026-08-03
// §0). So exactly when the gate matters most, the thing that clears it cannot
// run: the gate sticks ON forever, no thumbnail ever loads again, and the
// activity chip freezes mid-count. That is the "tiles never populate again,
// loader stuck" state reported on nightly.4/.5.
//
// A deferral is an optimization, never a correctness requirement, so it gets an
// absolute ceiling of its own. Whatever happens to the caller, dispatch resumes.
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
      pump();
      jobReport();
    }, MAX_DEFER_MS);
  } else if (was) {
    pump(); // settled — drain the deferred viewport now
    jobReport(); // and refresh the progress chip we held quiet during the fling
  }
}
/** Whether a fast scroll is currently in flight. Read by grid tiles so heavy
 *  per-tile work (the WebCodecs skim decoder) never spins up mid-fling. */
export function isScrolling(): boolean {
  return scrolling;
}

function pump() {
  if (scrolling) return; // defer every fetch until the fling settles
  while (inflight < MAX_INFLIGHT && queue.length) {
    // LIFO among runnable work: the current viewport wins, but a wall of
    // uncached videos cannot occupy every slot while image thumbs wait.
    let i = queue.length - 1;
    while (i >= 0 && queue[i].heavy && heavyInflight >= MAX_HEAVY_INFLIGHT) i--;
    if (i < 0) return;
    const [next] = queue.splice(i, 1);
    next.run();
  }
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
let jobDone = 0; // completions since this batch began
let jobShown = false;
let jobTimer: ReturnType<typeof setTimeout> | undefined;

/** Outstanding = queued but not started, plus running. Reported as part of the
 *  total so the denominator is honest while a scroll keeps adding work. */
function jobReport() {
  // Stay silent during a fast fling. Enqueue/cancel churn calls this ~2× per
  // recycled tile, and each emission is a reactive `activity` store write that
  // re-renders the chip — hundreds per second mid-scroll, for a number nobody
  // reads while flinging (and it was the "36% → 28% going backwards" whipsaw).
  // `setScrolling(false)` calls jobReport() again on settle, so the chip catches
  // up the moment motion stops.
  if (scrolling) return;
  const outstanding = queue.length + inflight;
  if (outstanding === 0) {
    // Drained. Close the job out (if it was ever shown) and reset the batch.
    clearTimeout(jobTimer);
    jobTimer = undefined;
    if (jobShown) {
      // A fast scroll can cancel every queued request before any starts. That is
      // still a completed batch; previously jobDone===0 left the visible
      // "Loading thumbnails" activity stuck in its running state forever even
      // though queue, pending and inflight were all zero.
      if (jobDone > 0) activity.local(JOB_ID, JOB_LABEL, jobDone, jobDone);
      else activity.end(JOB_ID);
    }
    jobShown = false;
    jobDone = 0;
    return;
  }
  if (jobShown) activity.local(JOB_ID, JOB_LABEL, jobDone, jobDone + outstanding);
  else if (!jobTimer) {
    jobTimer = setTimeout(() => {
      jobTimer = undefined;
      // Still busy after the grace period — this one is worth showing.
      if (queue.length + inflight > 0) {
        jobShown = true;
        jobReport();
      }
    }, ANNOUNCE_AFTER_MS);
  }
}

/** A queued request finished (or failed — either way it stopped being work). */
function jobFinished() {
  jobDone++;
  jobReport();
}

/** Folder switch: whatever was queued is abandoned, so the batch is over. */
function jobReset() {
  clearTimeout(jobTimer);
  jobTimer = undefined;
  if (jobShown) activity.end(JOB_ID);
  jobShown = false;
  jobDone = 0;
}

/** Abandon queued (not-yet-started) work — call when the folder changes. */
export function resetThumbs() {
  generation++;
  scrolling = false; // a folder switch is never a "still flinging" state
  jobReset();
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
  if (pending.has(key) || stripPending.has(key)) {
    const i = queue.findIndex((q) => q.key === key);
    if (i >= 0) {
      queue.splice(i, 1);
      pending.delete(key);
      stripPending.delete(key);
      // The batch just got smaller without anything completing — re-report, or
      // a job whose whole remainder scrolled away would sit at its last
      // percentage forever.
      jobReport();
    }
  }
}

/** Lightweight stats for the diagnostic memory log. */
export function loaderStats() {
  return {
    memo: memo.size,
    loupe: loupeDecoded.size,
    pending: pending.size,
    stripPending: stripPending.size,
    queue: queue.length,
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
  // instant resolution is exactly the src churn that chokes the compositor. When
  // scrolling, fall through to the queue so it's held until settle (its run()
  // resolves the memo without a fetch, so it's still instant the moment we stop).
  const cached = memoGet(key);
  if (cached && !scrolling) return Promise.resolve(cached);

  const existing = pending.get(key);
  if (existing) {
    // Already queued/in-flight — bump it to the front (it's wanted again, now).
    const i = queue.findIndex((q) => q.key === key);
    if (i >= 0) {
      const [it] = queue.splice(i, 1);
      queue.push(it);
    }
    return existing;
  }

  const myGen = generation;
  const promise = new Promise<string | null>((resolve) => {
    const run = () => {
      if (myGen !== generation) {
        pending.delete(key);
        resolve(null);
        pump();
        jobFinished();
        return;
      }
      // Deferred memo hit (queued during a fling): resolve instantly, no fetch.
      const c = memoGet(key);
      if (c) {
        pending.delete(key);
        resolve(c);
        pump();
        jobFinished();
        return;
      }
      inflight++;
      if (heavy) heavyInflight++;
      fetchFsPath()
        .then((fsPath) => {
          const url = api.fileSrc(fsPath);
          memoSet(key, url);
          resolve(myGen === generation ? url : null);
        })
        .catch(() => resolve(null))
        .finally(() => {
          inflight--;
          if (heavy) heavyInflight--;
          pending.delete(key);
          pump();
          jobFinished();
        });
    };
    const drop = () => {
      pending.delete(key);
      resolve(null);
    };
    queue.push({ key, heavy, run, drop });
    trimQueue();
    pump();
    jobReport();
  });

  pending.set(key, promise);
  return promise;
}

function enqueueStrip(key: string, fetchInfo: () => Promise<FilmstripInfo>): Promise<FilmstripInfo | null> {
  const cached = stripMemoGet(key);
  if (cached) return Promise.resolve(cached);

  const existing = stripPending.get(key);
  if (existing) {
    const i = queue.findIndex((q) => q.key === key);
    if (i >= 0) {
      const [it] = queue.splice(i, 1);
      queue.push(it);
    }
    return existing;
  }

  const myGen = generation;
  const promise = new Promise<FilmstripInfo | null>((resolve) => {
    const run = () => {
      if (myGen !== generation) {
        stripPending.delete(key);
        resolve(null);
        pump();
        jobFinished();
        return;
      }
      inflight++;
      heavyInflight++;
      fetchInfo()
        .then((info) => {
          const hydrated = { ...info, src: api.fileSrc(info.src) };
          stripMemoSet(key, hydrated);
          resolve(myGen === generation ? hydrated : null);
        })
        .catch(() => resolve(null))
        .finally(() => {
          inflight--;
          heavyInflight--;
          // Only clear our own registration — a cancelled build's promise may
          // outlive it while a FRESH request for the same clip is registered.
          if (stripPending.get(key) === promise) stripPending.delete(key);
          pump();
          jobFinished();
        });
    };
    const drop = () => {
      if (stripPending.get(key) === promise) stripPending.delete(key);
      resolve(null);
    };
    queue.push({ key, heavy: true, run, drop });
    trimQueue();
    pump();
    jobReport();
  });

  stripPending.set(key, promise);
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
  const wasQueued = queue.some((q) => q.key === key);
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
  const wasQueued = queue.some((q) => q.key === key);
  cancel(key);
  if (!wasQueued && stripPending.has(key)) {
    stripPending.delete(key);
    api.cancelSprite(path, "scrub");
  }
}
