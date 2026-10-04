<script lang="ts">
  // The Merge window's preview of one clip (owner, 2026-10-04): it opens at
  // the clip's first in point and plays only what goes into the merge, its
  // marked segments one after another (the whole clip when it goes in whole).
  // The bar shows the whole clip with those parts lit; drag along it to scrub
  // the video, hover to glimpse a frame from the cached filmstrip (so merely
  // passing over the bar never makes a 4K original seek).
  //
  // Nothing autoplays: the window used to have no Play at all because playing
  // an original from a card was slow, so playing is always the owner's click.
  import { onDestroy } from "svelte";
  import { api } from "$lib/api";
  import type { FilmstripInfo, MediaItem } from "$lib/types";
  import { fmtT, type Seg } from "$lib/segments";
  import Thumb from "./Thumb.svelte";

  let {
    item,
    path,
    duration,
    segments,
  }: {
    /** For the fallback tile when the webview can't decode the clip. */
    item: MediaItem | undefined;
    path: string;
    /** From the probe; the video's own metadata wins once it's loaded. */
    duration: number;
    /** The parts that go in, in order. Empty = the whole clip. */
    segments: Seg[];
  } = $props();

  let video = $state<HTMLVideoElement | null>(null);
  let src = $state<string | null>(null);
  let failed = $state(false);
  let playing = $state(false);
  let t = $state(0);
  let metaDur = $state(0);
  let strip = $state<FilmstripInfo | null>(null);
  let hoverT = $state<number | null>(null);
  let hoverX = $state(0);
  let barEl = $state<HTMLDivElement | null>(null);
  let dragging = false;
  let raf = 0;

  let dur = $derived(metaDur || duration || 0);
  let parts = $derived(segments.length ? segments : dur > 0 ? [{ in_s: 0, out_s: dur }] : []);
  let firstIn = $derived(parts[0]?.in_s ?? 0);
  /** Which part the playhead is in (or the next one), for the label. */
  let partIdx = $derived(parts.findIndex((s) => t < s.out_s - 0.02));

  // A new clip: drop the old decoder before the next one opens.
  $effect(() => {
    const p = path;
    stopLoop();
    playing = false;
    failed = false;
    src = null;
    metaDur = 0;
    strip = null;
    hoverT = null;
    let alive = true;
    api.loupeSrc(p).then(
      (s) => alive && (src = api.fileSrc(s)),
      () => alive && (failed = true),
    );
    api.videoFilmstripCached(p).then((f) => {
      if (alive && f) strip = { ...f, src: api.fileSrc(f.src) };
    });
    return () => {
      alive = false;
    };
  });

  // The choice changed (segments ↔ whole clip, or the segments themselves):
  // when stopped, go back to the start of what plays.
  let partsKey = $derived(parts.map((s) => `${s.in_s.toFixed(3)}-${s.out_s.toFixed(3)}`).join(","));
  $effect(() => {
    void partsKey;
    if (video && !playing && video.readyState >= 1) seek(firstIn);
  });

  function onMeta() {
    if (!video) return;
    if (Number.isFinite(video.duration)) metaDur = video.duration;
    seek(firstIn);
  }

  function seek(s: number, fast = false) {
    if (!video) return;
    s = Math.max(0, Math.min(s, dur || s));
    t = s;
    // fastSeek lands on a keyframe: right for dragging, wrong for landing.
    if (fast && typeof video.fastSeek === "function") video.fastSeek(s);
    else video.currentTime = s;
  }

  function stopLoop() {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
  }

  // Keep playback inside the parts: at a part's out, jump to the next in;
  // after the last, stop back at the first in.
  function tick() {
    raf = 0;
    if (!video || video.paused) return;
    const now = video.currentTime;
    t = now;
    const i = parts.findIndex((s) => now < s.out_s - 0.02);
    if (i < 0) {
      video.pause();
      seek(firstIn);
      return;
    }
    if (now < parts[i].in_s - 0.05) video.currentTime = parts[i].in_s;
    raf = requestAnimationFrame(tick);
  }

  export async function toggle() {
    if (!video || failed || !parts.length) return;
    if (!video.paused) {
      video.pause();
      return;
    }
    // From wherever the playhead is, if that's inside a part; else the next
    // part (or the first, from the end).
    const now = video.currentTime;
    const inside = parts.some((s) => now >= s.in_s - 0.05 && now < s.out_s - 0.05);
    if (!inside) {
      const next = parts.find((s) => s.in_s > now);
      seek(next ? next.in_s : firstIn);
    }
    try {
      await video.play();
    } catch {
      /* the element reports the failure through onerror */
    }
  }

  function onPlay() {
    playing = true;
    stopLoop();
    raf = requestAnimationFrame(tick);
  }
  function onPause() {
    playing = false;
    stopLoop();
    if (video) t = video.currentTime;
  }

  // ── the bar ───────────────────────────────────────────────────────────────
  function timeAt(clientX: number) {
    const r = barEl?.getBoundingClientRect();
    if (!r || !r.width || !dur) return 0;
    return Math.max(0, Math.min(1, (clientX - r.left) / r.width)) * dur;
  }
  function onBarDown(e: PointerEvent) {
    if (e.button !== 0 || !video) return;
    e.preventDefault();
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    seek(timeAt(e.clientX), true);
  }
  function onBarMove(e: PointerEvent) {
    const r = barEl?.getBoundingClientRect();
    // The glimpse stays inside the bar's ends (it's 128px wide).
    if (r) hoverX = Math.max(Math.min(66, r.width / 2), Math.min(r.width - 66, e.clientX - r.left));
    hoverT = timeAt(e.clientX);
    if (dragging) seek(hoverT, true);
  }
  function onBarUp(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    seek(timeAt(e.clientX)); // land on the exact frame
  }
  function onBarKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 5 : 1;
    if (e.key === "ArrowLeft") seek(t - step);
    else if (e.key === "ArrowRight") seek(t + step);
    else if (e.key === "Home") seek(firstIn);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  const pct = (s: number) => (dur ? `${(Math.max(0, Math.min(s, dur)) / dur) * 100}%` : "0%");

  /** The filmstrip frame for a time, as a CSS background on a box `w` wide. */
  function frameStyle(s: number, w: number) {
    const f = strip;
    if (!f || !f.count || !f.tile_w) return "";
    const i = Math.min(f.count - 1, Math.max(0, Math.floor((s / (f.duration || dur || 1)) * f.count)));
    const scale = w / f.tile_w;
    const col = i % f.cols;
    const row = Math.floor(i / f.cols);
    return `width:${w}px;height:${Math.round(f.tile_h * scale)}px;background-image:url("${f.src}");background-size:${f.cols * f.tile_w * scale}px ${f.rows * f.tile_h * scale}px;background-position:-${col * f.tile_w * scale}px -${row * f.tile_h * scale}px`;
  }

  /** Free the decoder when an element goes (next clip, window closed): a 4K
   *  HEVC decoder holds a lot of memory until the element is collected. */
  function release(el: HTMLVideoElement) {
    return {
      destroy() {
        el.pause();
        el.removeAttribute("src");
        el.load();
      },
    };
  }

  onDestroy(stopLoop);
