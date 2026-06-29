<script lang="ts">
  import { api } from "$lib/api";
  import type {
    EditAdjustments,
    EditExportRequest,
    EditSnapshotRequest,
    EditSourceItem,
    MediaItem,
    MediaProbe,
  } from "$lib/types";
  import ContextMenu, { type MenuEntry } from "./ContextMenu.svelte";
  import Thumb from "./Thumb.svelte";

  let {
    active,
    selectedItems,
    sourceItems = [],
    currentDir = null,
    recursive = true,
    refreshKey = 0,
  }: {
    active: MediaItem | null;
    selectedItems: MediaItem[];
    sourceItems?: MediaItem[];
    currentDir?: string | null;
    recursive?: boolean;
    refreshKey?: number;
  } = $props();

  type PresetId = "original" | "landscape" | "square" | "reels";
  type Encoder = "auto" | "x264" | "nvenc";
  type Quality = "best" | "high" | "standard" | "small";
  type SourceView = "details" | "list" | "thumbs";
  type SourceFilter = "all" | "video" | "audio";
  type DragMode = "move" | "trimIn" | "trimOut";

  type TimelineClip = {
    id: string;
    path: string;
    name: string;
    src: string;
    inS: number;
    outS: number;
    duration: number;
    start: number;
    lane: number;
    cropX: number;
    cropY: number;
    zoom: number;
  };

  type AudioClip = {
    id: string;
    path: string;
    name: string;
    start: number;
    duration: number;
    lane: number;
  };

  type TimelineDrag = {
    id: string;
    kind: "video" | "audio";
    mode: DragMode;
    startX: number;
    start: number;
    inS: number;
    outS: number;
    duration: number;
  };

  const PRESETS: Record<PresetId, { label: string; detail: string; w: number; h: number; fit: "crop" | "original" }> = {
    original: { label: "Original", detail: "Stream copy", w: 0, h: 0, fit: "original" },
    landscape: { label: "16:9", detail: "1920x1080", w: 1920, h: 1080, fit: "crop" },
    square: { label: "1:1", detail: "1080x1080", w: 1080, h: 1080, fit: "crop" },
    reels: { label: "9:16", detail: "Reels/Stories", w: 1080, h: 1920, fit: "crop" },
  };

  const VIDEO_LANES = [0, 1, 2];
  const AUDIO_LANES = [0, 1, 2];
  const SNAP = 0.16;
  const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  const extOf = (p: string) => (basename(p).match(/\.([^.]+)$/)?.[1] ?? "").toLowerCase();
  const normPath = (p: string) =>
    p
      .replace(/^\\\\\?\\/, "")
      .replace(/\//g, "\\")
      .replace(/\\+$/g, "")
      .toLowerCase();

  let clips = $state<TimelineClip[]>([]);
  let audioClips = $state<AudioClip[]>([]);
  let selectedId = $state<string | null>(null);
  let selectedAudioId = $state<string | null>(null);
  let preset = $state<PresetId>("reels");
  let encoder = $state<Encoder>("auto");
  let quality = $state<Quality>("high");
  let preserveSourceAudio = $state(true);
  let exporting = $state(false);
  let snapshotting = $state(false);
  let exportNote = $state<string | null>(null);
  let previewVideo = $state<HTMLVideoElement | null>(null);
  let previewBox = $state<HTMLDivElement | null>(null);
  let previewW = $state(0);
  let previewH = $state(0);
  let videoW = $state(16);
  let videoH = $state(9);
  let currentTime = $state(0);
  let previewPreparing = $state(false);
  let sourceBase = $state<EditSourceItem[]>([]);
  let sourceLoading = $state(false);
  let sourceFocusPath = $state<string | null>(null);
  let sourceView = $state<SourceView>("details");
  let sourceFilter = $state<SourceFilter>("all");
  let probes = $state<Record<string, MediaProbe>>({});
  let timelineScale = $state(26);
  let dragSourcePath = $state<string | null>(null);
  let seededKey = $state("");
  let seeding = $state(false);
  let lastPreviewClipId = "";
  let timelineDrag: TimelineDrag | null = null;
  let sourceMenu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  let exportOptionsOpen = $state(false);
  let cropDrag:
    | { x: number; y: number; cropX: number; cropY: number; imgW: number; imgH: number; cropW: number; cropH: number }
    | null = null;
  const probing = new Set<string>();

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
  let outPreset = $derived(PRESETS[preset]);
  let outAspect = $derived(outPreset.fit === "original" ? videoW / videoH : outPreset.w / outPreset.h);
  let selectedClip = $derived(clips.find((c) => c.id === selectedId) ?? clips[0] ?? null);
  let selectedAudio = $derived(audioClips.find((c) => c.id === selectedAudioId) ?? null);
  let orderedClips = $derived.by(() => [...clips].sort((a, b) => a.start - b.start || a.lane - b.lane));
  let timelineSeconds = $derived(clips.reduce((sum, c) => sum + Math.max(0, c.outS - c.inS), 0));
  let audioEnd = $derived(audioClips.reduce((max, c) => Math.max(max, c.start + c.duration), 0));
  let videoEnd = $derived(clips.reduce((max, c) => Math.max(max, c.start + Math.max(0, c.outS - c.inS)), 0));
  let timelineEnd = $derived(Math.max(10, videoEnd, audioEnd));
  let timelineWidth = $derived(Math.max(980, timelineEnd * timelineScale + 220));
  let previewFilter = $derived(
    `brightness(${Math.max(0, 1 + adjustments.brightness)}) contrast(${adjustments.contrast}) saturate(${adjustments.saturation})`,
  );
  let needsRender = $derived(
    outPreset.fit !== "original" ||
      audioClips.length > 0 ||
      Math.abs(adjustments.brightness) > 0.001 ||
      Math.abs(adjustments.contrast - 1) > 0.001 ||
      Math.abs(adjustments.saturation - 1) > 0.001 ||
      Math.abs(adjustments.warmth) > 0.001 ||
      Math.abs(adjustments.sharpen) > 0.001,
  );

  function mediaToSource(item: MediaItem): EditSourceItem {
    return {
      name: item.name,
      path: item.path,
      kind: "video",
      ext: item.ext,
      mtime: item.mtime,
      size: item.size,
    };
  }

  function sourceToMedia(item: EditSourceItem): MediaItem {
    return {
      name: item.name,
      path: item.path,
      rel: item.name,
      kind: "video",
      ext: item.ext,
      mtime: item.mtime,
      size: item.size,
      rating: 0,
      label: null,
      flag: null,
      tags: [],
    };
  }

  let sources = $derived.by(() => {
    const seen = new Set<string>();
    const out: EditSourceItem[] = [];
    for (const item of [...sourceBase, ...initialVideos.map(mediaToSource), ...sourceItems.filter((i) => i.kind === "video").map(mediaToSource)]) {
      const key = normPath(item.path);
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(item);
    }
    return out;
  });

  let filteredSources = $derived.by(() => {
    const arr = sourceFilter === "all" ? sources : sources.filter((s) => s.kind === sourceFilter);
    return [...arr].sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }));
  });

  let focusedSource = $derived(filteredSources.find((item) => item.path === sourceFocusPath) ?? filteredSources[0] ?? null);

  $effect(() => {
    const dir = currentDir;
    const rec = recursive;
    refreshKey;
    let alive = true;
    sourceBase = [];
    if (!dir) return;
    sourceLoading = true;
    api
      .listEditSources(dir, rec)
      .then((items) => {
        if (alive) sourceBase = items;
      })
      .catch(() => {
        if (alive) sourceBase = sourceItems.filter((i) => i.kind === "video").map(mediaToSource);
      })
      .finally(() => {
        if (alive) sourceLoading = false;
      });
    return () => {
      alive = false;
    };
  });

  $effect(() => {
    const first = filteredSources[0]?.path ?? null;
    if (!sourceFocusPath || !filteredSources.some((item) => item.path === sourceFocusPath)) {
      sourceFocusPath = first;
    }
  });

  $effect(() => {
    for (const src of filteredSources.slice(0, 80)) ensureProbe(src);
  });

  $effect(() => {
    const key = initialVideos.map((i) => i.path).join("|");
    if (!key || clips.length || seeding || seededKey === key) return;
    seededKey = key;
    seeding = true;
    void addVideos(initialVideos.map(mediaToSource)).finally(() => {
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

  $effect(() => {
    const el = previewBox;
    if (!el) return;
    const measure = () => {
      previewW = el.clientWidth;
      previewH = el.clientHeight;
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });

  function uid() {
    return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
  }

  function fmt(s: number) {
    if (!Number.isFinite(s) || s < 0) s = 0;
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = Math.floor(s % 60);
    return h ? `${h}:${m.toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}` : `${m}:${sec.toString().padStart(2, "0")}`;
  }

  function fmtSize(n: number): string {
    if (!n) return "-";
    if (n < 1024 * 1024) return `${Math.max(1, Math.round(n / 1024))} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
    return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function fmtDate(epochSecs: number | null | undefined): string {
    if (!epochSecs) return "-";
    return new Date(epochSecs * 1000).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "2-digit",
    });
  }

  function fmtSpec(src: EditSourceItem) {
    const p = probes[src.path];
    if (!p) return src.kind === "audio" ? src.ext.toUpperCase() : "Reading...";
    if (src.kind === "audio") return [fmt(p.duration), p.codec ?? src.ext.toUpperCase()].filter(Boolean).join(" · ");
    const res = p.width && p.height ? `${p.width}x${p.height}` : "";
    const fps = p.fps ? `${Math.round(p.fps)}fps` : "";
    return [fmt(p.duration), res, fps, p.codec ?? src.ext.toUpperCase()].filter(Boolean).join(" · ");
  }

  function ensureProbe(src: EditSourceItem) {
    if (probes[src.path] || probing.has(src.path)) return;
    probing.add(src.path);
    api
      .probeMediaInfo(src.path)
      .then((p) => {
        probes = { ...probes, [src.path]: p };
      })
      .catch(() => {})
      .finally(() => probing.delete(src.path));
  }

  async function durationFor(src: EditSourceItem): Promise<number> {
    ensureProbe(src);
    const cached = probes[src.path]?.duration;
    if (cached && cached > 0) return cached;
    try {
      const p = await api.probeMediaInfo(src.path);
      probes = { ...probes, [src.path]: p };
      if (p.duration > 0) return p.duration;
    } catch {
      /* fall through */
    }
    return src.kind === "audio" ? 30 : 1;
  }

  function nextVideoStart(lane = 0) {
    return clips.filter((c) => c.lane === lane).reduce((max, c) => Math.max(max, c.start + c.outS - c.inS), 0);
  }

  function nextAudioStart(lane = 0) {
    return audioClips.filter((c) => c.lane === lane).reduce((max, c) => Math.max(max, c.start + c.duration), 0);
  }

  async function makeClip(src: EditSourceItem, lane = 0, start = nextVideoStart(lane)): Promise<TimelineClip> {
    const duration = await durationFor(src);
    const out = Math.max(0.1, duration || 1);
    const cachedProxy = await api.videoProxyCached(src.path);
    return {
      id: uid(),
      path: src.path,
      name: src.name,
      src: api.fileSrc(cachedProxy ?? src.path),
      inS: 0,
      outS: out,
      duration: out,
      start,
      lane,
      cropX: 0.5,
      cropY: 0.5,
      zoom: 1,
    };
  }

  async function addVideos(items: EditSourceItem[], lane = 0, start?: number) {
    const made: TimelineClip[] = [];
    let cursor = start ?? nextVideoStart(lane);
    for (const item of items.filter((s) => s.kind === "video")) {
      const clip = await makeClip(item, lane, cursor);
      made.push(clip);
      cursor += clip.outS - clip.inS;
    }
    if (!made.length) return;
    clips = [...clips, ...made];
    selectedId = made[made.length - 1].id;
    selectedAudioId = null;
    exportNote = null;
  }

  async function addAudio(src: EditSourceItem, lane = 0, start = nextAudioStart(lane)) {
    if (src.kind !== "audio") return;
    const duration = await durationFor(src);
    const clip = { id: uid(), path: src.path, name: src.name, start, duration: Math.max(1, duration || 30), lane };
    audioClips = [...audioClips, clip];
    selectedAudioId = clip.id;
    selectedId = null;
    preserveSourceAudio = false;
    exportNote = null;
  }

  function removeClip(id: string) {
    clips = clips.filter((c) => c.id !== id);
    if (selectedId === id) selectedId = clips[0]?.id ?? null;
    exportNote = null;
  }

  function removeAudio(id: string) {
    audioClips = audioClips.filter((c) => c.id !== id);
    if (selectedAudioId === id) selectedAudioId = null;
    exportNote = null;
  }

  function duplicateClip(clip: TimelineClip) {
    const copy = { ...clip, id: uid(), start: clip.start + Math.max(0.1, clip.outS - clip.inS) };
    clips = [...clips, copy];
    selectedId = copy.id;
    selectedAudioId = null;
    exportNote = null;
  }

  function duplicateAudio(clip: AudioClip) {
    const copy = { ...clip, id: uid(), start: clip.start + Math.max(0.1, clip.duration) };
    audioClips = [...audioClips, copy];
    selectedAudioId = copy.id;
    selectedId = null;
    exportNote = null;
  }

  function updateClip(id: string, patch: Partial<TimelineClip>) {
    clips = clips.map((clip) => (clip.id === id ? { ...clip, ...patch } : clip));
  }

  function updateSelectedClip(patch: Partial<TimelineClip>) {
    if (selectedClip) updateClip(selectedClip.id, patch);
  }

  function updateAudio(id: string, patch: Partial<AudioClip>) {
    audioClips = audioClips.map((clip) => (clip.id === id ? { ...clip, ...patch } : clip));
  }

  function clampTrim() {
    const clip = selectedClip;
    if (!clip) return;
    const inS = Math.max(0, Math.min(clip.inS, Math.max(0, clip.outS - 0.05)));
    const outS = Math.min(clip.duration, Math.max(clip.outS, inS + 0.05));
    updateClip(clip.id, { inS, outS });
  }

  async function ensurePreviewProxy(clip: TimelineClip) {
    if (previewPreparing || clip.src !== api.fileSrc(clip.path)) return;
    previewPreparing = true;
    exportNote = "Preparing preview";
    try {
      const proxy = await api.videoProxy(clip.path);
      updateClip(clip.id, { src: api.fileSrc(proxy) });
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

  function onMeta() {
    if (!previewVideo || !selectedClip) return;
    const d = Number.isFinite(previewVideo.duration) ? previewVideo.duration : selectedClip.duration;
    videoW = previewVideo.videoWidth || probes[selectedClip.path]?.width || videoW;
    videoH = previewVideo.videoHeight || probes[selectedClip.path]?.height || videoH;
    if (d > 0 && Math.abs(d - selectedClip.duration) > 0.01) {
      updateClip(selectedClip.id, { duration: d, outS: Math.min(selectedClip.outS || d, d) });
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
    updateClip(selectedClip.id, { inS: Math.min(currentTime, selectedClip.outS - 0.05) });
    clampTrim();
  }

  function setOut() {
    if (!selectedClip) return;
    updateClip(selectedClip.id, { outS: Math.max(currentTime, selectedClip.inS + 0.05) });
    clampTrim();
  }

  function resetColor() {
    adjustments = { brightness: 0, contrast: 1, saturation: 1, warmth: 0, sharpen: 0 };
  }

  function exportName() {
    const first = orderedClips[0]?.name ?? "clip";
    const stem = first.replace(/\.[^.]+$/, "");
    if (preset === "reels") return `${stem}_reel`;
    if (preset === "original") return `${stem}_edit`;
    return `${stem}_${preset}`;
  }

  async function pickAudio() {
    const picked = await api.pickAudio();
    if (!picked) return;
    const src: EditSourceItem = {
      name: basename(picked),
      path: picked,
      kind: "audio",
      ext: extOf(picked),
      mtime: 0,
      size: 0,
    };
    sourceBase = [src, ...sourceBase.filter((s) => s.path !== picked)];
    sourceFocusPath = picked;
    await addAudio(src);
  }

  function openSourceMenu(e: MouseEvent, item: EditSourceItem) {
    e.preventDefault();
    e.stopPropagation();
    sourceFocusPath = item.path;
    ensureProbe(item);
    const entries: MenuEntry[] = [
      {
        label: item.kind === "audio" ? "Add to A1" : "Add to V1",
        icon: "+",
        action: () => (item.kind === "audio" ? void addAudio(item, 0) : void addVideos([item], 0)),
      },
      {
        label: item.kind === "audio" ? "Add to next audio gap" : "Add to end",
        icon: "→",
        action: () => (item.kind === "audio" ? void addAudio(item) : void addVideos([item])),
      },
      { separator: true },
      { label: "Reveal in Explorer", icon: "↗", action: () => api.reveal(item.path) },
      { label: "Open externally", icon: "□", action: () => api.openExternal(item.path) },
      {
        label: "Copy path",
        icon: "⧉",
        action: () => navigator.clipboard?.writeText(item.path).catch(() => {}),
      },
    ];
    sourceMenu = { x: e.clientX, y: e.clientY, entries };
  }

  function openTimelineMenu(e: MouseEvent, clip: TimelineClip) {
    e.preventDefault();
    e.stopPropagation();
    selectClip(clip.id);
    sourceMenu = {
      x: e.clientX,
      y: e.clientY,
      entries: [
        { label: "Duplicate clip", icon: "+", action: () => duplicateClip(clip) },
        { label: "Remove clip", icon: "×", danger: true, action: () => removeClip(clip.id) },
        { separator: true },
        { label: "Reveal source", icon: "↗", action: () => api.reveal(clip.path) },
        { label: "Copy source path", icon: "⧉", action: () => navigator.clipboard?.writeText(clip.path).catch(() => {}) },
      ],
    };
  }

  function openAudioMenu(e: MouseEvent, clip: AudioClip) {
    e.preventDefault();
    e.stopPropagation();
    selectAudio(clip.id);
    sourceMenu = {
      x: e.clientX,
      y: e.clientY,
      entries: [
        { label: "Duplicate audio", icon: "+", action: () => duplicateAudio(clip) },
        { label: "Remove audio", icon: "×", danger: true, action: () => removeAudio(clip.id) },
        { separator: true },
        { label: "Reveal source", icon: "↗", action: () => api.reveal(clip.path) },
        { label: "Copy source path", icon: "⧉", action: () => navigator.clipboard?.writeText(clip.path).catch(() => {}) },
      ],
    };
  }

  async function exportTimeline() {
    if (!orderedClips.length || exporting) return;
    exporting = true;
    exportNote = "Exporting";
    try {
      const music = audioClips[0]?.path ?? null;
      const req: EditExportRequest = {
        clips: orderedClips.map((clip) => ({
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
        music_path: music,
        preserve_source_audio: preserveSourceAudio && !music && orderedClips.length === 1,
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

  async function takeSnapshot() {
    if (!selectedClip || snapshotting) return;
    snapshotting = true;
    exportNote = "Saving frame";
    try {
      const req: EditSnapshotRequest = {
        path: selectedClip.path,
        time_s: currentTime,
        output_w: outPreset.w,
        output_h: outPreset.h,
        fit: outPreset.fit,
        crop_x: selectedClip.cropX,
        crop_y: selectedClip.cropY,
        zoom: selectedClip.zoom,
        adjustments,
        basename: `${selectedClip.name.replace(/\.[^.]+$/, "")}_frame`,
      };
      const out = await api.editSnapshot(req);
      exportNote = `Saved frame ${basename(out)}`;
      api.reveal(out);
    } catch (e) {
      exportNote = `Frame failed: ${e}`;
    } finally {
      snapshotting = false;
    }
  }

  function selectClip(id: string) {
    selectedId = id;
    selectedAudioId = null;
  }

  function selectAudio(id: string) {
    selectedAudioId = id;
    selectedId = null;
  }

  function snapTime(t: number, excludeId?: string) {
    let best = Math.max(0, t);
    let bestDist = SNAP;
    const edges = [0, currentTime + (selectedClip?.start ?? 0)];
    for (const c of clips) {
      if (c.id === excludeId) continue;
      edges.push(c.start, c.start + c.outS - c.inS);
    }
    for (const a of audioClips) {
      if (a.id === excludeId) continue;
      edges.push(a.start, a.start + a.duration);
    }
    for (const edge of edges) {
      const d = Math.abs(t - edge);
      if (d < bestDist) {
        best = edge;
        bestDist = d;
      }
    }
    return Math.max(0, best);
  }

  function startTimelinePointer(e: PointerEvent, kind: "video" | "audio", id: string, mode: DragMode) {
    e.stopPropagation();
    e.preventDefault();
    if (kind === "video") {
      const clip = clips.find((c) => c.id === id);
      if (!clip) return;
      selectClip(id);
      timelineDrag = {
        id,
        kind,
        mode,
        startX: e.clientX,
        start: clip.start,
        inS: clip.inS,
        outS: clip.outS,
        duration: clip.duration,
      };
    } else {
      const clip = audioClips.find((c) => c.id === id);
      if (!clip) return;
      selectAudio(id);
      timelineDrag = {
        id,
        kind,
        mode,
        startX: e.clientX,
        start: clip.start,
        inS: 0,
        outS: clip.duration,
        duration: clip.duration,
      };
    }
    window.addEventListener("pointermove", onTimelineDrag);
    window.addEventListener("pointerup", endTimelineDrag, { once: true });
  }

  function onTimelineDrag(e: PointerEvent) {
    if (!timelineDrag) return;
    const d = (e.clientX - timelineDrag.startX) / timelineScale;
    if (timelineDrag.kind === "audio") {
      updateAudio(timelineDrag.id, { start: snapTime(timelineDrag.start + d, timelineDrag.id) });
      return;
    }
    const clip = clips.find((c) => c.id === timelineDrag?.id);
    if (!clip) return;
    if (timelineDrag.mode === "move") {
      updateClip(clip.id, { start: snapTime(timelineDrag.start + d, clip.id) });
    } else if (timelineDrag.mode === "trimIn") {
      const snappedStart = snapTime(timelineDrag.start + d, clip.id);
      const nextIn = Math.max(0, Math.min(timelineDrag.inS + (snappedStart - timelineDrag.start), timelineDrag.outS - 0.05));
      updateClip(clip.id, { start: timelineDrag.start + (nextIn - timelineDrag.inS), inS: nextIn });
    } else {
      const right = snapTime(timelineDrag.start + (timelineDrag.outS - timelineDrag.inS) + d, clip.id);
      const nextOut = Math.max(clip.inS + 0.05, Math.min(timelineDrag.duration, clip.inS + Math.max(0.05, right - clip.start)));
      updateClip(clip.id, { outS: nextOut });
    }
  }

  function endTimelineDrag() {
    timelineDrag = null;
    window.removeEventListener("pointermove", onTimelineDrag);
  }

  function startSourceDrag(e: DragEvent, item: EditSourceItem) {
    dragSourcePath = item.path;
    e.dataTransfer?.setData("application/x-foxcull-edit-path", item.path);
    e.dataTransfer?.setData("text/plain", item.path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "copy";
  }

  function endSourceDrag() {
    dragSourcePath = null;
  }

  function allowDrop(e: DragEvent) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
  }

  function dropOnLane(e: DragEvent, kind: "video" | "audio", lane: number) {
    e.preventDefault();
    const path = e.dataTransfer?.getData("application/x-foxcull-edit-path") || dragSourcePath;
    const item = sources.find((v) => v.path === path);
    if (!item) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const start = snapTime((e.clientX - rect.left) / timelineScale);
    if (kind === "video" && item.kind === "video") void addVideos([item], lane, start);
    if (kind === "audio" && item.kind === "audio") void addAudio(item, lane, start);
    dragSourcePath = null;
  }

  let imageRect = $derived.by(() => {
    const vw = Math.max(1, videoW);
    const vh = Math.max(1, videoH);
    const boxW = Math.max(1, previewW);
    const boxH = Math.max(1, previewH);
    const videoAspect = vw / vh;
    const boxAspect = boxW / boxH;
    let w = boxW;
    let h = boxH;
    let left = 0;
    let top = 0;
    if (boxAspect > videoAspect) {
      h = boxH;
      w = h * videoAspect;
      left = (boxW - w) / 2;
    } else {
      w = boxW;
      h = w / videoAspect;
      top = (boxH - h) / 2;
    }
    return { left, top, w, h };
  });

  let cropRect = $derived.by(() => {
    if (!selectedClip || outPreset.fit === "original") return null;
    const img = imageRect;
    const aspect = outAspect;
    let cropW = img.w;
    let cropH = cropW / aspect;
    if (cropH > img.h) {
      cropH = img.h;
      cropW = cropH * aspect;
    }
    cropW = Math.min(img.w, cropW / selectedClip.zoom);
    cropH = Math.min(img.h, cropH / selectedClip.zoom);
    const rangeX = Math.max(0, img.w - cropW);
    const rangeY = Math.max(0, img.h - cropH);
    return {
      left: img.left + rangeX * selectedClip.cropX,
      top: img.top + rangeY * selectedClip.cropY,
      w: cropW,
      h: cropH,
      imgW: img.w,
      imgH: img.h,
    };
  });

  function startCropDrag(e: PointerEvent) {
    if (!selectedClip || !cropRect) return;
    e.preventDefault();
    cropDrag = {
      x: e.clientX,
      y: e.clientY,
      cropX: selectedClip.cropX,
      cropY: selectedClip.cropY,
      imgW: cropRect.imgW,
      imgH: cropRect.imgH,
      cropW: cropRect.w,
      cropH: cropRect.h,
    };
    window.addEventListener("pointermove", onCropDrag);
    window.addEventListener("pointerup", endCropDrag, { once: true });
  }

  function onCropDrag(e: PointerEvent) {
    if (!cropDrag || !selectedClip) return;
    const rangeX = Math.max(1, cropDrag.imgW - cropDrag.cropW);
    const rangeY = Math.max(1, cropDrag.imgH - cropDrag.cropH);
    updateClip(selectedClip.id, {
      cropX: Math.max(0, Math.min(1, cropDrag.cropX + (e.clientX - cropDrag.x) / rangeX)),
      cropY: Math.max(0, Math.min(1, cropDrag.cropY + (e.clientY - cropDrag.y) / rangeY)),
    });
  }

  function endCropDrag() {
    cropDrag = null;
    window.removeEventListener("pointermove", onCropDrag);
  }

  function onCropWheel(e: WheelEvent) {
    if (!selectedClip || !e.ctrlKey) return;
    e.preventDefault();
    const next = Math.max(1, Math.min(4, selectedClip.zoom + (e.deltaY > 0 ? -0.08 : 0.08)));
    updateClip(selectedClip.id, { zoom: next });
  }
</script>

<div class="editShell">
  <aside class="sourcePane">
    <div class="sourceHead">
      <div>
        <strong>Source</strong>
        <span>{sources.filter((s) => s.kind === "video").length} video · {sources.filter((s) => s.kind === "audio").length} audio</span>
      </div>
      <div class="sourceTools">
        <div class="seg">
          <button class="chip" class:on={sourceFilter === "all"} onclick={() => (sourceFilter = "all")}>All</button>
          <button class="chip" class:on={sourceFilter === "video"} onclick={() => (sourceFilter = "video")}>Video</button>
          <button class="chip" class:on={sourceFilter === "audio"} onclick={() => (sourceFilter = "audio")}>Audio</button>
        </div>
        <div class="iconSeg" title="Source view">
          <button class:on={sourceView === "thumbs"} onclick={() => (sourceView = "thumbs")}>▦</button>
          <button class:on={sourceView === "list"} onclick={() => (sourceView = "list")}>☰</button>
          <button class:on={sourceView === "details"} onclick={() => (sourceView = "details")}>≡</button>
        </div>
      </div>
    </div>

    <div class="sourceList {sourceView}">
      {#if sourceLoading && !filteredSources.length}
        <div class="emptyState">Reading source folder.</div>
      {:else if filteredSources.length}
        {#each filteredSources as item (item.path)}
          <button
            class="sourceItem"
            class:audio={item.kind === "audio"}
            class:focused={sourceFocusPath === item.path}
            draggable={true}
            ondragstart={(e) => startSourceDrag(e, item)}
            ondragend={endSourceDrag}
            oncontextmenu={(e) => openSourceMenu(e, item)}
            onclick={() => {
              sourceFocusPath = item.path;
              ensureProbe(item);
            }}
            ondblclick={() => item.kind === "video" ? addVideos([item]) : addAudio(item)}
            title={item.path}
          >
            <span class="sourceThumb">
              {#if item.kind === "video"}
                <Thumb item={sourceToMedia(item)} size={192} />
              {:else}
                <span class="audioIcon">♪</span>
              {/if}
            </span>
            <span class="sourceName">
              <strong>{item.name}</strong>
              <em>{fmtSpec(item)}</em>
            </span>
            {#if sourceView === "details"}
              <span>{probes[item.path]?.camera ?? (item.kind === "audio" ? "Audio" : "-")}</span>
              <span>{fmtDate(probes[item.path]?.captured ?? item.mtime)}</span>
              <span>{fmtSize(item.size)}</span>
            {/if}
          </button>
        {/each}
      {:else}
        <div class="emptyState">No edit-ready video or audio in this folder.</div>
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
      <span class="status" class:warn={needsRender}>{needsRender ? "Render required" : "Stream copy ready"}</span>
      <span class="spacer"></span>
      <button class="miniBtn" onclick={takeSnapshot} disabled={!selectedClip || snapshotting}>
        {snapshotting ? "Saving" : "Frame"}
      </button>
      <div class="exportOpts">
        <button class="miniBtn" class:on={exportOptionsOpen} onclick={() => (exportOptionsOpen = !exportOptionsOpen)}>
          Options
        </button>
        {#if exportOptionsOpen}
          <div class="exportMenu">
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
            <label class="check"><input type="checkbox" bind:checked={preserveSourceAudio} disabled={audioClips.length > 0 || orderedClips.length !== 1} /> Keep source audio</label>
            <button class="miniBtn" onclick={() => { exportOptionsOpen = false; void pickAudio(); }}>Choose audio</button>
          </div>
        {/if}
      </div>
      <button class="exportBtn" onclick={exportTimeline} disabled={!clips.length || exporting}>
        {exporting ? "Exporting" : "Export"}
      </button>
    </div>

    <div class="preview" bind:this={previewBox} onwheel={onCropWheel}>
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
        {#if cropRect}
          <button
            class="cropFrame"
            style="left:{cropRect.left}px; top:{cropRect.top}px; width:{cropRect.w}px; height:{cropRect.h}px"
            onpointerdown={startCropDrag}
            title="Drag crop. Ctrl + mouse wheel zooms."
            aria-label="Drag crop"
          >
            <span></span>
          </button>
        {/if}
      {:else}
        <div class="emptyState">Drag a video to the timeline.</div>
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

    <section class="timeline" aria-label="Edit timeline">
      <div class="timelineHead">
        <strong>Timeline</strong>
        <span>{clips.length} video · {audioClips.length} audio · {fmt(timelineSeconds)}</span>
        <label class="scale">Zoom <input type="range" min="12" max="60" bind:value={timelineScale} /></label>
        <span class="snap">Snap</span>
        <span class="spacer"></span>
        <button class="ghost" onclick={() => { clips = []; audioClips = []; }} disabled={!clips.length && !audioClips.length}>Clear</button>
      </div>
      <div class="timelineViewport">
        <div class="timelineCanvas" style="width:{timelineWidth}px">
          <div class="ruler">
            {#each Array(Math.ceil(timelineEnd / 5) + 1) as _, i}
              <span style="left:{i * 5 * timelineScale}px">{fmt(i * 5)}</span>
            {/each}
          </div>

          {#each VIDEO_LANES as lane}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="track videoTrack" ondragover={allowDrop} ondrop={(e) => dropOnLane(e, "video", lane)}>
              <span class="trackLabel">V{lane + 1}</span>
              {#each clips.filter((c) => c.lane === lane) as clip (clip.id)}
                <button
                  class="timelineClip video"
                  class:on={clip.id === selectedId}
                  style="left:{clip.start * timelineScale}px; width:{Math.max(42, (clip.outS - clip.inS) * timelineScale)}px"
                  onclick={() => selectClip(clip.id)}
                  oncontextmenu={(e) => openTimelineMenu(e, clip)}
                  onpointerdown={(e) => startTimelinePointer(e, "video", clip.id, "move")}
                  title={clip.path}
                >
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span class="handle left" onpointerdown={(e) => startTimelinePointer(e, "video", clip.id, "trimIn")}></span>
                  <strong>{clip.name}</strong>
                  <em>{fmt(clip.outS - clip.inS)}</em>
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span class="handle right" onpointerdown={(e) => startTimelinePointer(e, "video", clip.id, "trimOut")}></span>
                </button>
              {/each}
            </div>
          {/each}

          {#each AUDIO_LANES as lane}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="track audioTrack" class:firstAudio={lane === 0} ondragover={allowDrop} ondrop={(e) => dropOnLane(e, "audio", lane)}>
              <span class="trackLabel">A{lane + 1}</span>
              {#each audioClips.filter((c) => c.lane === lane) as clip (clip.id)}
                <button
                  class="timelineClip audio"
                  class:on={clip.id === selectedAudioId}
                  style="left:{clip.start * timelineScale}px; width:{Math.max(80, clip.duration * timelineScale)}px"
                  onclick={() => selectAudio(clip.id)}
                  oncontextmenu={(e) => openAudioMenu(e, clip)}
                  onpointerdown={(e) => startTimelinePointer(e, "audio", clip.id, "move")}
                  title={clip.path}
                >
                  <strong>{clip.name}</strong>
                  <em>{fmt(clip.duration)}</em>
                </button>
              {/each}
            </div>
          {/each}
        </div>
      </div>
    </section>
  </section>

  <aside class="inspector">
    <div class="block segmentBlock">
      <h3>Segment</h3>
      {#if selectedClip}
        <div class="row">
          <button class="miniBtn" onclick={setIn}>Set in</button>
          <input type="number" min="0" max={selectedClip.outS} step="0.01" value={selectedClip.inS} oninput={(e) => updateSelectedClip({ inS: Number((e.currentTarget as HTMLInputElement).value) })} onchange={clampTrim} />
        </div>
        <div class="row">
          <button class="miniBtn" onclick={setOut}>Set out</button>
          <input type="number" min={selectedClip.inS} max={selectedClip.duration} step="0.01" value={selectedClip.outS} oninput={(e) => updateSelectedClip({ outS: Number((e.currentTarget as HTMLInputElement).value) })} onchange={clampTrim} />
        </div>
        <div class="small">Length {fmt(selectedClip.outS - selectedClip.inS)} · Track V{selectedClip.lane + 1}</div>
        {#if outPreset.fit !== "original"}
          <label>Crop X <input type="range" min="0" max="1" step="0.001" value={selectedClip.cropX} oninput={(e) => updateSelectedClip({ cropX: Number((e.currentTarget as HTMLInputElement).value) })} /></label>
          <label>Crop Y <input type="range" min="0" max="1" step="0.001" value={selectedClip.cropY} oninput={(e) => updateSelectedClip({ cropY: Number((e.currentTarget as HTMLInputElement).value) })} /></label>
          <label>Zoom <input type="range" min="1" max="4" step="0.01" value={selectedClip.zoom} oninput={(e) => updateSelectedClip({ zoom: Number((e.currentTarget as HTMLInputElement).value) })} /></label>
        {/if}
        <button class="dangerBtn" onclick={() => removeClip(selectedClip.id)}>Remove clip</button>
      {:else if selectedAudio}
        <p class="small">{selectedAudio.name}</p>
        <button class="dangerBtn" onclick={() => removeAudio(selectedAudio.id)}>Remove audio</button>
      {:else}
        <p class="small">Select a timeline clip.</p>
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

    {#if exportNote}<p class="note sideNote">{exportNote}</p>{/if}

    <div class="block exportBlock">
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
      <label class="check"><input type="checkbox" bind:checked={preserveSourceAudio} disabled={audioClips.length > 0 || orderedClips.length !== 1} /> Keep source audio</label>
      <div class="music">
        <button class="miniBtn" onclick={pickAudio}>Choose audio</button>
        {#if audioClips.length}
          <span class="small">{audioClips[0].name}</span>
        {/if}
      </div>
      {#if exportNote}<p class="note">{exportNote}</p>{/if}
    </div>
  </aside>
  {#if sourceMenu}
    <ContextMenu x={sourceMenu.x} y={sourceMenu.y} entries={sourceMenu.entries} onclose={() => (sourceMenu = null)} />
  {/if}
</div>

<style>
  .editShell {
    width: 100%;
    height: 100%;
    display: grid;
    grid-template-columns: minmax(300px, 25%) minmax(520px, 1fr) minmax(250px, 23%);
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
  .sourceHead,
  .editTop,
  .timelineHead {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 46px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-panel);
  }
  .sourceHead {
    align-items: flex-start;
    justify-content: space-between;
  }
  .sourceHead > div:first-child {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .sourceTools {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 5px;
  }
  .sourceHead span,
  .timelineHead span,
  .small,
  .note,
  .time,
  .sourceName em {
    color: var(--text-faint);
    font-size: 12px;
  }
  .seg,
  .iconSeg {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .chip {
    padding: 4px 8px;
    border-radius: 6px;
    color: var(--text-dim);
    border: 1px solid var(--border);
    background: var(--bg-elev);
    font-size: 12px;
  }
  .chip.on,
  .iconSeg button.on {
    background: var(--accent);
    color: var(--accent-on);
    border-color: var(--accent);
  }
  .iconSeg {
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--bg-elev);
  }
  .iconSeg button {
    width: 25px;
    height: 22px;
    border-radius: 5px;
    color: var(--text-dim);
    font-size: 12px;
  }
  .sourceList {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .sourceList.thumbs {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    align-content: start;
  }
  .sourceItem {
    display: grid;
    grid-template-columns: 62px minmax(0, 1fr) minmax(54px, 0.55fr) 74px 64px;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 62px;
    padding: 6px;
    border-radius: 8px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text);
    cursor: grab;
    text-align: left;
  }
  .sourceList.list .sourceItem {
    grid-template-columns: 54px minmax(0, 1fr);
  }
  .sourceList.thumbs .sourceItem {
    grid-template-columns: 1fr;
    grid-template-rows: 98px auto;
    min-height: 150px;
    align-items: stretch;
  }
  .sourceList.list .sourceItem > span:nth-child(n + 3),
  .sourceList.thumbs .sourceItem > span:nth-child(n + 3) {
    display: none;
  }
  .sourceItem:hover,
  .sourceItem.focused {
    background: var(--bg-hover);
    border-color: color-mix(in srgb, var(--accent) 42%, var(--border));
  }
  .sourceThumb {
    width: 58px;
    height: 48px;
    border-radius: 6px;
    overflow: hidden;
    background: var(--viewport-bg);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sourceList.thumbs .sourceThumb {
    width: 100%;
    height: 98px;
  }
  .audioIcon {
    width: 34px;
    height: 34px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    font-weight: 800;
  }
  .sourceName {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .sourceName strong,
  .timelineClip strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sourceItem > span:not(.sourceThumb):not(.sourceName) {
    color: var(--text-faint);
    font-size: 11.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .workPane {
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: auto minmax(240px, 1fr) auto 260px;
  }
  .editTop {
    overflow: hidden;
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
    min-width: 76px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 5px 8px;
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
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    color: var(--accent);
  }
  .exportOpts {
    position: relative;
    flex: 0 0 auto;
  }
  .miniBtn.on {
    border-color: var(--accent);
    color: var(--accent);
  }
  .exportMenu {
    position: absolute;
    right: 0;
    top: 34px;
    z-index: 40;
    width: 230px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 9px;
    box-shadow: var(--shadow);
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
    background: #050505;
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
  .emptyState {
    color: var(--text-faint);
    font-size: 13px;
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    width: 100%;
    min-height: 120px;
    border: 1px dashed color-mix(in srgb, var(--text-faint) 36%, transparent);
    border-radius: 8px;
  }
  .cropFrame {
    position: absolute;
    border: 2px solid rgba(255, 255, 255, 0.95);
    box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.34), 0 6px 26px rgba(0, 0, 0, 0.45);
    cursor: move;
    padding: 0;
    background: transparent;
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
  .transport input,
  label input[type="range"],
  .scale input {
    flex: 1;
    accent-color: var(--accent);
  }
  .play,
  .miniBtn,
  .ghost,
  .dangerBtn {
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
  .dangerBtn:hover {
    background: var(--bg-hover);
  }
  .dangerBtn {
    color: var(--reject);
    border-color: color-mix(in srgb, var(--reject) 55%, var(--border));
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
  .timelineHead {
    min-height: 38px;
  }
  .scale {
    width: 140px;
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-faint);
    font-size: 12px;
  }
  .snap {
    padding: 3px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
  }
  .timelineViewport {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .timelineCanvas {
    position: relative;
    min-height: 100%;
    padding-top: 24px;
  }
  .ruler {
    position: absolute;
    top: 0;
    left: 44px;
    right: 0;
    height: 24px;
    border-bottom: 1px solid var(--border);
  }
  .ruler span {
    position: absolute;
    top: 5px;
    color: var(--text-faint);
    font-size: 11px;
    transform: translateX(-1px);
  }
  .track {
    position: relative;
    height: 36px;
    margin-left: 44px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
    background: color-mix(in srgb, var(--viewport-bg) 70%, transparent);
  }
  .audioTrack.firstAudio {
    margin-top: 10px;
    border-top: 2px solid color-mix(in srgb, var(--accent) 65%, var(--border));
  }
  .audioTrack.firstAudio::before {
    content: "Audio";
    position: absolute;
    left: -38px;
    top: -12px;
    color: var(--accent);
    font-size: 10px;
    font-weight: 800;
    text-transform: uppercase;
  }
  .track:hover {
    background: color-mix(in srgb, var(--accent) 8%, var(--viewport-bg));
  }
  .trackLabel {
    position: absolute;
    left: -38px;
    top: 7px;
    width: 30px;
    text-align: right;
    color: var(--text-faint);
    font-size: 11px;
    font-weight: 700;
  }
  .timelineClip {
    position: absolute;
    top: 5px;
    bottom: 5px;
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 36px;
    padding: 0 10px;
    border-radius: 6px;
    color: var(--text);
    text-align: left;
    cursor: grab;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border));
    background: color-mix(in srgb, var(--accent) 22%, var(--bg-elev));
  }
  .timelineClip.audio {
    border-color: color-mix(in srgb, var(--pick) 55%, var(--border));
    background: color-mix(in srgb, var(--pick) 18%, var(--bg-elev));
  }
  .timelineClip.on {
    box-shadow: inset 0 0 0 1px var(--accent), 0 0 0 1px var(--accent);
  }
  .timelineClip em {
    color: var(--text-faint);
    font-size: 11px;
    font-style: normal;
    margin-left: auto;
  }
  .handle {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 7px;
    background: rgba(255, 255, 255, 0.22);
    cursor: ew-resize;
  }
  .handle.left { left: 0; }
  .handle.right { right: 0; }
  .block {
    padding: 12px;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  .segmentBlock,
  .exportBlock {
    display: none;
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
  .note {
    margin: 0;
  }
  .sideNote {
    margin: 10px 12px 0;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-elev);
    color: var(--text-dim);
    font-size: 12px;
  }
  @media (max-width: 1180px) {
    .editShell {
      grid-template-columns: 270px minmax(0, 1fr);
    }
    .inspector {
      display: none;
    }
    .presetGroup button {
      min-width: 64px;
    }
  }
</style>
