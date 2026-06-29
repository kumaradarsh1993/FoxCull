<script lang="ts">
  import { api } from "$lib/api";
  import type { EditAdjustments, EditExportRequest, MediaItem } from "$lib/types";
  import Thumb from "./Thumb.svelte";

  let {
    active,
    selectedItems,
    sourceItems = [],
  }: {
    active: MediaItem | null;
    selectedItems: MediaItem[];
    sourceItems?: MediaItem[];
  } = $props();

  type PresetId = "instagram" | "square" | "landscape" | "original";
  type Encoder = "auto" | "x264" | "nvenc";
  type Quality = "best" | "high" | "standard" | "small";

  type TimelineClip = {
    id: string;
    path: string;
    name: string;
    src: string;
    inS: number;
    outS: number;
    duration: number;
    cropX: number;
    cropY: number;
    zoom: number;
  };

  const PRESETS: Record<PresetId, { label: string; detail: string; w: number; h: number; fit: "crop" | "original" }> = {
    instagram: { label: "9:16", detail: "1080x1920", w: 1080, h: 1920, fit: "crop" },
    square: { label: "1:1", detail: "1080x1080", w: 1080, h: 1080, fit: "crop" },
    landscape: { label: "16:9", detail: "1920x1080", w: 1920, h: 1080, fit: "crop" },
    original: { label: "Original", detail: "Stream-copy", w: 0, h: 0, fit: "original" },
  };

  const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  const extOf = (p: string) => (basename(p).match(/\.([^.]+)$/)?.[1] ?? "mp4").toLowerCase();
  const videoItemFromPath = (path: string): MediaItem => ({
    name: basename(path),
    path,
    rel: basename(path),
    kind: "video",
    ext: extOf(path),
    mtime: 0,
    size: 0,
    rating: 0,
    label: null,
    flag: null,
    tags: [],
  });
  const fmt = (s: number) => {
    if (!Number.isFinite(s) || s < 0) s = 0;
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, "0")}`;
  };

  let clips = $state<TimelineClip[]>([]);
  let selectedId = $state<string | null>(null);
  let preset = $state<PresetId>("instagram");
  let encoder = $state<Encoder>("auto");
  let quality = $state<Quality>("high");
  let musicPath = $state<string | null>(null);
  let preserveSourceAudio = $state(true);
  let exporting = $state(false);
  let exportNote = $state<string | null>(null);
  let previewVideo = $state<HTMLVideoElement | null>(null);
  let currentTime = $state(0);
  let extraSources = $state<MediaItem[]>([]);
  let sourceFocusPath = $state<string | null>(null);
  let pickingVideos = $state(false);
  let previewPreparing = $state(false);
  let dragSourcePath = $state<string | null>(null);
  let seededKey = $state("");
  let seeding = $state(false);
  let lastPreviewClipId = "";
  let dragStart:
    | { x: number; y: number; cropX: number; cropY: number; w: number; h: number }
    | null = null;

  let adjustments = $state<EditAdjustments>({
    brightness: 0,
    contrast: 1,
    saturation: 1,
    warmth: 0,
    sharpen: 0,
  });

  let activeVideo = $derived(active?.kind === "video" ? active : null);
  let selectedVideos = $derived(selectedItems.filter((i) => i.kind === "video"));
  let initialVideos = $derived.by(() => (selectedVideos.length ? selectedVideos : activeVideo ? [activeVideo] : []));
  let previewFilter = $derived(
    `brightness(${Math.max(0, 1 + adjustments.brightness)}) contrast(${adjustments.contrast}) saturate(${adjustments.saturation})`,
  );
  let sourceVideos = $derived.by(() => {
    const seen = new Set<string>();
    const out: MediaItem[] = [];
    for (const item of [...initialVideos, ...extraSources, ...sourceItems.filter((i) => i.kind === "video")]) {
      if (seen.has(item.path)) continue;
      seen.add(item.path);
      out.push(item);
    }
    return out;
  });

  let selectedClip = $derived(clips.find((c) => c.id === selectedId) ?? clips[0] ?? null);
  let focusedSource = $derived(sourceVideos.find((item) => item.path === sourceFocusPath) ?? sourceVideos[0] ?? null);
  let outPreset = $derived(PRESETS[preset]);
  let outAspect = $derived(outPreset.fit === "original" ? 9 / 16 : outPreset.w / outPreset.h);
  let timelineSeconds = $derived(clips.reduce((sum, c) => sum + Math.max(0, c.outS - c.inS), 0));
  let needsEncode = $derived(
    outPreset.fit !== "original" ||
      !!musicPath ||
      Math.abs(adjustments.brightness) > 0.001 ||
      Math.abs(adjustments.contrast - 1) > 0.001 ||
      Math.abs(adjustments.saturation - 1) > 0.001 ||
      Math.abs(adjustments.warmth) > 0.001 ||
      Math.abs(adjustments.sharpen) > 0.001,
  );

  $effect(() => {
    if (!clips.length) {
      selectedId = null;
      lastPreviewClipId = "";
      return;
    }
    if (!selectedId || !clips.some((c) => c.id === selectedId)) selectedId = clips[0].id;
  });

  $effect(() => {
    const first = sourceVideos[0]?.path ?? null;
    if (!sourceFocusPath || !sourceVideos.some((item) => item.path === sourceFocusPath)) {
      sourceFocusPath = first;
    }
  });

  $effect(() => {
    const key = initialVideos.map((i) => i.path).join("|");
    if (!key || clips.length || seeding || seededKey === key) return;
    seededKey = key;
    seeding = true;
    void addItems(initialVideos).finally(() => {
      seeding = false;
    });
  });

  $effect(() => {
    const clip = selectedClip;
    const video = previewVideo;
    if (!clip || !video || clip.id === lastPreviewClipId) return;
    lastPreviewClipId = clip.id;
    currentTime = clip.inS;
    video.currentTime = clip.inS;
  });

  function uid() {
    return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
  }

  async function durationFor(path: string): Promise<number> {
    const browserDuration = await new Promise<number>((resolve) => {
      const video = document.createElement("video");
      const done = (duration: number) => {
        clearTimeout(timer);
        resolve(duration);
      };
      const timer = setTimeout(() => done(0), 2500);
      video.preload = "metadata";
      video.src = api.fileSrc(path);
      video.onloadedmetadata = () => done(Number.isFinite(video.duration) ? video.duration : 0);
      video.onerror = () => done(0);
    });
    if (browserDuration > 0) return browserDuration;
    try {
      const filmstrip = await api.videoFilmstrip(path);
      if (filmstrip.duration > 0) return filmstrip.duration;
    } catch {
      /* fall through to a tiny editable segment */
    }
    return 0;
  }

  async function makeClip(item: MediaItem): Promise<TimelineClip> {
    const duration = await durationFor(item.path);
    const out = Math.max(0.1, duration || 1);
    const cachedProxy = await api.videoProxyCached(item.path);
    return {
      id: uid(),
      path: item.path,
      name: item.name,
      src: api.fileSrc(cachedProxy ?? item.path),
      inS: 0,
      outS: out,
      duration: out,
      cropX: 0.5,
      cropY: 0.5,
      zoom: 1,
    };
  }

  async function addItems(items: MediaItem[]) {
    const made: TimelineClip[] = [];
    for (const item of items) {
      if (item.kind === "video") made.push(await makeClip(item));
    }
    if (!made.length) return;
    clips = [...clips, ...made];
    selectedId = made[made.length - 1].id;
    exportNote = null;
  }

  async function addItem(item: MediaItem | null) {
    if (!item || item.kind !== "video") return;
    await addItems([item]);
  }

  async function chooseVideos() {
    if (pickingVideos) return;
    pickingVideos = true;
    exportNote = null;
    try {
      const picked = await api.pickVideos();
      const incoming = picked.map(videoItemFromPath);
      if (!incoming.length) return;

      const known = new Set(sourceVideos.map((item) => item.path));
      const fresh = incoming.filter((item) => !known.has(item.path));
      if (fresh.length) extraSources = [...extraSources, ...fresh];

      sourceFocusPath = incoming[incoming.length - 1].path;
      await addItems(incoming);
    } catch (e) {
      exportNote = `Could not add video: ${e}`;
    } finally {
      pickingVideos = false;
    }
  }

  async function ensurePreviewProxy(clip: TimelineClip) {
    if (previewPreparing || clip.src !== api.fileSrc(clip.path)) return;
    previewPreparing = true;
    exportNote = "Preparing preview";
    try {
      const proxy = await api.videoProxy(clip.path);
      clips = clips.map((c) => (c.id === clip.id ? { ...c, src: api.fileSrc(proxy) } : c));
      exportNote = null;
    } catch (e) {
      exportNote = `Preview unavailable: ${e}`;
    } finally {
      previewPreparing = false;
    }
  }

  function onPreviewError() {
    if (selectedClip) void ensurePreviewProxy(selectedClip);
  }

  function removeClip(id: string) {
    clips = clips.filter((c) => c.id !== id);
    exportNote = null;
  }

  function moveClip(id: string, delta: number) {
    const i = clips.findIndex((c) => c.id === id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= clips.length) return;
    const next = [...clips];
    [next[i], next[j]] = [next[j], next[i]];
    clips = next;
  }

  function duplicateClip(clip: TimelineClip) {
    const copy = { ...clip, id: uid(), name: `${clip.name} copy` };
    const i = clips.findIndex((c) => c.id === clip.id);
    clips = [...clips.slice(0, i + 1), copy, ...clips.slice(i + 1)];
    selectedId = copy.id;
  }

  function updateClip(patch: Partial<TimelineClip>) {
    if (!selectedClip) return;
    const id = selectedClip.id;
    clips = clips.map((clip) => (clip.id === id ? { ...clip, ...patch } : clip));
  }

  function clampTrim() {
    const clip = selectedClip;
    if (!clip) return;
    const inS = Math.max(0, Math.min(clip.inS, Math.max(0, clip.outS - 0.05)));
    const outS = Math.min(clip.duration, Math.max(clip.outS, inS + 0.05));
    updateClip({ inS, outS });
  }

  function onMeta() {
    if (!previewVideo || !selectedClip) return;
    const d = Number.isFinite(previewVideo.duration) ? previewVideo.duration : selectedClip.duration;
    if (d > 0 && Math.abs(d - selectedClip.duration) > 0.01) {
      updateClip({ duration: d, outS: Math.min(selectedClip.outS || d, d) });
    }
  }

  function onTime() {
    if (!previewVideo || !selectedClip) return;
    currentTime = previewVideo.currentTime || 0;
    if (currentTime > selectedClip.outS) {
      previewVideo.pause();
      previewVideo.currentTime = selectedClip.inS;
    }
  }

  function seek(t: number) {
    if (!previewVideo || !selectedClip) return;
    const next = Math.max(0, Math.min(t, selectedClip.duration));
    previewVideo.currentTime = next;
    currentTime = next;
  }

  function togglePlay() {
    if (!previewVideo || !selectedClip) return;
    if (previewVideo.paused) {
      if (previewVideo.currentTime < selectedClip.inS || previewVideo.currentTime >= selectedClip.outS) {
        previewVideo.currentTime = selectedClip.inS;
      }
      previewVideo.play().catch(() => {});
    } else {
      previewVideo.pause();
    }
  }

  function setIn() {
    if (!selectedClip) return;
    updateClip({ inS: Math.min(currentTime, selectedClip.outS - 0.05) });
    clampTrim();
  }

  function setOut() {
    if (!selectedClip) return;
    updateClip({ outS: Math.max(currentTime, selectedClip.inS + 0.05) });
    clampTrim();
  }

  function resetColor() {
    adjustments = { brightness: 0, contrast: 1, saturation: 1, warmth: 0, sharpen: 0 };
  }

  async function pickMusic() {
    const picked = await api.pickAudio();
    if (picked) musicPath = picked;
  }

  function exportName() {
    const first = clips[0]?.name ?? "clip";
    const stem = first.replace(/\.[^.]+$/, "");
    if (preset === "instagram") return `${stem}_reel`;
    if (preset === "original") return `${stem}_edit`;
    return `${stem}_${preset}`;
  }

  async function exportTimeline() {
    if (!clips.length || exporting) return;
    exporting = true;
    exportNote = "Exporting";
    try {
      const req: EditExportRequest = {
        clips: clips.map((clip) => ({
          path: clip.path,
          in_s: clip.inS,
          out_s: clip.outS,
          crop_x: clip.cropX,
          crop_y: clip.cropY,
          zoom: clip.zoom,
        })),
        output_w: outPreset.w,
        output_h: outPreset.h,
        fit: outPreset.fit,
        encoder,
        quality,
        adjustments,
        music_path: musicPath,
        preserve_source_audio: preserveSourceAudio && !musicPath && clips.length === 1,
        destination: null,
        basename: exportName(),
      };
      const out = await api.editExport(req);
      exportNote = `Saved ${basename(out.path)} (${out.reencoded ? out.mode : "stream copy"})`;
      api.reveal(out.path);
    } catch (e) {
      exportNote = `Export failed: ${e}`;
    } finally {
      exporting = false;
    }
  }

  function startCropDrag(e: PointerEvent) {
    if (!selectedClip || outPreset.fit === "original") return;
    const stage = (e.currentTarget as HTMLElement).parentElement?.getBoundingClientRect();
    if (!stage) return;
    dragStart = {
      x: e.clientX,
      y: e.clientY,
      cropX: selectedClip.cropX,
      cropY: selectedClip.cropY,
      w: stage.width,
      h: stage.height,
    };
    window.addEventListener("pointermove", onCropDrag);
    window.addEventListener("pointerup", endCropDrag, { once: true });
  }

  function onCropDrag(e: PointerEvent) {
    if (!dragStart || !selectedClip) return;
    const cropX = Math.max(0, Math.min(1, dragStart.cropX + (e.clientX - dragStart.x) / (dragStart.w * 0.42)));
    const cropY = Math.max(0, Math.min(1, dragStart.cropY + (e.clientY - dragStart.y) / (dragStart.h * 0.42)));
    updateClip({ cropX, cropY });
  }

  function endCropDrag() {
    dragStart = null;
    window.removeEventListener("pointermove", onCropDrag);
  }

  function startSourceDrag(e: DragEvent, item: MediaItem) {
    dragSourcePath = item.path;
    e.dataTransfer?.setData("application/x-foxcull-edit-path", item.path);
    e.dataTransfer?.setData("text/plain", item.path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "copy";
  }

  function endSourceDrag() {
    dragSourcePath = null;
  }

  function allowTimelineDrop(e: DragEvent) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
  }

  function dropOnTimeline(e: DragEvent) {
    e.preventDefault();
    const path = e.dataTransfer?.getData("application/x-foxcull-edit-path") || dragSourcePath;
    const item = sourceVideos.find((v) => v.path === path);
    if (item) void addItem(item);
    dragSourcePath = null;
  }
</script>

<div class="editShell">
  <aside class="sourcePane">
    <div class="paneHead">
      <div>
        <strong>Source</strong>
        <span>{sourceVideos.length} video{sourceVideos.length === 1 ? "" : "s"}</span>
      </div>
      <div class="sourceActions">
        <button class="miniBtn" onclick={chooseVideos} disabled={pickingVideos}>
          {pickingVideos ? "Choosing" : "Choose videos"}
        </button>
        <button class="miniBtn" onclick={() => addItem(focusedSource)} disabled={!focusedSource}>Add source</button>
        <button class="miniBtn" onclick={() => addItems(initialVideos)} disabled={!initialVideos.length}>Add selected</button>
      </div>
    </div>

    <div class="sourceList">
      {#if sourceVideos.length}
        {#each sourceVideos as item (item.path)}
          <div
            class="sourceItem"
            class:active={selectedClip?.path === item.path}
            class:focused={sourceFocusPath === item.path}
            role="button"
            tabindex="0"
            draggable={true}
            ondragstart={(e) => startSourceDrag(e, item)}
            ondragend={endSourceDrag}
            onclick={() => (sourceFocusPath = item.path)}
            ondblclick={() => addItem(item)}
            onkeydown={(e) => {
              if (e.key === "Enter") sourceFocusPath = item.path;
              if (e.key === " ") {
                e.preventDefault();
                void addItem(item);
              }
            }}
            title={item.path}
          >
            <span class="sourceThumb"><Thumb {item} size={192} /></span>
            <span class="sourceText">
              <strong>{item.name}</strong>
              <em>{item.ext.toUpperCase()}</em>
            </span>
            <button
              class="rowAdd"
              onclick={(e) => {
                e.stopPropagation();
                sourceFocusPath = item.path;
                void addItem(item);
              }}
            >Add</button>
          </div>
        {/each}
      {:else}
        <div class="emptyState">No videos available.</div>
      {/if}
    </div>
  </aside>

  <section class="workPane">
    <div class="editTop">
      <div class="presetGroup">
        {#each Object.entries(PRESETS) as [id, p]}
          <button class:on={preset === id} onclick={() => (preset = id as PresetId)}>
            <strong>{p.label}</strong>
            <span>{p.detail}</span>
          </button>
        {/each}
      </div>
      <span class="status" class:warn={needsEncode}>{needsEncode ? "Encode" : "Stream copy"}</span>
      <span class="spacer"></span>
      <button class="miniBtn" onclick={() => addItem(activeVideo)} disabled={!activeVideo}>Add active</button>
      <button class="exportBtn" onclick={exportTimeline} disabled={!clips.length || exporting}>
        {exporting ? "Exporting" : "Export"}
      </button>
    </div>

    <div class="preview">
      {#if selectedClip}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          bind:this={previewVideo}
          src={selectedClip.src}
          style:filter={previewFilter}
          onloadedmetadata={onMeta}
          ontimeupdate={onTime}
          onerror={onPreviewError}
          onclick={togglePlay}
        ></video>
        {#if previewPreparing}
          <div class="previewBusy">Preparing preview</div>
        {/if}
        {#if outPreset.fit !== "original"}
          <button
            class="cropFrame"
            class:wide={outAspect > 1}
            style="--aspect:{outAspect}; left:{50 + (selectedClip.cropX - 0.5) * 42}%; top:{50 + (selectedClip.cropY - 0.5) * 42}%"
            onpointerdown={startCropDrag}
            title="Drag crop"
            aria-label="Drag crop"
          >
            <span></span>
          </button>
        {/if}
      {:else}
        <div class="emptyState">Add a video to start editing.</div>
      {/if}
    </div>

    <div class="transport">
      <button class="play" onclick={togglePlay} disabled={!selectedClip}>{previewVideo?.paused === false ? "Pause" : "Play"}</button>
      <span class="time">{fmt(currentTime)} / {fmt(selectedClip?.duration ?? 0)}</span>
      <input
        type="range"
        min="0"
        max={selectedClip?.duration ?? 1}
        step="0.01"
        value={currentTime}
        disabled={!selectedClip}
        oninput={(e) => seek(Number((e.currentTarget as HTMLInputElement).value))}
      />
    </div>

    <section class="timeline" aria-label="Edit timeline" ondragover={allowTimelineDrop} ondrop={dropOnTimeline}>
      <div class="timelineHead">
        <strong>Timeline</strong>
        <span>{clips.length} clip{clips.length === 1 ? "" : "s"}</span>
        <span>{fmt(timelineSeconds)}</span>
        <span class="spacer"></span>
        <button class="ghost" onclick={() => (clips = [])} disabled={!clips.length}>Clear</button>
      </div>
      <div class="rail">
        {#if clips.length}
          {#each clips as clip, i (clip.id)}
            <div
              class="clip"
              class:on={clip.id === selectedId}
              role="button"
              tabindex="0"
              onclick={() => (selectedId = clip.id)}
              onkeydown={(e) => e.key === "Enter" && (selectedId = clip.id)}
            >
              <span class="idx">{i + 1}</span>
              <span class="cn" title={clip.path}>{clip.name}</span>
              <span class="dur">{fmt(clip.outS - clip.inS)}</span>
              <span class="acts">
                <button onclick={(e) => { e.stopPropagation(); moveClip(clip.id, -1); }} disabled={i === 0}>Up</button>
                <button onclick={(e) => { e.stopPropagation(); moveClip(clip.id, 1); }} disabled={i === clips.length - 1}>Down</button>
                <button onclick={(e) => { e.stopPropagation(); duplicateClip(clip); }}>Duplicate</button>
                <button class="danger" onclick={(e) => { e.stopPropagation(); removeClip(clip.id); }}>Remove</button>
              </span>
            </div>
          {/each}
        {:else}
          <div class="emptyTrack">Drop videos here or use Add.</div>
        {/if}
      </div>
    </section>
  </section>

  <aside class="inspector">
    <div class="block">
      <h3>Segment</h3>
      {#if selectedClip}
        <div class="row">
          <button class="miniBtn" onclick={setIn}>Set in</button>
          <input
            type="number"
            min="0"
            max={selectedClip.outS}
            step="0.01"
            value={selectedClip.inS}
            oninput={(e) => updateClip({ inS: Number((e.currentTarget as HTMLInputElement).value) })}
            onchange={clampTrim}
          />
        </div>
        <div class="row">
          <button class="miniBtn" onclick={setOut}>Set out</button>
          <input
            type="number"
            min={selectedClip.inS}
            max={selectedClip.duration}
            step="0.01"
            value={selectedClip.outS}
            oninput={(e) => updateClip({ outS: Number((e.currentTarget as HTMLInputElement).value) })}
            onchange={clampTrim}
          />
        </div>
        <div class="small">Length {fmt(selectedClip.outS - selectedClip.inS)}</div>
        {#if outPreset.fit !== "original"}
          <label>Crop X <input type="range" min="0" max="1" step="0.001" value={selectedClip.cropX} oninput={(e) => updateClip({ cropX: Number((e.currentTarget as HTMLInputElement).value) })} /></label>
          <label>Crop Y <input type="range" min="0" max="1" step="0.001" value={selectedClip.cropY} oninput={(e) => updateClip({ cropY: Number((e.currentTarget as HTMLInputElement).value) })} /></label>
          <label>Zoom <input type="range" min="1" max="4" step="0.01" value={selectedClip.zoom} oninput={(e) => updateClip({ zoom: Number((e.currentTarget as HTMLInputElement).value) })} /></label>
        {/if}
      {:else}
        <p class="small">No timeline clip selected.</p>
      {/if}
    </div>

    <div class="block">
      <h3>Look</h3>
      <label>Brightness <input type="range" min="-0.25" max="0.25" step="0.005" bind:value={adjustments.brightness} /></label>
      <label>Contrast <input type="range" min="0.6" max="1.6" step="0.01" bind:value={adjustments.contrast} /></label>
      <label>Saturation <input type="range" min="0" max="2" step="0.01" bind:value={adjustments.saturation} /></label>
      <label>Warmth <input type="range" min="-0.2" max="0.2" step="0.005" bind:value={adjustments.warmth} /></label>
      <label>Sharpen <input type="range" min="0" max="1" step="0.01" bind:value={adjustments.sharpen} /></label>
      <button class="miniBtn" onclick={resetColor}>Reset</button>
    </div>

    <div class="block">
      <h3>Audio & Export</h3>
      <label>Encoder
        <select bind:value={encoder}>
          <option value="auto">Auto</option>
          <option value="x264">x264</option>
          <option value="nvenc">NVIDIA</option>
        </select>
      </label>
      <label>Quality
        <select bind:value={quality}>
          <option value="best">Best</option>
          <option value="high">High</option>
          <option value="standard">Standard</option>
          <option value="small">Small</option>
        </select>
      </label>
      <label class="check"><input type="checkbox" bind:checked={preserveSourceAudio} disabled={!!musicPath || clips.length !== 1} /> Source audio</label>
      <div class="music">
        <button class="miniBtn" onclick={pickMusic}>Choose audio</button>
        {#if musicPath}
          <button class="ghost" onclick={() => (musicPath = null)} title={musicPath}>{basename(musicPath)}</button>
        {/if}
      </div>
      {#if exportNote}<p class="note">{exportNote}</p>{/if}
    </div>
  </aside>
</div>

<style>
  .editShell {
    width: 100%;
    height: 100%;
    display: grid;
    grid-template-columns: minmax(190px, 22%) minmax(360px, 1fr) minmax(230px, 26%);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .sourcePane,
  .inspector {
    min-width: 0;
    min-height: 0;
    background: var(--bg-panel);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
  }
  .inspector {
    border-right: 0;
    border-left: 1px solid var(--border);
    overflow-y: auto;
  }
  .paneHead,
  .editTop,
  .timelineHead {
    display: flex;
    align-items: center;
    gap: 7px;
    min-height: 44px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-panel);
  }
  .paneHead {
    align-items: flex-start;
    justify-content: space-between;
  }
  .editTop {
    overflow: hidden;
  }
  .editTop .miniBtn {
    flex: 0 0 auto;
  }
  .paneHead > div {
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.15;
  }
  .sourceActions {
    min-width: 0;
    display: flex;
    flex-direction: row;
    justify-content: flex-end;
    gap: 5px;
    flex-wrap: wrap;
  }
  .paneHead span,
  .timelineHead span,
  .small,
  .note,
  .time,
  .sourceText em {
    color: var(--text-faint);
    font-size: 12px;
  }
  .sourceList {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .sourceItem {
    display: grid;
    grid-template-columns: 58px minmax(0, 1fr) auto;
    align-items: center;
    gap: 9px;
    width: 100%;
    min-height: 66px;
    padding: 6px;
    border-radius: 8px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    text-align: left;
  }
  .sourceItem:hover {
    background: var(--bg-hover);
  }
  .sourceItem.active {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .sourceItem.focused:not(.active) {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .sourceThumb {
    width: 58px;
    height: 50px;
    border-radius: 6px;
    overflow: hidden;
    background: var(--viewport-bg);
  }
  .sourceText {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .sourceText strong,
  .cn {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rowAdd {
    border: 1px solid var(--border);
    background: var(--bg-elev);
    border-radius: 7px;
    padding: 5px 8px;
    font-size: 12px;
    white-space: nowrap;
  }
  .rowAdd:hover {
    background: var(--bg-hover);
  }
  .workPane {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto 190px;
  }
  .presetGroup {
    display: flex;
    flex: 0 0 auto;
    gap: 4px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-elev);
  }
  .presetGroup button {
    min-width: 64px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 5px 7px;
    border-radius: 6px;
    text-align: left;
    color: var(--text-dim);
  }
  .presetGroup button.on {
    background: var(--accent);
    color: var(--accent-on);
  }
  .presetGroup span {
    font-size: 10.5px;
    opacity: 0.75;
  }
  .status {
    padding: 5px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--pick) 18%, transparent);
    color: var(--pick);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .status.warn {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    color: var(--accent);
  }
  .spacer {
    flex: 1 1 auto;
    min-width: 8px;
  }
  .preview {
    position: relative;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #0c0b0a;
    overflow: hidden;
  }
  .preview video {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .previewBusy {
    position: absolute;
    left: 12px;
    bottom: 12px;
    padding: 6px 9px;
    border-radius: 7px;
    background: color-mix(in srgb, var(--bg-elev) 88%, transparent);
    border: 1px solid var(--border);
    color: var(--text-dim);
    font-size: 12px;
  }
  .emptyState,
  .emptyTrack {
    color: var(--text-faint);
    font-size: 13px;
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    width: 100%;
    border: 1px dashed color-mix(in srgb, var(--text-faint) 36%, transparent);
    border-radius: 8px;
  }
  .sourceList .emptyState {
    min-height: 120px;
  }
  .cropFrame {
    position: absolute;
    height: 82%;
    aspect-ratio: var(--aspect);
    transform: translate(-50%, -50%);
    border: 2px solid rgba(255, 255, 255, 0.92);
    box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.34), 0 6px 26px rgba(0, 0, 0, 0.45);
    cursor: move;
    padding: 0;
    background: transparent;
  }
  .cropFrame.wide {
    width: 82%;
    height: auto;
  }
  .cropFrame span {
    position: absolute;
    inset: 33.333% 0;
    border-top: 1px solid rgba(255, 255, 255, 0.45);
    border-bottom: 1px solid rgba(255, 255, 255, 0.45);
  }
  .cropFrame::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 33.333%;
    width: 33.333%;
    border-left: 1px solid rgba(255, 255, 255, 0.45);
    border-right: 1px solid rgba(255, 255, 255, 0.45);
  }
  .transport {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 42px;
    padding: 7px 10px;
    border-top: 1px solid var(--border);
    background: var(--bg-panel);
  }
  .transport input {
    flex: 1;
    accent-color: var(--accent);
  }
  .play,
  .miniBtn,
  .ghost,
  .clip .acts button {
    border: 1px solid var(--border);
    background: var(--bg-elev);
    border-radius: 7px;
    padding: 5px 9px;
    font-size: 12px;
    white-space: nowrap;
  }
  .play:hover,
  .miniBtn:hover,
  .ghost:hover,
  .clip .acts button:hover {
    background: var(--bg-hover);
  }
  .exportBtn {
    padding: 7px 12px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-on);
    font-weight: 700;
    white-space: nowrap;
  }
  button:disabled {
    opacity: 0.42;
    cursor: not-allowed;
  }
  .timeline {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--border);
    background: var(--bg-panel);
  }
  .rail {
    flex: 1;
    display: flex;
    gap: 8px;
    overflow-x: auto;
    padding: 10px;
    align-items: stretch;
  }
  .clip {
    flex: 0 0 250px;
    display: grid;
    grid-template-columns: auto 1fr auto;
    grid-template-rows: auto auto;
    gap: 7px 8px;
    align-content: start;
    text-align: left;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 9px;
    background: var(--bg-elev);
  }
  .clip.on {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .idx,
  .dur {
    color: var(--text-faint);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .acts {
    grid-column: 1 / 4;
    display: flex;
    gap: 5px;
    flex-wrap: wrap;
  }
  .acts .danger {
    color: var(--reject);
    border-color: color-mix(in srgb, var(--reject) 55%, var(--border));
  }
  .block {
    padding: 12px;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  .block h3 {
    margin: 0 0 2px;
    font-size: 13px;
  }
  .row {
    display: grid;
    grid-template-columns: 72px 1fr;
    gap: 8px;
    align-items: center;
  }
  input[type="number"],
  select {
    width: 100%;
    background: var(--bg-elev);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 5px 7px;
  }
  label {
    display: grid;
    gap: 4px;
    color: var(--text-dim);
    font-size: 12px;
  }
  label input[type="range"] {
    width: 100%;
    accent-color: var(--accent);
  }
  .check {
    display: flex;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 7px;
  }
  .music {
    display: flex;
    gap: 7px;
    align-items: center;
    min-width: 0;
  }
  .music .ghost {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .note {
    margin: 0;
  }
  @media (max-width: 1120px) {
    .editShell {
      grid-template-columns: 210px minmax(0, 1fr);
    }
    .inspector {
      display: none;
    }
    .presetGroup button {
      min-width: 58px;
    }
  }
</style>
