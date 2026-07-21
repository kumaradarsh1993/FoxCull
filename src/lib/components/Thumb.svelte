<script lang="ts">
  import {
    loadThumb,
    loadVideoPoster,
    loadVideoFilmstrip,
    cancelThumb,
    cancelVideoPoster,
    cancelVideoFilmstrip,
  } from "$lib/thumbnail-loader";
  import { api } from "$lib/api";
  import { settings } from "$lib/settings.svelte";
  import { activity } from "$lib/activity.svelte";
  import { ScrubEngine, paintFrame } from "$lib/scrub-engine";
  import type { FilmstripInfo, MediaItem } from "$lib/types";

  // `armed` = this tile is the selected/active item. Hover-scrub only runs when
  // armed, so sweeping the pointer across a wall of videos never kicks off strip
  // builds — you click a clip to arm it, THEN hover it to skim frames.
  let { item, size = 320, armed = false }: { item: MediaItem; size?: number; armed?: boolean } = $props();

  const SCRUB_BUILD_DELAY_MS = 140;

  let thumbEl = $state<HTMLDivElement | null>(null);
  let thumbW = $state(1);
  let thumbH = $state(1);
  let src = $state<string | null>(null);
  let failed = $state(false);
  let loaded = $state(false); // drives the fade-in once the bitmap is painted
  let mediaAspect = $state(16 / 9);
  let strip = $state<FilmstripInfo | null>(null);
  let scrub = $state<number | null>(null);
  let hovering = $state(false);
  let building = $state(false);
  let scrubTimer: ReturnType<typeof setTimeout> | null = null;

  // ── live-decode skim ──────────────────────────────────────────────────────
  // The same decoder Focus uses. Viable here only because of the ARMED rule:
  // exactly one tile skims at a time, so there is exactly one decoder — the
  // objection that killed this idea earlier ("a decoder per grid tile is not a
  // thing") never applies. With this in place the sprite sheet has no remaining
  // consumer except clips the decoder can't take.
  let tileEngine = $state.raw<ScrubEngine | null>(null);
  let tileReady = $state(false);
  let tilePending = $state(false);
  let tileCanvas = $state<HTMLCanvasElement | null>(null);
  let tilePainted = $state(false);

  let isVideo = $derived(item.kind === "video");
  let scrubBox = $derived.by(() => {
    const aspect = mediaAspect || (strip?.tile_w && strip.tile_h ? strip.tile_w / strip.tile_h : 16 / 9);
    const boxAspect = thumbW / thumbH;
    if (aspect >= boxAspect) return { w: thumbW, h: thumbW / aspect };
    return { w: thumbH * aspect, h: thumbH };
  });

  $effect(() => {
    const el = thumbEl;
    if (!el) return;
    const measure = () => {
      const rect = el.getBoundingClientRect();
      thumbW = Math.max(1, rect.width);
      thumbH = Math.max(1, rect.height);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });

  // Images/RAW -> cached orientation-baked thumbnail. Videos -> a real poster
  // frame extracted by the bundled ffmpeg. Optional Live Scrub is separate: the
  // sprite sheet is shared with Focus view and only requested once the tile is
  // armed and hovered.
  $effect(() => {
    const it = item;
    src = null;
    failed = false;
    loaded = false;
    mediaAspect = 16 / 9;
    strip = null;
    scrub = null;
    building = false;
    if (it.kind === "other") return;
    let alive = true;
    const p = it.kind === "video" ? loadVideoPoster(it.path) : loadThumb(it.path, size);
    p.then((s) => {
      if (!alive) return;
      if (s) src = s;
      else failed = true;
    });
    // Free instant skim for anything already extracted — never builds. Reads the
    // legacy light `s` strip too, so folders Prepared before the sprites were
    // unified keep skimming without a re-extraction.
    if (it.kind === "video") {
      const take = (s: FilmstripInfo | null) => {
        if (alive && s && !strip) strip = { ...s, src: api.fileSrc(s.src) };
      };
      api.videoFilmstripCached(it.path).then(take).catch(() => {});
      api.videoScrubstripCached(it.path).then(take).catch(() => {});
    }
    return () => {
      alive = false;
      if (it.kind === "video") {
        cancelVideoPoster(it.path);
        // Only an UNARMED tile abandons its build on teardown. The armed tile is
        // the one you just double-clicked into Focus: cancelling here is what
        // made the build appear to "restart from 10%" the moment the clip
        // opened, because Focus then had to start it over.
        if (!armed) cancelVideoFilmstrip(it.path);
      } else if (it.kind !== "other") {
        cancelThumb(it.path, size);
      }
      if (scrubTimer) clearTimeout(scrubTimer);
      scrubTimer = null;
      closeTileEngine();
    };
  });

  $effect(() => {
    if (!settings.s.liveScrub) {
      strip = null;
      scrub = null;
      building = false;
      if (scrubTimer) clearTimeout(scrubTimer);
      scrubTimer = null;
      if (item.kind === "video") cancelVideoFilmstrip(item.path);
    }
  });

  // Disarming (selection moves to another tile) stops any pending build and
  // clears the skim overlay immediately.
  $effect(() => {
    if (!armed) {
      scrub = null;
      building = false;
      if (scrubTimer) {
        clearTimeout(scrubTimer);
        scrubTimer = null;
      }
      if (isVideo && !strip) cancelVideoFilmstrip(item.path);
      closeTileEngine();
    }
  });

  // THE build trigger — an effect, deliberately, not the pointerenter handler.
  //
  // You arm a tile by CLICKING it, and by then the pointer is already inside:
  // `pointerenter` fired long before the tile was armed and never fires again,
  // so a handler-only path scheduled a build for every tile you swept past and
  // for none of the tile you actually selected. That is the whole "the scrub
  // bar appears but the frames never change" bug. Keying off (armed && hovering)
  // as *state* makes arming-under-the-cursor and hovering-an-armed-tile the
  // same thing, whichever order they happen in.
  // Preferred path: open the decoder for the armed tile. Same delay as the
  // sprite build so a pointer merely passing over an armed tile doesn't start
  // disk work. Falls through to the sprite effect below if the clip is
  // unsupported.
  $effect(() => {
    if (!isVideo || !armed || !hovering) return;
    if (!settings.s.liveScrub || !settings.s.liveDecodeScrub) return;
    if (tileEngine || tilePending) return;
    const path = item.path;
    tilePending = true;
    const timer = setTimeout(() => {
      ScrubEngine.open(path, () => item.path !== path)
        .then((e) => {
          if (item.path !== path) {
            e.close();
            return;
          }
          tileEngine = e;
          tileReady = true;
          tilePending = false;
        })
        .catch(() => {
          // Unsupported clip — release the sprite path we were holding back.
          if (item.path === path) tilePending = false;
        });
    }, SCRUB_BUILD_DELAY_MS);
    return () => clearTimeout(timer);
  });

  /** Decode + paint the frame at `frac` through the clip into the tile. */
  function paintTile(frac: number) {
    const e = tileEngine;
    if (!e) return;
    const d = e.index.durationS;
    if (d <= 0) return;
    e.request(frac * d, false, (f) => {
      if (!tileCanvas) return;
      paintFrame(tileCanvas, f, scrubBox.w, scrubBox.h, e.index.rotation);
      tilePainted = true;
    });
  }

  function closeTileEngine() {
    tileEngine?.close();
    tileEngine = null;
    tileReady = false;
    tilePending = false;
    tilePainted = false;
  }

  $effect(() => {
    if (!isVideo || !armed || !hovering) return;
    // The decoder supersedes the sprite entirely; only build one while it is
    // still opening-or-unavailable, exactly as Focus does.
    if (tileReady || tilePending) return;
    if (!settings.s.liveScrub || strip || scrubTimer) return;
    const path = item.path;
    building = true;
    scrubTimer = setTimeout(() => {
      scrubTimer = null;
      loadVideoFilmstrip(path)
        .then((s) => {
          if (item.path !== path) return;
          if (settings.s.liveScrub && s) strip = s;
        })
        .finally(() => {
          if (item.path === path) building = false;
        });
    }, SCRUB_BUILD_DELAY_MS);
  });

  function framePos(frac: number) {
    if (!strip) return { x: 0, y: 0 };
    const i = Math.max(0, Math.min(strip.count - 1, Math.floor(frac * strip.count)));
    return { x: i % strip.cols, y: Math.floor(i / strip.cols) };
  }

  // The skim position maps across the WHOLE CELL, not across the letterboxed
  // picture inside it. Mapping to the picture made portrait clips wildly
  // oversensitive: a 9:16 clip only paints ~30% of a landscape cell's width, so
  // the full timeline was crammed into that sliver while the pillarboxed
  // remainder was dead travel. The cell is what the hand actually aims at.
  function updateScrub(e: PointerEvent) {
    if (!isVideo || !armed || !settings.s.liveScrub) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const w = Math.max(1, rect.width);
    scrub = Math.max(0, Math.min(0.999, (e.clientX - rect.left) / w));
    if (tileReady) paintTile(scrub);
  }

  function enterThumb(e: PointerEvent) {
    hovering = true;
    updateScrub(e);
  }

  function leaveThumb() {
    hovering = false;
    scrub = null;
    building = false;
    // The decoder holds GPU frames and a file handle; it exists only for the
    // duration of a skim.
    closeTileEngine();
    if (scrubTimer) {
      clearTimeout(scrubTimer);
      scrubTimer = null;
    }
    // Leaving an UNARMED tile stops the disk work (queued or already extracting
    // frames on the backend) — that's the sweep-across-a-wall-of-videos case.
    // An ARMED tile's build is left to finish: you selected that clip on
    // purpose, and cancelling a 10-second extraction because the pointer
    // drifted off, then restarting it from zero on the way back, is how
    // skimming ended up feeling like it never worked. The disarm effect above
    // is what cancels it if the selection genuinely moves on.
    if (isVideo && !armed && !strip) cancelVideoFilmstrip(item.path);
  }

  // Live build feedback while the hover strip is being extracted: the backend
  // streams per-frame progress through the activity store.
  let scrubJob = $derived.by(() => {
    if (tileReady || tilePending) return null; // decoder path builds nothing
    if (!isVideo || strip || (!building && scrub == null)) return null;
    const j = activity.jobs[`strip:${item.path}`];
    return j && j.state === "running" ? j : null;
  });

  function mediaLoaded(e: Event) {
    loaded = true;
    const img = e.currentTarget as HTMLImageElement;
    if (img.naturalWidth > 0 && img.naturalHeight > 0) {
      mediaAspect = img.naturalWidth / img.naturalHeight;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="thumb" bind:this={thumbEl} onpointerenter={enterThumb} onpointermove={updateScrub} onpointerleave={leaveThumb}>
  {#if src}
    <img
      class="media"
      class:in={loaded}
      {src}
      alt={item.name}
      draggable="false"
      decoding="async"
      onload={mediaLoaded}
    />
    <!-- Decoded skim frame. Always mounted while the decoder is up (so there is
         a canvas to paint into) and revealed only once it holds pixels. -->
    {#if isVideo && tileReady}
      <canvas class="scrubLayer live" class:shown={tilePainted && scrub != null} bind:this={tileCanvas}></canvas>
    {/if}
    {#if isVideo && !tileReady && strip && scrub != null}
      {@const cell = framePos(scrub)}
      <div
        class="scrubLayer"
        style="width:{scrubBox.w}px; height:{scrubBox.h}px; background-image:url('{strip.src}'); background-size:{strip.cols * 100}% {strip.rows * 100}%; background-position:{strip.cols <= 1 ? 0 : (cell.x / (strip.cols - 1)) * 100}% {strip.rows <= 1 ? 0 : (cell.y / (strip.rows - 1)) * 100}%"
      ></div>
    {/if}
    {#if isVideo && settings.s.liveScrub && scrub != null}
      <span class="scrubRail"><span style="width:{scrub * 100}%"></span></span>
      {#if !strip && !tileReady}<span class="scrubHint" style="left:{scrub * 100}%"></span>{/if}
    {/if}
    {#if isVideo && settings.s.liveScrub && (scrubJob || (building && !strip))}
      <span class="scrubBuild">
        {scrubJob && scrubJob.total > 0
          ? `scrub ${Math.round((scrubJob.done / scrubJob.total) * 100)}%`
          : "scrub…"}
      </span>
    {/if}
    {#if isVideo}<span class="play">▶</span>{/if}
  {:else if isVideo}
    <div class="ph vid">
      <span class="film">▶</span>
      <span class="vext">{item.ext.toUpperCase()}</span>
    </div>
  {:else if failed}
    <div class="ph">{item.kind === "raw" ? "RAW" : item.ext.toUpperCase()}</div>
  {:else}
    <div class="ph dim">.</div>
  {/if}
  {#if item.kind === "raw"}<span class="badge">RAW</span>{/if}
</div>

<style>
  .thumb {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--text-faint) 12%, var(--viewport-bg));
    overflow: hidden;
  }
  .media {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    opacity: 0;
    transition: opacity 0.18s ease;
  }
  .media.in {
    opacity: 1;
  }
  .ph {
    color: var(--text-faint);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.5px;
  }
  .ph.dim { opacity: 0; }
  .ph.vid {
    flex-direction: column;
    gap: 5px;
    background: repeating-linear-gradient(
      45deg,
      color-mix(in srgb, var(--text-faint) 8%, var(--viewport-bg)),
      color-mix(in srgb, var(--text-faint) 8%, var(--viewport-bg)) 10px,
      color-mix(in srgb, var(--text-faint) 14%, var(--viewport-bg)) 10px,
      color-mix(in srgb, var(--text-faint) 14%, var(--viewport-bg)) 20px
    );
  }
  .ph.vid .film {
    font-size: 20px;
    color: var(--text);
    background: color-mix(in srgb, var(--text) 14%, transparent);
    width: 34px;
    height: 34px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding-left: 3px;
  }
  .ph.vid .vext { font-size: 10px; font-weight: 700; color: var(--text-dim); letter-spacing: 0.5px; }
  .scrubLayer {
    position: absolute;
    top: 50%;
    left: 50%;
    z-index: 1;
    transform: translate(-50%, -50%);
    background-repeat: no-repeat;
    background-color: #050505;
  }
  /* Decoded skim frame — opacity, not {#if}, so the canvas exists before the
     first frame arrives and never flashes empty over the poster. */
  .scrubLayer.live {
    display: block;
    opacity: 0;
    visibility: hidden;
  }
  .scrubLayer.live.shown {
    opacity: 1;
    visibility: visible;
  }
  .scrubRail {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 7px;
    z-index: 3;
    height: 3px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.45);
    overflow: hidden;
    pointer-events: none;
  }
  .scrubRail span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
  }
  .scrubHint {
    position: absolute;
    bottom: 12px;
    z-index: 3;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    transform: translateX(-50%);
    background: var(--accent);
    box-shadow: 0 0 0 4px rgba(0, 0, 0, 0.28);
    pointer-events: none;
  }
  /* Tiny build-progress tag while the hover strip is being extracted. */
  .scrubBuild {
    position: absolute;
    top: 5px;
    right: 6px;
    z-index: 3;
    padding: 1px 6px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.62);
    color: #fff;
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }
  .play {
    position: absolute;
    z-index: 2;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.5);
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
    padding-left: 2px;
    pointer-events: none;
  }
  .thumb:has(.scrubRail) .play {
    display: none;
  }
  .badge {
    position: absolute;
    bottom: 4px;
    left: 4px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.5px;
    padding: 1px 5px;
    border-radius: 3px;
    background: rgba(0, 0, 0, 0.6);
    color: #fff;
  }
</style>
