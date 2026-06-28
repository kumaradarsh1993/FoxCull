<script lang="ts">
  import { api } from "$lib/api";
  import type { EditAdjustments, EditExportRequest, MediaItem } from "$lib/types";

  let {
    active,
    selectedItems,
  }: {
    active: MediaItem | null;
    selectedItems: MediaItem[];
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

  const PRESETS: Record<PresetId, { label: string; w: number; h: number; fit: "crop" | "original" }> = {
    instagram: { label: "9:16", w: 1080, h: 1920, fit: "crop" },
    square: { label: "1:1", w: 1080, h: 1080, fit: "crop" },
    landscape: { label: "16:9", w: 1920, h: 1080, fit: "crop" },
    original: { label: "Original", w: 0, h: 0, fit: "original" },
  };

  const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
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

  let selectedClip = $derived(clips.find((c) => c.id === selectedId) ?? clips[0] ?? null);
  let activeVideo = $derived(active?.kind === "video" ? active : null);
  let selectedVideos = $derived(selectedItems.filter((i) => i.kind === "video"));
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
      return;
    }
    if (!selectedId || !clips.some((c) => c.id === selectedId)) selectedId = clips[0].id;
  });

  $effect(() => {
    const clip = selectedClip;
    if (!clip || !previewVideo) return;
    currentTime = clip.inS;
    previewVideo.currentTime = clip.inS;
  });

  function uid() {
    return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
  }

  function durationFor(path: string): Promise<number> {
    return new Promise((resolve) => {
      const video = document.createElement("video");
      video.preload = "metadata";
      video.src = api.fileSrc(path);
      video.onloadedmetadata = () => resolve(Number.isFinite(video.duration) ? video.duration : 0);
      video.onerror = () => resolve(0);
    });
  }

  async function addItem(item: MediaItem | null) {
    if (!item || item.kind !== "video") return;
    const duration = await durationFor(item.path);
    const out = Math.max(0.1, duration || 1);
    const clip: TimelineClip = {
      id: uid(),
      path: item.path,
      name: item.name,
      src: api.fileSrc(item.path),
      inS: 0,
      outS: out,
      duration: out,
      cropX: 0.5,
      cropY: 0.5,
      zoom: 1,
    };
    clips = [...clips, clip];
    selectedId = clip.id;
    exportNote = null;
  }

  async function addSelected() {
    const batch = selectedVideos.length ? selectedVideos : activeVideo ? [activeVideo] : [];
    for (const item of batch) await addItem(item);
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
    exportNote = "Exporting...";
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
        preserve_source_audio: preserveSourceAudio && !musicPath,
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
</script>

<div class="edit">
  <section class="stagePane">
    <div class="editTop">
      <div class="seg">
        {#each Object.entries(PRESETS) as [id, p]}
          <button class="chip" class:on={preset === id} onclick={() => (preset = id as PresetId)}>
            {p.label}
          </button>
        {/each}
      </div>
      <span class="status" class:warn={needsEncode}>{needsEncode ? "Encode" : "Stream copy"}</span>
      <span class="spacer"></span>
      <button class="btn sm" onclick={() => addItem(activeVideo)} disabled={!activeVideo}>Add active</button>
      <button class="btn sm" onclick={addSelected} disabled={!selectedVideos.length && !activeVideo}>
        Add selected{selectedVideos.length > 1 ? ` ${selectedVideos.length}` : ""}
      </button>
    </div>

    <div class="preview">
      {#if selectedClip}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          bind:this={previewVideo}
          src={selectedClip.src}
          onloadedmetadata={onMeta}
          ontimeupdate={onTime}
          onclick={togglePlay}
        ></video>
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
        <div class="empty">Add a video to start editing.</div>
      {/if}
    </div>

    {#if selectedClip}
      <div class="scrub">
        <button class="play" onclick={togglePlay}>{previewVideo?.paused === false ? "Pause" : "Play"}</button>
        <span class="time">{fmt(currentTime)} / {fmt(selectedClip.duration)}</span>
        <input
          type="range"
          min="0"
          max={selectedClip.duration}
          step="0.01"
          value={currentTime}
          oninput={(e) => seek(Number((e.currentTarget as HTMLInputElement).value))}
        />
      </div>
    {/if}
  </section>

  <aside class="inspector">
    <div class="block">
      <h3>Segment</h3>
      {#if selectedClip}
        <div class="row">
          <button class="btn sm" onclick={setIn}>Set in</button>
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
          <button class="btn sm" onclick={setOut}>Set out</button>
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
      <button class="btn sm" onclick={resetColor}>Reset</button>
    </div>

    <div class="block">
      <h3>Export</h3>
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
        <button class="btn sm" onclick={pickMusic}>Music</button>
        {#if musicPath}
          <button class="ghost" onclick={() => (musicPath = null)} title={musicPath}>{basename(musicPath)}</button>
        {/if}
      </div>
      <button class="export" onclick={exportTimeline} disabled={!clips.length || exporting}>
        {exporting ? "Exporting..." : "Export"}
      </button>
      {#if exportNote}<p class="note">{exportNote}</p>{/if}
    </div>
  </aside>

  <section class="timeline">
    <div class="timelineHead">
      <strong>{clips.length} clip{clips.length === 1 ? "" : "s"}</strong>
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
              <button onclick={(e) => { e.stopPropagation(); duplicateClip(clip); }}>Copy</button>
              <button class="danger" onclick={(e) => { e.stopPropagation(); removeClip(clip.id); }}>Remove</button>
            </span>
          </div>
        {/each}
      {:else}
        <div class="drop">Timeline is empty.</div>
      {/if}
    </div>
  </section>
</div>

<style>
  .edit {
    height: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 300px;
    grid-template-rows: minmax(0, 1fr) 170px;
    background: var(--bg);
    color: var(--text);
    min-width: 0;
  }
  .stagePane {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
  }
  .editTop,
  .timelineHead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-panel);
  }
  .seg {
    display: flex;
    gap: 3px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-elev);
  }
  .chip {
    padding: 4px 9px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-dim);
  }
  .chip.on {
    background: var(--accent);
    color: var(--accent-on);
  }
  .status {
    padding: 4px 9px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--pick) 18%, transparent);
    color: var(--pick);
    font-size: 12px;
  }
  .status.warn {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    color: var(--accent);
  }
  .spacer {
    flex: 1;
  }
  .preview {
    position: relative;
    flex: 1;
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
  .empty,
  .drop {
    color: var(--text-faint);
    font-size: 13px;
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
  .cropFrame::before,
  .cropFrame::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    width: 33.333%;
    border-left: 1px solid rgba(255, 255, 255, 0.45);
    border-right: 1px solid rgba(255, 255, 255, 0.45);
  }
  .cropFrame::before {
    left: 33.333%;
  }
  .cropFrame::after {
    display: none;
  }
  .scrub {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 10px;
    border-top: 1px solid var(--border);
    background: var(--bg-panel);
  }
  .scrub input {
    flex: 1;
    accent-color: var(--accent);
  }
  .play,
  .btn.sm,
  .ghost,
  .clip .acts button {
    border: 1px solid var(--border);
    background: var(--bg-elev);
    border-radius: 7px;
    padding: 5px 10px;
    font-size: 12px;
  }
  .play:hover,
  .btn.sm:hover,
  .ghost:hover,
  .clip .acts button:hover {
    background: var(--bg-hover);
  }
  button:disabled {
    opacity: 0.42;
    cursor: not-allowed;
  }
  .time,
  .small,
  .note,
  .timelineHead span {
    color: var(--text-faint);
    font-size: 12px;
  }
  .inspector {
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
    background: var(--bg-panel);
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
    grid-template-columns: auto 1fr;
    align-items: center;
    display: flex;
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
  .export {
    margin-top: 2px;
    padding: 9px 12px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-on);
    font-weight: 700;
  }
  .timeline {
    grid-column: 1 / 3;
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
    flex: 0 0 260px;
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
  .idx {
    color: var(--text-faint);
    font-size: 12px;
  }
  .cn {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
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
</style>
