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
  import type { FilmstripInfo, MediaItem } from "$lib/types";
  import { untrack } from "svelte";

  // `armed` = this tile is the selected/active item. Hover-scrub only runs when
  // armed, so sweeping the pointer across a wall of videos never kicks off strip
  // builds — you click a clip to arm it, THEN hover it to skim frames.
  let {
    item,
    size = 320,
    armed = false,
    deferUntilVisible = false,
  }: {
    item: MediaItem;
    size?: number;
    armed?: boolean;
    /** For genuinely unvirtualized lists only. Virtual grids already mount just
     *  the viewport and must not depend on a second visibility observer. */
    deferUntilVisible?: boolean;
  } = $props();

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

  // NOTE (2026-08-03): the grid tile no longer runs a WebCodecs decoder.
  // v1.2.0 added a per-armed-tile `ScrubEngine` + `<canvas>` here ("grid skim on
  // the decoder"), against the architecture DECIDED in
  // docs/design/video-player-migration.md §10 ("Sprites stay for grid tiles — a
  // decoder per tile is not a thing"). It shipped without a fast-scroll test and
  // is the v1.2.0 scroll-freeze regression: a live `<canvas>` is its own
  // WebView2 GPU layer and `VideoDecoder.configure()` pins D3D11/Media-Foundation
  // resources, so arming/hovering tiles while scrolling or keyboard-navigating
  // churned GPU layers + kernel handles until the compositor stalled (GPU idle,
  // +16k handles — a pipeline stall). Focus view keeps its own decoder in
  // Loupe.svelte, untouched — that is where live scrubbing was proven and wanted.
  // Grid skim falls back to the sprite path, opt-in via Live Scrub, exactly as
  // v1.1.0 did when fast scroll was smooth.

  let isVideo = $derived(item.kind === "video");
  // A primitive load identity prevents parent virtualization updates from
  // resetting an already-loaded image when the actual media did not change.
  let mediaLoadKey = $derived(`${item.kind}:${item.path}@${size}`);
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

  // ── don't fetch anything for a tile nobody can see ────────────────────────
  // The library grid is virtualized (VirtualGrid/VirtualStrip), so only visible
  // tiles are ever mounted and this observer costs nothing there. EditStudio's
  // source pane is NOT virtualized — it renders one Thumb per file in a plain
  // {#each} — so on the owner's 229-clip folder of 4K60 Osmo footage, opening
  // Edit mounted 229 tiles at once, each firing an ffmpeg poster extraction and
  // two IPC calls on mount, against files 2-15 minutes long. Gating on real
  // visibility makes an unvirtualized list behave like a virtualized one for
  // the expensive part, without touching either component's layout.
  let onScreen = $state(false);
  $effect(() => {
    if (!deferUntilVisible) {
      onScreen = true;
      return;
    }
    const el = thumbEl;
    if (!el) return;
    // A generous look-ahead is cheap for regular images and prevents a quick
    // scroll from outrunning observation. Heavy video work is independently
    // limited by the loader, so this no longer needs to be reduced globally.
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) if (e.isIntersecting) onScreen = true;
      },
      { rootMargin: "400px" },
    );
    io.observe(el);
    return () => io.disconnect();
  });

  // Images/RAW -> cached orientation-baked thumbnail. Videos -> a real poster
  // frame extracted by the bundled ffmpeg. Optional Live Scrub is separate: the
  // sprite sheet is shared with Focus view and only requested once the tile is
  // armed and hovered.
  $effect(() => {
    void mediaLoadKey;
    // `item` is deliberately untracked here. Virtual containers can refresh a
    // prop binding during scroll; only the primitive key above should restart
    // image state.
    const it = untrack(() => item);
    const visible = onScreen;
    src = null;
    failed = false;
    loaded = false;
    mediaAspect = 16 / 9;
    strip = null;
    scrub = null;
    building = false;
    // A recycled slot inherits the DOM element (and thus any live pointer that is
    // still over it), but it is now a DIFFERENT item. Clearing `hovering` here
    // stops a stale hover from auto-arming skim/scrub work for the new item the
    // instant it is repointed during a scroll.
    hovering = false;
    if (it.kind === "other") return;
    // Off-screen: no poster extraction, no cached-strip probes, nothing. See
    // the IntersectionObserver above for why this matters so much in Edit mode.
    if (!visible) return;
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
    // Cached sprite skim is only consumed in Live-Scrub (sprite) mode; the default
    // live-decode path needs no sprite. Gating on the setting avoids two per-tile
    // IPC probes (a .json read + stat each) on EVERY video tile and every recycle
    // while scrolling a video folder — pure churn when it can't be used. The dense
    // `f` strip is the only one still built, so the legacy `s` probe is dropped.
    if (it.kind === "video" && settings.s.liveScrub) {
      api.videoFilmstripCached(it.path)
        .then((s) => {
          if (alive && s && !strip) strip = { ...s, src: api.fileSrc(s.src) };
        })
        .catch(() => {});
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
  // Grid skim uses the pre-built sprite sheet, and only when the user opted into
  // Live Scrub. (The WebCodecs decoder that briefly lived here in v1.2.0 is gone
  // — see the note at the top of this component.)
  $effect(() => {
    if (!isVideo || !armed || !hovering) return;
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
  /// Grid skimming is available only when the user opted into building sprite
  /// sheets (Live Scrub). Focus view scrubs live via WebCodecs; the grid does not.
  let canSkim = $derived(settings.s.liveScrub);

  function updateScrub(e: PointerEvent) {
    if (!isVideo || !armed || !canSkim) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const w = Math.max(1, rect.width);
    scrub = Math.max(0, Math.min(0.999, (e.clientX - rect.left) / w));
  }

  function enterThumb(e: PointerEvent) {
    hovering = true;
    updateScrub(e);
  }

  function leaveThumb() {
    hovering = false;
    scrub = null;
    building = false;
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
    {#if isVideo && strip && scrub != null}
      {@const cell = framePos(scrub)}
      <div
        class="scrubLayer"
        style="width:{scrubBox.w}px; height:{scrubBox.h}px; background-image:url('{strip.src}'); background-size:{strip.cols * 100}% {strip.rows * 100}%; background-position:{strip.cols <= 1 ? 0 : (cell.x / (strip.cols - 1)) * 100}% {strip.rows <= 1 ? 0 : (cell.y / (strip.rows - 1)) * 100}%"
      ></div>
    {/if}
    {#if isVideo && canSkim && scrub != null}
      <span class="scrubRail"><span style="width:{scrub * 100}%"></span></span>
      {#if !strip}<span class="scrubHint" style="left:{scrub * 100}%"></span>{/if}
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
