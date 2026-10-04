<script module lang="ts">
  /** An in point still waiting for its out, per clip, for this session. */
  const openIns = new Map<string, number>();
</script>

<script lang="ts">
  import { api } from "$lib/api";
  import { activity } from "$lib/activity.svelte";
  import {
    loadThumb,
    loadVideoPosterHires,
    loadVideoFilmstrip,
    cancelVideoFilmstrip,
  } from "$lib/thumbnail-loader";
  import { settings } from "$lib/settings.svelte";
  import { ScrubEngine, paintFrame } from "$lib/scrub-engine";
  import type { MediaItem, FilmstripInfo, MediaProbe, VideoSegment, ClipRef } from "$lib/types";
  import ContextMenu, { type MenuEntry } from "./ContextMenu.svelte";
  import {
    emptyMarks,
    markIn as ruleIn,
    markOut as ruleOut,
    moveEdge,
    moveOpen,
    removeIn,
    removeOut,
    removeSeg,
    normalize,
    snapFrame,
    totalLen,
    fmtT,
    type MarkState,
  } from "$lib/segments";

  let {
    item,
    showInfo = false,
    onchanged = () => {},
    ontransport = () => {},
    casting = false,
    castPlayerState = null,
    oncasttoggle = () => {},
    onranges = () => {},
  }: {
    item: MediaItem | null;
    showInfo?: boolean;
    onchanged?: (selectPath?: string | null) => void;
    /** Reports local player events. Cast mode ignores local play/pause and only
     *  forwards landed timeline seeks because the TV owns playback. */
    ontransport?: (state: { kind: "play" | "pause" | "seek"; paused: boolean; time: number }) => void;
    casting?: boolean;
    castPlayerState?: string | null;
    oncasttoggle?: () => void;
    /** The clip's marked in/out ranges changed (the library's ✂ badge and
     *  what a drag to the Edit window carries). */
    onranges?: (path: string, ranges: VideoSegment[]) => void;
  } = $props();

  function reportRanges() {
    if (item) onranges(item.path, marks.segments.map((x) => ({ ...x })));
  }

  // Image transitions: the PREVIOUS photo stays painted until the next sharp
  // preview is fully decoded, then we swap in one frame — no black gap, and no
  // blur flash when flipping through an already-prepared folder. The blur-up
  // placeholder only appears when a load is genuinely slow (cold cache / heavy
  // file), where it's useful feedback instead of an artifact.
  let curSrc = $state<string | null>(null); // sharp image currently painted
  let lowSrc = $state<string | null>(null); // blurred placeholder (slow loads only)
  let showLow = $state(false);
  let vsrc = $state<string | null>(null); // video src (originals play directly)
  let failed = $state(false);
  let videoErr = $state(false);
  let epoch = 0; // bumps on every item change; stale async work checks it
  const SLOW_MS = 180; // how long a sharp load may take before we blur-up

  // ── H.264 proxy playback (clips the webview can't decode) ──
  let usingProxy = $state(false); // currently playing the converted preview
  let converting = $state(false);
  let proxyNote = $state<string | null>(null);
  // Live transcode progress, fed by the backend's activity events.
  let proxyPct = $derived.by(() => {
    const j = item ? activity.jobs[`proxy:${item.path}`] : undefined;
    return j && j.state === "running" && j.total > 0
      ? `${Math.round((j.done / j.total) * 100)}%`
      : "";
  });

  /** One-time ffmpeg convert to a cached H.264 preview, then play that. */
  async function convertAndPlay() {
    if (!item || converting) return;
    converting = true;
    proxyNote = null;
    const my = epoch;
    try {
      const p = await api.videoProxy(item.path);
      if (my === epoch) {
        usingProxy = true;
        videoErr = false;
        vsrc = api.fileSrc(p);
      }
    } catch (e) {
      if (my === epoch) proxyNote = `Couldn't convert this clip (${e})`;
    } finally {
      converting = false;
    }
  }

  // ── video trim state ──
  let vid = $state<HTMLVideoElement | null>(null);
  let paused = $state(true); // mirrors the element; default video behavior is paused
  let displayPaused = $derived(
    casting ? castPlayerState !== "PLAYING" && castPlayerState !== "BUFFERING" : paused,
  );
  let dur = $state(0);
  let cur = $state(0);
  // In/out segments, marked with [ and ] (rules: $lib/segments.ts). Saved to
  // the catalog as the clip's segments; an in still waiting for its out is
  // remembered for the session (per clip) but not saved.
  let marks = $state<MarkState>(emptyMarks());
  let segments = $derived(marks.segments);
  /** The marker picked for ←/→ frame nudges (seg -1 = the open in). */
  let selMark = $state<{ seg: number; edge: "in" | "out" } | null>(null);
  /** A brief note on the stage after [ or ] (the transport may be hidden). */
  let markToast = $state<string | null>(null);
  let markToastTimer: ReturnType<typeof setTimeout> | undefined;
  let menu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  let exporting = $state(false);
  let exportingSegments = $state(false);
  let exportNote = $state<string | null>(null);
  let probe = $state<MediaProbe | null>(null);
  let clipToolsOpen = $state(false);

  // ── live-decode scrub (WebCodecs) ─────────────────────────────────────────
  // The primary scrub path. Decodes the real frame under the cursor on demand
  // instead of painting a pre-built sprite, so scrubbing is full-resolution and
  // available the instant a clip opens — no Prepare, no cache, no progress bar.
  // Everything below degrades to the sprite path automatically if a clip can't
  // be indexed or its codec can't be decoded this way; the two are deliberately
  // independent so a failure is invisible rather than fatal.
  // Full rationale + measurements: docs/design/video-player-migration.md §10-11.
  // `$state.raw`, not `$state`: the template and `previewBox` read through this
  // so reassignment must be reactive, but the engine is an opaque class holding
  // a decoder and a 30k-entry sample table — deep-proxying it would be pure
  // overhead (and mutation of its internals is never what we react to).
  let engine = $state.raw<ScrubEngine | null>(null);
  let engineReady = $state(false);
  /// True from the moment we start indexing a clip until it succeeds or fails.
  /// Holds the sprite build back so the two paths never both run.
  let enginePending = false;
  let stageCanvas = $state<HTMLCanvasElement | null>(null);
  let prevCanvas = $state<HTMLCanvasElement | null>(null);
  /// Keeps the decoded still on screen after release until <video> has actually
  /// landed on that frame, so the hand-off shows no flicker or jump.
  let canvasHold = $state(false);

  /// Which hold a decode belongs to. Bumped every time the still is handed back
  /// to <video>, so a frame that finishes decoding AFTER the hand-off is
  /// discarded instead of re-covering the stage.
  ///
  /// This is the whole of the "video freezes on one frame while the audio keeps
  /// playing" bug (reported against nightly.2, intermittent, and sticky once it
  /// started on a clip). Clicking the timeline during playback races two async
  /// events: the exact-frame decode (~150 ms) and the element's `seeked`. When
  /// `seeked` won, it cleared the hold AND its 1500 ms safety timer — then the
  /// late frame raised the hold again with nothing left alive to ever lower it.
  /// Whether `seeked` won was a property of the file's seek latency, which is
  /// why it looked random but then stuck to one clip.
  let holdGen = 0;
  /// Safety net for the case where the element never fires `seeked` (it was
  /// already at that exact time), so the still can't sit there indefinitely.
  let holdTimer: ReturnType<typeof setTimeout> | undefined;
  /** Give the stage back to <video>: drop the still and invalidate any decode
   *  still in flight for it. */
  function releaseHold() {
    holdGen++;
    clearTimeout(holdTimer);
    canvasHold = false;
  }

  /** Paint the frame at `t` onto the full-stage canvas (drag/step scrub).
   *  `canvasHold` is raised INSIDE the callback — i.e. only once real pixels
   *  are on the canvas — so the still never flashes empty over the video. */
  function paintStage(t: number, exact = false) {
    const e = engine;
    if (!e) return;
    const gen = holdGen;
    e.request(t, exact, (f) => {
      // Stale: the stage went back to live video while this was decoding.
      // Dropping it is not a compromise — <video> has already landed on the
      // same frame, which is exactly what the still would have shown.
      if (gen !== holdGen) return;
      if (!stageCanvas) return;
      paintFrame(stageCanvas, f, stageW, stageH, e.index.rotation);
      canvasHold = true;
    });
  }
  // ── Glimpse: sweep the clip by its keyframes ──────────────────────────────
  // The culling problem this solves: a long clip's cover frame tells you almost
  // nothing, and dragging the playhead by hand to find out is work. Glimpse
  // flips through the clip's own keyframes fast enough to be quick and slow
  // enough to read — the way an editor thumbs through footage. It costs nothing
  // extra: the keyframes are already indexed and the decoder is already hot.
  let glimpsing = $state(false);
  let glimpseTimer: ReturnType<typeof setTimeout> | undefined;
  /// ~9 steps/sec. Faster reads as a smear; slower stops feeling like a sweep.
  const GLIMPSE_TICK_MS = 110;

  function stopGlimpse(land = true) {
    if (!glimpsing) return;
    glimpsing = false;
    clearTimeout(glimpseTimer);
    // Land properly: the exact frame, then hand back to <video> — same
    // treatment as releasing a drag, so stopping never leaves a rough frame up.
    if (land && vid && engineReady) {
      paintStage(cur, true);
      vid.currentTime = cur;
      releaseHoldSoon();
    }
  }

  export function toggleGlimpse() {
    if (glimpsing) {
      stopGlimpse();
      return;
    }
    const e = engine;
    const total = dur || e?.index.durationS || 0;
    if (!e || !engineReady || total <= 0) return;
    vid?.pause();
    glimpsing = true;
    // CONSTANT speed — a plain multiple of realtime, exactly like a player's
    // 2x/5x. 5x means a 20-second clip takes 4 seconds and a 10-minute clip
    // takes 2 minutes; the rate you see never depends on how long the clip is.
    //
    // It used to compress every clip into a fixed-length sweep instead (a
    // 4-second floor, so short and long clips ran at wildly different apparent
    // speeds). That is unlearnable — you could never build an instinct for what
    // you were watching. The owner asked for the player-style multiplier and he
    // is right.
    //
    // The picture still comes from keyframes, so at low multiples a tick can
    // land inside the keyframe already on screen and simply repeat it. That is
    // the honest trade: the CLOCK advances at exactly Nx, and the picture
    // refreshes as often as the clip's own keyframes allow.
    const perTick = settings.s.glimpseSpeed * (GLIMPSE_TICK_MS / 1000);
    const step = () => {
      if (!glimpsing) return;
      const next = cur + perTick;
      if (next >= total) {
        cur = total;
        stopGlimpse();
        return;
      }
      cur = next;
      paintStage(cur, false);
      glimpseTimer = setTimeout(step, GLIMPSE_TICK_MS);
    };
    step();
  }

  function releaseHoldSoon() {
    clearTimeout(holdTimer);
    holdTimer = setTimeout(releaseHold, 1500);
  }
  function onSeeked() {
    releaseHold();
    // A landed seek — the page throttles these before they reach the TV, so a
    // held shuttle trigger doesn't become a SEEK storm on the Chromecast.
    if (vid) ontransport({ kind: "seek", paused: vid.paused, time: vid.currentTime || 0 });
  }
  /** Paint the frame at `t` into the small hover thumbnail above the timeline. */
  function paintPreview(t: number) {
    const e = engine;
    if (!e) return;
    e.request(t, false, (f) => {
      if (prevCanvas) paintFrame(prevCanvas, f, previewBox.w, previewBox.h, e.index.rotation);
    });
  }

  // ── filmstrip scrub state (fallback path) ──
  let strip = $state<FilmstripInfo | null>(null);
  /// Whether the DENSE filmstrip build has been requested for the current item
  /// (cached loads don't count — this guards the expensive path only).
  let denseRequested = false;
  // `strip.src` is always already an asset URL — every path that assigns `strip`
  // hydrates it once (the shared loader does it; the cached-only reads below do
  // it explicitly). Converting again here would mangle it.
  let stripSrc = $derived(strip ? strip.src : null);
  const hydrate = (s: FilmstripInfo) => ({ ...s, src: api.fileSrc(s.src) });
  let posterSrc = $state<string | null>(null); // cached poster: instant first paint
  let preview = $state<number | null>(null); // fraction 0..1 to preview, or null
  let scrubbing = $state(false);
  /// The decoded still covers the stage while dragging, and stays up after
  /// release until <video> has landed on the same frame (declared here so it
  /// sits after `scrubbing`; Svelte's checker rejects a $derived that reads a
  /// `let` declared below it).
  let liveScrubActive = $derived(engineReady && canvasHold);
  let trackEl = $state<HTMLDivElement | null>(null);
  let infoVisible = $state(false);
  let pendingSeek: number | null = null;
  let seekRAF = 0;
  let resumeAfterScrub = false;
  let lastSeekAt = 0;
  let seekIdleTimer: ReturnType<typeof setTimeout> | undefined;
  const SEEK_THROTTLE_MS = 55;
  // The hover thumbnail floating above the timeline. Sized by BOTH edges: a
  // fixed width alone made portrait clips tower over the bar (a 9:16 clip at
  // 200px wide is 356px tall — it covered a third of the picture), so the height
  // is capped and the width derived from it when the clip is taller than wide.
  const PREVIEW_W = 200;
  const PREVIEW_MAX_H = 132;
  let previewBox = $derived.by(() => {
    // Aspect comes from the decoder's index when live-decode scrub is running
    // (rotation-aware — a portrait phone clip is stored landscape), else from
    // the sprite tile.
    const idx = engineReady ? engine?.index : null;
    const aspect = idx
      ? (idx.rotation === 90 || idx.rotation === 270
          ? idx.codedHeight / Math.max(1, idx.codedWidth)
          : idx.codedWidth / Math.max(1, idx.codedHeight))
      : strip
        ? strip.tile_w / Math.max(1, strip.tile_h)
        : 0;
    if (!aspect) return { w: 0, h: 0 };
    let w = PREVIEW_W;
    let h = w / aspect;
    if (h > PREVIEW_MAX_H) {
      h = PREVIEW_MAX_H;
      w = h * aspect;
    }
    return { w: Math.round(w), h: Math.round(h) };
  });

  // ── auto-hiding transport ─────────────────────────────────────────────────
  // In minimal mode (default) the transport collapses to a thin, unobtrusive
  // progress line at the bottom of the stage and only unfolds into the full
  // bar when you reach for it — pointer in the bottom band, an active scrub, or
  // Clip tools open. This keeps the picture edge-to-edge (Focus AND full-screen)
  // instead of a permanent thick bar eating vertical space. Turn the setting off
  // to pin the bar open like a classic player.
  let bottomHover = $state(false);
  let minimalBar = $derived(settings.s.minimalVideoBar);
  let showTransport = $derived(
    !minimalBar || bottomHover || scrubbing || clipToolsOpen,
  );
  function onStagePointerMove(e: PointerEvent) {
    const el = stageEl;
    if (!el) return;
    const r = el.getBoundingClientRect();
    // Bottom ~26% of the stage (at least 90px) is the reveal band.
    const band = Math.max(90, r.height * 0.26);
    bottomHover = e.clientY >= r.bottom - band;
  }
  function onStagePointerLeave() {
    bottomHover = false;
  }

  // Full-canvas scrub overlay geometry: the sprite cell keeps the clip's aspect
  // inside whatever the video stage measures (same approach as Thumb's scrubBox).
  let stageEl = $state<HTMLDivElement | null>(null);
  let stageW = $state(1);
  let stageH = $state(1);
  $effect(() => {
    const el = stageEl;
    if (!el) return;
    const measure = () => {
      const r = el.getBoundingClientRect();
      stageW = Math.max(1, r.width);
      stageH = Math.max(1, r.height);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });
  let stageFrame = $derived.by(() => {
    if (!strip) return { w: 0, h: 0 };
    const aspect = strip.tile_w / Math.max(1, strip.tile_h);
    const boxAspect = stageW / stageH;
    if (aspect >= boxAspect) return { w: stageW, h: stageW / aspect };
    return { w: stageH * aspect, h: stageH };
  });

  $effect(() => {
    const it = item;
    const my = ++epoch;
    failed = false;
    videoErr = false;
    usingProxy = false;
    converting = false;
    proxyNote = null;
    paused = !settings.s.videoAutoplay;
    dur = 0;
    cur = 0;
    marks = emptyMarks();
    selMark = null;
    markToast = null;
    menu = null;
    exportNote = null;
    probe = null;
    clipToolsOpen = false;
    // Reset the live-decode flags unconditionally, not just in the video
    // branch's cleanup: stepping from a video to a PHOTO runs no cleanup for
    // the photo, and a stale `enginePending` would block the sprite fallback
    // for every clip after it.
    engineReady = false;
    enginePending = false;
    releaseHold();
    glimpsing = false;
    clearTimeout(glimpseTimer);
    strip = null;
    preview = null;
    scrubbing = false;
    showLow = false;
    lowSrc = null;
    if (!it) {
      curSrc = null;
      vsrc = null;
      return;
    }
    if (it.kind === "video") {
      curSrc = null;
      vsrc = null;
      posterSrc = null;
      // Segments are the one model now. A clip trimmed under the old single
      // in/out (the `trims` table) becomes one segment, once.
      Promise.all([api.getVideoSegments(it.path).catch(() => [] as VideoSegment[]), api.getTrim(it.path).catch(() => null)]).then(([segs, trim]) => {
        if (my !== epoch) return;
        let list = normalize(segs);
        if (!list.length && trim && trim[1] > trim[0]) {
          list = normalize([{ in_s: trim[0], out_s: trim[1] }]);
          if (list.length) {
            void api.setVideoSegments(it.path, list);
            api.clearTrim(it.path);
          }
        }
        marks = { segments: list, open: openIns.get(it.path) ?? null, last: null };
      });
      api.probeMediaInfo(it.path).then((p) => {
        if (my === epoch) probe = p;
      }).catch(() => {});
      // Poster first: the cached frame paints the stage instantly while the
      // real <video> element is still opening the file. Focus view uses the
      // SHARP ~1280px poster so the first frame isn't a pixelated 480px blowup
      // on a full-screen stage (the grid keeps the light 480px one).
      loadVideoPosterHires(it.path).then((s) => {
        if (my === epoch && s) posterSrc = s;
      });
      // Scrub filmstrip: CACHED-ONLY on open. Anything already built (grid
      // hover, Prepare, an earlier session) paints for free — coarse strip
      // first, dense strip over it if it exists. The EXPENSIVE dense build
      // never fires on open: it is gated on the Live Scrub setting AND first
      // scrub intent (hovering/touching the seek bar, or key/controller
      // seeks) — see ensureFilmstrip(). The nightly.3 rework built it
      // unconditionally here, which on an HDD library meant ~a minute of
      // unwanted ffmpeg work per clip with Live Scrub OFF (2026-07-20 RCA).
      denseRequested = false;
      // Live-decode scrub: index the clip and warm a decoder. Costs a few MB of
      // reads and ~300 ms even on a multi-GB file (the moov hunt skips the
      // mdat), so it runs unconditionally on open rather than waiting for
      // "scrub intent" the way sprite builds must. If it succeeds, the sprite
      // build below never fires for this clip.
      if (settings.s.liveDecodeScrub) {
        enginePending = true;
        const openedAt = performance.now();
        ScrubEngine.open(it.path, () => my !== epoch)
          .then((e) => {
            if (my !== epoch) {
              e.close();
              return;
            }
            engine = e;
            engineReady = true;
            enginePending = false;
            // One line per clip, so any machine's log says plainly whether the
            // hardware decoder took it. This is the only way to answer "does
            // live scrub work on that other laptop" without being sat at it —
            // GPU support for HEVC Main 10 varies by iGPU generation, and a
            // rejection is otherwise completely silent by design.
            api.logNote(
              `scrub-engine OK codec=${e.index.codec} ${e.index.codedWidth}x${e.index.codedHeight}` +
                ` keyframes=${e.index.syncIdx.length} open=${Math.round(performance.now() - openedAt)}ms`,
            );
          })
          .catch((err) => {
            // Unsupported codec/container, or a stale open. Release the sprite
            // path we were holding back, so the clip still scrubs the old way.
            if (my !== epoch) return;
            engineReady = false;
            enginePending = false;
            api.logNote(`scrub-engine FALLBACK ${it.ext} — ${err?.message ?? err}`);
            if (settings.s.liveScrub) ensureFilmstrip();
          });
      }
      api.videoScrubstripCached(it.path).then((s) => {
        if (my === epoch && s && !strip) strip = hydrate(s);
      });
      api.videoFilmstripCached(it.path).then((f) => {
        if (my === epoch && f) strip = hydrate(f);
      });
      api
        .loupeSrc(it.path)
        .then((p) => {
          if (my === epoch) vsrc = api.fileSrc(p);
        })
        .catch(() => {
          if (my === epoch) failed = true;
        });
      // Moving to another item cancels this clip's filmstrip build mid-flight —
      // flipping quickly through a folder of videos must not stack up builds.
      return () => {
        cancelVideoFilmstrip(it.path);
        // Free the decoder + any frame it still holds the moment we leave this
        // clip — a 4K VideoFrame is ~12 MB of GPU memory.
        engine?.close();
        engine = null;
        engineReady = false;
        releaseHold();
        glimpsing = false;
        clearTimeout(glimpseTimer);
      };
    }
    if (it.kind === "other") {
      curSrc = null;
      vsrc = null;
      failed = true;
      return;
    }
    // Image/RAW. Keep the previous photo painted; swap only when the new sharp
    // preview is DECODED (img.decode), so the swap is a single clean frame. If
    // the sharp load is slow (cold cache), fall back to the classic blur-up so
    // the user still gets instant feedback.
    vsrc = null;
    let sharpDone = false;
    const slow = setTimeout(() => {
      if (my !== epoch || sharpDone) return;
      loadThumb(it.path, 320).then((s) => {
        if (my === epoch && !sharpDone && s) {
          lowSrc = s;
          showLow = true;
          curSrc = null; // drop the stale previous photo under the placeholder
        }
      });
    }, SLOW_MS);
    (async () => {
      try {
        const p = await api.loupeSrc(it.path);
        if (my !== epoch) return;
        const url = api.fileSrc(p);
        const img = new Image();
        img.decoding = "async";
        img.src = url;
        try {
          await img.decode();
        } catch {
          /* decode() can reject for valid images — paint anyway */
        }
        if (my !== epoch) return;
        sharpDone = true;
        curSrc = url;
        showLow = false;
        lowSrc = null;
      } catch {
        if (my === epoch) {
          curSrc = null;
          failed = true;
        }
      }
    })();
    return () => clearTimeout(slow);
  });

  // With Live Scrub ON, the strip build starts the moment a clip OPENS — not
  // when the pointer happens to graze the seek bar. Waiting for "scrub intent"
  // was a nightly.5 measure against building strips nobody asked for, but the
  // toggle already says whether they're wanted: with it on, reaching the
  // timeline and finding a 10-second build still ahead of you is the exact
  // friction the feature exists to remove. Live Scrub OFF still builds nothing.
  // (Separate effect, not folded into the open effect above, so flipping the
  // setting mid-clip doesn't tear down and re-open the video.)
  $effect(() => {
    const it = item;
    if (!it || it.kind !== "video" || !settings.s.liveScrub) return;
    ensureFilmstrip();
  });

  /// Live build feedback for the dense Focus filmstrip, so a clip that's still
  /// extracting frames says so instead of looking like a dead seek bar.
  let stripJob = $derived.by(() => {
    if (!item || strip) return null;
    const j = activity.jobs[`strip:${item.path}`];
    return j && j.state === "running" ? j : null;
  });

  $effect(() => {
    infoVisible = showInfo;
  });

  function onMeta() {
    if (vid) dur = vid.duration || 0;
  }

  // ── playback (exposed to the page's global key handler) ──
  export function togglePlay() {
    if (casting) {
      oncasttoggle();
      return;
    }
    if (!vid) return;
    // Play/pause always wins over a sweep in progress — pressing Space to stop
    // a Glimpse and start watching is the obvious expectation.
    if (glimpsing) {
      stopGlimpse();
      return;
    }
    if (vid.paused) vid.play().catch(() => {});
    else vid.pause();
  }
  // Cast mode is intentionally single-output: the TV owns playback and audio.
  // Muting in markup closes the autoplay race; this effect then keeps the local
  // decoder parked on its current/first frame.
  $effect(() => {
    const video = vid;
    if (casting && video && !video.paused) video.pause();
  });
  export function seekBy(d: number) {
    if (!vid) return;
    ensureFilmstrip(); // key/controller seeking is scrub intent too
    const max = dur || strip?.duration || vid.duration || 0;
    if (max <= 0) return;
    // Optimistic: step from the last COMMANDED position (`cur`), not the
    // decoder's current frame — holding the key / controller trigger then
    // shuttles smoothly instead of stalling on each slow HEVC seek. Fast
    // (keyframe) seeks paint while moving; a trailing accurate seek lands the
    // exact frame when the presses stop.
    let t = cur + d;
    if (t < 0) t = 0;
    if (t > max) t = max;
    cur = t;
    seekTo(t / max);
    clearTimeout(seekIdleTimer);
    seekIdleTimer = setTimeout(() => seekTo(t / max, true), 240);
  }
  export function setInPoint() {
    markInHere();
  }
  export function setOutPoint() {
    markOutHere();
  }
  /** ←/→ while a marker is picked: move it one frame (Shift: ten). Returns
   *  whether it took the key (else the page moves to the next item). */
  export function nudgeMark(dir: -1 | 1, big = false): boolean {
    const m = selMark;
    if (!m || !item) return false;
    const step = (big ? 10 : 1) / frameRate();
    if (m.seg < 0) {
      if (marks.open == null) return false;
      marks = moveOpen(marks, snapFrame(marks.open + dir * step, frameRate()), dur);
      showFrame(marks.open ?? 0);
      openIns.set(item.path, marks.open ?? 0);
      return true;
    }
    const s = marks.segments[m.seg];
    if (!s) return false;
    const t = snapFrame((m.edge === "in" ? s.in_s : s.out_s) + dir * step, frameRate());
    marks = moveEdge(marks, m.seg, m.edge, t, dur);
    const now = marks.segments[m.seg];
    showFrame(m.edge === "in" ? now.in_s : now.out_s);
    persistMarks();
    return true;
  }
  /** Esc: let go of a picked marker first. */
  export function clearMarkSelection(): boolean {
    if (!selMark && !menu) return false;
    selMark = null;
    menu = null;
    return true;
  }

  // ── timeline scrub: hover previews a frame, drag seeks the real video ──
  function fracFromEvent(e: PointerEvent): number {
    if (!trackEl) return 0;
    const r = trackEl.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  }
  function applySeek(frac: number, final = false) {
    const d = dur || engine?.index.durationS || strip?.duration || 0;
    // ── live-decode path ────────────────────────────────────────────────────
    // While MOVING we never touch `currentTime`: each of those is a precise
    // seek (pipeline flush + decode from the previous keyframe + audio
    // re-sync), and thirty in a row is exactly why dragging used to lag the
    // cursor. Instead we paint the real keyframe under the cursor, ~20 ms each,
    // with the engine coalescing so the newest position always wins.
    // On RELEASE we decode the exact frame, keep it on screen, and let the
    // element perform ONE precise seek underneath; `seeked` swaps back to live
    // video showing the same frame, so the hand-off is invisible.
    if (engineReady && d > 0) {
      const t = frac * d;
      cur = t;
      paintStage(t, final);
      if (final && vid) {
        vid.currentTime = t;
        releaseHoldSoon();
      }
      return;
    }
    if (vid && d > 0) {
      const t = frac * d;
      cur = t;
      const now = performance.now();
      if (!final && now - lastSeekAt < SEEK_THROTTLE_MS) return;
      lastSeekAt = now;
      // While MOVING, keyframe-fast seeks keep the picture flowing; the FINAL
      // seek (drag release / presses stopped) is frame-accurate.
      if (!final && "fastSeek" in vid && typeof vid.fastSeek === "function") {
        try {
          vid.fastSeek(t);
          return;
        } catch {
          /* fall back */
        }
      }
      vid.currentTime = t;
    }
  }
  function seekTo(frac: number, final = false) {
    if (final) {
      if (seekRAF) cancelAnimationFrame(seekRAF);
      seekRAF = 0;
      pendingSeek = null;
      applySeek(frac, true);
      return;
    }
    pendingSeek = frac;
    if (seekRAF) return;
    seekRAF = requestAnimationFrame(() => {
      const next = pendingSeek;
      pendingSeek = null;
      seekRAF = 0;
      if (next != null) applySeek(next);
    });
  }
  /// Build the dense filmstrip on first scrub INTENT — and only when Live
  /// Scrub is enabled. Watching a clip costs nothing; the build starts the
  /// moment you actually reach for the timeline (hover, drag, key/controller
  /// seek). Cached strips are painted by the open effect regardless.
  function ensureFilmstrip() {
    const it = item;
    if (!it || it.kind !== "video") return;
    // Live-decode scrub makes the sprite sheet redundant in Focus — that was
    // the whole point of the migration. Hold off while the engine is still
    // opening too, or a clip would pay for a sprite build that finishes just as
    // the decoder makes it pointless. The catch handler above re-enters here if
    // the engine can't take the clip. (The GRID still uses sprites: one decoder
    // per tile is not viable.)
    if (engineReady || enginePending) return;
    if (!settings.s.liveScrub || denseRequested) return;
    denseRequested = true;
    const my = epoch;
    // Through the SHARED loader, not a direct invoke: if the grid tile already
    // started this clip's sprite (you armed it, then double-clicked in), this
    // joins that in-flight build instead of racing it. A direct invoke took a
    // fresh cancel-token on the backend, which killed the grid's half-finished
    // extraction and started over — the "it restarts from 10%" report.
    loadVideoFilmstrip(it.path)
      .then((f) => {
        if (my === epoch && f) strip = f;
      })
      .catch(() => {});
  }
  function onTrackDown(e: PointerEvent) {
    stopGlimpse(false); // taking the playhead by hand supersedes the sweep
    selMark = null; // and lets go of a picked in/out marker
    ensureFilmstrip();
    scrubbing = true;
    resumeAfterScrub = !!vid && !vid.paused;
    vid?.pause();
    api.cancelWarm();
    try {
      trackEl?.setPointerCapture(e.pointerId);
    } catch {}
    const f = fracFromEvent(e);
    preview = f;
    seekTo(f);
  }
  function onTrackMove(e: PointerEvent) {
    ensureFilmstrip(); // first hover over the timeline = scrub intent
    const f = fracFromEvent(e);
    preview = f;
    if (scrubbing) seekTo(f);
    // Hover (not dragging): decode the frame into the floating thumbnail. Same
    // engine, same coalescing — no sprite sheet needed for hover either.
    else if (engineReady) {
      const d = dur || engine?.index.durationS || 0;
      if (d > 0) paintPreview(f * d);
    }
  }
  function onTrackUp(e: PointerEvent) {
    if (!scrubbing) return;
    scrubbing = false;
    const f = fracFromEvent(e);
    preview = f;
    seekTo(f, true);
    if (resumeAfterScrub) vid?.play().catch(() => {});
    resumeAfterScrub = false;
    try {
      trackEl?.releasePointerCapture(e.pointerId);
    } catch {}
  }
  function onTrackLeave() {
    if (!scrubbing) preview = null;
  }
  /** Sprite background-position (%) for the frame nearest `frac`. */
  function cellPos(frac: number): { x: number; y: number } {
    if (!strip) return { x: 0, y: 0 };
    const idx = Math.min(
      strip.count - 1,
      Math.max(0, Math.round(frac * (strip.count - 1))),
    );
    const col = idx % strip.cols;
    const row = Math.floor(idx / strip.cols);
    return {
      x: strip.cols > 1 ? (col / (strip.cols - 1)) * 100 : 0,
      y: strip.rows > 1 ? (row / (strip.rows - 1)) * 100 : 0,
    };
  }
  function onTime() {
    if (!vid) return;
    cur = vid.currentTime || 0;
    // Invariant, enforced rather than assumed: a PLAYING video is never sitting
    // under the scrub still. Every deliberate hold happens with the element
    // paused (drag, Glimpse) or lasts only until `seeked`, so if we are here
    // with the video running and no gesture in progress, the hold is stale.
    // `timeupdate` fires ~4x/second, so any path that leaks a hold self-heals
    // in ~250 ms instead of freezing the picture until the clip is changed.
    if (canvasHold && !scrubbing && !glimpsing && !vid.paused) releaseHold();
  }
  // ── in/out segments ([ and ], markers, menus) ─────────────────────────────
  function frameRate(): number {
    const f = probe?.fps ?? 0;
    return f > 1 ? f : 30;
  }
  function toast(note: string | null) {
    if (!note) return;
    markToast = note;
    clearTimeout(markToastTimer);
    markToastTimer = setTimeout(() => (markToast = null), 2600);
  }
  function persistMarks() {
    if (!item) return;
    const path = item.path;
    api.setVideoSegments(path, marks.segments.map((x) => ({ ...x }))).catch((e) => {
      // Loud on purpose: in/out writes once failed silently for months.
      exportNote = `Couldn't save the segments (${e})`;
    });
    if (marks.open != null) openIns.set(path, marks.open);
    else openIns.delete(path);
    reportRanges();
  }
  function markInHere() {
    if (!item || item.kind !== "video") return;
    const r = ruleIn(marks, snapFrame(cur, frameRate()), dur || cur + 1);
    toast(r.note);
    if (!r.changed) return;
    marks = r.state;
    selMark = r.state.open != null ? { seg: -1, edge: "in" } : r.state.last != null ? { seg: r.state.last, edge: "in" } : null;
    persistMarks();
  }
  function markOutHere() {
    if (!item || item.kind !== "video") return;
    const r = ruleOut(marks, snapFrame(cur, frameRate()), dur || cur);
    toast(r.note);
    if (!r.changed) return;
    marks = r.state;
    selMark = r.state.last != null ? { seg: r.state.last, edge: "out" } : null;
    persistMarks();
  }
  /** Show the exact frame at `t` (a marker being placed) without playing. */
  function showFrame(t: number) {
    cur = t;
    if (engineReady) paintStage(t, true);
    if (vid) {
      vid.currentTime = t;
      if (engineReady) releaseHoldSoon();
    }
  }
  function jumpToSegment(i: number) {
    const s2 = marks.segments[i];
    if (!s2) return;
    selMark = null;
    showFrame(s2.in_s);
  }
  // Play the marked parts one after another (Clip tools → Play segments).
  let playingSegs = $state(false);
  function playSegments() {
    if (!vid || !marks.segments.length) return;
    playingSegs = true;
    showFrame(marks.segments[0].in_s);
    vid.play().catch(() => {});
  }
  $effect(() => {
    // While playing the segments: at each out, jump to the next in; stop after
    // the last. (Reads `cur`, which timeupdate keeps current.)
    if (!playingSegs || !vid) return;
    const t = cur;
    const segs = marks.segments;
    const i = segs.findIndex((x) => t >= x.in_s - 0.05 && t < x.out_s);
    if (i >= 0) return;
    const next = segs.find((x) => x.in_s > t);
    if (next && segs.some((x) => x.out_s <= t + 0.05)) {
      vid.currentTime = next.in_s;
    } else {
      playingSegs = false;
      vid.pause();
    }
  });

  // Marker drag: frame-accurate, showing the exact frame as it moves.
  let markDrag: { seg: number; edge: "in" | "out"; id: number } | null = null;
  function onMarkDown(e: PointerEvent, seg: number, edge: "in" | "out") {
    e.stopPropagation();
    e.preventDefault();
    if (e.button !== 0) return;
    selMark = { seg, edge };
    vid?.pause();
    markDrag = { seg, edge, id: e.pointerId };
    try {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {}
  }
  function onMarkMove(e: PointerEvent) {
    if (!markDrag || !trackEl) return;
    const r = trackEl.getBoundingClientRect();
    const t = snapFrame(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * dur, frameRate());
    if (markDrag.seg < 0) {
      marks = moveOpen(marks, t, dur);
      showFrame(marks.open ?? t);
    } else {
      marks = moveEdge(marks, markDrag.seg, markDrag.edge, t, dur);
      const sg = marks.segments[markDrag.seg];
      if (sg) showFrame(markDrag.edge === "in" ? sg.in_s : sg.out_s);
    }
  }
  function onMarkUp(e: PointerEvent) {
    if (!markDrag) return;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {}
    markDrag = null;
    persistMarks();
  }

  function openMarkMenu(e: MouseEvent, seg: number, edge: "in" | "out" | null) {
    e.preventDefault();
    e.stopPropagation();
    const sg = marks.segments[seg];
    const entries: MenuEntry[] = [];
    if (seg < 0) {
      entries.push({ label: "Remove this in point", icon: "⌫", action: () => { marks = { ...marks, open: null }; selMark = null; persistMarks(); } });
    } else if (sg) {
      if (edge === "in") {
        entries.push({ label: seg > 0 ? `Remove in point (start where segment ${seg} ends)` : "Remove in point (start at the beginning)", icon: "⌫", action: () => { marks = removeIn(marks, seg); persistMarks(); } });
      } else if (edge === "out") {
        entries.push({ label: marks.open == null ? "Remove out point (press ] to set a new one)" : "Remove out point (run to the next segment)", icon: "⌫", action: () => { marks = removeOut(marks, seg, dur); selMark = null; persistMarks(); } });
      }
      entries.push(
        { label: "Play this segment", icon: "▶", action: () => { jumpToSegment(seg); vid?.play().catch(() => {}); } },
        { label: "Save this segment as a clip", icon: "⤓", action: () => void saveSegment(seg) },
        { separator: true },
        { label: `Remove segment ${seg + 1}`, icon: "✕", danger: true, action: () => { marks = removeSeg(marks, seg); selMark = null; persistMarks(); } },
      );
    }
    if (marks.segments.length > 1 || (marks.segments.length && marks.open != null)) {
      entries.push({ label: "Remove all segments", icon: "✕", danger: true, action: () => { marks = emptyMarks(); selMark = null; persistMarks(); } });
    }
    menu = { x: e.clientX, y: e.clientY, entries };
  }

  async function saveSegment(i: number) {
    const sg = marks.segments[i];
    if (!item || !sg || exporting) return;
    exporting = true;
    exportNote = `Saving segment ${i + 1}…`;
    try {
      const out = await api.trimVideo(item.path, sg.in_s, sg.out_s);
      exportNote = `Saved ${out.split(/[\\/]/).pop()}`;
      api.reveal(out);
      onchanged(out);
    } catch (e) {
      exportNote = `Couldn't save it (${e})`;
    } finally {
      exporting = false;
    }
  }

  // Drag to the Edit window: the grip takes the clip with all its segments;
  // a segment's span takes just that segment.
  function dragClip(e: DragEvent, only: number | null) {
    if (!item || !e.dataTransfer) return;
    const ranges = only == null ? marks.segments.map((x) => ({ ...x })) : marks.segments[only] ? [{ ...marks.segments[only] }] : [];
    const ref: ClipRef = { path: item.path, name: item.name, kind: item.kind, ext: item.ext, mtime: item.mtime, size: item.size, ranges };
    e.dataTransfer.effectAllowed = "copy";
    e.dataTransfer.setData("application/x-foxcull-clips", JSON.stringify([ref]));
    e.dataTransfer.setData("text/plain", item.path);
    void api.stashSet("drag", [ref]);
  }

  async function exportSegments() {
    if (!item || exportingSegments || !segments.length) return;
    const sortedSegments = () => marks.segments.map((x) => ({ ...x }));
    exportingSegments = true;
    exportNote = "Exporting subclips...";
    try {
      const r = await api.exportVideoSegments(item.path, sortedSegments());
      if (r.exported.length) {
        exportNote = r.failed.length
          ? `Saved ${r.exported.length}; ${r.failed.length} failed${r.errors[0] ? ` (${r.errors[0]})` : ""}`
          : `Saved ${r.exported.length} subclip${r.exported.length === 1 ? "" : "s"}`;
        api.reveal(r.exported[0]);
        onchanged(r.exported[0]);
      } else {
        exportNote = `No subclips saved${r.errors[0] ? `: ${r.errors[0]}` : ""}`;
      }
    } catch (e) {
      exportNote = `Couldn't export subclips (${e})`;
    } finally {
      exportingSegments = false;
    }
  }

  function fmt(s: number): string {
    if (!isFinite(s) || s < 0) s = 0;
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, "0")}`;
  }
  function fmtSize(n: number): string {
    if (!n) return "-";
    if (n < 1024 * 1024) return `${Math.max(1, Math.round(n / 1024))} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
    return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }
  let pct = (s: number) => (dur > 0 ? (s / dur) * 100 : 0);
  let infoRows = $derived.by(() => {
    if (!item) return [];
    const rows = [
      item.name,
      `${item.kind.toUpperCase()} · ${item.ext.toUpperCase()} · ${fmtSize(item.size)}`,
    ];
    if (item.kind === "video" && probe) {
      const res = probe.width && probe.height ? `${probe.width}x${probe.height}` : "";
      const fps = probe.fps ? `${Math.round(probe.fps)}fps` : "";
      rows.push([fmt(probe.duration), res, fps, probe.codec, probe.camera].filter(Boolean).join(" · "));
    }
    rows.push(new Date(item.mtime * 1000).toLocaleString());
    return rows;
  });
