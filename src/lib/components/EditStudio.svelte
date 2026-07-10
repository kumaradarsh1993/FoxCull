<script module lang="ts">
  type TrimMemory = { inS: number; outS: number };
  const sessionTrimMemory = new Map<string, TrimMemory>();
</script>

<script lang="ts">
  import { tick } from "svelte";
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

  type PresetId = "original" | "landscape" | "square" | "reels" | "mobile";
  type ExportTarget = "instagram_reels" | "instagram_square" | "instagram_landscape" | "whatsapp" | "archive";
  type LookPresetId = "neutral" | "drone" | "osmo" | "phone" | "sunset" | "lowlight";
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
    startY: number;
    start: number;
    lane: number;
    inS: number;
    outS: number;
    duration: number;
    // For a group "move" drag: the initial start/lane of every selected clip so
    // they all shift by the same delta.
    group: { id: string; start: number; lane: number }[];
  };

  type ProgramSeg = { start: number; end: number; clip: TimelineClip | null };
  type ProgramClip = {
    path: string;
    name: string;
    in_s: number;
    out_s: number;
    crop_x: number;
    crop_y: number;
    zoom: number;
  };

  const PRESETS: Record<PresetId, { label: string; detail: string; w: number; h: number; fit: "crop" | "original" }> = {
    original: { label: "Original", detail: "Stream copy", w: 0, h: 0, fit: "original" },
    landscape: { label: "16:9", detail: "1920x1080", w: 1920, h: 1080, fit: "crop" },
    square: { label: "1:1", detail: "1080x1080", w: 1080, h: 1080, fit: "crop" },
    reels: { label: "9:16", detail: "Reels/Stories", w: 1080, h: 1920, fit: "crop" },
    mobile: { label: "Mobile", detail: "720x1280", w: 720, h: 1280, fit: "crop" },
  };

  const EXPORT_TARGETS: Record<ExportTarget, { label: string; preset: PresetId; quality: Quality; detail: string }> = {
    instagram_reels: { label: "Instagram Reels/Stories", preset: "reels", quality: "high", detail: "1080x1920 H.264, source FPS" },
    instagram_square: { label: "Instagram square", preset: "square", quality: "high", detail: "1080x1080 H.264" },
    instagram_landscape: { label: "Instagram landscape", preset: "landscape", quality: "high", detail: "1920x1080 H.264" },
    whatsapp: { label: "WhatsApp/mobile", preset: "mobile", quality: "standard", detail: "Smaller 720x1280 file" },
    archive: { label: "Archive/original", preset: "original", quality: "best", detail: "Stream-copy when possible" },
  };

  const LOOK_PRESETS: Record<LookPresetId, { label: string; hint: string; values: EditAdjustments }> = {
    neutral: { label: "Neutral", hint: "Reset", values: { brightness: 0, contrast: 1, saturation: 1, warmth: 0, sharpen: 0 } },
    drone: { label: "Drone pop", hint: "Mavic Mini", values: { brightness: 0.015, contrast: 1.11, saturation: 1.16, warmth: 0.015, sharpen: 0.18 } },
    osmo: { label: "Osmo clean", hint: "Pocket 3", values: { brightness: 0.005, contrast: 1.06, saturation: 1.08, warmth: 0.02, sharpen: 0.12 } },
    phone: { label: "Phone natural", hint: "Samsung/iPhone", values: { brightness: 0, contrast: 1.04, saturation: 1.04, warmth: 0, sharpen: 0.08 } },
    sunset: { label: "Warm travel", hint: "Golden hour", values: { brightness: 0.01, contrast: 1.08, saturation: 1.12, warmth: 0.08, sharpen: 0.1 } },
    lowlight: { label: "Low light", hint: "Night clips", values: { brightness: 0.035, contrast: 0.95, saturation: 1.03, warmth: 0.02, sharpen: 0.05 } },
  };

  const VIDEO_LANES = [0, 1, 2];
  const AUDIO_LANES = [0, 1, 2];
  const TRACK_HEIGHT = 36;
  const SNAP = 0.16;
  const TIMELINE_ZOOM_MIN = 12;
  const TIMELINE_ZOOM_MAX = 60;
  const TIMELINE_TRACK_OFFSET = 44;
  const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  const extOf = (p: string) => (basename(p).match(/\.([^.]+)$/)?.[1] ?? "").toLowerCase();
  const normPath = (p: string) =>
    p
      .replace(/^\\\\\?\\/, "")
      .replace(/\//g, "\\")
      .replace(/\\+$/g, "")
      .toLowerCase();

  function rememberedTrim(path: string, duration: number): TrimMemory {
    const key = normPath(path);
    const saved = sessionTrimMemory.get(key);
    const full = Math.max(0.1, duration || 1);
    if (!saved) return { inS: 0, outS: full };
    const inS = Math.max(0, Math.min(saved.inS, Math.max(0, full - 0.05)));
    const outS = Math.min(full, Math.max(saved.outS, inS + 0.05));
    return { inS, outS };
  }

  function rememberTrim(clip: TimelineClip) {
    sessionTrimMemory.set(normPath(clip.path), { inS: clip.inS, outS: clip.outS });
  }

  let clips = $state<TimelineClip[]>([]);
  let audioClips = $state<AudioClip[]>([]);
  let selectedId = $state<string | null>(null);
  let selectedIds = $state<Set<string>>(new Set());
  let selectedAudioId = $state<string | null>(null);
  // Program playback: the top player follows a single timeline playhead across
  // every clip/gap, not one clicked clip.
  let playheadS = $state(0);
  let playing = $state(false);
  // While dragging a clip's trim handle we park the player on the moving edge.
  let trimPreview = $state<{ id: string; time: number } | null>(null);
  let preset = $state<PresetId>("reels");
  let exportTarget = $state<ExportTarget>("instagram_reels");
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
  let sourcePanelW = $state(360);
  let inspectorPanelW = $state(320);
  let timelinePanelH = $state(260);
  let sourceCollapsed = $state(false);
  let inspectorCollapsed = $state(false);
  let timelineCollapsed = $state(false);
  let productionPreview = $state(false);
  let dragSourcePath = $state<string | null>(null);
  let seededKey = $state("");
  let seeding = $state(false);
  let timelineDrag: TimelineDrag | null = null;
  let sourceMenu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  let exportMenuOpen = $state(false);
  type DlgMode = "instagram" | "lossless" | "custom";
  let exportDlg = $state(false);
  let dlgMode = $state<DlgMode>("instagram");
  let keepHdr = $state(false);
  let frameToast = $state<string | null>(null);
  let pendingPreviewSeek = $state<number | null>(null);
  let previewSeekRAF = 0;
  // Program engine internals (plain refs — not reactive state).
  let engineRAF = 0;
  let lastWall = 0;
  let loadedSrc = "";
  let pendingSeek: number | null = null;
  let pendingPlay = false;
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
  let audioEnd = $derived(audioClips.reduce((max, c) => Math.max(max, c.start + c.duration), 0));
  let videoEnd = $derived(clips.reduce((max, c) => Math.max(max, c.start + Math.max(0, c.outS - c.inS)), 0));
  // The "program": video clips resolved into an ordered, gap-inclusive sequence
  // covering [0, videoEnd]. Where clips overlap across lanes the LOWER lane index
  // wins (V1 beats V2 beats V3). Null-clip segments are gaps (played black).
  let program = $derived.by<ProgramSeg[]>(() => {
    const end = videoEnd;
    if (end <= 0) return [];
    const bounds = new Set<number>([0, end]);
    for (const c of clips) {
      const s = Math.max(0, Math.min(c.start, end));
      const e = Math.max(0, Math.min(c.start + clipLen(c), end));
      if (e > s) {
        bounds.add(s);
        bounds.add(e);
      }
    }
    const marks = [...bounds].sort((a, b) => a - b);
    const raw: ProgramSeg[] = [];
    for (let i = 0; i < marks.length - 1; i++) {
      const a = marks[i];
      const b = marks[i + 1];
      if (b - a < 1e-4) continue;
      const mid = (a + b) / 2;
      let winner: TimelineClip | null = null;
      for (const c of clips) {
        if (mid >= c.start && mid < c.start + clipLen(c)) {
          if (!winner || c.lane < winner.lane) winner = c;
        }
      }
      raw.push({ start: a, end: b, clip: winner });
    }
    // Merge adjacent segments that resolve to the same clip (or both gaps).
    const merged: ProgramSeg[] = [];
    for (const seg of raw) {
      const last = merged[merged.length - 1];
      if (last && (last.clip?.id ?? null) === (seg.clip?.id ?? null) && Math.abs(last.end - seg.start) < 1e-4) {
        last.end = seg.end;
      } else {
        merged.push({ ...seg });
      }
    }
    return merged;
  });
  // The flattened, gap-free export list: each program segment mapped back to its
  // source in/out — this (top-lane-wins) is what export sends.
  let programClips = $derived.by<ProgramClip[]>(() =>
    program
      .filter((s) => s.clip)
      .map((s) => {
        const c = s.clip as TimelineClip;
        return {
          path: c.path,
          name: c.name,
          in_s: c.inS + (s.start - c.start),
          out_s: c.inS + (s.end - c.start),
          crop_x: c.cropX,
          crop_y: c.cropY,
          zoom: c.zoom,
        };
      }),
  );
  let programSeconds = $derived(programClips.reduce((sum, c) => sum + Math.max(0, c.out_s - c.in_s), 0));
  // Distinct source resolutions across the program (drives the "conform" step).
  let programResCount = $derived.by(() => {
    const set = new Set<string>();
    for (const pc of programClips) {
      const p = probes[pc.path];
      if (p?.width && p?.height) set.add(`${p.width}x${p.height}`);
    }
    return set.size;
  });
  // Mixed resolutions OR codecs across the program: the backend refuses to
  // stream-copy-concat these (it would produce a broken file) and silently
  // re-encodes instead — the dialog's stream-copy promises must match that.
  let mixedSources = $derived.by(() => {
    if (programResCount >= 2) return true;
    const codecs = new Set<string>();
    for (const pc of programClips) {
      const c = probes[pc.path]?.codec;
      if (c) codecs.add(c);
    }
    return codecs.size >= 2;
  });
  let timelineEnd = $derived(Math.max(10, videoEnd, audioEnd));
  let timelineWidth = $derived(Math.max(980, timelineEnd * timelineScale + 220));
  let previewFilter = $derived(
    `brightness(${Math.max(0, 1 + adjustments.brightness)}) contrast(${adjustments.contrast}) saturate(${adjustments.saturation})`,
  );
  // Dialog data: the source clip we're about to optimise, its probe, and a rough
  // export-time estimate (HDR tone-mapping is slower than a plain re-encode).
  let igSourceClip = $derived(selectedClip ?? orderedClips[0] ?? null);
  let igSourceProbe = $derived(igSourceClip ? probes[igSourceClip.path] ?? null : null);
  let igEstimateSecs = $derived.by(() => {
    const secs = programSeconds || (igSourceClip ? igSourceClip.outS - igSourceClip.inS : 0);
    const hdrHeavy = igSourceProbe?.hdr && dlgMode === "instagram";
    const streamCopy = dlgMode === "lossless" && outPreset.fit === "original" && !mixedSources;
    const factor = hdrHeavy ? 1.5 : streamCopy ? 0.05 : 0.7;
    return Math.max(streamCopy ? 1 : 4, Math.round(secs * factor));
  });
  // The effective post-crop source rectangle (in source pixels) for the current
  // output aspect + clip zoom. Single source of truth for the compare card, the
  // soft-crop warning, and the time breakdown.
  function cropDims(p: MediaProbe | null, clip: { zoom: number } | null): { cropW: number; cropH: number } | null {
    if (!p?.width || !p?.height) return null;
    const aspect = outPreset.w / outPreset.h;
    const zoom = clip?.zoom && clip.zoom > 0 ? clip.zoom : 1;
    const cropW = Math.min(p.width, p.height * aspect) / zoom;
    const cropH = Math.min(p.height, p.width / aspect) / zoom;
    return { cropW, cropH };
  }
  let igCrop = $derived(outPreset.fit === "original" ? null : cropDims(igSourceProbe, igSourceClip));
  let neutralLook = $derived(
    Math.abs(adjustments.brightness) < 0.001 &&
      Math.abs(adjustments.contrast - 1) < 0.001 &&
      Math.abs(adjustments.saturation - 1) < 0.001 &&
      Math.abs(adjustments.warmth) < 0.001 &&
      adjustments.sharpen < 0.001,
  );
  // Will a vertical crop of this source have to upscale (→ soft)? True when the
  // crop's pixel width is below the output width (e.g. a 1080p landscape → 9:16).
  let softCrop = $derived(!!igCrop && igCrop.cropW < outPreset.w - 1);
  // Rule-based breakdown of what drives the export time (shown under the estimate).
  let exportSteps = $derived.by(() => {
    const steps: { label: string; note: string }[] = [];
    const p = igSourceProbe;
    if (dlgMode === "lossless" && outPreset.fit === "original" && neutralLook && !mixedSources) {
      steps.push({ label: "Trim (stream copy)", note: "instant — no re-encode" });
      return steps;
    }
    if (p?.hdr && dlgMode === "instagram" && keepHdr) steps.push({ label: "Keep HDR (10-bit HEVC)", note: "heavy" });
    else if (p?.hdr && dlgMode === "instagram") steps.push({ label: "HDR → SDR tone-map", note: "heaviest step" });
    if (igCrop && outPreset.fit !== "original") {
      const cw = Math.round(igCrop.cropW);
      if (cw > outPreset.w + 1) steps.push({ label: `Downscale ${cw}→${outPreset.w}px`, note: "moderate" });
      else if (cw < outPreset.w - 1) steps.push({ label: `Upscale ${cw}→${outPreset.w}px`, note: "light" });
    }
    if (mixedSources) steps.push({ label: "Conform mixed sources", note: "moderate" });
    if (softCrop) steps.push({ label: "Sharpen soft crop", note: "light" });
    if (p?.fps && p.fps > 31 && dlgMode === "instagram") steps.push({ label: `${Math.round(p.fps)}→30 fps`, note: "light" });
    if (!neutralLook) steps.push({ label: "Look adjustments", note: "light" });
    steps.push({ label: keepHdr && dlgMode === "instagram" ? "HEVC encode" : "H.264 encode", note: "scales with clip length" });
    return steps;
  });
  let needsRender = $derived(
    outPreset.fit !== "original" ||
      audioClips.length > 0 ||
      Math.abs(adjustments.brightness) > 0.001 ||
      Math.abs(adjustments.contrast - 1) > 0.001 ||
      Math.abs(adjustments.saturation - 1) > 0.001 ||
      Math.abs(adjustments.warmth) > 0.001 ||
      Math.abs(adjustments.sharpen) > 0.001,
  );
  // Warmth is a blend overlay (CSS filter can't warm/cool) — see the preview tint.
  let warmthTint = $derived.by(() => {
    const w = adjustments.warmth;
    if (Math.abs(w) < 0.001) return null;
    return { color: w > 0 ? "#ff8a2a" : "#3b7dff", opacity: Math.min(0.6, Math.abs(w) * 1.8) };
  });
  // The crop overlay only makes sense when the clip under the playhead IS the
  // selected clip (that's the frame the player is showing + the crop applies to).
  let cropVisible = $derived(
    !productionPreview && !!selectedClip && (segAt(playheadS)?.clip?.id ?? null) === (selectedClip?.id ?? null),
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

  let sourceMetaByPath = $derived.by(() => {
    const m = new Map<string, MediaItem>();
    for (const item of [...sourceItems, ...selectedItems, ...(active ? [active] : [])]) {
      m.set(normPath(item.path), item);
    }
    return m;
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
    void seedInitial(initialVideos.map(mediaToSource)).finally(() => {
      seeding = false;
    });
  });

  // Keep the paused player parked on the correct frame: the clip under the
  // playhead (or a trim edge, mid-drag). While PLAYING, the engine owns the
  // video and this effect bows out (it reads `playing` first, so it doesn't even
  // subscribe to playheadS during playback → no per-frame re-seeks).
  $effect(() => {
    const v = previewVideo;
    const pp = productionPreview;
    const tp = trimPreview;
    const isPlaying = playing;
    if (!v || pp || isPlaying) return;
    if (tp) {
      const c = clips.find((x) => x.id === tp.id);
      if (c) syncVideoImperative(c, tp.time, false);
      return;
    }
    const ph = playheadS;
    const seg = segAt(ph);
    const clip = seg?.clip ?? null;
    syncVideoImperative(clip, clip ? clip.inS + (ph - clip.start) : 0, false);
  });

  // Production preview swaps in a different <video> element with a reactive src,
  // so drop the imperative src bookkeeping when the mode flips.
  $effect(() => {
    productionPreview;
    loadedSrc = "";
    pendingSeek = null;
    pendingPlay = false;
  });

  // Clamp the playhead into range as the timeline changes.
  $effect(() => {
    if (playheadS > videoEnd) playheadS = Math.max(0, videoEnd);
  });

  $effect(() => {
    if (!selectedClip && productionPreview) productionPreview = false;
  });

  $effect(() => {
    if (!productionPreview) return;
    const onPreviewKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      productionPreview = false;
      e.preventDefault();
      e.stopImmediatePropagation();
    };
    window.addEventListener("keydown", onPreviewKey, { capture: true });
    return () => window.removeEventListener("keydown", onPreviewKey, { capture: true });
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

  const clipLen = (c: { inS: number; outS: number }) => Math.max(0.05, c.outS - c.inS);

  // The program segment (clip or gap) that contains timeline time t.
  function segAt(t: number): ProgramSeg | null {
    for (const s of program) if (t >= s.start - 1e-4 && t < s.end - 1e-4) return s;
    return program.length ? program[program.length - 1] : null;
  }

  // Push a candidate placement right until it no longer overlaps an existing clip
  // on the same lane — the "never overwrite what's already there" rule.
  function freeStart(lane: number, start: number, len: number, ignoreId?: string): number {
    let s = Math.max(0, start);
    for (let guard = 0; guard < 400; guard++) {
      const conflict = clips.find(
        (o) => o.id !== ignoreId && o.lane === lane && s < o.start + clipLen(o) - 1e-4 && s + len > o.start + 1e-4,
      );
      if (!conflict) break;
      s = conflict.start + clipLen(conflict);
    }
    return s;
  }

  // After a group move, shove each moved clip clear of any NON-moved clip it now
  // overlaps on its lane (moved clips keep their relative layout).
  function resolveOverlaps(movedIds: string[]) {
    const moved = new Set(movedIds);
    const next = clips.map((c) => ({ ...c }));
    const movedClips = next.filter((c) => moved.has(c.id)).sort((a, b) => a.start - b.start);
    for (const mc of movedClips) {
      for (let guard = 0; guard < 400; guard++) {
        const conflict = next.find(
          (o) =>
            o.id !== mc.id &&
            !moved.has(o.id) &&
            o.lane === mc.lane &&
            mc.start < o.start + clipLen(o) - 1e-4 &&
            mc.start + clipLen(mc) > o.start + 1e-4,
        );
        if (!conflict) break;
        mc.start = conflict.start + clipLen(conflict);
      }
    }
    clips = next;
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

  function sourceDuration(src: EditSourceItem): string {
    const p = probes[src.path];
    if (p?.duration) return fmt(p.duration);
    return src.kind === "audio" ? "Audio" : "Video";
  }

  function sourceSubline(src: EditSourceItem): string {
    const p = probes[src.path];
    if (!p) return src.kind === "audio" ? src.ext.toUpperCase() : "Reading details...";
    if (src.kind === "audio") return [p.codec ?? src.ext.toUpperCase(), fmtSize(src.size)].filter(Boolean).join(" - ");
    const res = p.width && p.height ? `${p.width}x${p.height}` : "";
    const fps = p.fps ? `${Math.round(p.fps)}fps` : "";
    return [res, fps, p.codec ?? src.ext.toUpperCase()].filter(Boolean).join(" - ");
  }

  function sourceMetaChips(src: EditSourceItem): string[] {
    const p = probes[src.path];
    if (src.kind === "audio") return [fmtDate(src.mtime), fmtSize(src.size)].filter((v) => v && v !== "-");
    return [p?.camera ?? "", fmtDate(p?.captured ?? src.mtime), fmtSize(src.size)].filter((v) => v && v !== "-");
  }

  function sourceStateChips(src: EditSourceItem): string[] {
    const meta = sourceMetaByPath.get(normPath(src.path));
    if (!meta) return [];
    const out: string[] = [];
    if (meta.flag === "pick") out.push("Pick");
    if (meta.flag === "reject") out.push("Reject");
    if (meta.rating > 0) out.push(`${meta.rating} star${meta.rating === 1 ? "" : "s"}`);
    if (meta.label) out.push(meta.label);
    if (meta.tags.length) out.push(...meta.tags.slice(0, 2).map((t) => `#${t}`));
    return out;
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
    const trim = rememberedTrim(src.path, out);
    const cachedProxy = await api.videoProxyCached(src.path);
    return {
      id: uid(),
      path: src.path,
      name: src.name,
      src: api.fileSrc(cachedProxy ?? src.path),
      inS: trim.inS,
      outS: trim.outS,
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
      // Never overwrite an existing clip on the lane — push right if it collides.
      clip.start = freeStart(lane, cursor, clipLen(clip), clip.id);
      made.push(clip);
      cursor = clip.start + clipLen(clip);
    }
    if (!made.length) return;
    clips = [...clips, ...made];
    selectClip(made[made.length - 1].id);
    exportNote = null;
  }

  // Seed the timeline from the library selection, carrying over any in/out marks
  // made in Focus/Loupe (persisted per-path in the catalog): marked subclips seed
  // one timeline clip each; otherwise the saved trim (or the full clip) is used.
  async function seedInitial(items: EditSourceItem[]) {
    const made: TimelineClip[] = [];
    let cursor = nextVideoStart(0);
    for (const item of items.filter((s) => s.kind === "video")) {
      const duration = await durationFor(item);
      const full = Math.max(0.1, duration || 1);
      let ranges: { inS: number; outS: number }[] = [];
      try {
        const segs = await api.getVideoSegments(item.path);
        if (segs?.length) {
          ranges = segs.map((s) => ({
            inS: Math.max(0, Math.min(s.in_s, full - 0.05)),
            outS: Math.min(full, Math.max(s.out_s, Math.max(0, s.in_s) + 0.05)),
          }));
        }
      } catch {
        /* no segments — fall through to trim */
      }
      if (!ranges.length) {
        let t = rememberedTrim(item.path, full);
        try {
          const bt = await api.getTrim(item.path);
          if (bt) {
            const inS = Math.max(0, Math.min(bt[0], full - 0.05));
            t = { inS, outS: Math.min(full, Math.max(bt[1], inS + 0.05)) };
          }
        } catch {
          /* no saved trim — keep session/default */
        }
        ranges = [t];
      }
      const cachedProxy = await api.videoProxyCached(item.path);
      const src = api.fileSrc(cachedProxy ?? item.path);
      for (const r of ranges) {
        made.push({
          id: uid(),
          path: item.path,
          name: item.name,
          src,
          inS: r.inS,
          outS: r.outS,
          duration: full,
          start: cursor,
          lane: 0,
          cropX: 0.5,
          cropY: 0.5,
          zoom: 1,
        });
        cursor += Math.max(0.1, r.outS - r.inS);
      }
    }
    if (!made.length) return;
    clips = [...clips, ...made];
    selectClip(made[made.length - 1].id);
    exportNote = null;
  }

  // Default to A3 so picked music doesn't sit under V1's source-audio mirror bars.
  async function addAudio(src: EditSourceItem, lane = 2, start = nextAudioStart(lane)) {
    if (src.kind !== "audio") return;
    const duration = await durationFor(src);
    const clip = { id: uid(), path: src.path, name: src.name, start, duration: Math.max(1, duration || 30), lane };
    audioClips = [...audioClips, clip];
    selectAudio(clip.id);
    preserveSourceAudio = false;
    exportNote = null;
  }

  function removeClip(id: string) {
    clips = clips.filter((c) => c.id !== id);
    if (selectedIds.has(id)) {
      const next = new Set(selectedIds);
      next.delete(id);
      selectedIds = next;
    }
    if (selectedId === id) selectedId = clips[0]?.id ?? null;
    exportNote = null;
  }

  function removeAudio(id: string) {
    audioClips = audioClips.filter((c) => c.id !== id);
    if (selectedAudioId === id) selectedAudioId = null;
    exportNote = null;
  }

  function duplicateClip(clip: TimelineClip) {
    const len = clipLen(clip);
    const copy = { ...clip, id: uid(), start: freeStart(clip.lane, clip.start + len, len) };
    clips = [...clips, copy];
    selectClip(copy.id);
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
    clips = clips.map((clip) => {
      if (clip.id !== id) return clip;
      const next = { ...clip, ...patch };
      if ("inS" in patch || "outS" in patch) rememberTrim(next);
      return next;
    });
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
    const clip = productionPreview ? selectedClip : segAt(playheadS)?.clip ?? selectedClip;
    if (clip) void ensurePreviewProxy(clip);
  }

  // The single funnel that positions the top <video>: which source it holds and
  // where in that source it sits. Skips redundant loads (a repeated src is just a
  // seek) and defers seeks until metadata is ready.
  function syncVideoImperative(clip: TimelineClip | null, srcTime: number, play: boolean) {
    const v = previewVideo;
    if (!v || productionPreview) return;
    if (!clip) {
      // Gap → black: drop the source entirely.
      if (loadedSrc) {
        try {
          v.pause();
        } catch {
          /* ignore */
        }
        v.removeAttribute("src");
        v.load();
        loadedSrc = "";
      }
      return;
    }
    if (loadedSrc !== clip.src) {
      loadedSrc = clip.src;
      pendingSeek = srcTime;
      pendingPlay = play;
      v.src = clip.src;
      v.load();
      return;
    }
    if (v.readyState < 1) {
      pendingSeek = srcTime;
      pendingPlay = play;
      return;
    }
    if (Math.abs(v.currentTime - srcTime) > 0.033) {
      try {
        v.currentTime = srcTime;
      } catch {
        /* ignore */
      }
    }
    if (play) {
      if (v.paused) v.play().catch(() => {});
    } else if (!v.paused) {
      v.pause();
    }
  }

  function onMeta() {
    const v = previewVideo;
    if (!v) return;
    const clip = productionPreview ? selectedClip : segAt(playheadS)?.clip ?? null;
    if (clip) {
      videoW = v.videoWidth || probes[clip.path]?.width || videoW;
      videoH = v.videoHeight || probes[clip.path]?.height || videoH;
      const d = Number.isFinite(v.duration) ? v.duration : clip.duration;
      if (d > 0 && Math.abs(d - clip.duration) > 0.01) {
        updateClip(clip.id, { duration: d, outS: Math.min(clip.outS || d, d) });
      }
    }
    if (productionPreview && selectedClip) {
      if (v.currentTime < selectedClip.inS || v.currentTime > selectedClip.outS) {
        v.currentTime = selectedClip.inS;
        currentTime = selectedClip.inS;
      }
      return;
    }
    if (pendingSeek != null) {
      try {
        v.currentTime = pendingSeek;
      } catch {
        /* ignore */
      }
      pendingSeek = null;
    }
    if (pendingPlay) {
      v.play().catch(() => {});
      pendingPlay = false;
    }
  }

  // ---- Program engine (sequence playback across clips + gaps) ----

  function scheduleTick() {
    if (engineRAF) return;
    engineRAF = requestAnimationFrame(engineTick);
  }

  // Read the video's own clock into the playhead; advance at the segment edge.
  // Returns true when it handled a live clip segment.
  function sampleFromVideo(): boolean {
    const v = previewVideo;
    if (!v) return false;
    const seg = segAt(playheadS);
    if (!seg?.clip) return false;
    const clip = seg.clip;
    if (loadedSrc !== clip.src || v.readyState < 1) return false;
    const segOutSrc = clip.inS + (seg.end - seg.start);
    const ph = seg.start + (v.currentTime - clip.inS);
    if (v.currentTime >= segOutSrc - 1e-3 || ph >= seg.end - 1e-3) {
      advanceFrom(seg);
      return true;
    }
    if (ph > playheadS) playheadS = Math.min(ph, seg.end);
    return true;
  }

  function advanceFrom(seg: ProgramSeg) {
    const next = seg.end;
    if (next >= videoEnd - 1e-3) {
      playheadS = videoEnd;
      stopPlayback();
      return;
    }
    playheadS = next;
    const nseg = segAt(playheadS);
    if (nseg?.clip) syncVideoImperative(nseg.clip, nseg.clip.inS + (playheadS - nseg.clip.start), true);
    else syncVideoImperative(null, 0, false);
  }

  function engineTick() {
    engineRAF = 0;
    if (!playing) return;
    const seg = segAt(playheadS);
    if (!seg) {
      stopPlayback();
      return;
    }
    const now = performance.now();
    const dt = Math.max(0, (now - lastWall) / 1000);
    lastWall = now;
    if (seg.clip) {
      const v = previewVideo;
      if (v && loadedSrc === seg.clip.src && v.readyState >= 1) {
        if (v.paused) v.play().catch(() => {});
        sampleFromVideo();
      } else {
        syncVideoImperative(seg.clip, seg.clip.inS + (playheadS - seg.clip.start), true);
      }
    } else {
      const ph = playheadS + dt;
      if (ph >= seg.end - 1e-3) advanceFrom(seg);
      else playheadS = ph;
    }
    if (playing) scheduleTick();
  }

  function onNormalTime() {
    if (playing) sampleFromVideo();
  }

  function startPlayback() {
    if (!clips.length) return;
    if (playheadS >= videoEnd - 1e-3) playheadS = 0;
    playing = true;
    lastWall = performance.now();
    const seg = segAt(playheadS);
    if (seg?.clip) syncVideoImperative(seg.clip, seg.clip.inS + (playheadS - seg.clip.start), true);
    else syncVideoImperative(null, 0, false);
    scheduleTick();
  }

  function stopPlayback() {
    playing = false;
    if (engineRAF) {
      cancelAnimationFrame(engineRAF);
      engineRAF = 0;
    }
    try {
      previewVideo?.pause();
    } catch {
      /* ignore */
    }
  }

  function seekTimeline(t: number) {
    const clamped = Math.max(0, Math.min(t, videoEnd));
    playheadS = clamped;
    lastWall = performance.now();
    if (playing) {
      const seg = segAt(clamped);
      if (seg?.clip) syncVideoImperative(seg.clip, seg.clip.inS + (clamped - seg.clip.start), true);
      else syncVideoImperative(null, 0, false);
    }
    // Paused: the frame-sync $effect repositions the video off playheadS.
  }

  export function togglePlay() {
    if (productionPreview) {
      togglePlayProduction();
      return;
    }
    if (playing) stopPlayback();
    else startPlayback();
  }

  export function seekBy(delta: number) {
    if (productionPreview) {
      seekProduction(currentTime + delta);
      return;
    }
    seekTimeline(playheadS + delta);
  }

  // ---- Production ("Preview" / fullscreen output) — per-clip, engine bypassed ----

  function togglePlayProduction() {
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

  function onProdTime() {
    if (!previewVideo || !selectedClip) return;
    currentTime = previewVideo.currentTime || 0;
    if (currentTime > selectedClip.outS) {
      previewVideo.pause();
      previewVideo.currentTime = selectedClip.inS;
    }
  }

  function seekProduction(t: number) {
    if (!selectedClip) return;
    const next = Math.max(selectedClip.inS, Math.min(t, selectedClip.outS));
    currentTime = next;
    pendingPreviewSeek = next;
    if (previewSeekRAF) return;
    previewSeekRAF = requestAnimationFrame(() => {
      const target = pendingPreviewSeek;
      pendingPreviewSeek = null;
      previewSeekRAF = 0;
      if (target != null && previewVideo) {
        if ("fastSeek" in previewVideo && typeof previewVideo.fastSeek === "function") {
          try {
            previewVideo.fastSeek(target);
            return;
          } catch {
            /* fall back */
          }
        }
        previewVideo.currentTime = target;
      }
    });
  }

  async function syncProductionPreviewTime() {
    await tick();
    if (productionPreview && previewVideo && selectedClip) {
      previewVideo.currentTime = Math.max(selectedClip.inS, Math.min(currentTime, selectedClip.outS));
    }
  }

  export async function setOutputPreview(on: boolean) {
    if (on && !selectedClip) return;
    if (on) stopPlayback();
    productionPreview = on && !!selectedClip;
    exportMenuOpen = false;
    exportDlg = false;
    if (productionPreview) {
      currentTime = Math.max(selectedClip!.inS, Math.min(currentTime, selectedClip!.outS));
      await syncProductionPreviewTime();
    }
  }

  async function toggleProductionPreview() {
    await setOutputPreview(!productionPreview);
  }

  // [ / ] trim the SELECTED clip at the playhead (only when the playhead is
  // inside it). Trimming the in-point keeps the remaining content in place by
  // shifting `start`, mirroring the trimIn drag.
  export function setIn() {
    const clip = selectedClip;
    if (!clip) return;
    const len = clipLen(clip);
    if (playheadS < clip.start - 1e-3 || playheadS > clip.start + len + 1e-3) return;
    const local = clip.inS + (playheadS - clip.start);
    const nextIn = Math.max(0, Math.min(local, clip.outS - 0.05));
    updateClip(clip.id, { start: clip.start + (nextIn - clip.inS), inS: nextIn });
  }

  export function setOut() {
    const clip = selectedClip;
    if (!clip) return;
    const len = clipLen(clip);
    if (playheadS < clip.start - 1e-3 || playheadS > clip.start + len + 1e-3) return;
    const local = clip.inS + (playheadS - clip.start);
    const nextOut = Math.max(clip.inS + 0.05, Math.min(local, clip.duration));
    updateClip(clip.id, { outS: nextOut });
  }

  // Razor-split at the playhead: the selected clips under it, else every clip the
  // playhead strictly contains. Clip A keeps [inS, cutLocal]; a new B holds
  // [cutLocal, outS] at start = playhead.
  export function cutAtPlayhead() {
    const t = playheadS;
    const contains = (c: TimelineClip) => c.start < t - 1e-3 && c.start + clipLen(c) > t + 1e-3;
    const sel = clips.filter((c) => selectedIds.has(c.id) && contains(c));
    const targets = new Set((sel.length ? sel : clips.filter(contains)).map((c) => c.id));
    if (!targets.size) return;
    const additions: TimelineClip[] = [];
    const updated = clips.map((c) => {
      if (!targets.has(c.id)) return c;
      const cutLocal = c.inS + (t - c.start);
      additions.push({ ...c, id: uid(), inS: cutLocal, start: t });
      return { ...c, outS: cutLocal };
    });
    clips = [...updated, ...additions];
    exportNote = null;
  }

  export function deleteSelected() {
    if (selectedIds.size) {
      clips = clips.filter((c) => !selectedIds.has(c.id));
      selectedIds = new Set();
      selectedId = clips[0]?.id ?? null;
    }
    if (selectedAudioId) {
      audioClips = audioClips.filter((a) => a.id !== selectedAudioId);
      selectedAudioId = null;
    }
    exportNote = null;
  }

  const NEUTRAL_ADJ: EditAdjustments = { brightness: 0, contrast: 1, saturation: 1, warmth: 0, sharpen: 0 };

  function resetColor() {
    adjustments = { ...NEUTRAL_ADJ };
  }

  // Lightroom habit: double-click a slider to snap that one control back to its
  // neutral value.
  function resetAdj(field: keyof EditAdjustments) {
    adjustments = { ...adjustments, [field]: NEUTRAL_ADJ[field] };
  }

  function applyLook(id: LookPresetId) {
    adjustments = { ...LOOK_PRESETS[id].values };
  }

  // Small numeric readout beside each slider label (signed for brightness/warmth).
  function adjReadout(field: keyof EditAdjustments): string {
    const v = adjustments[field];
    if (field === "brightness" || field === "warmth") {
      if (Math.abs(v) < 0.0005) return "0";
      return `${v > 0 ? "+" : ""}${v.toFixed(2)}`;
    }
    return v.toFixed(2);
  }

  // A live CSS-filter preview of a look, used for the preset swatches so they
  // show what they do instead of being flat text tiles.
  function lookFilter(v: EditAdjustments): string {
    const warm = Math.max(0, v.warmth) * 3;
    return `brightness(${(1 + v.brightness).toFixed(3)}) contrast(${v.contrast}) saturate(${v.saturation}) sepia(${warm.toFixed(3)})`;
  }

  function setPreset(id: PresetId) {
    preset = id;
    const match = (Object.entries(EXPORT_TARGETS) as [ExportTarget, (typeof EXPORT_TARGETS)[ExportTarget]][]).find(
      ([, target]) => target.preset === id,
    );
    if (match) {
      exportTarget = match[0];
    }
  }

  function clampPanel(n: number, min: number, max: number) {
    return Math.max(min, Math.min(max, n));
  }

  function clampTimelineScale(n: number) {
    return clampPanel(n, TIMELINE_ZOOM_MIN, TIMELINE_ZOOM_MAX);
  }

  function startSourceResize(e: PointerEvent) {
    e.preventDefault();
    sourceCollapsed = false;
    const startX = e.clientX;
    const startW = sourcePanelW;
    const move = (ev: PointerEvent) => {
      sourcePanelW = clampPanel(startW + ev.clientX - startX, 260, 560);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function startInspectorResize(e: PointerEvent) {
    e.preventDefault();
    inspectorCollapsed = false;
    const startX = e.clientX;
    const startW = inspectorPanelW;
    const move = (ev: PointerEvent) => {
      inspectorPanelW = clampPanel(startW - (ev.clientX - startX), 240, 480);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function startTimelineResize(e: PointerEvent) {
    e.preventDefault();
    timelineCollapsed = false;
    const startY = e.clientY;
    const startH = timelinePanelH;
    const move = (ev: PointerEvent) => {
      timelinePanelH = clampPanel(startH - (ev.clientY - startY), 120, 460);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  // Traceable derivative names: the thumbnail badges (in the library grid) are
  // parsed straight from these suffixes, so a file's history reads off its name.
  //   DJI_0674_IG_reel   — Instagram export
  //   DJI_0674_trim_crop — lossless trim + crop
  //   A+B_mix            — composite of two sources
  function exportName() {
    const seq = programClips;
    const first = seq[0]?.name ?? "clip";
    const stem = first.replace(/\.[^.]+$/, "");
    const distinct = new Set(seq.map((c) => c.path));
    if (distinct.size > 1) {
      const other = seq.find((c) => c.path !== seq[0].path)?.name ?? "";
      const otherStem = other.replace(/\.[^.]+$/, "");
      return `${stem}+${otherStem || distinct.size - 1}_mix`;
    }
    const c = seq[0];
    const dur = clips.find((x) => x.path === c?.path)?.duration ?? 0;
    const trimmed = !!c && (c.in_s > 0.05 || c.out_s < dur - 0.05);
    const cropped = outPreset.fit !== "original";
    const parts = [stem];
    if (exportTarget.startsWith("instagram")) {
      parts.push("IG");
      if (preset === "reels") parts.push("reel");
      else if (preset === "square") parts.push("sq");
      else if (preset === "landscape") parts.push("wide");
    } else if (exportTarget === "whatsapp") {
      parts.push("mobile");
    } else {
      if (trimmed) parts.push("trim");
      if (cropped) parts.push("crop");
      if (parts.length === 1) parts.push("edit");
    }
    return parts.join("_");
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
        { label: "Split at playhead", icon: "✂", action: () => cutAtPlayhead() },
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

  // Aspect is now chosen in the edit screen (the preset row) — the dialog only
  // picks the delivery mode. Every entry opens the SAME dialog.
  function applyDlgMode(mode: DlgMode) {
    dlgMode = mode;
  }

  function openExport(mode: DlgMode) {
    exportMenuOpen = false;
    applyDlgMode(mode);
    exportDlg = true;
  }

  // Map the edit-screen aspect preset to the Instagram export target (drives the
  // filename suffix + quality).
  function instagramTargetForPreset(): ExportTarget {
    if (preset === "square") return "instagram_square";
    if (preset === "landscape") return "instagram_landscape";
    return "instagram_reels";
  }

  async function runDialogExport() {
    exportDlg = false;
    if (dlgMode === "instagram") {
      exportTarget = instagramTargetForPreset();
      quality = "high";
      await exportTimeline(true, keepHdr);
    } else if (dlgMode === "lossless") {
      // Keep the current aspect/crop; just max quality (stream-copies with no
      // crop/adjust, re-encodes at best quality when a crop is set).
      exportTarget = "archive";
      quality = "best";
      await exportTimeline(false, false);
    } else {
      await exportTimeline(false, false);
    }
  }

  async function exportTimeline(normalize = false, keep_hdr = false) {
    if (!programClips.length || exporting) return;
    exportMenuOpen = false;
    exporting = true;
    exportNote = "Exporting";
    try {
      const music = audioClips[0]?.path ?? null;
      const req: EditExportRequest = {
        clips: programClips.map((clip) => ({
          path: clip.path,
          in_s: clip.in_s,
          out_s: clip.out_s,
          crop_x: clip.crop_x,
          crop_y: clip.crop_y,
          zoom: clip.zoom,
        })),
        output_w: outPreset.w,
        output_h: outPreset.h,
        fit: outPreset.fit,
        encoder,
        quality,
        adjustments,
        music_path: music,
        preserve_source_audio: preserveSourceAudio && !music,
        destination: null,
        basename: exportName(),
        normalize,
        keep_hdr,
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
    // Snapshot the frame under the playhead — the visible program clip mapped to
    // its source time (falls back to the selected clip's in-point).
    const seg = productionPreview ? null : segAt(playheadS);
    const clip = seg?.clip ?? selectedClip;
    if (!clip || snapshotting) return;
    const timeS = productionPreview ? currentTime : Math.max(clip.inS, Math.min(clip.inS + (playheadS - clip.start), clip.outS));
    snapshotting = true;
    exportNote = "Saving frame";
    try {
      const req: EditSnapshotRequest = {
        path: clip.path,
        time_s: timeS,
        output_w: outPreset.w,
        output_h: outPreset.h,
        fit: outPreset.fit,
        crop_x: clip.cropX,
        crop_y: clip.cropY,
        zoom: clip.zoom,
        adjustments,
        basename: `${clip.name.replace(/\.[^.]+$/, "")}_frame`,
      };
      const out = await api.editSnapshot(req);
      const saved = basename(out);
      exportNote = `Saved frame ${saved}`;
      frameToast = `Frame saved: ${saved}`;
      setTimeout(() => {
        if (frameToast === `Frame saved: ${saved}`) frameToast = null;
      }, 2600);
      api.reveal(out);
    } catch (e) {
      exportNote = `Frame failed: ${e}`;
    } finally {
      snapshotting = false;
    }
  }

  function selectClip(id: string) {
    selectedId = id;
    selectedIds = new Set([id]);
    selectedAudioId = null;
  }

  // Plain click = single-select (+ jump the playhead into the clip if it's
  // outside, so "click a clip → see it" still holds). Ctrl/Cmd = toggle in/out
  // of the multi-selection.
  function onClipClick(e: MouseEvent, clip: TimelineClip) {
    e.stopPropagation();
    if (e.ctrlKey || e.metaKey) {
      const next = new Set(selectedIds);
      if (next.has(clip.id)) next.delete(clip.id);
      else next.add(clip.id);
      selectedIds = next;
      selectedId = next.has(clip.id) ? clip.id : next.values().next().value ?? null;
      selectedAudioId = null;
      return;
    }
    selectClip(clip.id);
    const len = clipLen(clip);
    if (playheadS < clip.start - 1e-3 || playheadS > clip.start + len + 1e-3) seekTimeline(clip.start);
  }

  function selectAudio(id: string) {
    selectedAudioId = id;
    selectedId = null;
    selectedIds = new Set();
  }

  function snapTime(t: number, exclude?: string | Set<string>) {
    const skip = (id: string) => (typeof exclude === "string" ? exclude === id : !!exclude?.has(id));
    let best = Math.max(0, t);
    let bestDist = SNAP;
    const edges = [0, playheadS];
    for (const c of clips) {
      if (skip(c.id)) continue;
      edges.push(c.start, c.start + c.outS - c.inS);
    }
    for (const a of audioClips) {
      if (skip(a.id)) continue;
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
      // A move-drag on a clip that isn't in the current selection resets to a
      // single selection; otherwise the whole selection moves together.
      if (mode === "move" && !selectedIds.has(id)) selectClip(id);
      else if (mode !== "move") selectClip(id);
      const group =
        mode === "move"
          ? clips.filter((c) => selectedIds.has(c.id)).map((c) => ({ id: c.id, start: c.start, lane: c.lane }))
          : [{ id: clip.id, start: clip.start, lane: clip.lane }];
      timelineDrag = {
        id,
        kind,
        mode,
        startX: e.clientX,
        startY: e.clientY,
        start: clip.start,
        lane: clip.lane,
        inS: clip.inS,
        outS: clip.outS,
        duration: clip.duration,
        group,
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
        startY: e.clientY,
        start: clip.start,
        lane: clip.lane,
        inS: 0,
        outS: clip.duration,
        duration: clip.duration,
        group: [],
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
      // Snap the primary clip, then shift the whole selection by that delta.
      // Vertical motion re-lanes the selection (V1–V3), clamped per clip.
      const groupIds = new Set(timelineDrag.group.map((g) => g.id));
      const snappedStart = snapTime(timelineDrag.start + d, groupIds);
      const delta = snappedStart - timelineDrag.start;
      const deltaLanes = Math.round((e.clientY - timelineDrag.startY) / TRACK_HEIGHT);
      clips = clips.map((c) => {
        const g = timelineDrag!.group.find((x) => x.id === c.id);
        if (!g) return c;
        return { ...c, start: Math.max(0, g.start + delta), lane: Math.max(0, Math.min(2, g.lane + deltaLanes)) };
      });
    } else if (timelineDrag.mode === "trimIn") {
      const snappedStart = snapTime(timelineDrag.start + d, clip.id);
      const nextIn = Math.max(0, Math.min(timelineDrag.inS + (snappedStart - timelineDrag.start), timelineDrag.outS - 0.05));
      updateClip(clip.id, { start: timelineDrag.start + (nextIn - timelineDrag.inS), inS: nextIn });
      trimPreview = { id: clip.id, time: nextIn };
    } else {
      const right = snapTime(timelineDrag.start + (timelineDrag.outS - timelineDrag.inS) + d, clip.id);
      const nextOut = Math.max(clip.inS + 0.05, Math.min(timelineDrag.duration, clip.inS + Math.max(0.05, right - clip.start)));
      updateClip(clip.id, { outS: nextOut });
      trimPreview = { id: clip.id, time: nextOut };
    }
  }

  function endTimelineDrag() {
    const drag = timelineDrag;
    timelineDrag = null;
    trimPreview = null;
    window.removeEventListener("pointermove", onTimelineDrag);
    // On release, shove moved clips clear of anything they landed on.
    if (drag && drag.kind === "video" && drag.mode === "move") resolveOverlaps(drag.group.map((g) => g.id));
  }

  // Scrub the playhead by dragging the ruler.
  function startRulerScrub(e: PointerEvent) {
    e.preventDefault();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const toTime = (clientX: number) => Math.max(0, Math.min(videoEnd, (clientX - rect.left) / timelineScale));
    seekTimeline(toTime(e.clientX));
    const move = (ev: PointerEvent) => seekTimeline(toTime(ev.clientX));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  // Clicking empty track background seeks to that time.
  function onTrackClick(e: MouseEvent) {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    seekTimeline(Math.max(0, Math.min(videoEnd, (e.clientX - rect.left) / timelineScale)));
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

  let sourceCrop = $derived.by(() => {
    if (!selectedClip) return null;
    const vw = Math.max(1, videoW);
    const vh = Math.max(1, videoH);
    const aspect = outPreset.fit === "original" ? vw / vh : outAspect;
    let cropW = vw;
    let cropH = cropW / aspect;
    if (cropH > vh) {
      cropH = vh;
      cropW = cropH * aspect;
    }
    if (outPreset.fit !== "original") {
      cropW = Math.min(vw, cropW / selectedClip.zoom);
      cropH = Math.min(vh, cropH / selectedClip.zoom);
    }
    const rangeX = Math.max(0, vw - cropW);
    const rangeY = Math.max(0, vh - cropH);
    return {
      left: rangeX * selectedClip.cropX,
      top: rangeY * selectedClip.cropY,
      w: cropW,
      h: cropH,
      videoW: vw,
      videoH: vh,
    };
  });

  let productionFrame = $derived.by(() => {
    if (!selectedClip || !sourceCrop) return null;
    const boxW = Math.max(1, previewW);
    const boxH = Math.max(1, previewH);
    const aspect = sourceCrop.w / sourceCrop.h;
    let w = boxW;
    let h = w / aspect;
    if (h > boxH) {
      h = boxH;
      w = h * aspect;
    }
    return {
      left: (boxW - w) / 2,
      top: (boxH - h) / 2,
      w,
      h,
    };
  });

  let productionVideoRect = $derived.by(() => {
    if (!sourceCrop || !productionFrame) return null;
    const scale = productionFrame.w / sourceCrop.w;
    return {
      left: -sourceCrop.left * scale,
      top: -sourceCrop.top * scale,
      w: sourceCrop.videoW * scale,
      h: sourceCrop.videoH * scale,
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

  async function onTimelineWheel(e: WheelEvent) {
    if (!e.ctrlKey) return;
    const viewport = e.currentTarget as HTMLDivElement;
    const wheelDelta = Math.abs(e.deltaY) >= Math.abs(e.deltaX) ? e.deltaY : e.deltaX;
    if (wheelDelta === 0) return;

    e.preventDefault();
    const rect = viewport.getBoundingClientRect();
    const cursorX = e.clientX - rect.left;
    const oldScale = timelineScale;
    const nextScale = clampTimelineScale(timelineScale + (wheelDelta > 0 ? -2 : 2));
    if (nextScale === oldScale) return;

    const anchorTime = Math.max(0, (viewport.scrollLeft + cursorX - TIMELINE_TRACK_OFFSET) / oldScale);
    timelineScale = nextScale;
    await tick();
    viewport.scrollLeft = Math.max(0, anchorTime * nextScale + TIMELINE_TRACK_OFFSET - cursorX);
  }
</script>

<div
  class="editShell"
  class:sourceCollapsed
  class:inspectorCollapsed
  class:timelineCollapsed
  class:productionPreviewMode={productionPreview}
  style={`--source-w:${sourceCollapsed ? 0 : sourcePanelW}px; --source-splitter-w:${sourceCollapsed ? 0 : 6}px; --inspector-w:${inspectorCollapsed ? 0 : inspectorPanelW}px; --inspector-splitter-w:${inspectorCollapsed ? 0 : 6}px; --timeline-h:${timelineCollapsed ? 0 : timelinePanelH}px;`}
>
  <aside class="sourcePane">
    <div class="sourceHead">
      <div>
        <strong>Source</strong>
        <span>{sources.filter((s) => s.kind === "video").length} video · {sources.filter((s) => s.kind === "audio").length} audio</span>
      </div>
      <div class="sourceTools">
        <button class="miniIcon" onclick={() => (sourceCollapsed = true)} title="Collapse source" aria-label="Collapse source">|&lt;</button>
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
              <span class="sourceTitle">
                <strong>{item.name}</strong>
                <span>{sourceDuration(item)}</span>
              </span>
              <em>{sourceSubline(item)}</em>
              <span class="sourceChips">
                {#each sourceMetaChips(item).slice(0, sourceView === "thumbs" ? 2 : 4) as chip}
                  <span>{chip}</span>
                {/each}
              </span>
              {#if sourceStateChips(item).length}
                <span class="sourceChips stateChips">
                  {#each sourceStateChips(item).slice(0, sourceView === "thumbs" ? 2 : 4) as chip}
                    <span>{chip}</span>
                  {/each}
                </span>
              {/if}
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

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="panelSplitter sourceSplitter" onpointerdown={startSourceResize} role="separator" title="Resize source"></div>

  <section class="workPane">
    <div class="editTop">
      <div class="presetGroup">
        {#each Object.entries(PRESETS) as [id, p]}
          <button class:on={preset === id} onclick={() => setPreset(id as PresetId)}>
            <strong>{p.label}</strong>
            <span>{p.detail}</span>
          </button>
        {/each}
      </div>
      <span class="status" class:warn={needsRender}>{needsRender ? "Render required" : "Stream copy ready"}</span>
      <div class="layoutTools">
        <button class="miniBtn" class:on={!sourceCollapsed} onclick={() => (sourceCollapsed = !sourceCollapsed)}>Source</button>
        <button class="miniBtn" class:on={!timelineCollapsed} onclick={() => (timelineCollapsed = !timelineCollapsed)}>Timeline</button>
        <button class="miniBtn" class:on={!inspectorCollapsed} onclick={() => (inspectorCollapsed = !inspectorCollapsed)}>Look</button>
      </div>
      <span class="spacer"></span>
      <button class="miniBtn" class:on={productionPreview} onclick={toggleProductionPreview} disabled={!selectedClip}>
        Preview
      </button>
      <button class="miniBtn" onclick={takeSnapshot} disabled={!clips.length || snapshotting}>
        {snapshotting ? "Saving" : "Frame"}
      </button>
      {#if frameToast}<span class="topToast" aria-live="polite">{frameToast}</span>{/if}
      <div class="exportOpts">
        <div class="exportGroup">
          <button class="exportBtn main" onclick={() => openExport("custom")} disabled={!clips.length || exporting} title="Open the export dialog (all settings)">
            {exporting ? "Exporting…" : "Export"}
          </button>
          <button
            class="exportBtn caret"
            class:on={exportMenuOpen}
            onclick={() => (exportMenuOpen = !exportMenuOpen)}
            disabled={!clips.length || exporting}
            aria-label="Export options"
            title="Quick export"
          >▾</button>
        </div>
        {#if exportMenuOpen}
          <div class="exportMenu choices">
            <button class="exportChoice" onclick={() => openExport("instagram")} disabled={!clips.length}>
              <strong>Export to Instagram</strong>
              <span>{outPreset.fit === "original" ? "Original aspect — uses your edit-screen aspect" : `${outPreset.label} · ${outPreset.detail} — uses your edit-screen aspect`}</span>
            </button>
            <button class="exportChoice" onclick={() => openExport("lossless")} disabled={!clips.length}>
              <strong>Export lossless</strong>
              <span>Original quality · keeps your aspect/crop</span>
            </button>
            <div class="menuSep"></div>
            <button class="exportChoice sub" onclick={() => openExport("custom")}>All export settings…</button>
          </div>
        {/if}
      </div>
    </div>

    <div class="preview" bind:this={previewBox} onwheel={onCropWheel}>
      {#if inspectorCollapsed}
        <button class="restoreTab restoreLook" onclick={() => (inspectorCollapsed = false)} title="Show Look panel">Look</button>
      {/if}
      {#if timelineCollapsed}
        <button class="restoreTab restoreTimeline" onclick={() => (timelineCollapsed = false)} title="Show timeline">Timeline</button>
      {/if}
      {#if clips.length}
        {#if productionPreview && selectedClip && productionFrame && productionVideoRect}
          <div
            class="productionFrame"
            style="left:{productionFrame.left}px; top:{productionFrame.top}px; width:{productionFrame.w}px; height:{productionFrame.h}px"
          >
            <!-- svelte-ignore a11y_media_has_caption -->
            <video
              bind:this={previewVideo}
              src={selectedClip.src}
              preload="auto"
              playsinline
              class="productionVideo"
              style="left:{productionVideoRect.left}px; top:{productionVideoRect.top}px; width:{productionVideoRect.w}px; height:{productionVideoRect.h}px; filter:{previewFilter}"
              onloadedmetadata={onMeta}
              ontimeupdate={onProdTime}
              onerror={onPreviewError}
              onclick={togglePlay}
            ></video>
            {#if warmthTint}
              <div class="warmthTint prod" style="background:{warmthTint.color}; opacity:{warmthTint.opacity}"></div>
            {/if}
          </div>
          <div class="productionControls">
            <button class="play" onclick={togglePlay}>{previewVideo?.paused === false ? "Pause" : "Play"}</button>
            <span class="time">{fmt(currentTime)} / {fmt(selectedClip.duration)}</span>
            <input
              type="range"
              min={selectedClip.inS}
              max={selectedClip.outS}
              step="0.01"
              value={currentTime}
              oninput={(e) => seekProduction(Number((e.currentTarget as HTMLInputElement).value))}
            />
            <button class="miniBtn" onclick={() => setOutputPreview(false)}>Exit</button>
          </div>
        {:else}
          <!-- svelte-ignore a11y_media_has_caption -->
          <video
            bind:this={previewVideo}
            preload="auto"
            playsinline
            style:filter={previewFilter}
            onloadedmetadata={onMeta}
            ontimeupdate={onNormalTime}
            onerror={onPreviewError}
            onclick={togglePlay}
          ></video>
          {#if warmthTint}
            <div
              class="warmthTint"
              style="left:{imageRect.left}px; top:{imageRect.top}px; width:{imageRect.w}px; height:{imageRect.h}px; background:{warmthTint.color}; opacity:{warmthTint.opacity}"
            ></div>
          {/if}
        {/if}
        {#if previewPreparing}
          <div class="previewBusy">Preparing preview</div>
        {/if}
        {#if trimPreview}
          <div class="trimCaption">Trimming · {fmt(trimPreview.time)}</div>
        {/if}
        {#if cropRect && cropVisible}
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
      <button class="play" onclick={togglePlay} disabled={!clips.length}>{playing ? "Pause" : "Play"}</button>
      <span class="time">{fmt(playheadS)} / {fmt(videoEnd)}</span>
      <input
        type="range"
        min="0"
        max={videoEnd || 1}
        step="0.01"
        value={playheadS}
        disabled={!clips.length}
        oninput={(e) => seekTimeline(Number((e.currentTarget as HTMLInputElement).value))}
      />
    </div>

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="timelineResize" onpointerdown={startTimelineResize} role="separator" title="Resize timeline"></div>

    <section class="timeline" aria-label="Edit timeline">
      <div class="timelineHead">
        <strong>Timeline</strong>
        <span>{clips.length} video · {audioClips.length} audio · {fmt(programSeconds)}</span>
        <label class="scale">Zoom <input type="range" min={TIMELINE_ZOOM_MIN} max={TIMELINE_ZOOM_MAX} bind:value={timelineScale} /></label>
        <span class="snap">Snap</span>
        <span class="spacer"></span>
        <button class="ghost" onclick={cutAtPlayhead} disabled={!clips.length} title="Split at playhead (C)">✂ Cut</button>
        <button class="ghost" onclick={() => (timelineCollapsed = true)}>Collapse</button>
        <button class="ghost" onclick={() => { clips = []; audioClips = []; }} disabled={!clips.length && !audioClips.length}>Clear</button>
      </div>
      <div class="timelineViewport" onwheel={onTimelineWheel}>
        <div class="timelineCanvas" style="width:{timelineWidth}px">
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="ruler" onpointerdown={startRulerScrub} role="slider" tabindex="-1" aria-label="Scrub playhead" aria-valuenow={Math.round(playheadS)}>
            {#each Array(Math.ceil(timelineEnd / 5) + 1) as _, i}
              <span style="left:{i * 5 * timelineScale}px">{fmt(i * 5)}</span>
            {/each}
          </div>

          {#each VIDEO_LANES as lane}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="track videoTrack" onpointerdown={onTrackClick} ondragover={allowDrop} ondrop={(e) => dropOnLane(e, "video", lane)}>
              <span class="trackLabel">V{lane + 1}</span>
              {#each clips.filter((c) => c.lane === lane) as clip (clip.id)}
                <button
                  class="timelineClip video"
                  class:on={selectedIds.has(clip.id)}
                  style="left:{clip.start * timelineScale}px; width:{Math.max(42, clipLen(clip) * timelineScale)}px"
                  onclick={(e) => onClipClick(e, clip)}
                  oncontextmenu={(e) => openTimelineMenu(e, clip)}
                  onpointerdown={(e) => startTimelinePointer(e, "video", clip.id, "move")}
                  title={clip.path}
                >
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span class="handle left" onpointerdown={(e) => startTimelinePointer(e, "video", clip.id, "trimIn")}></span>
                  <strong>{clip.name}</strong>
                  <em>{fmt(clipLen(clip))}</em>
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span class="handle right" onpointerdown={(e) => startTimelinePointer(e, "video", clip.id, "trimOut")}></span>
                </button>
              {/each}
            </div>
          {/each}

          {#each AUDIO_LANES as lane}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="track audioTrack" class:firstAudio={lane === 0} onpointerdown={onTrackClick} ondragover={allowDrop} ondrop={(e) => dropOnLane(e, "audio", lane)}>
              <span class="trackLabel">A{lane + 1}</span>
              <!-- Source-audio mirrors: slim, non-interactive bars echoing every video clip
                   on Vn. Derived from `clips`, not stored — they play/export with the clip. -->
              {#each clips.filter((c) => c.lane === lane) as v (v.id)}
                <div
                  class="sourceAudioBar"
                  style="left:{v.start * timelineScale}px; width:{Math.max(42, clipLen(v) * timelineScale)}px"
                  title="Source audio — plays and exports with the clip"
                ></div>
              {/each}
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

          {#if clips.length}
            <div class="playhead" style="left:{TIMELINE_TRACK_OFFSET + playheadS * timelineScale}px"></div>
          {/if}
        </div>
      </div>
    </section>
  </section>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="panelSplitter inspectorSplitter" onpointerdown={startInspectorResize} role="separator" title="Resize look panel"></div>

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

    <div class="block lookBlock">
      <button class="blockHead" onclick={() => (inspectorCollapsed = true)} title="Collapse look panel">
        <h3>Look</h3>
        <span>Hide</span>
      </button>
      <p class="groupLabel">Presets</p>
      <div class="lookPresets">
        {#each Object.entries(LOOK_PRESETS) as [id, look]}
          <button class="lookPreset" onclick={() => applyLook(id as LookPresetId)} title={look.hint}>
            <span class="swatch" style="filter:{lookFilter(look.values)}"></span>
            <span class="lpText">
              <strong>{look.label}</strong>
              <span>{look.hint}</span>
            </span>
          </button>
        {/each}
      </div>
      <div class="groupDivider">
        <span class="groupLabel">Adjust</span>
        <button class="miniBtn ghost" onclick={resetColor} title="Reset all adjustments">Reset all</button>
      </div>
      <p class="adjHint">Double-click a slider to reset just that control.</p>
      <label><span class="adjLabel">Brightness <em class="adjVal">{adjReadout("brightness")}</em></span><input type="range" min="-0.4" max="0.4" step="0.005" bind:value={adjustments.brightness} ondblclick={() => resetAdj("brightness")} /></label>
      <label><span class="adjLabel">Contrast <em class="adjVal">{adjReadout("contrast")}</em></span><input type="range" min="0.5" max="1.8" step="0.01" bind:value={adjustments.contrast} ondblclick={() => resetAdj("contrast")} /></label>
      <label><span class="adjLabel">Saturation <em class="adjVal">{adjReadout("saturation")}</em></span><input type="range" min="0" max="2" step="0.01" bind:value={adjustments.saturation} ondblclick={() => resetAdj("saturation")} /></label>
      <label><span class="adjLabel">Warmth <em class="adjVal">{adjReadout("warmth")}</em></span><input type="range" min="-0.3" max="0.3" step="0.005" bind:value={adjustments.warmth} ondblclick={() => resetAdj("warmth")} /></label>
      <label><span class="adjLabel">Sharpen <em class="adjVal">{adjReadout("sharpen")}</em></span><input type="range" min="0" max="1" step="0.01" bind:value={adjustments.sharpen} ondblclick={() => resetAdj("sharpen")} /></label>
    </div>

    {#if selectedClip}
      <div class="block clipInfo">
        <p class="groupLabel">Source → Output</p>
        <dl>
          <div><dt>Resolution</dt><dd>{probes[selectedClip.path]?.width ?? "–"}×{probes[selectedClip.path]?.height ?? "–"}</dd></div>
          <div><dt>Frame rate</dt><dd>{probes[selectedClip.path]?.fps ? `${Math.round(probes[selectedClip.path]?.fps ?? 0)} fps` : "–"}</dd></div>
          <div><dt>Codec</dt><dd>{probes[selectedClip.path]?.codec ?? "–"}{probes[selectedClip.path]?.hdr ? " · HDR" : ""}</dd></div>
          <div><dt>Trim</dt><dd>{fmt(selectedClip.outS - selectedClip.inS)} of {fmt(selectedClip.duration)}</dd></div>
          <div><dt>Output</dt><dd>{outPreset.fit === "original" ? "Original" : `${outPreset.w}×${outPreset.h}`}</dd></div>
        </dl>
      </div>
    {/if}

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
      <label class="check"><input type="checkbox" bind:checked={preserveSourceAudio} disabled={audioClips.length > 0} /> Keep source audio</label>
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

  {#if exportDlg}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="igBackdrop" onclick={() => (exportDlg = false)}>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="igDialog" onclick={(e) => e.stopPropagation()}>
        <h2>Export</h2>
        <div class="dlgModes">
          <button class:on={dlgMode === "instagram"} onclick={() => applyDlgMode("instagram")}>Instagram</button>
          <button class:on={dlgMode === "lossless"} onclick={() => applyDlgMode("lossless")}>Lossless</button>
          <button class:on={dlgMode === "custom"} onclick={() => applyDlgMode("custom")}>Custom</button>
        </div>

        {#if dlgMode === "instagram"}
          <p class="igAspectLine">
            {#if outPreset.fit === "original"}Aspect: Original aspect{:else}Aspect: from your edit — <strong>{outPreset.label} · {outPreset.detail}</strong>{/if}
          </p>
          <p class="igDisclaim">FoxCull auto-applies the best Instagram settings for what it detected in your clip. It's good to go by default — tweak below only if you want to.</p>
          <div class="igCompare">
            <div class="igCol">
              <span class="igColHead">Your clip</span>
              <ul>
                <li>{igSourceProbe?.width ?? "?"}×{igSourceProbe?.height ?? "?"}</li>
                {#if igCrop && outPreset.fit !== "original"}
                  <li><em>crop ≈ {Math.round(igCrop.cropW)}×{Math.round(igCrop.cropH)} px</em></li>
                {/if}
                <li>{igSourceProbe?.fps ? `${Math.round(igSourceProbe?.fps ?? 0)} fps` : "source fps"}</li>
                <li>{igSourceProbe?.codec ?? "source codec"}</li>
                <li class:warn={igSourceProbe?.hdr}>{igSourceProbe?.hdr ? "HDR (HLG/PQ)" : "SDR"}</li>
              </ul>
            </div>
            <div class="igArrow">→</div>
            <div class="igCol optimised">
              <span class="igColHead">Optimised</span>
              <ul>
                <li>{outPreset.w}×{outPreset.h}{#if igCrop && igCrop.cropW > outPreset.w + 1}<em> (downscaled)</em>{:else if igCrop && igCrop.cropW < outPreset.w - 1}<em> (upscaled — slightly soft)</em>{/if}</li>
                <li>30 fps{#if (igSourceProbe?.fps ?? 0) > 31}<em> (from {Math.round(igSourceProbe?.fps ?? 0)})</em>{/if}</li>
                <li>{keepHdr && igSourceProbe?.hdr ? "HEVC 10-bit · MP4" : "H.264 · MP4 · faststart"}</li>
                <li>{igSourceProbe?.hdr ? (keepHdr ? "HDR (HLG kept)" : "SDR Rec.709 (tone-mapped)") : "SDR Rec.709"}</li>
              </ul>
            </div>
          </div>
          {#if igSourceProbe?.hdr}
            <div class="dlgHdr">
              <span class="igColHead">HDR handling</span>
              <label><input type="radio" name="hdr" checked={!keepHdr} onchange={() => (keepHdr = false)} /> Convert to SDR <em>— recommended, looks consistent on every device</em></label>
              <label><input type="radio" name="hdr" checked={keepHdr} onchange={() => (keepHdr = true)} /> Keep HDR (HLG) <em>— punchier on modern phones; the SDR fallback others see may look flat</em></label>
            </div>
          {/if}
          {#if softCrop}
            <p class="dlgWarn">⚠ Cropping this {igSourceProbe?.width}×{igSourceProbe?.height} clip to vertical upscales past its pixels, so it'll be slightly soft. FoxCull adds a light sharpen; for crisp crops shoot higher-res (2.7K/4K) when you plan to crop.</p>
          {/if}
        {:else if dlgMode === "lossless"}
          <p class="igDisclaim">
            Keeps your current aspect{outPreset.fit === "original" ? "" : " and crop"} at original quality.
            {#if outPreset.fit === "original" && neutralLook && !mixedSources}Trim only — stream-copied, no re-encode, no quality loss.{:else if mixedSources}Your clips differ in resolution or codec, so they're joined with a best-quality re-encode (a straight stream copy would produce a broken file).{:else}A crop/adjustment needs a render, so this re-encodes at best quality.{/if}
          </p>
        {:else}
          <p class="igAspectLine">
            {#if outPreset.fit === "original"}Aspect: Original aspect{:else}Aspect: from your edit — <strong>{outPreset.label} · {outPreset.detail}</strong>{/if}
          </p>
          <label class="dlgField">Encoder
            <select bind:value={encoder}>
              <option value="auto">Auto</option>
              <option value="x264">x264</option>
              <option value="nvenc">NVIDIA</option>
            </select>
          </label>
          <label class="dlgField">Quality
            <select bind:value={quality}>
              <option value="best">Best</option>
              <option value="high">High</option>
              <option value="standard">Standard</option>
              <option value="small">Small</option>
            </select>
          </label>
          <label class="check"><input type="checkbox" bind:checked={preserveSourceAudio} disabled={audioClips.length > 0} /> Keep source audio</label>
          <button class="miniBtn" onclick={() => void pickAudio()}>Choose audio…</button>
        {/if}

        <div class="dlgTime">
          <div class="dlgTimeHead">
            <strong>Estimated time ~{fmt(igEstimateSecs)}</strong>
            <span class="dlgInfo" title="Rule-of-thumb breakdown of what drives the time">ⓘ what drives this</span>
          </div>
          <ul class="dlgSteps">
            {#each exportSteps as s}
              <li><span>{s.label}</span><em>{s.note}</em></li>
            {/each}
          </ul>
        </div>
        <p class="dlgLoc">Saves next to the source as <strong>{exportName()}.mp4</strong></p>

        <div class="igActions">
          <button class="miniBtn" onclick={() => (exportDlg = false)}>Cancel</button>
          <button class="exportBtn" onclick={runDialogExport} disabled={exporting || !programClips.length}>{exporting ? "Exporting…" : "Export"}</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .editShell {
    width: 100%;
    height: 100%;
    display: grid;
    grid-template-columns:
      var(--source-w, 360px)
      var(--source-splitter-w, 6px)
      minmax(0, 1fr)
      var(--inspector-splitter-w, 6px)
      var(--inspector-w, 320px);
    background: var(--bg);
    color: var(--text);
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .productionPreviewMode,
  .productionPreviewMode.sourceCollapsed,
  .productionPreviewMode.inspectorCollapsed,
  .productionPreviewMode.sourceCollapsed.inspectorCollapsed {
    grid-template-columns: 0 0 minmax(0, 1fr) 0 0;
    background: #000;
  }
  .sourcePane,
  .inspector {
    grid-row: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg-panel);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
  }
  .sourcePane {
    grid-column: 1;
  }
  .inspector {
    grid-column: 5;
    border-right: 0;
    border-left: 1px solid var(--border);
    overflow-y: auto;
    position: relative;
    z-index: 2;
  }
  .sourceCollapsed .sourcePane,
  .inspectorCollapsed .inspector {
    border: 0;
    overflow: hidden;
  }
  .sourceCollapsed .sourcePane > *,
  .inspectorCollapsed .inspector > * {
    display: none;
  }
  .panelSplitter {
    grid-row: 1;
    min-width: 6px;
    cursor: col-resize;
    background: color-mix(in srgb, var(--border) 35%, transparent);
    transition: background 0.12s ease;
  }
  .sourceSplitter {
    grid-column: 2;
  }
  .inspectorSplitter {
    grid-column: 4;
  }
  .sourceCollapsed .sourceSplitter,
  .inspectorCollapsed .inspectorSplitter {
    display: none;
  }
  .panelSplitter:hover,
  .panelSplitter:active {
    background: color-mix(in srgb, var(--accent) 58%, var(--border));
  }
  .productionPreviewMode .sourcePane,
  .productionPreviewMode .panelSplitter,
  .productionPreviewMode .inspector {
    display: none;
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
  .miniIcon {
    width: 28px;
    height: 24px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-elev);
    color: var(--text-dim);
    font-size: 11px;
    line-height: 1;
  }
  .miniIcon:hover {
    background: var(--bg-hover);
    color: var(--text);
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
    grid-template-columns: 72px minmax(0, 1fr);
    align-items: start;
    gap: 10px;
    width: 100%;
    min-height: 76px;
    padding: 8px;
    border-radius: 8px;
    border: 1px solid transparent;
    background: color-mix(in srgb, var(--bg-elev) 72%, transparent);
    color: var(--text);
    cursor: grab;
    text-align: left;
  }
  .sourceList.list .sourceItem {
    grid-template-columns: 58px minmax(0, 1fr);
  }
  .sourceList.thumbs .sourceItem {
    grid-template-columns: 1fr;
    grid-template-rows: 102px auto;
    min-height: 170px;
    align-items: stretch;
  }
  .sourceItem > span:nth-child(n + 3),
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
    width: 68px;
    height: 54px;
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
  .sourceTitle {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: baseline;
    gap: 8px;
  }
  .sourceTitle > span {
    color: var(--text-dim);
    font-size: 11.5px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
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
  .sourceChips {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
    max-height: 36px;
    overflow: hidden;
  }
  .sourceChips span {
    max-width: 112px;
    padding: 2px 5px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--viewport-bg) 76%, transparent);
    color: var(--text-faint);
    font-size: 10.5px;
    line-height: 1.1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stateChips span {
    background: color-mix(in srgb, var(--accent) 14%, var(--viewport-bg));
    color: color-mix(in srgb, var(--text) 86%, var(--accent));
  }
  .workPane {
    grid-column: 3;
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: auto minmax(180px, 1fr) auto 6px var(--timeline-h, 260px);
    position: relative;
    overflow: visible;
    z-index: 1;
  }
  .productionPreviewMode .workPane {
    grid-column: 3;
    grid-template-rows: minmax(0, 1fr);
  }
  .productionPreviewMode .editTop,
  .productionPreviewMode .transport,
  .productionPreviewMode .timelineResize,
  .productionPreviewMode .timeline {
    display: none;
  }
  .editTop {
    position: relative;
    z-index: 180;
    overflow: visible;
  }
  .timelineCollapsed .workPane {
    grid-template-rows: auto minmax(180px, 1fr) auto 0 0;
  }
  .timelineCollapsed .timelineResize {
    display: none;
  }
  .layoutTools {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: 0 0 auto;
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
  /* Keep the top bar (and its export dropdowns) stacked above the inspector /
     preview so an open Export menu or the "Exporting…" state never slides behind
     the Look panel. */
  .editTop {
    position: relative;
    z-index: 6;
  }
  .exportOpts {
    position: relative;
    flex: 0 0 auto;
  }
  /* Source→Output facts card — fills the inspector space below the sliders. */
  .clipInfo dl {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .clipInfo dl > div {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    font-size: 12px;
  }
  .clipInfo dt {
    color: var(--text-faint);
  }
  .clipInfo dd {
    margin: 0;
    color: var(--text);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .miniBtn.on {
    border-color: var(--accent);
    color: var(--accent);
  }
  .exportMenu {
    position: absolute;
    right: 0;
    top: 34px;
    z-index: 240;
    width: 280px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 9px;
    box-shadow: var(--shadow);
  }
  .topToast {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 5px 8px;
    border: 1px solid color-mix(in srgb, var(--pick) 50%, var(--border));
    border-radius: 7px;
    background: color-mix(in srgb, var(--pick) 16%, var(--bg-elev));
    color: var(--text);
    font-size: 12px;
  }
  .spacer {
    flex: 1 1 auto;
    min-width: 8px;
  }
  .preview {
    position: relative;
    z-index: 0;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #050505;
    overflow: hidden;
  }
  .restoreTab {
    position: absolute;
    z-index: 75;
    border: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-elev) 92%, transparent);
    color: var(--text);
    box-shadow: var(--shadow);
    font-weight: 700;
    font-size: 12px;
  }
  .restoreLook {
    right: 10px;
    top: 12px;
    padding: 7px 10px;
    border-radius: 8px;
  }
  .restoreTimeline {
    left: 50%;
    bottom: 12px;
    transform: translateX(-50%);
    padding: 7px 12px;
    border-radius: 999px;
  }
  .preview video {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .productionPreviewMode .preview {
    grid-row: 1;
    background: #000;
  }
  .productionFrame {
    position: absolute;
    overflow: hidden;
    background: #000;
    box-shadow: 0 18px 70px rgba(0, 0, 0, 0.5);
  }
  .preview video.productionVideo {
    position: absolute;
    max-width: none;
    max-height: none;
    object-fit: fill;
  }
  .productionControls {
    position: absolute;
    left: 50%;
    bottom: 18px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 10px;
    width: min(760px, calc(100% - 48px));
    padding: 9px 10px;
    border: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
    border-radius: 9px;
    background: color-mix(in srgb, var(--bg-panel) 84%, transparent);
    box-shadow: var(--shadow);
  }
  .productionControls input {
    flex: 1;
    accent-color: var(--accent);
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
  /* Warmth can't be a CSS filter — a soft-light tint over the exact video area
     stands in for it in the preview. */
  .warmthTint {
    position: absolute;
    pointer-events: none;
    mix-blend-mode: soft-light;
  }
  .warmthTint.prod {
    inset: 0;
  }
  .trimCaption {
    position: absolute;
    left: 12px;
    top: 12px;
    padding: 5px 9px;
    border-radius: 7px;
    background: color-mix(in srgb, var(--bg-elev) 88%, transparent);
    border: 1px solid var(--border);
    color: var(--text);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
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
  .timelineResize {
    min-height: 6px;
    cursor: row-resize;
    background: color-mix(in srgb, var(--border) 35%, transparent);
    border-top: 1px solid var(--border);
    transition: background 0.12s ease;
  }
  .timelineResize:hover,
  .timelineResize:active {
    background: color-mix(in srgb, var(--accent) 58%, var(--border));
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
  .exportGroup {
    display: inline-flex;
    align-items: stretch;
  }
  .exportBtn {
    padding: 7px 12px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-on);
    font-weight: 700;
    white-space: nowrap;
  }
  /* Split button: main Export + a caret that opens the presets menu. */
  .exportBtn.main {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .exportBtn.caret {
    padding: 7px 8px;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
    border-left: 1px solid color-mix(in srgb, var(--accent-on) 28%, var(--accent));
    font-weight: 700;
  }
  .exportBtn.caret.on {
    background: color-mix(in srgb, var(--accent) 82%, #000);
  }
  .exportMenu.choices {
    width: 260px;
    gap: 4px;
    padding: 6px;
  }
  .exportChoice {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    text-align: left;
    padding: 7px 9px;
    border: 1px solid transparent;
    border-radius: 7px;
    background: transparent;
    color: var(--text);
  }
  .exportChoice:hover {
    background: var(--bg-hover);
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .exportChoice strong {
    font-size: 12.5px;
    font-weight: 700;
  }
  .exportChoice span {
    font-size: 11px;
    color: var(--text-faint);
  }
  .exportChoice.sub {
    color: var(--text-dim);
    font-size: 12px;
  }
  .menuSep {
    height: 1px;
    margin: 3px 2px;
    background: var(--border);
  }
  /* Export dialog */
  .igBackdrop {
    position: fixed;
    inset: 0;
    z-index: 400;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.55);
    padding: 20px;
  }
  .igDialog {
    width: min(520px, 100%);
    max-height: 90vh;
    overflow-y: auto;
    padding: 18px 20px;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--bg-panel);
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .igDialog h2 {
    margin: 0;
    font-size: 16px;
  }
  .igDisclaim {
    margin: 0;
    font-size: 12px;
    color: var(--text-dim);
    line-height: 1.45;
  }
  .igCompare {
    display: flex;
    align-items: stretch;
    gap: 10px;
  }
  .igCol {
    flex: 1;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg-elev);
  }
  .igCol.optimised {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
    background: color-mix(in srgb, var(--accent) 10%, var(--bg-elev));
  }
  .igColHead {
    display: block;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
    margin-bottom: 6px;
  }
  .igCol ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }
  .igCol li em {
    color: var(--text-faint);
    font-style: normal;
    font-size: 11px;
  }
  .igCol li.warn {
    color: var(--accent);
  }
  .igArrow {
    align-self: center;
    color: var(--text-faint);
    font-size: 18px;
  }
  /* Mode segmented control */
  .dlgModes {
    display: flex;
    gap: 4px;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--bg-elev);
  }
  .dlgModes button {
    flex: 1;
    padding: 6px 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    color: var(--text-dim);
    font-size: 12px;
    font-weight: 600;
  }
  .dlgModes button.on {
    background: var(--accent);
    color: var(--accent-on);
  }
  .igAspectLine {
    margin: 0;
    font-size: 12px;
    color: var(--text-dim);
  }
  .igAspectLine strong {
    color: var(--text);
    font-weight: 600;
  }
  .dlgHdr {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg-elev);
  }
  .dlgHdr label {
    display: flex;
    align-items: baseline;
    gap: 7px;
    font-size: 12.5px;
    color: var(--text);
  }
  .dlgHdr label em {
    color: var(--text-faint);
    font-style: normal;
    font-size: 11px;
  }
  .dlgWarn {
    margin: 0;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border));
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 12%, var(--bg-elev));
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--text);
  }
  .dlgField {
    display: grid;
    grid-template-columns: 88px 1fr;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
  }
  .dlgTime {
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg-elev);
  }
  .dlgTimeHead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .dlgTimeHead strong {
    font-size: 13px;
  }
  .dlgInfo {
    font-size: 10.5px;
    color: var(--text-faint);
  }
  .dlgSteps {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .dlgSteps li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 11.5px;
    color: var(--text-dim);
  }
  .dlgSteps li em {
    font-style: normal;
    color: var(--text-faint);
  }
  .dlgLoc {
    margin: 0;
    font-size: 11.5px;
    color: var(--text-dim);
  }
  .dlgLoc strong {
    color: var(--text);
    font-weight: 600;
  }
  .igActions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 2px;
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
  .timelineCollapsed .timeline {
    overflow: hidden;
    border-top: 0;
  }
  .timelineCollapsed .timeline > * {
    display: none;
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
    cursor: pointer;
    z-index: 3;
  }
  /* Playhead: a vertical accent line spanning ruler + all tracks. */
  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--accent);
    pointer-events: none;
    z-index: 5;
  }
  .playhead::before {
    content: "";
    position: absolute;
    top: 0;
    left: -4px;
    border: 5px solid transparent;
    border-top-color: var(--accent);
  }
  /* Slim, non-interactive mirror of a video clip's linked source audio. */
  .sourceAudioBar {
    position: absolute;
    top: 12px;
    bottom: 12px;
    border-radius: 4px;
    pointer-events: none;
    background: color-mix(in srgb, var(--pick) 22%, transparent);
    border: 1px solid color-mix(in srgb, var(--pick) 30%, transparent);
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
  .videoTrack {
    background: color-mix(in srgb, var(--accent) 6%, var(--viewport-bg));
  }
  .audioTrack {
    background: color-mix(in srgb, var(--pick) 7%, var(--viewport-bg));
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
  .blockHead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 0;
    color: var(--text);
    background: transparent;
    text-align: left;
  }
  .blockHead span {
    color: var(--text-faint);
    font-size: 11.5px;
  }
  .lookPresets {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(104px, 1fr));
    gap: 6px;
  }
  .lookPreset {
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-elev);
    color: var(--text);
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 0;
    overflow: hidden;
  }
  .lookPreset:hover {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
  }
  .lookPreset .swatch {
    height: 26px;
    width: 100%;
    background: linear-gradient(120deg, #2b6cb0 0%, #38a169 45%, #dd9b34 100%);
  }
  .lpText {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 6px 8px;
  }
  .lpText strong {
    font-size: 12px;
  }
  .lpText span {
    color: var(--text-faint);
    font-size: 10.5px;
  }
  /* Section labels + divider that separate the one-tap Presets from the manual
     Adjust sliders (Lightroom-style demarcation). */
  .groupLabel {
    margin: 0;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .groupDivider {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-top: 2px;
    padding-top: 9px;
    border-top: 1px solid var(--border);
  }
  .adjHint {
    margin: -4px 0 0;
    font-size: 10.5px;
    color: var(--text-faint);
  }
  .miniBtn.ghost {
    background: transparent;
    border-color: transparent;
    color: var(--text-faint);
  }
  .miniBtn.ghost:hover {
    color: var(--text);
    background: var(--bg-hover);
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
  .adjLabel {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  .adjVal {
    font-style: normal;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
    font-size: 11px;
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
      grid-template-columns:
        minmax(0, var(--source-w, 270px))
        var(--source-splitter-w, 6px)
        minmax(0, 1fr)
        var(--inspector-splitter-w, 6px)
        minmax(0, var(--inspector-w, 260px));
    }
    .presetGroup button {
      min-width: 64px;
    }
  }
</style>
