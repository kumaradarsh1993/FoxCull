<script lang="ts">
  import {
    loadThumb,
    loadVideoPoster,
    loadVideoScrubstrip,
    cancelThumb,
    cancelVideoPoster,
    cancelVideoScrubstrip,
  } from "$lib/thumbnail-loader";
  import { settings } from "$lib/settings.svelte";
  import type { FilmstripInfo, MediaItem } from "$lib/types";

  let { item, size = 320 }: { item: MediaItem; size?: number } = $props();

  let src = $state<string | null>(null);
  let failed = $state(false);
  let loaded = $state(false); // drives the fade-in once the bitmap is painted
  let strip = $state<FilmstripInfo | null>(null);
  let scrub = $state<number | null>(null);
  let scrubTimer: ReturnType<typeof setTimeout> | null = null;

  let isVideo = $derived(item.kind === "video");

  // Images/RAW -> cached orientation-baked thumbnail. Videos -> a real poster
  // frame extracted by the bundled ffmpeg. Optional Live Scrub is separate and
  // uses a much lighter sprite, requested only after the user actually hovers.
  $effect(() => {
    const it = item;
    src = null;
    failed = false;
    loaded = false;
    strip = null;
    scrub = null;
    if (it.kind === "other") return;
    let alive = true;
    const p = it.kind === "video" ? loadVideoPoster(it.path) : loadThumb(it.path, size);
    p.then((s) => {
      if (!alive) return;
      if (s) src = s;
      else failed = true;
    });
    return () => {
      alive = false;
      if (it.kind === "video") {
        cancelVideoPoster(it.path);
        cancelVideoScrubstrip(it.path);
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
      if (scrubTimer) clearTimeout(scrubTimer);
      scrubTimer = null;
      if (item.kind === "video") cancelVideoScrubstrip(item.path);
    }
  });

  function framePos(frac: number) {
    if (!strip) return { x: 0, y: 0 };
    const i = Math.max(0, Math.min(strip.count - 1, Math.floor(frac * strip.count)));
    return { x: i % strip.cols, y: Math.floor(i / strip.cols) };
  }

  function updateScrub(e: PointerEvent) {
    if (!isVideo || !settings.s.liveScrub) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    scrub = Math.max(0, Math.min(0.999, (e.clientX - rect.left) / Math.max(1, rect.width)));
  }

  function enterThumb(e: PointerEvent) {
    updateScrub(e);
    if (!isVideo || !settings.s.liveScrub || strip || scrubTimer) return;
    const path = item.path;
    scrubTimer = setTimeout(() => {
      scrubTimer = null;
      loadVideoScrubstrip(path).then((s) => {
        if (item.path === path && settings.s.liveScrub && s) strip = s;
      });
    }, 160);
  }

  function leaveThumb() {
    scrub = null;
    if (scrubTimer) {
      clearTimeout(scrubTimer);
      scrubTimer = null;
      if (item.kind === "video") cancelVideoScrubstrip(item.path);
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="thumb" onpointerenter={enterThumb} onpointermove={updateScrub} onpointerleave={leaveThumb}>
  {#if src}
    <img
      class="media"
      class:in={loaded}
      {src}
      alt={item.name}
      draggable="false"
      decoding="async"
      onload={() => (loaded = true)}
    />
    {#if isVideo && strip && scrub != null}
      {@const cell = framePos(scrub)}
      <div
        class="scrubLayer"
        style="background-image:url('{strip.src}'); background-size:{strip.cols * 100}% {strip.rows * 100}%; background-position:{strip.cols <= 1 ? 0 : (cell.x / (strip.cols - 1)) * 100}% {strip.rows <= 1 ? 0 : (cell.y / (strip.rows - 1)) * 100}%"
      ></div>
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
    inset: 0;
    z-index: 1;
    background-repeat: no-repeat;
    background-color: #050505;
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