</script>

<div class="loupe">
  {#if !item}
    <div class="empty">No selection</div>
  {:else if item.missing}
    <!-- A "?" catalog entry: there is nothing to render, and saying so beats a
         silent black frame. The relink actions live in the right-click menu. -->
    <div class="empty vfail">
      <p class="vt">{item.name}</p>
      <p>This file isn't where the catalog expects it — its ratings, labels, tags and events are still here.</p>
      <p class="vpath">{item.path}</p>
      <p>Right-click it (in the grid or here) to locate the file, locate the folder it moved to, or forget the entry.</p>
    </div>
  {:else if item.kind === "video"}
    {#if videoErr}
      <div class="empty vfail">
        <p class="vt">{item.name}</p>
        <p>This clip can't play in-app — likely HEVC/H.265 this machine has no codec for.</p>
        <button class="obtn" onclick={convertAndPlay} disabled={converting}>
          {converting ? `⏳ Converting…${proxyPct ? ` ${proxyPct}` : ""}` : "▶ Convert & play here"}
        </button>
        <p class="subnote">
          One-time: the bundled ffmpeg makes an H.264 preview, cached on the drive.
          The original file is never touched.
        </p>
        <button class="obtn ghost" onclick={() => item && api.openExternal(item.path)}>
          Open in system player instead
        </button>
        {#if proxyNote}<p class="subnote err">{proxyNote}</p>{/if}
      </div>
    {:else if vsrc}
      <div class="vwrap">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="stagewrap"
          bind:this={stageEl}
          onpointermove={onStagePointerMove}
          onpointerleave={onStagePointerLeave}
        >
        <!-- svelte-ignore a11y_media_has_caption -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <video
          bind:this={vid}
          src={vsrc}
          poster={posterSrc ?? undefined}
          autoplay={settings.s.videoAutoplay && !casting}
          muted={casting}
          preload="auto"
          playsinline
          onclick={togglePlay}
          onloadedmetadata={onMeta}
          ontimeupdate={onTime}
          onseeked={onSeeked}
          onplay={() => {
            paused = false;
            ontransport({ kind: "play", paused: false, time: vid?.currentTime ?? cur });
          }}
          onpause={() => {
            paused = true;
            ontransport({ kind: "pause", paused: true, time: vid?.currentTime ?? cur });
          }}
          onerror={() => {
            // Only a REAL decode/format failure shows the fallback card. An
            // aborted load (we switched clips mid-load) also fires `error` —
            // treating that as failure flashed the "can't play HEVC" card for
            // every clip that was actually about to play fine.
            const code = vid?.error?.code;
            if (!code || code === MediaError.MEDIA_ERR_ABORTED) return;
            // If a converted H.264 preview is already cached for this clip,
            // switch to it silently instead of asking again.
            if (item && !usingProxy) {
              const my = epoch;
              const p = item.path;
              api.videoProxyCached(p).then((cached) => {
                if (my !== epoch) return;
                if (cached) {
                  usingProxy = true;
                  vsrc = api.fileSrc(cached);
                } else {
                  videoErr = true;
                }
              });
            } else {
              videoErr = true;
            }
          }}
        ></video>
        <!-- Live-decode scrub surface. Always in the DOM (so there is always a
             canvas to paint into — a conditional one would be null exactly when
             the first frame arrives) and revealed only once it holds real
             pixels. It shows the frame under the cursor while dragging, and
             stays up after release until <video> has landed on the same frame,
             so the swap back is invisible. -->
        <!-- No timestamp pill over the picture: it floated in the middle of the
             frame while dragging and read as a glitch, not a readout. The
             transport's own clock (bolder since nightly.3) carries the time. -->
        <div class="liveScrub" class:shown={liveScrubActive}>
          <canvas bind:this={stageCanvas}></canvas>
        </div>
        <!-- Paused state, YouTube-style: a scrim plus one big play button, so a
             stopped clip reads as "paused video" rather than "a slightly odd
             photo". Hidden while scrubbing or glimpsing — during those the user
             is watching frames go by and a button over the middle is in the way.
             It is a real button, not decoration: the whole picture already
             toggles play on click, and this gives that affordance a target. -->
        {#if displayPaused && !scrubbing && !glimpsing}
          <button class="playOverlay" onclick={togglePlay} title="Play (Space)" aria-label="Play">
            <span class="poIcon">
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.2v13.6L19 12z" /></svg>
            </span>
          </button>
        {/if}
        {#if !engineReady && scrubbing && preview != null && strip && stripSrc}
          <!-- Final Cut-style drag scrub: the sprite frame under the cursor
               paints the WHOLE stage instantly (decode-free) while the real
               decoder chases underneath; releasing lands the accurate frame. -->
          {@const c = cellPos(preview)}
          <div class="scrubStage">
            <div
              class="scrubFrame"
              style="width:{stageFrame.w}px; height:{stageFrame.h}px;
                     background-image:url('{stripSrc}');
                     background-size:{strip.cols * 100}% {strip.rows * 100}%;
                     background-position:{c.x}% {c.y}%;"
            ></div>
          </div>
        {/if}
        {#if usingProxy}
          <span class="proxytag" title="The original couldn't decode in-app; you're watching the cached H.264 conversion. Trim still cuts the original.">converted preview</span>
        {/if}
        {#if markToast}<div class="marktoast" aria-live="polite">{markToast}</div>{/if}
        {#if stripJob}
          <span class="stripbuild" title="Extracting preview frames so scrubbing this clip is instant. Playback is unaffected.">
            scrub preview{stripJob.total > 0 ? ` ${Math.round((stripJob.done / stripJob.total) * 100)}%` : "…"}
          </span>
        {/if}
        <!-- Collapsed state: a thin, unobtrusive progress line at the very bottom
             so you sense position/length without any bar eating the picture. -->
        {#if minimalBar && !showTransport}
          <div class="thinline">
            {#each segments as sg, i (i)}<div class="thinseg" style="left:{pct(sg.in_s)}%; width:{Math.max(0.4, pct(sg.out_s) - pct(sg.in_s))}%"></div>{/each}
            <div class="thinfill" style="width:{pct(cur)}%"></div>
          </div>
        {/if}
        <div class="trim" class:shown={showTransport}>
          <!-- Compact single-row transport: play, time, inline scrubber, then the
               Info + Clip tools toggles. Trim/mark/export controls only unfold
               below when Clip tools is open. Overlays the bottom of the stage;
               reveals on hover in minimal mode (default), pinned otherwise. -->
          <div class="playrow">
            <button class="pp" onclick={togglePlay} title={displayPaused ? "Play (Space)" : "Pause (Space)"}>
              {displayPaused ? "▶" : "⏸"}
            </button>
            <button
              class="pp glimpse"
              class:on={glimpsing}
              onclick={toggleGlimpse}
              disabled={!engineReady}
              title={engineReady
                ? (glimpsing ? "Stop Glimpse (Ctrl+Space)" : "Glimpse — sweep the clip's keyframes to see what's in it (Ctrl+Space)")
                : "Glimpse needs the live decoder, which this clip doesn't support"}
            >{glimpsing ? "⏹" : "⏩"}</button>
            <span class="time"><b>{fmt(cur)}</b><span class="sep">/</span>{fmt(dur)}</span>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="track"
              class:scrubbing
              bind:this={trackEl}
              title="Drag to scrub · Space play · , . step 5s"
              onpointerdown={onTrackDown}
              onpointermove={onTrackMove}
              onpointerup={onTrackUp}
              onpointerleave={onTrackLeave}
            >
              <!-- Marked segments: a highlighted span each, with an in and an
                   out marker. Click a span to jump to it, drag it to the Edit
                   window for just that part, right-click for more. Markers
                   drag frame by frame (and ←/→ nudge the picked one). -->
              {#each segments as sg, i (i)}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div
                  class="seg"
                  class:last={marks.last === i}
                  style="left:{pct(sg.in_s)}%; width:{Math.max(0.6, pct(sg.out_s) - pct(sg.in_s))}%"
                  title={`Segment ${i + 1}: ${fmtT(sg.in_s)}–${fmtT(sg.out_s)} · click to jump · drag to Edit · right-click for options`}
                  draggable="true"
                  ondragstart={(e) => dragClip(e, i)}
                  onpointerdown={(e) => e.stopPropagation()}
                  onclick={(e) => { e.stopPropagation(); jumpToSegment(i); }}
                  oncontextmenu={(e) => openMarkMenu(e, i, null)}
                ><span class="segno">{i + 1}</span></div>
                {#each ["in", "out"] as const as edge (edge)}
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <div
                    class="mark {edge}"
                    class:sel={selMark?.seg === i && selMark?.edge === edge}
                    style="left:{pct(edge === 'in' ? sg.in_s : sg.out_s)}%"
                    title={`${edge === "in" ? "In" : "Out"} ${fmtT(edge === "in" ? sg.in_s : sg.out_s)} · drag to adjust (←/→ one frame) · right-click to remove`}
                    onpointerdown={(e) => onMarkDown(e, i, edge)}
                    onpointermove={onMarkMove}
                    onpointerup={onMarkUp}
                    oncontextmenu={(e) => openMarkMenu(e, i, edge)}
                  ></div>
                {/each}
              {/each}
              {#if marks.open != null}
                {#if cur > marks.open}<div class="seg pending" style="left:{pct(marks.open)}%; width:{pct(cur) - pct(marks.open)}%"></div>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="mark in open"
                  class:sel={selMark?.seg === -1}
                  style="left:{pct(marks.open)}%"
                  title={`In ${fmtT(marks.open)}, waiting for ] · drag to adjust · right-click to remove`}
                  onpointerdown={(e) => onMarkDown(e, -1, "in")}
                  onpointermove={onMarkMove}
                  onpointerup={onMarkUp}
                  oncontextmenu={(e) => openMarkMenu(e, -1, "in")}
                ></div>
              {/if}
              <div class="cursor" style="left:{pct(cur)}%"></div>
              {#if preview != null && !scrubbing && engineReady}
                <!-- Hover thumbnail, decoded on demand (no sprite sheet). -->
                <div
                  class="scrubprev live"
                  style="left:{preview * 100}%; width:{previewBox.w}px; height:{previewBox.h}px;"
                >
                  <canvas bind:this={prevCanvas}></canvas>
                  <span class="ts">{fmt(preview * (dur || engine?.index.durationS || 0))}</span>
                </div>
              {:else if preview != null && !scrubbing && strip && stripSrc}
                {@const c = cellPos(preview)}
                <div
                  class="scrubprev"
                  style="left:{preview * 100}%; width:{previewBox.w}px; height:{previewBox.h}px;
                         background-image:url('{stripSrc}');
                         background-size:{strip.cols * 100}% {strip.rows * 100}%;
                         background-position:{c.x}% {c.y}%;"
                >
                  <span class="ts">{fmt(preview * (dur || strip.duration))}</span>
                </div>
              {/if}
            </div>
            <span
              class="miniToggle grip"
              role="button"
              tabindex="-1"
              draggable="true"
              ondragstart={(e) => dragClip(e, null)}
              title={segments.length ? `Drag to the Edit window: its ${segments.length} segment${segments.length === 1 ? "" : "s"}` : "Drag to the Edit window (the whole clip)"}
            >⠿ To Edit</span>
            <button class="miniToggle" class:on={infoVisible} onclick={() => (infoVisible = !infoVisible)} title="Show file information overlay">Info</button>
            <button
              class="miniToggle"
              class:on={clipToolsOpen}
              onclick={() => (clipToolsOpen = !clipToolsOpen)}
              title="Trim, mark ranges, and export subclips"
            >Clip tools{#if !clipToolsOpen && (segments.length || marks.open != null)}<span class="ctdot"></span>{/if}</button>
          </div>
          {#if clipToolsOpen}
            <!-- Clip tools live in their own bordered panel, visually separated
                 from the scrubber/timeline above. Everything — in/out, range,
                 mark, the marked-segment chips, and the save actions — sits in
                 ONE row (wraps only if it has to; the common ≤3-segment case
                 fits). The old separate range-header line was redundant: the
                 numbers already show on the In/Out buttons and the range span. -->
            <div class="clippanel">
              <div class="ctrls">
                <button onclick={markInHere} title="Mark an in point here ([). Again moves it.">[ In</button>
                <button onclick={markOutHere} title="Mark the out point here (]). Again moves it.">Out ]</button>
                {#each segments as sg, i (i)}
                  <span class="segmentPill">
                    <button class="segLabel" onclick={() => jumpToSegment(i)} oncontextmenu={(e) => openMarkMenu(e, i, null)} title={`Segment ${i + 1}: ${fmtT(sg.in_s)}–${fmtT(sg.out_s)} · click to jump`}>
                      <strong>{i + 1}</strong>
                      <span>{fmtT(sg.in_s)}–{fmtT(sg.out_s)}</span>
                    </button>
                    <button class="segRemove" onclick={() => { marks = removeSeg(marks, i); selMark = null; persistMarks(); }} title="Remove this segment">×</button>
                  </span>
                {/each}
                {#if marks.open != null}<span class="len">in at {fmt(marks.open)}, press ] to close it</span>{/if}
                {#if segments.length}<span class="len">{segments.length} segment{segments.length === 1 ? "" : "s"} · {fmt(totalLen(segments))}</span>{/if}
                <span class="spacer"></span>
                {#if segments.length}
                  <button onclick={playSegments} title="Play just the marked parts, one after another">▶ Play segments</button>
                  <button class="reset" onclick={() => { marks = emptyMarks(); selMark = null; persistMarks(); }}>Clear all</button>
                {/if}
                <button class="exp secondary" onclick={exportSegments} disabled={!segments.length || exportingSegments} title="Save each segment as its own file next to the clip (no re-encode)">
                  {exportingSegments ? "Saving..." : `Save ${segments.length || ""} as clips`}
                </button>
              </div>
              <p class="cthint">[ marks an in, ] the out; pressing again moves it. Then [ starts the next segment. Drag a marker to adjust it frame by frame (or pick it and use ←/→). Right-click a segment to remove it.</p>
            </div>
          {/if}
          {#if exportNote}<div class="note">{exportNote}</div>{/if}
        </div>
        </div>
      </div>
    {:else}
      <!-- src still resolving (an IPC round-trip) — stay quietly black. The old
           code showed the HEVC-failure card here, flashing it before EVERY clip. -->
      <div class="empty"></div>
    {/if}
  {:else if failed}
    <div class="empty">
      Can't preview this file{item.kind === "other" ? " (unsupported format)" : ""}.
    </div>
  {:else}
    <!-- The previous sharp photo stays painted until the next one has decoded,
         then swaps in a single frame — no fade, no glow, no black gap. The
         blurred placeholder appears only for genuinely slow (cold) loads. -->
    <div class="stage">
      {#if showLow && lowSrc}
        <img class="layer ph" src={lowSrc} alt="" draggable="false" />
      {/if}
      {#if curSrc}
        <img class="layer hi" src={curSrc} alt={item.name} draggable="false" />
      {/if}
    </div>
  {/if}
  {#if infoVisible && item}
    <div class="infoOverlay">
      {#each infoRows as row}
        <div>{row}</div>
      {/each}
    </div>
  {/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onclose={() => (menu = null)} />
{/if}

<style>
  .loupe {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    /* Near-black NEUTRAL in every theme: the Focus surround is the reference
       your eye judges the photo's colors against, so it never takes the UI
       theme's tint (the old #0a0805 had a warm cast). */
    background: #050607;
    overflow: hidden;
  }
  /* "Behind the pictures" (Settings): black, 18 % grey or light replace the
     neutral near-black; the theme setting keeps it, never the theme's tint. */
  :global(:root[data-surround="black"]) .loupe,
  :global(:root[data-surround="grey"]) .loupe,
  :global(:root[data-surround="light"]) .loupe {
    background: var(--viewport-bg);
  }
  img,
  video {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }
  .stage {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  /* low-res placeholder for slow loads: softened, edges clipped by the stage.
     No opacity transitions anywhere — fades between layers were the "glow at
     the edges" artifact when flipping through warm photos. Swaps are instant. */
  .ph {
    filter: blur(10px);
    transform: scale(1.03); /* mask blurred edges bleeding past the frame */
  }
  .empty {
    color: var(--text-faint);
    font-size: var(--fs-lg);
  }

  .vwrap {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  /* The stage wraps the video so the drag-scrub overlay can cover exactly the
     picture area (not the transport bar below). */
  .stagewrap {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .stagewrap video {
    width: 100%;
    height: 100%;
  }
  /* Paused overlay. Sits above the picture but BELOW the scrub layers (z 4-5)
     and the transport, so nothing it covers is ever interactive at the time. */
  .playOverlay {
    position: absolute;
    inset: 0;
    z-index: 3;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
  }
  /* A calmer play button (audit L11): smaller, lighter, still a big target
     (the whole stage is the button). */
  .poIcon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 58px;
    height: 58px;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.42);
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(3px);
    transition: transform 140ms ease, background 140ms ease;
  }
  .playOverlay:hover .poIcon {
    background: rgba(0, 0, 0, 0.66);
    transform: scale(1.06);
  }
  .poIcon svg {
    width: 24px;
    height: 24px;
    margin-left: 3px; /* optically centre the triangle in the circle */
    fill: #fff;
  }
  @media (prefers-reduced-motion: reduce) {
    .poIcon {
      transition: none;
    }
  }
  .scrubStage {
    position: absolute;
    inset: 0;
    z-index: 4;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #000;
    pointer-events: none;
  }
  .scrubFrame {
    background-repeat: no-repeat;
    background-color: #000;
  }
  /* Live-decode scrub surface. Present at all times so there is always a canvas
     to paint into; `opacity` (not display/{#if}) does the reveal, because the
     canvas must already hold pixels at the moment it becomes visible. No
     transition — a fade would smear one frame into the next while dragging. */
  .liveScrub {
    position: absolute;
    inset: 0;
    z-index: 4;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #000;
    pointer-events: none;
    opacity: 0;
    visibility: hidden;
  }
  .liveScrub.shown {
    opacity: 1;
    visibility: visible;
  }
  .liveScrub canvas {
    display: block;
  }
  /* Transport overlays the bottom of the stage (not a panel below it), so the
     picture stays edge-to-edge. A soft top-fading scrim keeps the controls
     legible over any frame. Shown/hidden by the .shown class (hover reveal). */
  .trim {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 6;
    padding: 42px 18px 15px;
    background: linear-gradient(
      to top,
      rgba(0, 0, 0, 0.86) 0%,
      rgba(0, 0, 0, 0.52) 48%,
      rgba(0, 0, 0, 0) 100%
    );
    transition:
      opacity 0.16s ease,
      transform 0.18s ease;
  }
  /* Hidden: faded in place (it used to slide 14 px below the stage, which
     the layout audit read as the Focus view overflowing). */
  .trim:not(.shown) {
    opacity: 0;
    pointer-events: none;
  }
  /* Collapsed progress: a hairline that just conveys position/length. Kept
     unobtrusive (white at low alpha) so it doesn't distract from the frame. */
  .thinline {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 3px;
    background: rgba(255, 255, 255, 0.12);
    z-index: 5;
    pointer-events: none;
  }
  .thinfill {
    height: 100%;
    background: rgba(255, 255, 255, 0.5);
  }
  /* Controls read on a dark scrim now, not the panel background. */
  .trim .time,
  .trim .time .sep {
    color: rgba(255, 255, 255, 0.86);
  }
  .playrow {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 0;
  }
  .playrow .pp {
    width: 34px;
    height: 30px;
    border-radius: var(--radius-sm);
    border: 1px solid rgba(255,255,255,.18);
    background: rgba(20,24,29,.78);
    color: var(--text);
    font-size: var(--fs-md);
    line-height: 1;
  }
  .playrow .pp:hover {
    background: var(--bg-hover);
  }
  /* Glimpse sits beside play and reads as a secondary action until it's
     running, when it takes the accent so the sweep is obviously live. */
  .playrow .pp.glimpse {
    color: var(--text-dim);
  }
  .playrow .pp.glimpse:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .playrow .pp.glimpse.on {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  /* The clock is the only time readout left on the stage now that the floating
     pill is gone, and it has to stay legible over an arbitrary bright frame —
     hence the shadow and the weight on the current position. */
  .playrow .time {
    font-size: var(--fs-md);
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.75);
  }
  .playrow .time b {
    font-size: var(--fs-lg);
    font-weight: var(--fw-semibold);
    color: #fff;
  }
  .playrow .time .sep {
    color: var(--text-faint);
    margin: 0 4px;
  }
  /* tiny dot on the collapsed Clip tools toggle when a trim/marks exist */
  .ctdot {
    display: inline-block;
    width: 5px;
    height: 5px;
    margin-left: 5px;
    border-radius: 50%;
    background: var(--accent);
    vertical-align: middle;
  }
  .miniToggle {
    padding: 3px 7px;
    border: 1px solid rgba(255,255,255,.16);
    border-radius: var(--radius-sm);
    background: rgba(20,24,29,.72);
    color: rgba(255,255,255,.78);
    font-size: var(--fs-sm);
    white-space: nowrap;
  }
  .miniToggle.on {
    border-color: var(--accent);
    color: var(--accent);
  }
  .track {
    position: relative;
    flex: 1;
    min-width: 0;
    height: 15px;
    border-radius: 999px;
    background: rgba(255,255,255,.16);
    margin-bottom: 0;
    cursor: pointer;
    touch-action: none; /* let pointer-drag scrub instead of scrolling */
  }
  .track.scrubbing {
    cursor: grabbing;
  }
  /* A marked segment: a bright span on the bar, numbered. The segment the
     keys last touched is a touch brighter. */
  .seg {
    position: absolute;
    top: -2px;
    bottom: -2px;
    min-width: 3px;
    border-radius: var(--radius-xs);
    background: color-mix(in srgb, var(--pick) 62%, transparent);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.55), 0 0 0 1px rgba(0, 0, 0, 0.3);
    cursor: grab;
    z-index: 3;
    overflow: hidden;
  }
  .seg.last { background: color-mix(in srgb, var(--pick) 78%, transparent); }
  /* An open in, running to the playhead until ] closes it. */
  .seg.pending {
    background: repeating-linear-gradient(90deg, color-mix(in srgb, var(--pick) 45%, transparent) 0 6px, transparent 6px 10px);
    box-shadow: none;
    pointer-events: none;
  }
  .segno {
    position: absolute;
    left: 4px;
    top: 50%;
    transform: translateY(-50%);
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    color: rgba(0, 0, 0, 0.7);
    pointer-events: none;
  }
  /* In / out markers: a tall bar with a bracket flag above the track, big
     enough to grab; dragged frame by frame. */
  .mark {
    position: absolute;
    top: -9px;
    bottom: -5px;
    width: 12px;
    margin-left: -6px;
    z-index: 5;
    cursor: ew-resize;
    touch-action: none;
  }
  .mark::before {
    content: "";
    position: absolute;
    left: 5px;
    top: 0;
    bottom: 0;
    width: 2px;
    border-radius: 1px;
    background: #fff;
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.45);
  }
  .mark::after {
    position: absolute;
    top: -12px;
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    color: #fff;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.8);
  }
  .mark.in::after { content: "["; left: 6px; }
  .mark.out::after { content: "]"; right: 6px; }
  .mark.open::before { background: var(--pick); }
  .mark.sel::before { background: var(--accent); width: 3px; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5), 0 0 8px var(--accent); }
  .mark.sel::after { color: var(--accent); }
  .thinseg {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, var(--pick) 70%, transparent);
  }
  .marktoast {
    position: absolute;
    left: 50%;
    top: 18px;
    transform: translateX(-50%);
    z-index: 30;
    padding: 6px 12px;
    border-radius: 999px;
    background: rgba(8, 10, 13, 0.78);
    color: #fff;
    font-size: var(--fs-md);
    pointer-events: none;
    white-space: nowrap;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
  }
  .grip { cursor: grab; user-select: none; }
  .cthint { margin: 8px 0 0; color: rgba(255, 255, 255, 0.55); font-size: var(--fs-sm); line-height: 1.4; }
  .cursor {
    position: absolute;
    top: -5px;
    width: 3px;
    height: 24px;
    background: #fff;
    transform: translateX(-1.5px);
    pointer-events: none;
    border-radius: 2px;
  }
  /* Floating frame preview shown under the scrub cursor (sprite cell). */
  .scrubprev {
    position: absolute;
    bottom: calc(100% + 9px);
    transform: translateX(-50%);
    border-radius: var(--radius-sm);
    border: 1px solid rgba(255, 255, 255, 0.18);
    background-color: #000;
    background-repeat: no-repeat;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.55);
    pointer-events: none;
    overflow: hidden;
    z-index: 60;
  }
  .scrubprev.live {
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .scrubprev.live canvas {
    display: block;
  }
  .scrubprev .ts {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    text-align: center;
    font-size: var(--fs-xs);
    line-height: 1.5;
    color: #fff;
    background: rgba(0, 0, 0, 0.55);
    font-variant-numeric: tabular-nums;
  }
  /* Grouped clip-tools panel: a quiet bordered card that sets the trim/mark/save
     controls apart from the scrubber above them. */
  .clippanel {
    margin-top: 10px;
    padding: 10px 12px;
    border: 1px solid rgba(255,255,255,.14);
    border-radius: var(--radius-md);
    background: rgba(18,22,27,.78);
    box-shadow: 0 12px 32px rgba(0,0,0,.28);
    backdrop-filter: blur(18px);
  }
  /* Single row: wraps only when it must (5-6 marked segments) — the common
     case of a couple of marks plus the fixed buttons fits on one line. */
  .ctrls {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    row-gap: 6px;
    margin-bottom: 0;
  }
  .ctrls button {
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-elev);
    color: var(--text);
    font-size: var(--fs-md);
  }
  .ctrls button:hover {
    background: var(--bg-hover);
  }
  .len {
    color: var(--text-dim);
    font-size: var(--fs-sm);
  }
  .spacer {
    flex: 1;
  }
  .ctrls .exp {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-on);
    font-weight: var(--fw-semibold);
  }
  .ctrls .exp.secondary {
    background: color-mix(in srgb, var(--pick) 22%, var(--bg-elev));
    border-color: color-mix(in srgb, var(--pick) 55%, var(--border));
    color: var(--text);
  }
  .ctrls .exp:disabled {
    opacity: 0.45;
  }
  /* A marked segment is ONE pill (label + a fused × button) instead of two
     separate chips, so it drops inline into the single clip-tools row. */
  .segmentPill {
    display: inline-flex;
    align-items: center;
    height: 24px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--pick) 55%, var(--border));
    background: color-mix(in srgb, var(--pick) 12%, var(--bg-elev));
    overflow: hidden;
  }
  .segLabel {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    padding: 0 4px 0 10px;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: var(--fs-sm);
    white-space: nowrap;
  }
  .segLabel strong {
    color: var(--pick);
  }
  .segLabel span {
    font-variant-numeric: tabular-nums;
  }
  .segRemove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 0 9px 0 5px;
    border: none;
    background: transparent;
    color: var(--reject);
    line-height: 1;
  }
  .segRemove:hover {
    background: color-mix(in srgb, var(--reject) 18%, transparent);
  }
  .note {
    margin-top: 6px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
  }
  .infoOverlay {
    position: absolute;
    left: 22px;
    top: 22px;
    z-index: 20;
    max-width: min(560px, calc(100% - 44px));
    padding: 11px 13px;
    border: 1px solid rgba(255,255,255,.13);
    border-radius: var(--radius-md);
    background: rgba(8, 10, 13, 0.64);
    color: #fff;
    font-size: var(--fs-lg);
    line-height: 1.45;
    box-shadow: 0 16px 38px rgba(0, 0, 0, 0.40);
    backdrop-filter: blur(14px) saturate(.8);
    pointer-events: none;
  }

  .vfail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    text-align: center;
    padding: 24px;
    max-width: 460px;
  }
  .vfail .vt {
    color: var(--text-dim);
    font-weight: var(--fw-semibold);
    font-size: var(--fs-lg);
    margin: 0;
  }
  .vfail p {
    margin: 0;
    line-height: 1.5;
  }
  /* The path a missing entry still points at — long, so it wraps and shrinks. */
  .vfail .vpath {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: var(--fs-sm);
    color: var(--text-faint);
    word-break: break-all;
  }
  .obtn {
    margin-top: 4px;
    padding: 9px 16px;
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: var(--accent-on);
    font-size: var(--fs-md);
    font-weight: var(--fw-semibold);
  }
  .obtn:hover {
    filter: brightness(1.06);
  }
  .obtn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .obtn.ghost {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-dim);
    font-weight: var(--fw-medium);
  }
  .vfail .subnote {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--text-faint);
    line-height: 1.5;
  }
  .vfail .subnote.err {
    color: var(--reject);
  }
  .vwrap {
    position: relative;
  }
  .proxytag {
    position: absolute;
    top: 10px;
    right: 12px;
    padding: 3px 9px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.55);
    color: rgba(255, 255, 255, 0.85);
    font-size: var(--fs-xs);
    pointer-events: auto;
    z-index: 5;
  }
  /* Build feedback for the scrub filmstrip — top-LEFT, so it never collides
     with the converted-preview tag on the right. */
  .stripbuild {
    position: absolute;
    top: 10px;
    left: 12px;
    padding: 3px 9px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.55);
    color: rgba(255, 255, 255, 0.85);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
    pointer-events: auto;
    z-index: 5;
  }
</style>