</script>

<div class="sp">
  {#if failed}
    <!-- The webview can't decode this one (10-bit HEVC on some Windows
         machines): the hover tile still shows what it is. -->
    <div class="stage">{#if item}<Thumb {item} size={480} armed />{/if}</div>
    <div class="fallback">This clip can't play in the preview here. Hover to scrub; it still merges.</div>
  {:else}
    {#if src}
      <!-- One element per clip, so each opens fresh at its own first in. -->
      {#key path}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video
        bind:this={video}
        use:release
        {src}
        preload="metadata"
        playsinline
        onloadedmetadata={onMeta}
        onplay={onPlay}
        onpause={onPause}
        onerror={() => (failed = true)}
        onclick={() => void toggle()}
      ></video>
      {/key}
    {/if}
    <div class="controls">
      <button class="play" onclick={() => void toggle()} disabled={!src || !parts.length} title={playing ? "Pause (Space)" : segments.length ? "Play the segments that go in (Space)" : "Play (Space)"} aria-label={playing ? "Pause" : "Play"}>
        {#if playing}
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1" /><rect x="14" y="5" width="4" height="14" rx="1" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true"><path d="M8 5.5v13l11-6.5z" /></svg>
        {/if}
      </button>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="bar"
        bind:this={barEl}
        role="slider"
        tabindex="0"
        aria-label="Scrub"
        aria-valuemin={0}
        aria-valuemax={Math.round(dur)}
        aria-valuenow={Math.round(t)}
        onpointerdown={onBarDown}
        onpointermove={onBarMove}
        onpointerup={onBarUp}
        onpointercancel={() => (dragging = false)}
        onpointerleave={() => (hoverT = null)}
        onkeydown={onBarKey}
      >
        <div class="track" class:whole={!segments.length}>
          {#each segments as s, i (i)}
            <span class="part" style:left={pct(s.in_s)} style:width={`calc(${pct(s.out_s)} - ${pct(s.in_s)})`}></span>
          {/each}
        </div>
        <span class="head" style:left={pct(t)}></span>
        {#if hoverT != null}
          <div class="glimpse" style:left={`${hoverX}px`}>
            {#if strip}<span class="gframe" style={frameStyle(hoverT, 128)}></span>{/if}
            <span class="gtime">{fmtT(hoverT)}</span>
          </div>
        {/if}
      </div>
      <span class="time">
        {fmtT(t)}{#if segments.length}<span class="which">{" · "}{partIdx >= 0 ? `segment ${partIdx + 1} of ${segments.length}` : `${segments.length} segment${segments.length === 1 ? "" : "s"}`}</span>{/if}
      </span>
    </div>
  {/if}
</div>

<style>
  .sp { position: absolute; inset: 0; display: flex; flex-direction: column; background: #050607; }
  .stage { flex: 1; min-height: 0; }
  video { flex: 1; min-height: 0; width: 100%; object-fit: contain; background: #050607; cursor: pointer; }
  .fallback { position: absolute; left: 0; right: 0; bottom: 0; padding: 8px 10px; background: linear-gradient(transparent, rgba(0, 0, 0, 0.75)); color: rgba(255, 255, 255, 0.78); font-size: 11.5px; pointer-events: none; }
  .controls { flex: none; display: flex; align-items: center; gap: 8px; padding: 6px 10px 8px; background: #0b0d10; color: rgba(255, 255, 255, 0.85); }
  .play { flex: none; display: grid; place-items: center; width: 28px; height: 28px; border-radius: 7px; color: #fff; background: rgba(255, 255, 255, 0.1); }
  .play:hover:not(:disabled) { background: rgba(255, 255, 255, 0.18); }
  .play:disabled { opacity: 0.4; }
  .bar { position: relative; flex: 1; min-width: 0; height: 22px; display: flex; align-items: center; cursor: pointer; outline: none; touch-action: none; }
  .bar:focus-visible .track { box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 60%, transparent); }
  /* The whole clip, dim; the parts that go in, lit. */
  .track { position: relative; width: 100%; height: 6px; border-radius: 3px; background: rgba(255, 255, 255, 0.14); }
  .track.whole { background: color-mix(in srgb, var(--accent) 70%, transparent); }
  .part { position: absolute; top: 0; bottom: 0; border-radius: 2px; background: var(--accent); box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 40%, transparent); }
  .head { position: absolute; top: 2px; bottom: 2px; width: 2px; margin-left: -1px; border-radius: 1px; background: #fff; box-shadow: 0 0 3px rgba(0, 0, 0, 0.6); pointer-events: none; }
  .glimpse { position: absolute; bottom: 26px; transform: translateX(-50%); display: flex; flex-direction: column; align-items: center; gap: 3px; pointer-events: none; z-index: 2; }
  .gframe { display: block; border: 1px solid rgba(255, 255, 255, 0.5); border-radius: 4px; background-repeat: no-repeat; box-shadow: 0 4px 14px rgba(0, 0, 0, 0.5); }
  .gtime { padding: 1px 6px; border-radius: 4px; background: rgba(0, 0, 0, 0.8); color: #fff; font-size: 11px; font-variant-numeric: tabular-nums; }
  .time { flex: none; font-size: 11.5px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .which { color: rgba(255, 255, 255, 0.6); }
</style>
