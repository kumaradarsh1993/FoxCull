<script lang="ts">
  import { api } from "$lib/api";
  import { activity } from "$lib/activity.svelte";
  import { loadThumb } from "$lib/thumbnail-loader";
  import { settings } from "$lib/settings.svelte";
  import type { MediaItem, FilmstripInfo, MediaProbe, VideoSegment } from "$lib/types";

  let {
    item,
    showInfo = false,
    onchanged = () => {},
  }: {
    item: MediaItem | null;
    showInfo?: boolean;
    onchanged?: (selectPath?: string | null) => void;
  } = $props();

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
  let dur = $state(0);
  let cur = $state(0);
  let inS = $state(0);
  let outS = $state<number | null>(null); // null = end
  let segments = $state<VideoSegment[]>([]);
  let exporting = $state(false);
  let exportingSegments = $state(false);
  let exportNote = $state<string | null>(null);
  let probe = $state<MediaProbe | null>(null);
  let clipToolsOpen = $state(false);

  // ── filmstrip scrub state ──
  let strip = $state<FilmstripInfo | null>(null);
  let stripSrc = $derived(strip ? api.fileSrc(strip.src) : null);
  let preview = $state<number | null>(null); // fraction 0..1 to preview, or null
  let scrubbing = $state(false);
  let trackEl = $state<HTMLDivElement | null>(null);
  let infoVisible = $state(false);
  let pendingSeek: number | null = null;
  let seekRAF = 0;
  let resumeAfterScrub = false;
  let lastSeekAt = 0;
  const SEEK_THROTTLE_MS = 55;
  const PREVIEW_W = 200;
  let previewH = $derived(
    strip ? Math.round((PREVIEW_W * strip.tile_h) / strip.tile_w) : 0,
  );

  $effect(() => {
    const it = item;
    const liveScrub = settings.s.liveScrub;
    const my = ++epoch;
    failed = false;
    videoErr = false;
    usingProxy = false;
    converting = false;
    proxyNote = null;
    paused = !settings.s.videoAutoplay;
    dur = 0;
    cur = 0;
    inS = 0;
    outS = null;
    segments = [];
    exportNote = null;
    probe = null;
    clipToolsOpen = false;
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
      api.getTrim(it.path).then((t) => {
        if (my === epoch && t) {
          inS = t[0];
          outS = t[1];
        }
      });
      api.getVideoSegments(it.path).then((s) => {
        if (my === epoch) segments = s;
      });
      api.probeMediaInfo(it.path).then((p) => {
        if (my === epoch) probe = p;
      }).catch(() => {});
      // Build/fetch the scrub filmstrip only when Live Scrub is enabled. Failure
      // leaves the timeline as a plain seek bar with no frame preview.
      if (liveScrub) {
        api
          .videoFilmstrip(it.path)
          .then((f) => {
            if (my === epoch && settings.s.liveScrub) strip = f;
          })
          .catch(() => {});
      }
      api
        .loupeSrc(it.path)
        .then((p) => {
          if (my === epoch) vsrc = api.fileSrc(p);
        })
        .catch(() => {
          if (my === epoch) failed = true;
        });
      return;
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

  $effect(() => {
    infoVisible = showInfo;
  });

  function onMeta() {
    if (vid) dur = vid.duration || 0;
  }

  // ── playback (exposed to the page's global key handler) ──
  export function togglePlay() {
    if (!vid) return;
    if (vid.paused) vid.play().catch(() => {});
    else vid.pause();
  }
  export function seekBy(d: number) {
    if (!vid) return;
    const max = dur || strip?.duration || vid.duration || 0;
    let t = vid.currentTime + d;
    if (t < 0) t = 0;
    if (max > 0 && t > max) t = max;
    vid.currentTime = t;
    cur = t;
  }
  export function setInPoint() {
    setIn();
  }
  export function setOutPoint() {
    setOut();
  }

  // ── timeline scrub: hover previews a frame, drag seeks the real video ──
  function fracFromEvent(e: PointerEvent): number {
    if (!trackEl) return 0;
    const r = trackEl.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  }
  function applySeek(frac: number, final = false) {
    const d = dur || strip?.duration || 0;
    if (vid && d > 0) {
      const t = frac * d;
      cur = t;
      const now = performance.now();
      if (!final && now - lastSeekAt < SEEK_THROTTLE_MS) return;
      lastSeekAt = now;
      if ("fastSeek" in vid && typeof vid.fastSeek === "function") {
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
  function onTrackDown(e: PointerEvent) {
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
    const f = fracFromEvent(e);
    preview = f;
    if (scrubbing) seekTo(f);
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
    if (vid) cur = vid.currentTime || 0;
  }
  function setIn() {
    inS = cur;
    if (outS != null && outS <= inS) outS = null;
    persist();
  }
  function setOut() {
    outS = cur;
    if (outS <= inS) inS = 0;
    persist();
  }
  function resetTrim() {
    inS = 0;
    outS = null;
    if (item) api.clearTrim(item.path);
    exportNote = null;
  }
  function persist() {
    if (item) api.setTrim(item.path, inS, outS ?? dur);
  }

  function sortedSegments(next = segments) {
    return [...next]
      .filter((s) => Number.isFinite(s.in_s) && Number.isFinite(s.out_s) && s.out_s > s.in_s)
      .sort((a, b) => a.in_s - b.in_s);
  }

  function persistSegments(next = segments) {
    if (item) api.setVideoSegments(item.path, sortedSegments(next));
  }

  function addSegment() {
    if (!item || !dur || !canExport) return;
    const end = outS ?? dur;
    const next = sortedSegments([...segments, { in_s: Math.max(0, inS), out_s: Math.min(dur, end) }]);
    segments = next;
    persistSegments(next);
    exportNote = `Marked ${next.length} subclip${next.length === 1 ? "" : "s"}`;
  }

  function removeSegment(idx: number) {
    const next = segments.filter((_, i) => i !== idx);
    segments = next;
    persistSegments(next);
  }

  function useSegment(segment: VideoSegment) {
    inS = segment.in_s;
    outS = segment.out_s;
    if (vid) vid.currentTime = segment.in_s;
    persist();
  }

  async function exportCut() {
    if (!item || exporting) return;
    const end = outS ?? dur;
    if (end <= inS) return;
    exporting = true;
    exportNote = "Cutting…";
    try {
      const out = await api.trimVideo(item.path, inS, end);
      exportNote = `Saved ${out.split(/[\\/]/).pop()}`;
      api.reveal(out);
      onchanged(out);
    } catch (e) {
      exportNote = `Couldn't cut (${e})`;
    } finally {
      exporting = false;
    }
  }

  async function exportSegments() {
    if (!item || exportingSegments || !segments.length) return;
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
  let canExport = $derived(dur > 0 && (outS ?? dur) > inS && (inS > 0 || (outS ?? dur) < dur));
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
        <!-- svelte-ignore a11y_media_has_caption -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <video
          bind:this={vid}
          src={vsrc}
          autoplay={settings.s.videoAutoplay}
          preload="auto"
          playsinline
          onclick={togglePlay}
          onloadedmetadata={onMeta}
          ontimeupdate={onTime}
          onplay={() => (paused = false)}
          onpause={() => (paused = true)}
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
        {#if usingProxy}
          <span class="proxytag" title="The original couldn't decode in-app; you're watching the cached H.264 conversion. Trim still cuts the original.">converted preview</span>
        {/if}
        <div class="trim">
          <div class="playrow">
            <button class="pp" onclick={togglePlay} title={paused ? "Play (Space)" : "Pause (Space)"}>
              {paused ? "▶" : "⏸"}
            </button>
            <span class="time">{fmt(cur)} <span class="sep">/</span> {fmt(dur)}</span>
            <span class="spacer"></span>
            <button class="miniToggle" class:on={infoVisible} onclick={() => (infoVisible = !infoVisible)} title="Show file information overlay">Info</button>
            <span class="khint">Space play · Shift+← → seek</span>
          </div>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="track"
            class:scrubbing
            bind:this={trackEl}
            onpointerdown={onTrackDown}
            onpointermove={onTrackMove}
            onpointerup={onTrackUp}
            onpointerleave={onTrackLeave}
          >
            <div class="range" style="left:{pct(inS)}%; right:{100 - pct(outS ?? dur)}%"></div>
            {#each segments as segment, i (i)}
              <button
                class="segmark"
                style="left:{pct(segment.in_s)}%; width:{Math.max(0.8, pct(segment.out_s) - pct(segment.in_s))}%"
                title={`Subclip ${i + 1}: ${fmt(segment.in_s)}-${fmt(segment.out_s)}`}
                onpointerdown={(e) => e.stopPropagation()}
                onclick={(e) => {
                  e.stopPropagation();
                  useSegment(segment);
                }}
              ></button>
            {/each}
            <div class="cursor" style="left:{pct(cur)}%"></div>
            {#if preview != null && strip && stripSrc}
              {@const c = cellPos(preview)}
              <div
                class="scrubprev"
                style="left:{preview * 100}%; width:{PREVIEW_W}px; height:{previewH}px;
                       background-image:url('{stripSrc}');
                       background-size:{strip.cols * 100}% {strip.rows * 100}%;
                       background-position:{c.x}% {c.y}%;"
              >
                <span class="ts">{fmt(preview * (dur || strip.duration))}</span>
              </div>
            {/if}
          </div>
          <div class="clipToolsBar">
            <button class="miniToggle" class:on={clipToolsOpen} onclick={() => (clipToolsOpen = !clipToolsOpen)}>
              Clip tools
            </button>
            <span>{fmt(inS)} - {fmt(outS ?? dur)} ({fmt((outS ?? dur) - inS)})</span>
            {#if segments.length}<span>{segments.length} marked</span>{/if}
          </div>
          {#if clipToolsOpen}
            <div class="ctrls">
              <button onclick={setIn} title="Set in point to current time">In {fmt(inS)}</button>
              <button onclick={setOut} title="Set out point to current time">Out {fmt(outS ?? dur)}</button>
              <span class="len">range {fmt((outS ?? dur) - inS)}</span>
              <button onclick={addSegment} disabled={!canExport} title="Remember this range as one subclip">Mark range</button>
              <span class="spacer"></span>
              {#if canExport}<button class="reset" onclick={resetTrim}>Reset</button>{/if}
              <button class="exp" onclick={exportCut} disabled={!canExport || exporting}>
                {exporting ? "Saving..." : "Save current range"}
              </button>
              <button class="exp secondary" onclick={exportSegments} disabled={!segments.length || exportingSegments}>
                {exportingSegments ? "Saving..." : `Save ${segments.length || ""} marked`}
              </button>
            </div>
          {/if}
          {#if clipToolsOpen && segments.length}
            <div class="segments">
              {#each segments as segment, i (i)}
                <button class="segmentPill" onclick={() => useSegment(segment)}>
                  <strong>{i + 1}</strong>
                  <span>{fmt(segment.in_s)}-{fmt(segment.out_s)}</span>
                  <em>{fmt(segment.out_s - segment.in_s)}</em>
                </button>
                <button class="segRemove" onclick={() => removeSegment(i)} title="Remove subclip">×</button>
              {/each}
            </div>
          {/if}
          {#if exportNote}<div class="note">{exportNote}</div>{/if}
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
    background: #0c0b0a;
    overflow: hidden;
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
    font-size: 14px;
  }

  .vwrap {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .vwrap video {
    flex: 1;
    min-height: 0;
    width: 100%;
  }
  .trim {
    flex: 0 0 auto;
    background: var(--bg-panel);
    border-top: 1px solid var(--border);
    padding: 8px 12px 10px;
  }
  .playrow {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 7px;
  }
  .playrow .pp {
    width: 34px;
    height: 30px;
    border-radius: 7px;
    border: 1px solid var(--border);
    background: var(--bg-elev);
    color: var(--text);
    font-size: 13px;
    line-height: 1;
  }
  .playrow .pp:hover {
    background: var(--bg-hover);
  }
  .playrow .time {
    font-size: 12.5px;
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
  }
  .playrow .time .sep {
    color: var(--text-faint);
    margin: 0 1px;
  }
  .playrow .khint {
    font-size: 11px;
    color: var(--text-faint);
  }
  .miniToggle {
    padding: 3px 7px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-elev);
    color: var(--text-dim);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .miniToggle.on {
    border-color: var(--accent);
    color: var(--accent);
  }
  .track {
    position: relative;
    height: 16px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--text-faint) 30%, transparent);
    margin-bottom: 8px;
    cursor: pointer;
    touch-action: none; /* let pointer-drag scrub instead of scrolling */
  }
  .track.scrubbing {
    cursor: grabbing;
  }
  .range {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, var(--accent) 55%, transparent);
    border-radius: 6px;
    pointer-events: none;
  }
  .segmark {
    position: absolute;
    top: -3px;
    bottom: -3px;
    min-width: 3px;
    border: 1px solid rgba(255, 255, 255, 0.75);
    border-radius: 5px;
    background: color-mix(in srgb, var(--pick) 56%, transparent);
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.28);
    cursor: pointer;
    z-index: 3;
  }
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
    border-radius: 7px;
    border: 1px solid rgba(255, 255, 255, 0.18);
    background-color: #000;
    background-repeat: no-repeat;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.55);
    pointer-events: none;
    overflow: hidden;
    z-index: 60;
  }
  .scrubprev .ts {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    text-align: center;
    font-size: 11px;
    line-height: 1.5;
    color: #fff;
    background: rgba(0, 0, 0, 0.55);
    font-variant-numeric: tabular-nums;
  }
  .clipToolsBar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    color: var(--text-dim);
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }
  .clipToolsBar span {
    white-space: nowrap;
  }
  .ctrls {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .ctrls button {
    padding: 4px 10px;
    border-radius: 7px;
    border: 1px solid var(--border);
    background: var(--bg-elev);
    color: var(--text);
    font-size: 12.5px;
  }
  .ctrls button:hover {
    background: var(--bg-hover);
  }
  .len {
    color: var(--text-dim);
    font-size: 12px;
  }
  .spacer {
    flex: 1;
  }
  .ctrls .exp {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-on);
    font-weight: 600;
  }
  .ctrls .exp.secondary {
    background: color-mix(in srgb, var(--pick) 22%, var(--bg-elev));
    border-color: color-mix(in srgb, var(--pick) 55%, var(--border));
    color: var(--text);
  }
  .ctrls .exp:disabled {
    opacity: 0.45;
  }
  .segments {
    display: flex;
    align-items: center;
    gap: 5px;
    flex-wrap: wrap;
    margin-top: 8px;
  }
  .segmentPill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 24px;
    padding: 3px 8px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--pick) 55%, var(--border));
    background: color-mix(in srgb, var(--pick) 12%, var(--bg-elev));
    color: var(--text);
    font-size: 11.5px;
  }
  .segmentPill strong {
    color: var(--pick);
  }
  .segmentPill em {
    color: var(--text-faint);
    font-style: normal;
  }
  .segRemove {
    width: 22px;
    height: 22px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--reject) 50%, var(--border));
    color: var(--reject);
    background: var(--bg-elev);
    line-height: 1;
  }
  .note {
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-dim);
  }
  .infoOverlay {
    position: absolute;
    left: 22px;
    top: 22px;
    z-index: 20;
    max-width: min(560px, calc(100% - 44px));
    padding: 11px 13px;
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.42);
    color: #fff;
    font-size: 14px;
    line-height: 1.45;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.35);
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
    font-weight: 600;
    font-size: 15px;
    margin: 0;
  }
  .vfail p {
    margin: 0;
    line-height: 1.5;
  }
  .obtn {
    margin-top: 4px;
    padding: 9px 16px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-on);
    font-size: 13.5px;
    font-weight: 600;
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
    font-weight: 500;
  }
  .vfail .subnote {
    margin: 0;
    font-size: 12px;
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
    font-size: 11px;
    pointer-events: auto;
    z-index: 5;
  }
</style>
