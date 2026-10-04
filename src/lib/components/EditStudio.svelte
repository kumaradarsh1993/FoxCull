<script module lang="ts">
  import type { EditAdjustments as Adj } from "$lib/types";
  type TrimMemory = { inS: number; outS: number };
  /** What a drop/paste/E brought, for the toast and the mismatch check. */
  export type AddResult = { clips: number; segments: number; photos: number; missing: number; other: number; paths: string[] };
  /** A timeline as saved between sessions (the window can be closed). */
  export type SavedClip = { path: string; name: string; inS: number; outS: number; duration: number; start: number; lane: number; cropX: number; cropY: number; zoom: number };
  export type SavedAudio = { path: string; name: string; start: number; duration: number; lane: number };
  export type TimelineState = { v: 1; clips: SavedClip[]; audio: SavedAudio[]; preset: string; adjustments: Adj; look: string | null; lookIntensity: number; keepSourceAudio: boolean };
  const sessionTrimMemory = new Map<string, TrimMemory>();
  // Which Look preset groups are expanded, remembered for the whole app session
  // (survives leaving and re-entering the edit studio). Missing key = default open.
  const sessionLookGroupOpen: Record<string, boolean> = {};
</script>

<script lang="ts">
  // The Edit window's studio: timeline, preview, Look and export. It has no
  // media picker of its own (2026-10-04, owner's spec): clips come from the
  // library window, by drag, by ⌘C/⌘V, or with E / "Add to Edit timeline",
  // and each in/out range marked in the library arrives as its own segment.
  // See docs/design/edit-window-rework.md.
  import { tick, untrack } from "svelte";
  import { api } from "$lib/api";
  import { loadVideoFilmstrip } from "$lib/thumbnail-loader";
  import type {
    ClipRef,
    EditAdjustments,
    EditExportRequest,
    EditSnapshotRequest,
    EditSourceItem,
    FilmstripInfo,
    MediaProbe,
  } from "$lib/types";
  import ContextMenu, { type MenuEntry } from "./ContextMenu.svelte";

  let {
    onchange,
    onsidebyside,
    ondropped,
  }: {
    /** The timeline changed (debounce and persist it). */
    onchange?: (state: TimelineState) => void;
    /** "Side by side" with the library. */
    onsidebyside?: () => void;
    /** A drop on a track added clips (the window reports what happened). */
    ondropped?: (r: AddResult) => void;
  } = $props();

  type PresetId = "original" | "landscape" | "square" | "reels" | "mobile";
  type ExportTarget = "instagram_reels" | "instagram_square" | "instagram_landscape" | "whatsapp" | "archive";
  type LookPresetId =
    | "warmportrait"
    | "softskin"
    | "goldenhour"
    | "vivid"
    | "orangeteal"
    | "mono"
    | "noir"
    | "tealorange"
    | "batman"
    | "moody"
    | "osmo"
    | "delog";
  type LookGroupId = "portrait" | "landscape" | "bw" | "cinematic" | "clean";
  type Encoder = "auto" | "x264" | "nvenc";
  type Quality = "best" | "high" | "standard" | "small";
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

  // Parameter-set presets: each is a full EditAdjustments, so the intensity
  // slider can scale any of them uniformly as neutral + (preset − neutral)×t.
  // Values are tuned so each look is CLEARLY visible at intensity 100% and still
  // tasteful at 50%. Warmth ±0.5 = ±25% R/B channel gain; splitTone 1.0 = a full
  // orange-highlight / teal-shadow separation (see lookMatrix / lookSplit below,
  // which the ffmpeg export mirrors channel-for-channel).
  const LOOK_PRESETS: Record<LookPresetId, { label: string; hint: string; values: EditAdjustments }> = {
    // — Vlog & Portrait — skin-friendly warmth, soft contrast.
    // Gentle lift + real warmth, saturation barely up so skin stays believable.
    warmportrait: { label: "Warm Portrait", hint: "Flattering skin warmth", values: { brightness: 0.03, contrast: 1.06, saturation: 1.05, warmth: 0.16, sharpen: 0.1, splitTone: 0 } },
    // Beauty look: blacks lifted (contrast < 1), soft, a touch of warm/teal glow.
    softskin: { label: "Soft Skin", hint: "Soft, lifted, gentle", values: { brightness: 0.05, contrast: 0.94, saturation: 0.98, warmth: 0.1, sharpen: 0.06, splitTone: 0.18 } },
    // Golden hour: strong warmth + a hint of orange/teal for that sunset glow.
    goldenhour: { label: "Golden Hour", hint: "Warm sunset glow", values: { brightness: 0.03, contrast: 1.08, saturation: 1.12, warmth: 0.3, sharpen: 0.1, splitTone: 0.22 } },

    // — Drone & Landscape — punchy clarity, vivid but not neon.
    // Big saturation + contrast + sharpen lift for Mavic / drone footage.
    vivid: { label: "Vivid Landscape", hint: "Punchy drone pop", values: { brightness: 0.02, contrast: 1.18, saturation: 1.4, warmth: 0.05, sharpen: 0.22, splitTone: 0 } },
    // The travel split-tone — curves do the teal/orange, params add saturation polish.
    orangeteal: { label: "Orange & Teal", hint: "Travel split-tone", values: { brightness: 0.02, contrast: 1.1, saturation: 1.16, warmth: 0.06, sharpen: 0.14, splitTone: 1 } },

    // — Black & White — a clean neutral and a hard noir.
    // Neutral monochrome, mild contrast, gentle clarity.
    mono: { label: "Mono", hint: "Classic neutral B&W", values: { brightness: 0.02, contrast: 1.12, saturation: 0, warmth: 0, sharpen: 0.12, splitTone: 0 } },
    // High-contrast noir: crushed blacks, hard whites.
    noir: { label: "Noir", hint: "High-contrast mono", values: { brightness: -0.02, contrast: 1.42, saturation: 0, warmth: 0, sharpen: 0.16, splitTone: 0 } },

    // — Cinematic — teal shadows, orange highlights, moody low sat.
    // Blockbuster grade: full orange/teal split, saturation pulled back for film feel.
    tealorange: { label: "Teal & Orange", hint: "Blockbuster grade", values: { brightness: 0, contrast: 1.06, saturation: 0.9, warmth: 0.08, sharpen: 0.1, splitTone: 1.25 } },
    // The Batman (2022): underexposed-but-lifted blacks, heavily desaturated,
    // cool/teal-leaning — Fraser/Cole's grade approximated globally.
    batman: { label: "The Batman", hint: "Dark, cool, gritty", values: { brightness: 0.05, contrast: 0.9, saturation: 0.58, warmth: -0.26, sharpen: 0.06, splitTone: 0.3 } },
    // Faded matte film: low contrast, low sat, slightly cool, soft split.
    moody: { label: "Moody Film", hint: "Faded matte film", values: { brightness: 0.06, contrast: 0.88, saturation: 0.8, warmth: -0.05, sharpen: 0.05, splitTone: 0.42 } },

    // — Clean & Correction — natural, and a rescue for flat footage.
    // Osmo Pocket 3 natural: barely-there polish that keeps colours true.
    osmo: { label: "Osmo Clean", hint: "Pocket 3 natural", values: { brightness: 0.01, contrast: 1.06, saturation: 1.08, warmth: 0.05, sharpen: 0.14, splitTone: 0 } },
    // De-log: strong contrast + saturation recovery for flat / log-ish footage.
    delog: { label: "De-Log Boost", hint: "Revive flat footage", values: { brightness: -0.02, contrast: 1.32, saturation: 1.3, warmth: 0.06, sharpen: 0.16, splitTone: 0 } },
  };

  // Presets, organised into the collapsible groups shown in the Look panel.
  const LOOK_GROUPS: { id: LookGroupId; label: string; presets: LookPresetId[] }[] = [
    { id: "portrait", label: "Vlog & Portrait", presets: ["warmportrait", "softskin", "goldenhour"] },
    { id: "landscape", label: "Drone & Landscape", presets: ["vivid", "orangeteal"] },
    { id: "bw", label: "Black & White", presets: ["mono", "noir"] },
    { id: "cinematic", label: "Cinematic", presets: ["tealorange", "batman", "moody"] },
    { id: "clean", label: "Clean & Correction", presets: ["osmo", "delog"] },
  ];

  const VIDEO_LANES = [0, 1, 2];
  const AUDIO_LANES = [0, 1, 2];
  /** Video track height; a vertical drag of this much moves a clip one track. */
  const TRACK_HEIGHT = 46;
  /** A video clip's height inside its track (thumbnails are drawn at this). */
  const CLIP_H = 38;
  /** Snap reach in screen pixels. It used to be a fixed 0.16 s, which is 1-4 px
   *  at the zoom levels people use, so clips never seemed to snap. */
  const SNAP_PX = 10;
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
  let probes = $state<Record<string, MediaProbe>>({});
  let timelineScale = $state(26);
  let timelineViewportEl = $state<HTMLDivElement | null>(null);
  let timelineViewportW = $state(0);
  let inspectorPanelW = $state(320);
  let timelinePanelH = $state(300);
  /** Width of the whole studio, for sharing it out below. */
  let shellW = $state(0);

  // The Look panel keeps the width the user dragged it to while there is room
  // and gives way when there isn't, so the work pane (preview, format bar,
  // Export) always gets at least WORK_MIN.
  const WORK_MIN = 460;
  const PANEL_MIN = 230;
  let panelW = $derived.by(() => {
    let insp = inspectorCollapsed ? 0 : inspectorPanelW;
    const over = insp + (insp ? 6 : 0) + WORK_MIN - shellW;
    if (shellW > 0 && over > 0 && insp) insp = Math.max(PANEL_MIN, insp - over);
    return { insp: Math.round(insp) };
  });
  let inspectorCollapsed = $state(false);
  let timelineCollapsed = $state(false);
  let productionPreview = $state(false);
  let timelineDrag: TimelineDrag | null = null;
  /** Snapping on (the toolbar chip); holding ⌥/Alt while dragging skips it. */
  let snapOn = $state(true);
  /** Where a drag just snapped, drawn as a guide line across the tracks. */
  let snapGuide = $state<number | null>(null);
  let sourceMenu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  let exportMenuOpen = $state(false);
  type DlgMode = "instagram" | "lossless" | "custom";
  type FpsChoice = "source" | "60" | "30";
  type ResChoice = "default" | "small";
  let exportDlg = $state(false);
  let dlgMode = $state<DlgMode>("instagram");
  let keepHdr = $state(false);
  // Dialog target settings — presets PRE-FILL these, every one stays editable.
  let dlgFps = $state<FpsChoice>("source");
  let dlgRes = $state<ResChoice>("default");
  let dlgName = $state("");
  let dlgNameTaken = $state(false);
  // Live export progress (0–100, fed by the backend's -progress stream) and the
  // two-step cancel arm (avoids a native confirm() inside the webview).
  let exportPct = $state(0);
  let cancelArmed = $state(false);
  let cancelTimer: ReturnType<typeof setTimeout> | null = null;
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
    splitTone: 0,
  });
  // Which preset is currently driving `adjustments`, and how strongly. A manual
  // slider edit detaches (activeLook = null) so the intensity control only shows
  // while an untouched preset is live.
  let activeLook = $state<LookPresetId | null>(null);
  let lookIntensity = $state(1);
  // Expanded/collapsed state per preset group. Groups default open; the session
  // record above remembers a user's collapse choices across edit-studio visits.
  let lookGroupOpen = $state<Record<string, boolean>>(
    Object.fromEntries(LOOK_GROUPS.map((g) => [g.id, sessionLookGroupOpen[g.id] ?? true])),
  );

  function toggleLookGroup(id: LookGroupId) {
    const next = !lookGroupOpen[id];
    lookGroupOpen[id] = next;
    sessionLookGroupOpen[id] = next;
  }

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
  // Zooming out bottoms out at "the whole program fits in the viewport" — never
  // at an arbitrary px/s floor that strands a long timeline half off-screen.
  let timelineZoomMin = $derived.by(() => {
    const avail = (timelineViewportW || 980) - TIMELINE_TRACK_OFFSET - 24;
    return Math.max(0.5, Math.min(TIMELINE_ZOOM_MIN, avail / Math.max(1, timelineEnd)));
  });
  // The canvas fills the viewport (content rests within the panel) and only
  // grows past it when the zoom actually needs the room.
  let timelineWidth = $derived(
    Math.max(timelineViewportW || 980, timelineEnd * timelineScale + TIMELINE_TRACK_OFFSET + 60),
  );
  // Brightness/contrast/saturation are native CSS filters; warmth + split-tone
  // ride an inline SVG filter (feColorMatrix + feComponentTransfer) so the same
  // channel maths as the ffmpeg export can run in the browser. The url() is only
  // appended when it does something, so the plain-look fast path stays untouched.
  let lookNeedsSvg = $derived(Math.abs(adjustments.warmth) >= 0.001 || adjustments.splitTone >= 0.001);
  let previewFilter = $derived(
    `brightness(${Math.max(0, 1 + adjustments.brightness)}) contrast(${adjustments.contrast}) saturate(${adjustments.saturation})` +
      (lookNeedsSvg ? " url(#foxLook)" : ""),
  );
  // feColorMatrix diagonal for warmth: red-gain up / blue-gain down, matching the
  // export's colorchannelmixer=rr:gg:bb. The 0.5 coefficient makes full-range
  // warmth an unmistakable ±25% R/B swing (e.g. mid-grey → warm amber at +0.5);
  // KEEP IT EQUAL to warmth_filter() in commands.rs or preview ≠ export.
  let lookMatrix = $derived.by(() => {
    const w = Math.max(-0.5, Math.min(0.5, adjustments.warmth));
    return `${(1 + 0.5 * w).toFixed(4)} 0 0 0 0  0 1 0 0 0  0 0 ${(1 - 0.5 * w).toFixed(4)} 0 0  0 0 0 1 0`;
  });
  // Per-channel split-tone tables mirroring the export's ffmpeg curves control
  // points (evenly spaced at 0/.25/.5/.75/1 — the SVG ramps them linearly).
  // Deltas strengthened so splitTone=1 reads as a clear orange-highlight /
  // teal-shadow separation (shadows gain ~+0.13 blue, highlights lose ~-0.12 blue
  // and gain red). KEEP THESE EQUAL to splittone_filter() in commands.rs.
  let lookSplit = $derived.by(() => {
    const a = Math.max(0, Math.min(1.5, adjustments.splitTone));
    const xs = [0, 0.25, 0.5, 0.75, 1];
    const rd = [-0.08, -0.03, 0.05, 0.13, 0.09];
    const gd = [0.04, 0.02, 0, -0.03, -0.05];
    const bd = [0.13, 0.07, 0, -0.07, -0.12];
    const tbl = (d: number[]) => xs.map((x, i) => Math.max(0, Math.min(1, x + a * d[i])).toFixed(4)).join(" ");
    return { r: tbl(rd), g: tbl(gd), b: tbl(bd) };
  });
  // Dialog data: the source clip we're about to optimise, its probe, and a rough
  // export-time estimate (HDR tone-mapping is slower than a plain re-encode).
  let igSourceClip = $derived(selectedClip ?? orderedClips[0] ?? null);
  let igSourceProbe = $derived(igSourceClip ? probes[igSourceClip.path] ?? null : null);
  const even = (n: number) => Math.max(2, Math.round(n / 2) * 2);
  let srcFps = $derived(Math.round(igSourceProbe?.fps ?? 0));
  let neutralLook = $derived(
    Math.abs(adjustments.brightness) < 0.001 &&
      Math.abs(adjustments.contrast - 1) < 0.001 &&
      Math.abs(adjustments.saturation - 1) < 0.001 &&
      Math.abs(adjustments.warmth) < 0.001 &&
      adjustments.sharpen < 0.001 &&
      adjustments.splitTone < 0.001,
  );
  // Highest fps across the whole program — a composite conforms to this (≤60).
  let programMaxFps = $derived.by(() => {
    let max = 0;
    for (const pc of programClips) {
      const f = probes[pc.path]?.fps ?? 0;
      if (f > max) max = f;
    }
    return Math.round(max);
  });
  // The output canvas actually sent to the backend: the edit-screen aspect at
  // full size, or a 720-short-edge variant (same aspect — aspect is decided in
  // the edit screen ONLY, never here).
  let dlgOut = $derived.by(() => {
    if (outPreset.fit === "original") return { w: 0, h: 0 };
    if (dlgRes === "small") {
      const f = 720 / Math.min(outPreset.w, outPreset.h);
      return { w: even(outPreset.w * f), h: even(outPreset.h * f) };
    }
    return { w: outPreset.w, h: outPreset.h };
  });
  let resOptions = $derived.by(() => {
    if (outPreset.fit === "original") {
      const w = igSourceProbe?.width;
      const h = igSourceProbe?.height;
      return [{ id: "default" as ResChoice, label: w && h ? `Original (${w}×${h})` : "Original" }];
    }
    const f = 720 / Math.min(outPreset.w, outPreset.h);
    return [
      { id: "default" as ResChoice, label: `${outPreset.w}×${outPreset.h}` },
      { id: "small" as ResChoice, label: `${even(outPreset.w * f)}×${even(outPreset.h * f)} — smaller file` },
    ];
  });
  let fpsOptions = $derived.by(() => {
    const opts: { id: FpsChoice; label: string }[] = [
      {
        id: "source",
        label: srcFps ? `Source (${Math.min(60, srcFps)} fps${srcFps > 60 ? ", capped" : ""})` : "Source",
      },
    ];
    if (!srcFps || srcFps > 31) opts.push({ id: "60", label: "60 fps" });
    opts.push({ id: "30", label: "30 fps" });
    return opts;
  });
  // The fps the export will actually play at (for the Output card).
  let shownFps = $derived(dlgFps === "source" ? Math.min(60, srcFps || 30) : Number(dlgFps));
  // Will this export re-encode? Single source of truth for the dialog's
  // stream-copy promises, the estimate and the re-render line (mirrors
  // edit_requires_reencode + concat_needs_reencode on the backend).
  let willRender = $derived(
    dlgMode === "instagram" ||
      outPreset.fit !== "original" ||
      mixedSources ||
      !neutralLook ||
      audioClips.length > 0 ||
      dlgFps !== "source" ||
      dlgRes !== "default",
  );
  // Container extension the backend will pick (mp4 on re-encode, source ext on
  // stream copy) — shown beside the filename field.
  let dlgExt = $derived(willRender ? "mp4" : extOf(programClips[0]?.path ?? "") || "mp4");
  let igEstimateSecs = $derived.by(() => {
    const secs = programSeconds || (igSourceClip ? igSourceClip.outS - igSourceClip.inS : 0);
    const hdrHeavy = igSourceProbe?.hdr && dlgMode === "instagram" ? true : false;
    const streamCopy = !willRender;
    const factor = hdrHeavy ? 1.5 : streamCopy ? 0.05 : 0.7;
    return Math.max(streamCopy ? 1 : 4, Math.round(secs * factor));
  });
  // Traffic-light effort dot beside the estimate.
  let effort = $derived(igEstimateSecs <= 20 ? "low" : igEstimateSecs <= 120 ? "medium" : "high");
  let effortLabel = $derived(effort === "low" ? "quick" : effort === "medium" ? "moderate" : "long");
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
  // Will a vertical crop of this source have to upscale (→ soft)? True when the
  // crop's pixel width is below the output width (e.g. a 1080p landscape → 9:16).
  let softCrop = $derived(!!igCrop && dlgOut.w > 0 && igCrop.cropW < dlgOut.w - 1);
  // Rule-based breakdown of what drives the export time (shown under the estimate).
  // `cost` is a relative weight (roughly the compute each step adds); the estimate
  // bar normalises these to proportions so the widest segment = the heaviest step.
  let exportSteps = $derived.by(() => {
    const steps: { label: string; note: string; cost: number }[] = [];
    const p = igSourceProbe;
    if (!willRender) {
      steps.push({ label: "Trim (stream copy)", note: "instant — no re-encode", cost: 1 });
      return steps;
    }
    if (p?.hdr && dlgMode === "instagram" && keepHdr) steps.push({ label: "Keep HDR (10-bit HEVC)", note: "heavy", cost: 4 });
    else if (p?.hdr && dlgMode === "instagram") steps.push({ label: "HDR → SDR tone-map", note: "heaviest step", cost: 5 });
    if (igCrop && dlgOut.w > 0) {
      const cw = Math.round(igCrop.cropW);
      if (cw > dlgOut.w + 1) steps.push({ label: `Downscale ${cw}→${dlgOut.w}px`, note: "moderate", cost: 2.5 });
      else if (cw < dlgOut.w - 1) steps.push({ label: `Upscale ${cw}→${dlgOut.w}px`, note: "light", cost: 1.2 });
    }
    if (mixedSources) steps.push({ label: "Conform mixed sources", note: "moderate", cost: 2.5 });
    if (softCrop) steps.push({ label: "Sharpen soft crop", note: "light", cost: 1 });
    if (srcFps && shownFps < srcFps - 1) steps.push({ label: `${srcFps}→${shownFps} fps`, note: "light", cost: 1 });
    if (!neutralLook) steps.push({ label: "Look adjustments", note: "light", cost: 1 });
    steps.push({ label: keepHdr && dlgMode === "instagram" ? "HEVC encode" : "H.264 encode", note: "scales with clip length", cost: 3.5 });
    return steps;
  });
  // Normalised time split for the estimate bar: each step gets a % of the total
  // cost, an approximate second count (share × total estimate), and a stable
  // colour so a legend key lines up with its segment.
  const COST_PALETTE = ["#5b9bff", "#f6a545", "#e5647d", "#54c1a0", "#b98bff", "#e0b64d", "#7bd0e0", "#ef8fb0"];
  let exportCost = $derived.by(() => {
    const steps = exportSteps;
    const total = steps.reduce((a, s) => a + s.cost, 0) || 1;
    return {
      steps: steps.map((s, i) => ({
        label: s.label,
        note: s.note,
        pct: (s.cost / total) * 100,
        secs: Math.max(1, Math.round((igEstimateSecs * s.cost) / total)),
        color: COST_PALETTE[i % COST_PALETTE.length],
      })),
    };
  });
  // Plain-language + technical (x264 CRF) meaning of the quality picker. High is
  // the Instagram default on purpose — the app recompresses uploads, so Best just
  // makes a bigger file for no visible gain.
  let qualityNote = $derived(
    quality === "best"
      ? "Near-lossless (CRF 16) — largest file, for archiving or re-editing."
      : quality === "high"
        ? "Visually lossless (CRF 18) — recommended; Instagram recompresses anyway."
        : quality === "standard"
          ? "Balanced (CRF 20) — noticeably smaller file."
          : "Most compressed (CRF 23) — smallest file, some quality loss.",
  );
  let needsRender = $derived(
    outPreset.fit !== "original" ||
      audioClips.length > 0 ||
      Math.abs(adjustments.brightness) > 0.001 ||
      Math.abs(adjustments.contrast - 1) > 0.001 ||
      Math.abs(adjustments.saturation - 1) > 0.001 ||
      Math.abs(adjustments.warmth) > 0.001 ||
      Math.abs(adjustments.sharpen) > 0.001 ||
      adjustments.splitTone > 0.001,
  );
  // The crop overlay only makes sense when the clip under the playhead IS the
  // selected clip (that's the frame the player is showing + the crop applies to).
  let cropVisible = $derived(
    !productionPreview && !!selectedClip && (segAt(playheadS)?.clip?.id ?? null) === (selectedClip?.id ?? null),
  );

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

  // Track the timeline viewport width for the zoom-out-to-fit floor.
  $effect(() => {
    const el = timelineViewportEl;
    if (!el) return;
    const measure = () => {
      timelineViewportW = el.clientWidth;
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });

  // Keep the zoom within its (dynamic) floor when the viewport or program changes.
  $effect(() => {
    const min = timelineZoomMin;
    if (timelineScale < min) timelineScale = min;
  });

  // "Smaller file" only exists for cropped aspects — snap back when the edit
  // screen switches to Original.
  $effect(() => {
    if (outPreset.fit === "original" && dlgRes !== "default") dlgRes = "default";
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

  // Every probe spawns an ffmpeg process on the backend, so a drop of many
  // clips must not fire them all at once — an unthrottled burst forks dozens
  // of ffmpeg processes and pins a thin laptop. A small queue keeps a few in
  // flight and drains in request order.
  const PROBE_PARALLEL = 4;
  const probeQueue: EditSourceItem[] = [];
  let probesInFlight = 0;

  function pumpProbes() {
    while (probesInFlight < PROBE_PARALLEL && probeQueue.length) {
      const src = probeQueue.shift()!;
      probesInFlight++;
      api
        .probeMediaInfo(src.path)
        .then((p) => {
          probes = { ...probes, [src.path]: p };
        })
        .catch(() => {})
        .finally(() => {
          probing.delete(src.path);
          probesInFlight--;
          pumpProbes();
        });
    }
  }

  function ensureProbe(src: EditSourceItem) {
    if (probes[src.path] || probing.has(src.path)) return;
    probing.add(src.path);
    probeQueue.push(src);
    pumpProbes();
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

  /// Put clips from the library on the timeline. Each in/out range marked in
  /// the library becomes its own segment, in order; a clip with none goes on
  /// whole. `at` = a track and time (a drop on the timeline); otherwise they
  /// go on V1 after the last clip (a drop elsewhere, a paste, E). Never on top
  /// of an existing clip: a collision pushes right. Photos and missing files
  /// can't go on a video timeline and are counted, not dropped silently.
  export async function addClips(refs: ClipRef[], at?: { lane: number; start: number } | null): Promise<AddResult> {
    const res: AddResult = { clips: 0, segments: 0, photos: 0, missing: 0, other: 0, paths: [] };
    const lane = at?.lane ?? 0;
    let cursor = at ? at.start : nextVideoStart(0);
    const made: TimelineClip[] = [];
    for (const ref of refs) {
      if (ref.missing) {
        res.missing++;
        continue;
      }
      if (ref.kind === "image" || ref.kind === "raw") {
        res.photos++;
        continue;
      }
      if (ref.kind !== "video") {
        res.other++;
        continue;
      }
      const src: EditSourceItem = { name: ref.name, path: ref.path, kind: "video", ext: ref.ext, mtime: ref.mtime, size: ref.size };
      const duration = await durationFor(src);
      const full = Math.max(0.1, duration || 1);
      // Ranges past the clip's end (a file replaced since it was marked) are
      // clamped, and inverted or sub-frame ones are dropped.
      let ranges = (ref.ranges ?? [])
        .map((r) => ({ inS: Math.max(0, Math.min(r.in_s, full - 0.05)), outS: Math.min(full, Math.max(r.out_s, 0)) }))
        .filter((r) => r.outS - r.inS >= 0.04);
      if (!ranges.length) ranges = [rememberedTrim(ref.path, full)];
      const cachedProxy = await api.videoProxyCached(ref.path);
      const vsrc = api.fileSrc(cachedProxy ?? ref.path);
      for (const r of ranges) {
        const len = Math.max(0.05, r.outS - r.inS);
        const clip: TimelineClip = {
          id: uid(),
          path: ref.path,
          name: ref.name,
          src: vsrc,
          inS: r.inS,
          outS: r.outS,
          duration: full,
          start: 0,
          lane,
          cropX: 0.5,
          cropY: 0.5,
          zoom: 1,
        };
        // Lay the new ones end to end, clear of what's already there AND of
        // each other (they aren't in `clips` until the end).
        let st = freeStart(lane, cursor, len);
        for (let g = 0; g < 400; g++) {
          const hit = made.find((o) => o.lane === lane && st < o.start + clipLen(o) - 1e-4 && st + len > o.start + 1e-4);
          if (!hit) break;
          st = freeStart(lane, hit.start + clipLen(hit), len);
        }
        clip.start = st;
        cursor = st + len;
        made.push(clip);
        res.segments++;
      }
      res.clips++;
      res.paths.push(ref.path);
    }
    if (made.length) {
      clips = [...clips, ...made];
      selectClip(made[made.length - 1].id);
      exportNote = null;
    }
    return res;
  }

  export function isEmpty() {
    return clips.length === 0 && audioClips.length === 0;
  }

  // ── saving the timeline (the window can be closed and reopened) ──────────
  /** Files of timeline clips that weren't there when the timeline came back
   *  (a drive unplugged): drawn dashed, and named in a notice. */
  let unavailable = $state<Set<string>>(new Set());

  export function serialize(): TimelineState {
    return {
      v: 1,
      clips: clips.map((c) => ({ path: c.path, name: c.name, inS: c.inS, outS: c.outS, duration: c.duration, start: c.start, lane: c.lane, cropX: c.cropX, cropY: c.cropY, zoom: c.zoom })),
      audio: audioClips.map((a) => ({ path: a.path, name: a.name, start: a.start, duration: a.duration, lane: a.lane })),
      preset,
      adjustments: { ...adjustments },
      look: activeLook,
      lookIntensity,
      keepSourceAudio: preserveSourceAudio,
    };
  }

  export async function restore(st: TimelineState): Promise<{ missing: string[] }> {
    const missing: string[] = [];
    const gone = new Set<string>();
    const made: TimelineClip[] = [];
    const checked = new Map<string, boolean>();
    for (const c of st.clips ?? []) {
      if (!checked.has(c.path)) checked.set(c.path, await api.pathExists(c.path).catch(() => false));
      const there = checked.get(c.path)!;
      if (!there) {
        gone.add(c.path);
        if (!missing.includes(c.name)) missing.push(c.name);
      }
      const proxy = there ? await api.videoProxyCached(c.path).catch(() => null) : null;
      made.push({ ...c, id: uid(), src: api.fileSrc(proxy ?? c.path) });
      if (there) ensureProbe({ name: c.name, path: c.path, kind: "video", ext: extOf(c.path), mtime: 0, size: 0 });
    }
    clips = made;
    audioClips = (st.audio ?? []).map((a) => ({ ...a, id: uid() }));
    if (st.preset && st.preset in PRESETS) preset = st.preset as PresetId;
    if (st.adjustments) adjustments = { ...NEUTRAL_ADJ, ...st.adjustments };
    activeLook = st.look && st.look in LOOK_PRESETS ? (st.look as LookPresetId) : null;
    lookIntensity = st.lookIntensity ?? 1;
    preserveSourceAudio = st.keepSourceAudio ?? true;
    unavailable = gone;
    selectedId = clips[0]?.id ?? null;
    selectedIds = new Set(selectedId ? [selectedId] : []);
    return { missing };
  }

  /** Start over: an empty timeline, neutral look (the aspect stays). */
  export function clearTimeline() {
    stopPlayback();
    clips = [];
    audioClips = [];
    selectedId = null;
    selectedIds = new Set();
    selectedAudioId = null;
    playheadS = 0;
    unavailable = new Set();
    exportNote = null;
  }

  $effect(() => {
    // Read everything that defines the timeline, so any change re-runs this.
    const st = serialize();
    onchange?.(st);
  });

  // ── does a clip match the rest? ───────────────────────────────────────────
  // The timeline's format is its first clip's (as in Premiere). A clip that
  // differs gets a ≠ badge saying how, and arriving clips that differ are
  // named in a notice with what the export will do about it, rather than the
  // owner finding out from a slow export or a squashed frame.
  const FPS_CLASSES = [12, 15, 24, 25, 30, 48, 50, 60, 72, 90, 100, 120, 240];
  const fpsClass = (f: number) => (f > 0 ? (FPS_CLASSES.find((c) => c >= f * 0.995) ?? Math.round(f)) : 0);
  const orient = (p: MediaProbe) => (p.width && p.height ? (p.height > p.width ? "vertical" : p.height === p.width ? "square" : "landscape") : "");
  let refClip = $derived(orderedClips.find((c) => probes[c.path]?.width) ?? null);
  let refProbe = $derived(refClip ? probes[refClip.path] : null);

  function mismatchOf(path: string): string | null {
    const ref = refProbe;
    const p = probes[path];
    if (!ref || !p || !refClip || path === refClip.path) return null;
    const notes: string[] = [];
    if (orient(p) && orient(ref) && orient(p) !== orient(ref)) notes.push(`${orient(p)}, the timeline is ${orient(ref)}`);
    else if (p.width !== ref.width || p.height !== ref.height) notes.push(`${p.width}×${p.height}, the timeline is ${ref.width}×${ref.height}`);
    if (fpsClass(p.fps) && fpsClass(ref.fps) && fpsClass(p.fps) !== fpsClass(ref.fps)) notes.push(`${fpsClass(p.fps)} fps, the timeline is ${fpsClass(ref.fps)}`);
    if (!!p.hdr !== !!ref.hdr) notes.push(p.hdr ? "HDR among SDR clips" : "SDR among HDR clips");
    if (p.codec && ref.codec && p.codec !== ref.codec && outPreset.fit === "original") notes.push(`${p.codec}, the timeline is ${ref.codec}`);
    return notes.length ? notes.join(" · ") : null;
  }

  /** Notices for clips that just arrived: what differs, and what the export
   *  will do about it. Waits for their probes (already queued by addClips). */
  export async function compatNotes(paths: string[]): Promise<string[]> {
    const distinct = [...new Set(paths)];
    for (const p of distinct) await durationFor({ name: basename(p), path: p, kind: "video", ext: extOf(p), mtime: 0, size: 0 });
    const out: string[] = [];
    const unread = distinct.filter((p) => !probes[p]?.width);
    if (unread.length) out.push(`Couldn't read ${unread.map(basename).join(", ")}: ${unread.length === 1 ? "it" : "they"} may not preview or export.`);
    const differ = distinct.map((p) => ({ p, why: mismatchOf(p) })).filter((x) => x.why);
    if (differ.length) {
      const list = differ.map((x) => `${basename(x.p)} (${x.why})`).join("; ");
      const what =
        outPreset.fit === "original"
          ? "They can't be joined as they are, so exporting re-encodes everything to match the first clip (slower than a straight copy)."
          : `Each is fitted to the ${outPreset.label} frame on export${differ.some((x) => /fps/.test(x.why ?? "")) ? ", and everything plays at one frame rate" : ""}${differ.some((x) => /HDR/.test(x.why ?? "")) ? "; HDR is converted to SDR so the colours match" : ""}.`;
      out.push(`${differ.length === 1 ? "This clip differs" : `${differ.length} clips differ`} from the timeline: ${list}. ${what}`);
    }
    return out;
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
      // What actually plays is the length: the probe reads the container,
      // which can run a few ms longer (see sampleFromVideo).
      if (d > 0 && (Math.abs(d - clip.duration) > 0.01 || clip.outS > d)) {
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
  //
  // A segment can start inside its clip (a clip on V2 shows from where the V1
  // clip above it ends), so source time maps through the CLIP's start, not the
  // segment's.
  //
  // The file can also end before the clip's out point: phone footage often
  // has a container a few milliseconds longer than its last frame, and the
  // probed length is the container's. The video then sits `ended` short of the
  // out point. That used to freeze the playhead there while the next tick's
  // play() restarted the ended file from 0, so the first clip looped forever
  // (owner, 2026-10-04). An ended video now counts as the end of its segment.
  function sampleFromVideo(): boolean {
    const v = previewVideo;
    if (!v) return false;
    const seg = segAt(playheadS);
    if (!seg?.clip) return false;
    const clip = seg.clip;
    if (loadedSrc !== clip.src || v.readyState < 1) return false;
    const segOutSrc = clip.inS + (seg.end - clip.start);
    const ph = clip.start + (v.currentTime - clip.inS);
    if (v.ended || v.currentTime >= segOutSrc - 1e-3 || ph >= seg.end - 1e-3) {
      advanceFrom(seg);
      return true;
    }
    // The video jumped back on its own (a source that looped or reloaded):
    // put it back where the playhead is instead of letting the two drift apart.
    if (!v.seeking && ph < playheadS - 0.3) {
      syncVideoImperative(clip, clip.inS + (playheadS - clip.start), true);
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
        // Never play() an ended video: that restarts it from 0.
        if (v.paused && !v.ended) v.play().catch(() => {});
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

  function onNormalEnded() {
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

  const NEUTRAL_ADJ: EditAdjustments = { brightness: 0, contrast: 1, saturation: 1, warmth: 0, sharpen: 0, splitTone: 0 };

  function resetColor() {
    adjustments = { ...NEUTRAL_ADJ };
    activeLook = null;
  }

  // Lightroom habit: double-click a slider to snap that one control back to its
  // neutral value. That's a manual edit, so it breaks the preset link too.
  function resetAdj(field: keyof EditAdjustments) {
    adjustments = { ...adjustments, [field]: NEUTRAL_ADJ[field] };
    activeLook = null;
  }

  // A preset is a full parameter set; the intensity slider scales it uniformly as
  // neutral + (preset − neutral) × t, which works for every field at once.
  function scaleAdj(v: EditAdjustments, t: number): EditAdjustments {
    const mix = (from: number, to: number) => from + (to - from) * t;
    return {
      brightness: mix(NEUTRAL_ADJ.brightness, v.brightness),
      contrast: mix(NEUTRAL_ADJ.contrast, v.contrast),
      saturation: mix(NEUTRAL_ADJ.saturation, v.saturation),
      warmth: mix(NEUTRAL_ADJ.warmth, v.warmth),
      sharpen: mix(NEUTRAL_ADJ.sharpen, v.sharpen),
      splitTone: mix(NEUTRAL_ADJ.splitTone, v.splitTone),
    };
  }

  function applyLook(id: LookPresetId) {
    activeLook = id;
    lookIntensity = 1;
    adjustments = scaleAdj(LOOK_PRESETS[id].values, lookIntensity);
    // Keep the active preset visible even if its group was collapsed.
    const g = LOOK_GROUPS.find((grp) => grp.presets.includes(id));
    if (g && !lookGroupOpen[g.id]) toggleLookGroup(g.id);
  }

  // Re-derive the live look from the active preset at the new strength.
  function setLookIntensity(t: number) {
    lookIntensity = t;
    if (activeLook) adjustments = scaleAdj(LOOK_PRESETS[activeLook].values, t);
  }

  // Any hand edit of a slider divorces the result from its preset (so the
  // intensity control hides and won't clobber the tweak on the next render).
  function detachLook() {
    activeLook = null;
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
  // show what they do instead of being flat text tiles. Decorative only (no SVG
  // filter here) — warm/cool and split-tone are faked with sepia + hue-rotate so
  // the tiles read distinct at a glance.
  function lookFilter(v: EditAdjustments): string {
    const warmSep = (Math.max(0, v.warmth) + v.splitTone * 0.25) * 3;
    const coolRot = v.warmth < 0 ? Math.min(35, -v.warmth * 90) : 0;
    return `brightness(${(1 + v.brightness).toFixed(3)}) contrast(${v.contrast}) saturate(${v.saturation}) sepia(${warmSep.toFixed(3)}) hue-rotate(${(-coolRot).toFixed(1)}deg)`;
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
    return clampPanel(n, timelineZoomMin, TIMELINE_ZOOM_MAX);
  }

  function startInspectorResize(e: PointerEvent) {
    e.preventDefault();
    inspectorCollapsed = false;
    const startX = e.clientX;
    const startW = panelW.insp || inspectorPanelW;
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
    await addAudio(src);
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

  // Aspect is chosen in the edit screen (the preset row) — the dialog never
  // re-decides it. Every entry point opens the SAME dialog; a mode is just a
  // pre-fill of the editable target settings on the right.
  function applyDlgMode(mode: DlgMode) {
    dlgMode = mode;
    if (mode === "instagram") {
      exportTarget = instagramTargetForPreset();
      quality = "high";
      dlgFps = "source";
      dlgRes = "default";
    } else if (mode === "lossless") {
      exportTarget = "archive";
      quality = "best";
      dlgFps = "source";
      dlgRes = "default";
    } else {
      exportTarget = "archive";
    }
    dlgName = exportName();
  }

  function openExport(mode: DlgMode) {
    exportMenuOpen = false;
    applyDlgMode(mode);
    exportDlg = true;
  }

  // The fps value sent to the backend. "Source" leaves timing untouched, EXCEPT
  // when the encode must unify anyway — Instagram normalisation or a mixed-source
  // composite — where every clip conforms to the program's max fps, capped at 60
  // (Instagram accepts 60; never silently halve the user's 60 fps footage).
  function requestFps(): number | null {
    if (dlgFps === "60") return 60;
    if (dlgFps === "30") return 30;
    if (dlgMode === "instagram" || (mixedSources && programClips.length > 1)) {
      return Math.min(60, Math.max(24, programMaxFps || 30));
    }
    return null;
  }

  // Two-step inline cancel (no native confirm in the webview): first click arms,
  // second click within 4s cancels — the backend kills ffmpeg and deletes the
  // partial file.
  function requestCancel() {
    if (!cancelArmed) {
      cancelArmed = true;
      if (cancelTimer) clearTimeout(cancelTimer);
      cancelTimer = setTimeout(() => (cancelArmed = false), 4000);
      return;
    }
    if (cancelTimer) clearTimeout(cancelTimer);
    cancelArmed = false;
    void api.cancelEditExport();
  }

  // Live export percentage from the backend's -progress stream.
  $effect(() => {
    let un: (() => void) | null = null;
    let alive = true;
    api
      .onExportProgress((pct) => {
        exportPct = Math.max(0, Math.min(100, pct));
      })
      .then((u) => {
        if (alive) un = u;
        else u();
      });
    return () => {
      alive = false;
      un?.();
    };
  });

  // Filename-taken hint: check the default/edited name against the destination
  // folder (the export itself still auto-uniquifies — nothing is overwritten).
  $effect(() => {
    if (!exportDlg) return;
    const name = dlgName.trim();
    const first = programClips[0]?.path;
    if (!name || !first) {
      dlgNameTaken = false;
      return;
    }
    const dir = first.replace(/[\\/][^\\/]*$/, "");
    const ext = willRender ? "mp4" : extOf(first) || "mp4";
    const sep = first.includes("\\") ? "\\" : "/";
    let alive = true;
    api.pathExists(`${dir}${sep}${name}.${ext}`).then((v) => {
      if (alive) dlgNameTaken = v;
    });
    return () => {
      alive = false;
    };
  });

  // Map the edit-screen aspect preset to the Instagram export target (drives the
  // filename suffix + quality).
  function instagramTargetForPreset(): ExportTarget {
    if (preset === "square") return "instagram_square";
    if (preset === "landscape") return "instagram_landscape";
    return "instagram_reels";
  }

  async function runDialogExport() {
    exportDlg = false;
    // Quality/fps/resolution were pre-filled by the mode and possibly edited in
    // the dialog — the mode itself only decides normalisation (Instagram).
    await exportTimeline(dlgMode === "instagram", dlgMode === "instagram" && keepHdr);
  }

  async function exportTimeline(normalize = false, keep_hdr = false) {
    if (!programClips.length || exporting) return;
    exportMenuOpen = false;
    exporting = true;
    exportPct = 0;
    cancelArmed = false;
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
        output_w: dlgOut.w,
        output_h: dlgOut.h,
        fit: outPreset.fit,
        encoder,
        quality,
        adjustments,
        music_path: music,
        preserve_source_audio: preserveSourceAudio && !music,
        destination: null,
        basename: dlgName.trim() || exportName(),
        normalize,
        keep_hdr,
        fps: requestFps(),
      };
      const out = await api.editExport(req);
      exportNote = `Saved ${basename(out.path)} (${out.reencoded ? out.mode : "stream copy"})`;
      api.reveal(out.path);
    } catch (e) {
      const msg = `${e}`;
      exportNote = msg.includes("export cancelled")
        ? "Export cancelled — the partial file was deleted."
        : `Export failed: ${e}`;
    } finally {
      exporting = false;
      cancelArmed = false;
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

  /** The edge (time 0, the playhead, any other clip's start or end) nearest
   *  to `t` within SNAP_PX on screen, or null. */
  function nearestEdge(t: number, exclude?: string | Set<string>): { t: number; d: number } | null {
    const skip = (id: string) => (typeof exclude === "string" ? exclude === id : !!exclude?.has(id));
    let best: { t: number; d: number } | null = null;
    const reach = SNAP_PX / Math.max(0.1, timelineScale);
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
      if (d <= reach && (!best || d < best.d)) best = { t: edge, d };
    }
    return best;
  }

  function snapTime(t: number, exclude?: string | Set<string>, free = false) {
    const hit = snapOn && !free ? nearestEdge(t, exclude) : null;
    snapGuide = hit ? hit.t : null;
    return Math.max(0, hit ? hit.t : t);
  }

  /** A moving clip snaps by whichever of its two edges is nearer to something. */
  function snapMove(start: number, len: number, exclude: Set<string>, free: boolean) {
    if (!snapOn || free) {
      snapGuide = null;
      return Math.max(0, start);
    }
    const a = nearestEdge(start, exclude);
    const b = nearestEdge(start + len, exclude);
    if (a && (!b || a.d <= b.d)) {
      snapGuide = a.t;
      return Math.max(0, a.t);
    }
    if (b && b.t - len >= 0) {
      snapGuide = b.t;
      return b.t - len;
    }
    snapGuide = null;
    return Math.max(0, start);
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
    const free = e.altKey;
    if (timelineDrag.kind === "audio") {
      const a = audioClips.find((x) => x.id === timelineDrag?.id);
      updateAudio(timelineDrag.id, { start: snapMove(timelineDrag.start + d, a?.duration ?? 0, new Set([timelineDrag.id]), free) });
      return;
    }
    const clip = clips.find((c) => c.id === timelineDrag?.id);
    if (!clip) return;
    if (timelineDrag.mode === "move") {
      // Snap the primary clip, then shift the whole selection by that delta.
      // Vertical motion re-lanes the selection (V1–V3), clamped per clip.
      const groupIds = new Set(timelineDrag.group.map((g) => g.id));
      const snappedStart = snapMove(timelineDrag.start + d, timelineDrag.outS - timelineDrag.inS, groupIds, free);
      const delta = snappedStart - timelineDrag.start;
      const deltaLanes = Math.round((e.clientY - timelineDrag.startY) / TRACK_HEIGHT);
      clips = clips.map((c) => {
        const g = timelineDrag!.group.find((x) => x.id === c.id);
        if (!g) return c;
        return { ...c, start: Math.max(0, g.start + delta), lane: Math.max(0, Math.min(2, g.lane + deltaLanes)) };
      });
    } else if (timelineDrag.mode === "trimIn") {
      const snappedStart = snapTime(timelineDrag.start + d, clip.id, free);
      const nextIn = Math.max(0, Math.min(timelineDrag.inS + (snappedStart - timelineDrag.start), timelineDrag.outS - 0.05));
      updateClip(clip.id, { start: timelineDrag.start + (nextIn - timelineDrag.inS), inS: nextIn });
      trimPreview = { id: clip.id, time: nextIn };
    } else {
      const right = snapTime(timelineDrag.start + (timelineDrag.outS - timelineDrag.inS) + d, clip.id, free);
      const nextOut = Math.max(clip.inS + 0.05, Math.min(timelineDrag.duration, clip.inS + Math.max(0.05, right - clip.start)));
      updateClip(clip.id, { outS: nextOut });
      trimPreview = { id: clip.id, time: nextOut };
    }
  }

  function endTimelineDrag() {
    const drag = timelineDrag;
    timelineDrag = null;
    trimPreview = null;
    snapGuide = null;
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

  /** Is this a drag of clips from the library? (Read during dragover, when
   *  only the types are visible, not the data.) */
  export function isClipDrag(e: DragEvent): boolean {
    const t = Array.from(e.dataTransfer?.types ?? []);
    return t.includes("application/x-foxcull-clips") || t.includes("application/x-foxcull-paths") || t.includes("text/plain");
  }

  /** The clips a drop carries: the library's own payload, else its paths,
   *  else the drag it parked in the backend (a drag from another window can
   *  arrive with only plain text, depending on the platform's webview). */
  export async function clipsFromDrop(e: DragEvent): Promise<ClipRef[]> {
    const dt = e.dataTransfer;
    const raw = dt?.getData("application/x-foxcull-clips");
    if (raw) {
      try {
        return JSON.parse(raw) as ClipRef[];
      } catch {
        /* fall through */
      }
    }
    const parked = await api.stashGet<ClipRef[]>("drag");
    const text = dt?.getData("application/x-foxcull-paths") || dt?.getData("text/plain") || "";
    let paths: string[] = [];
    try {
      paths = text.trim().startsWith("[") ? (JSON.parse(text) as string[]) : text.split(/\r?\n/).filter(Boolean);
    } catch {
      paths = [];
    }
    if (parked?.length && (!paths.length || paths.every((p) => parked.some((c) => c.path === p)))) return parked;
    return paths.map((p) => ({ path: p, name: basename(p), kind: "video", ext: extOf(p), mtime: 0, size: 0, ranges: [] }));
  }

  function allowDrop(e: DragEvent) {
    if (!isClipDrag(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
  }

  /** A drop ON a video track: the clips go on that track at that time. */
  async function dropOnLane(e: DragEvent, kind: "video" | "audio", lane: number) {
    if (!isClipDrag(e)) return;
    e.preventDefault();
    e.stopPropagation(); // the window's "drop anywhere" must not add them again
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const start = snapTime(Math.max(0, (e.clientX - rect.left) / timelineScale));
    snapGuide = null;
    const refs = await clipsFromDrop(e);
    // Audio tracks take music (Add music…), not clips: their sound already
    // rides with them on the matching A track.
    const res = await addClips(refs, kind === "video" ? { lane, start } : null);
    ondropped?.(res);
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

  // ── 2026-10 redesign helpers: thumbnails, transport, ruler, zoom ─────────

  /** Which inspector tab shows: the look of the whole edit, or the selected clip. */
  let inspTab = $state<"look" | "clip">("look");
  /** Group filter for the look presets ("all" or a LOOK_GROUPS id). */
  let lookFilterGroup = $state<"all" | LookGroupId>("all");

  // Clip thumbnails. A poster first (cheap, usually cached from the grid), then
  // the clip's Focus filmstrip, built one clip at a time and never while the
  // timeline is playing, so the frames along each clip are real.
  let posters = $state<Record<string, string | null>>({});
  let strips = $state<Record<string, FilmstripInfo | null>>({});
  const stripQueue: string[] = [];
  let stripBusy = false;
  $effect(() => {
    const paths = [...new Set(clips.map((c) => c.path))];
    const gone = unavailable;
    untrack(() => {
      for (const p of paths) {
        if (gone.has(p)) continue;
        if (!(p in posters)) {
          posters[p] = null;
          api.videoPoster(p).then((f) => (posters[p] = api.fileSrc(f))).catch(() => {});
        }
        if (!(p in strips) && !stripQueue.includes(p)) stripQueue.push(p);
      }
    });
    void pumpStrips();
  });
  async function pumpStrips() {
    if (stripBusy) return;
    stripBusy = true;
    while (stripQueue.length) {
      while (playing) await new Promise((r) => setTimeout(r, 600));
      const p = stripQueue.shift()!;
      if (p in strips) continue;
      strips[p] = null;
      try {
        const f = (await api.videoFilmstripCached(p)) ?? (await loadVideoFilmstrip(p));
        if (f) strips[p] = { ...f, src: api.fileSrc(f.src) };
      } catch {
        /* no strip: the clip keeps its poster */
      }
    }
    stripBusy = false;
  }

  /** The frames along a clip `w` px wide and `h` px tall, as positioned tiles. */
  function clipTiles(clip: TimelineClip, w: number, h: number): { x: number; w: number; css: string }[] {
    const f = strips[clip.path];
    if (!f || !f.count || !f.tile_w || !f.tile_h) return [];
    const tw = Math.max(24, Math.round((h * f.tile_w) / f.tile_h));
    const n = Math.min(160, Math.ceil(w / tw));
    const out: { x: number; w: number; css: string }[] = [];
    for (let k = 0; k < n; k++) {
      const x = k * tw;
      const t = Math.min(clip.outS, clip.inS + (x + tw / 2) / timelineScale);
      out.push({ x, w: tw, css: spriteCss(f, t, tw, h) });
    }
    return out;
  }
  function spriteCss(f: FilmstripInfo, t: number, w: number, h: number): string {
    const i = Math.min(f.count - 1, Math.max(0, Math.floor((t / (f.duration || 1)) * f.count)));
    const sc = Math.max(w / f.tile_w, h / f.tile_h);
    const col = i % f.cols;
    const row = Math.floor(i / f.cols);
    const dx = (w - f.tile_w * sc) / 2 - col * f.tile_w * sc;
    const dy = (h - f.tile_h * sc) / 2 - row * f.tile_h * sc;
    return `background-image:url("${f.src}");background-size:${f.cols * f.tile_w * sc}px ${f.rows * f.tile_h * sc}px;background-position:${dx}px ${dy}px`;
  }
  /** The picture the look tiles preview on: the selected clip, else the first. */
  let lookThumb = $derived((selectedClip && posters[selectedClip.path]) || (clips[0] && posters[clips[0].path]) || null);

  /** "1:04.37": minutes, seconds and hundredths, for the transport (fine
   *  enough that stepping one frame visibly moves it). */
  function fmtTC(s: number) {
    if (!Number.isFinite(s) || s < 0) s = 0;
    const cs = Math.floor(s * 100 + 1e-6);
    const h = Math.floor(cs / 360000);
    const m = Math.floor((cs % 360000) / 6000);
    const sec = Math.floor((cs % 6000) / 100);
    const c = cs % 100;
    const mm = h ? `${h}:${m.toString().padStart(2, "0")}` : `${m}`;
    return `${mm}:${sec.toString().padStart(2, "0")}.${c.toString().padStart(2, "0")}`;
  }

  /** One frame of the clip under the playhead (30 fps when unknown). */
  export function stepFrame(dir: -1 | 1) {
    if (productionPreview) return;
    if (playing) stopPlayback();
    const clip = segAt(playheadS)?.clip;
    const fps = (clip && probes[clip.path]?.fps) || 30;
    seekTimeline(playheadS + dir / fps);
  }
  export function goToEdge(end: boolean) {
    if (productionPreview) return;
    seekTimeline(end ? videoEnd : 0);
  }

  // The program bar under the preview: drag anywhere on it to scrub.
  function startProgramScrub(e: PointerEvent) {
    if (!clips.length || e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    const rect = el.getBoundingClientRect();
    const toTime = (x: number) => Math.max(0, Math.min(videoEnd, ((x - rect.left) / Math.max(1, rect.width)) * videoEnd));
    seekTimeline(toTime(e.clientX));
    const move = (ev: PointerEvent) => seekTimeline(toTime(ev.clientX));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  // Timeline ruler: labelled every `rulerStep` seconds (at least ~64 px
  // apart at any zoom), with minor ticks between.
  let rulerStep = $derived([1, 2, 5, 10, 15, 30, 60, 120, 300, 600].find((st) => st * timelineScale >= 64) ?? 600);
  let rulerMinor = $derived(rulerStep * timelineScale >= 120 ? rulerStep / 5 : rulerStep / 2);

  // The track names stay at the left edge while the timeline scrolls sideways.
  let tlScrollX = $state(0);

  // The timeline follows the playhead: when it runs (or is sent) off either
  // side, the view pages so the playhead sits near the left again. Not while
  // a clip is being dragged, which moves the view itself.
  // Only the playhead moving triggers this (zooming keeps its own anchor).
  $effect(() => {
    const ph = playheadS;
    const vp = timelineViewportEl;
    if (!vp || timelineDrag) return;
    untrack(() => {
      const x = TIMELINE_TRACK_OFFSET + ph * timelineScale;
      const left = vp.scrollLeft + TIMELINE_TRACK_OFFSET;
      const right = vp.scrollLeft + vp.clientWidth - 24;
      if (x < left || x > right) vp.scrollLeft = Math.max(0, x - TIMELINE_TRACK_OFFSET - 60);
    });
  });

  function zoomBy(f: number) {
    const vp = timelineViewportEl;
    const old = timelineScale;
    const next = clampTimelineScale(old * f);
    if (next === old) return;
    // Keep the playhead where it is on screen.
    const anchor = vp ? TIMELINE_TRACK_OFFSET + playheadS * old - vp.scrollLeft : 0;
    timelineScale = next;
    void tick().then(() => {
      if (vp) vp.scrollLeft = Math.max(0, TIMELINE_TRACK_OFFSET + playheadS * next - anchor);
    });
  }
  function fitTimeline() {
    timelineScale = timelineZoomMin;
    if (timelineViewportEl) timelineViewportEl.scrollLeft = 0;
  }

  /** A small rectangle in the output's shape, for the aspect buttons. */
  function aspectGlyph(id: string): string {
    const p = PRESETS[id as PresetId];
    if (!p || !p.w) return "width:14px;height:10px";
    const r = p.w / p.h;
    return r >= 1 ? `width:14px;height:${Math.round(14 / r)}px` : `width:${Math.round(14 * r)}px;height:14px`;
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
  class:inspectorCollapsed
  class:timelineCollapsed
  class:productionPreviewMode={productionPreview}
  bind:clientWidth={shellW}
  style={`--inspector-w:${panelW.insp}px; --inspector-splitter-w:${inspectorCollapsed ? 0 : 6}px; --timeline-h:${timelineCollapsed ? 0 : timelinePanelH}px;`}
>
  <section class="workPane">
    <div class="editTop">
      <div class="brand">
        <span class="brandIco" aria-hidden="true">
          <svg viewBox="0 0 24 24"><rect x="3" y="5" width="18" height="14" rx="3" /><path d="M3 15h18M8 15v4M13 15v4" /></svg>
        </span>
        <span class="brandText">
          <strong>Edit</strong>
          <span>{clips.length ? `${clips.length} clip${clips.length === 1 ? "" : "s"} · ${fmt(programSeconds)}` : "Empty timeline"}</span>
        </span>
      </div>
      <div class="aspects" role="radiogroup" aria-label="Output shape">
        {#each Object.entries(PRESETS) as [id, p] (id)}
          <button class:on={preset === id} role="radio" aria-checked={preset === id} onclick={() => setPreset(id as PresetId)} title={`${p.label} · ${p.detail}`}>
            <i class="ar" class:free={p.fit === "original"} style={aspectGlyph(id)}></i>
            <span>{p.label}</span>
          </button>
        {/each}
      </div>
      <span class="topGap"></span>
      {#if frameToast}<span class="topToast" aria-live="polite">{frameToast}</span>{/if}
      <div class="viewTools">
        {#if onsidebyside}
          <button class="iconBtn" onclick={onsidebyside} title="Library on the left, Edit on the right" aria-label="Put the library beside this window">
            <svg viewBox="0 0 24 24"><rect x="3" y="4.5" width="18" height="15" rx="2.5" /><path d="M10 4.5v15" /></svg>
          </button>
        {/if}
        <button class="iconBtn" class:on={!timelineCollapsed} onclick={() => (timelineCollapsed = !timelineCollapsed)} title={timelineCollapsed ? "Show the timeline" : "Hide the timeline"} aria-pressed={!timelineCollapsed} aria-label="Timeline">
          <svg viewBox="0 0 24 24"><rect x="3" y="4.5" width="18" height="15" rx="2.5" /><path d="M3 13h18M7 16.2h6M9 9h8" /></svg>
        </button>
        <button class="iconBtn" class:on={!inspectorCollapsed} onclick={() => (inspectorCollapsed = !inspectorCollapsed)} title={inspectorCollapsed ? "Show the Look panel" : "Hide the Look panel"} aria-pressed={!inspectorCollapsed} aria-label="Look panel">
          <svg viewBox="0 0 24 24"><rect x="3" y="4.5" width="18" height="15" rx="2.5" /><path d="M15 4.5v15M17.5 8.5h1M17.5 11.5h1" /></svg>
        </button>
      </div>
      <button class="pillBtn" class:on={productionPreview} onclick={toggleProductionPreview} disabled={!selectedClip} title="See the selected clip exactly as it will export (F for full screen)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M2.5 12s3.5-6.5 9.5-6.5S21.5 12 21.5 12s-3.5 6.5-9.5 6.5S2.5 12 2.5 12z" /><circle cx="12" cy="12" r="2.8" /></svg>
        <span class="pillText">Preview</span>
      </button>
      <div class="exportOpts">
        {#if exporting}
          <div class="exportProgress" title="Export in progress">
            <div class="epBar"><span style="width:{exportPct}%"></span></div>
            <span class="epPct">{exportPct}%</span>
            <button class="miniBtn epCancel" class:armed={cancelArmed} onclick={requestCancel}>
              {cancelArmed ? "Really cancel?" : "Cancel"}
            </button>
          </div>
        {:else}
        <div class="exportGroup">
          <button
            class="exportBtn main"
            class:needsRender
            onclick={() => openExport("custom")}
            disabled={!clips.length}
            title={needsRender ? "Export — this aspect/look needs a re-render (details in the dialog)" : "Export — stream copy ready (details in the dialog)"}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15V4M7.5 8.5 12 4l4.5 4.5M5 14v4.5A1.5 1.5 0 0 0 6.5 20h11a1.5 1.5 0 0 0 1.5-1.5V14" /></svg>
            Export{#if needsRender}<span class="reDot" aria-hidden="true"></span>{/if}
          </button>
          <button
            class="exportBtn caret"
            class:on={exportMenuOpen}
            onclick={() => (exportMenuOpen = !exportMenuOpen)}
            disabled={!clips.length}
            aria-label="Export options"
            title="Quick export"
          ><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg></button>
        </div>
        {/if}
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
            <button class="exportChoice sub" onclick={() => { exportMenuOpen = false; takeSnapshot(); }} disabled={!clips.length || snapshotting}>
              {snapshotting ? "Saving frame…" : "Save current frame (PNG)"}
            </button>
            <button class="exportChoice sub" onclick={() => openExport("custom")}>All export settings…</button>
          </div>
        {/if}
      </div>
    </div>

    <div class="preview" bind:this={previewBox} onwheel={onCropWheel}>
      <!-- Warmth + split-tone for the preview. sRGB interpolation so the maths
           lands in the same colour space as the ffmpeg export path. -->
      <svg class="lookFilterDefs" width="0" height="0" aria-hidden="true">
        <filter id="foxLook" color-interpolation-filters="sRGB">
          <feColorMatrix type="matrix" values={lookMatrix} />
          <feComponentTransfer>
            <feFuncR type="table" tableValues={lookSplit.r} />
            <feFuncG type="table" tableValues={lookSplit.g} />
            <feFuncB type="table" tableValues={lookSplit.b} />
          </feComponentTransfer>
        </filter>
      </svg>
      {#if inspectorCollapsed}
        <button class="restoreTab restoreLook" onclick={() => (inspectorCollapsed = false)} title="Show Look panel">Look</button>
      {/if}
      {#if timelineCollapsed}
        <button class="restoreTab restoreTimeline" onclick={() => (timelineCollapsed = false)} title="Show timeline">Timeline</button>
      {/if}
      {#if productionPreview}
        <button class="previewExit" onclick={() => setOutputPreview(false)} title="Return to the edit workspace">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6"/><path d="M9 12h10"/></svg>
          Back to edit
        </button>
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
            <button class="miniBtn" onclick={() => setOutputPreview(false)}>Back to edit</button>
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
            onended={onNormalEnded}
            onerror={onPreviewError}
            onclick={togglePlay}
          ></video>
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
        <div class="emptyState emptyEdit">
          <strong>Bring clips in from the library</strong>
          <span>Drag them here (or onto a track, at a time), press <kbd>E</kbd> with clips selected there, or copy them there with <kbd>⌘C</kbd> and paste here with <kbd>⌘V</kbd>.</span>
          <span class="dim">In/out ranges you marked in Focus come in as separate segments.</span>
          {#if onsidebyside}<button class="miniBtn" onclick={onsidebyside}>Put the library beside this window</button>{/if}
        </div>
      {/if}
    </div>

    <div class="transport">
      <div class="tc" aria-label="Playhead time">
        <span class="tcNow">{fmtTC(playheadS)}</span>
        <span class="tcTotal">{fmtTC(videoEnd)}</span>
      </div>
      <div class="tBtns">
        <button class="tBtn" onclick={() => goToEdge(false)} disabled={!clips.length} title="Go to start (Home)" aria-label="Go to start">
          <svg viewBox="0 0 24 24"><path d="M6 5v14" /><path d="M18 6.5v11a.6.6 0 0 1-.92.5L9.6 12.5a.6.6 0 0 1 0-1l7.48-5.5a.6.6 0 0 1 .92.5z" class="fill" /></svg>
        </button>
        <button class="tBtn" onclick={() => stepFrame(-1)} disabled={!clips.length} title="Back one frame (←)" aria-label="Back one frame">
          <svg viewBox="0 0 24 24"><path d="m14.5 6-6 6 6 6" /></svg>
        </button>
        <button class="tPlay" class:on={playing} onclick={togglePlay} disabled={!clips.length} title={playing ? "Pause (Space)" : "Play (Space)"} aria-label={playing ? "Pause" : "Play"}>
          {#if playing}
            <svg viewBox="0 0 24 24"><rect x="7" y="5.5" width="3.6" height="13" rx="1" class="fill" /><rect x="13.4" y="5.5" width="3.6" height="13" rx="1" class="fill" /></svg>
          {:else}
            <svg viewBox="0 0 24 24"><path d="M8.5 6.2v11.6a.7.7 0 0 0 1.06.6l9.2-5.8a.7.7 0 0 0 0-1.2l-9.2-5.8a.7.7 0 0 0-1.06.6z" class="fill" /></svg>
          {/if}
        </button>
        <button class="tBtn" onclick={() => stepFrame(1)} disabled={!clips.length} title="Forward one frame (→)" aria-label="Forward one frame">
          <svg viewBox="0 0 24 24"><path d="m9.5 6 6 6-6 6" /></svg>
        </button>
        <button class="tBtn" onclick={() => goToEdge(true)} disabled={!clips.length} title="Go to end (End)" aria-label="Go to end">
          <svg viewBox="0 0 24 24"><path d="M18 5v14" /><path d="M6 6.5v11a.6.6 0 0 0 .92.5l7.48-5.5a.6.6 0 0 0 0-1L6.92 6a.6.6 0 0 0-.92.5z" class="fill" /></svg>
        </button>
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="progBar"
        class:off={!clips.length}
        onpointerdown={startProgramScrub}
        role="slider"
        tabindex="-1"
        aria-label="Scrub the edit"
        aria-valuemin={0}
        aria-valuemax={Math.round(videoEnd)}
        aria-valuenow={Math.round(playheadS)}
      >
        <span class="pTrack">
          {#each program as seg (seg.start)}
            {#if seg.clip && videoEnd > 0}
              <i class="pSeg" style="left:{(seg.start / videoEnd) * 100}%; width:{((seg.end - seg.start) / videoEnd) * 100}%"></i>
            {/if}
          {/each}
          <i class="pFill" style="width:{videoEnd > 0 ? (playheadS / videoEnd) * 100 : 0}%"></i>
        </span>
        <i class="pKnob" style="left:{videoEnd > 0 ? (playheadS / videoEnd) * 100 : 0}%"></i>
      </div>
    </div>

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="timelineResize" onpointerdown={startTimelineResize} role="separator" title="Resize timeline"></div>

    <section class="timeline" aria-label="Edit timeline">
      <div class="timelineHead">
        <div class="tlTitle">
          <strong>Timeline</strong>
          <span>{clips.length} video · {audioClips.length} audio</span>
        </div>
        <div class="tlTools">
          <button class="tool" onclick={cutAtPlayhead} disabled={!clips.length} title="Split at the playhead (C)">
            <svg viewBox="0 0 24 24"><circle cx="6.5" cy="6.5" r="2.5" /><circle cx="6.5" cy="17.5" r="2.5" /><path d="M8.6 8 20 18.5M8.6 16 20 5.5" /></svg>
            <span>Split</span>
          </button>
          <button class="tool" class:on={snapOn} onclick={() => (snapOn = !snapOn)} aria-pressed={snapOn} title={snapOn ? "Snapping on: clip edges catch on other edges and the playhead. Hold ⌥ while dragging to place freely." : "Snapping off"}>
            <svg viewBox="0 0 24 24"><path d="M6 4v7a6 6 0 0 0 12 0V4" /><path d="M6 4h3.5v7a2.5 2.5 0 0 0 5 0V4H18" /><path d="M6 8h3.5M14.5 8H18" /></svg>
            <span>Snap</span>
          </button>
          <button class="tool" onclick={pickAudio} title="Add a song or a sound to the audio tracks">
            <svg viewBox="0 0 24 24"><path d="M9 18V5.5l10-2V16" /><circle cx="6.5" cy="18" r="2.5" /><circle cx="16.5" cy="16" r="2.5" /></svg>
            <span>Music</span>
          </button>
        </div>
        <span class="spacer"></span>
        <div class="zoomCtl" title="Zoom (pinch, or ⌘/Ctrl + scroll on the timeline)">
          <button class="tool icon" onclick={() => zoomBy(0.8)} aria-label="Zoom out"><svg viewBox="0 0 24 24"><path d="M6 12h12" /></svg></button>
          <input type="range" min={timelineZoomMin} max={TIMELINE_ZOOM_MAX} step="0.1" bind:value={timelineScale} aria-label="Zoom" />
          <button class="tool icon" onclick={() => zoomBy(1.25)} aria-label="Zoom in"><svg viewBox="0 0 24 24"><path d="M6 12h12M12 6v12" /></svg></button>
          <button class="tool" onclick={fitTimeline} title="Fit the whole edit">Fit</button>
        </div>
        <button class="tool" onclick={clearTimeline} disabled={!clips.length && !audioClips.length} title="Remove everything from the timeline">
          <svg viewBox="0 0 24 24"><path d="M4 7h16M9 7V5h6v2M6.5 7l1 12.5h9l1-12.5" /></svg>
          <span>Clear</span>
        </button>
        <button class="tool icon" onclick={() => (timelineCollapsed = true)} title="Hide the timeline" aria-label="Hide the timeline">
          <svg viewBox="0 0 24 24"><path d="m7 10 5 5 5-5" /></svg>
        </button>
      </div>
      <div class="timelineViewport" bind:this={timelineViewportEl} onwheel={onTimelineWheel} onscroll={(e) => (tlScrollX = (e.currentTarget as HTMLElement).scrollLeft)}>
        <div class="timelineCanvas" style="width:{timelineWidth}px">
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="ruler"
            onpointerdown={startRulerScrub}
            role="slider"
            tabindex="-1"
            aria-label="Scrub playhead"
            aria-valuenow={Math.round(playheadS)}
            style="--minor:{rulerMinor * timelineScale}px"
          >
            {#each Array(Math.ceil(timelineEnd / rulerStep) + 1) as _, i (i)}
              <span style="left:{i * rulerStep * timelineScale}px">{fmt(i * rulerStep)}</span>
            {/each}
          </div>

          {#each VIDEO_LANES as lane (lane)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="track videoTrack" onpointerdown={onTrackClick} ondragover={allowDrop} ondrop={(e) => dropOnLane(e, "video", lane)}>
              {#each clips.filter((c) => c.lane === lane) as clip (clip.id)}
                {@const why = mismatchOf(clip.path)}
                {@const w = Math.max(42, clipLen(clip) * timelineScale)}
                <button
                  class="timelineClip video"
                  class:on={selectedIds.has(clip.id)}
                  class:gone={unavailable.has(clip.path)}
                  style="left:{clip.start * timelineScale}px; width:{w}px"
                  onclick={(e) => onClipClick(e, clip)}
                  oncontextmenu={(e) => openTimelineMenu(e, clip)}
                  onpointerdown={(e) => startTimelinePointer(e, "video", clip.id, "move")}
                  title={unavailable.has(clip.path) ? `${clip.path}\nNot available — is its drive plugged in?` : why ? `${clip.path}\nDiffers from the timeline: ${why}` : clip.path}
                >
                  <span class="thumbs" style={!strips[clip.path] && posters[clip.path] ? `background-image:url("${posters[clip.path]}")` : ""} aria-hidden="true">
                    {#each clipTiles(clip, w, CLIP_H) as t (t.x)}
                      <i style="left:{t.x}px; width:{t.w}px; {t.css}"></i>
                    {/each}
                  </span>
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span class="handle left" onpointerdown={(e) => startTimelinePointer(e, "video", clip.id, "trimIn")}></span>
                  <span class="clipLabel">
                    {#if why}<span class="mm" aria-label="Differs from the timeline">≠</span>{/if}
                    <strong>{clip.name}</strong>
                    <em>{fmt(clipLen(clip))}</em>
                  </span>
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span class="handle right" onpointerdown={(e) => startTimelinePointer(e, "video", clip.id, "trimOut")}></span>
                </button>
              {/each}
            </div>
          {/each}

          <div class="laneGap" aria-hidden="true"></div>

          {#each AUDIO_LANES as lane (lane)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="track audioTrack" onpointerdown={onTrackClick} ondragover={allowDrop} ondrop={(e) => dropOnLane(e, "audio", lane)}>
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
                  <span class="clipLabel">
                    <svg class="note" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 18V5.5l10-2V16" /><circle cx="6.5" cy="18" r="2.5" /><circle cx="16.5" cy="16" r="2.5" /></svg>
                    <strong>{clip.name}</strong>
                    <em>{fmt(clip.duration)}</em>
                  </span>
                </button>
              {/each}
            </div>
          {/each}

          <!-- Track names: a column that stays at the left edge while the
               timeline scrolls sideways (clips slide under it). -->
          <div class="trackHeads" style="transform:translateX({tlScrollX}px)" aria-hidden="true">
            <div class="thCorner"></div>
            {#each VIDEO_LANES as lane (lane)}
              <div class="th video"><span>V{lane + 1}</span></div>
            {/each}
            <div class="thGap"></div>
            {#each AUDIO_LANES as lane (lane)}
              <div class="th audio"><span>A{lane + 1}</span></div>
            {/each}
          </div>

          {#if clips.length}
            <div class="playhead" style="left:{TIMELINE_TRACK_OFFSET + playheadS * timelineScale}px"></div>
          {/if}
          {#if snapGuide != null}
            <div class="snapGuide" style="left:{TIMELINE_TRACK_OFFSET + snapGuide * timelineScale}px"></div>
          {/if}
        </div>
      </div>
    </section>
  </section>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="panelSplitter inspectorSplitter" onpointerdown={startInspectorResize} role="separator" title="Resize look panel"></div>

  <aside class="inspector">
    <div class="inspHead">
      <div class="inspTabs" role="tablist" aria-label="Panel">
        <button role="tab" aria-selected={inspTab === "look"} class:on={inspTab === "look"} onclick={() => (inspTab = "look")}>Look</button>
        <button role="tab" aria-selected={inspTab === "clip"} class:on={inspTab === "clip"} onclick={() => (inspTab = "clip")}>Clip</button>
      </div>
      <button class="iconBtn sm" onclick={() => (inspectorCollapsed = true)} title="Hide this panel" aria-label="Hide this panel">
        <svg viewBox="0 0 24 24"><path d="m10 7 5 5-5 5" /></svg>
      </button>
    </div>

    {#if inspTab === "look"}
      <div class="inspBody">
        <div class="secHead">
          <span>Presets</span>
          <button class="linkBtn" onclick={resetColor} title="Clear the look: back to the untouched picture" disabled={neutralLook}>Reset</button>
        </div>
        <div class="lookChips" role="radiogroup" aria-label="Preset group">
          <button class:on={lookFilterGroup === "all"} role="radio" aria-checked={lookFilterGroup === "all"} onclick={() => (lookFilterGroup = "all")}>All</button>
          {#each LOOK_GROUPS as g (g.id)}
            <button class:on={lookFilterGroup === g.id} role="radio" aria-checked={lookFilterGroup === g.id} onclick={() => (lookFilterGroup = g.id)}>
              {g.label}{#if g.presets.some((id) => id === activeLook)}<i class="chipDot"></i>{/if}
            </button>
          {/each}
        </div>
        <div class="lookGrid">
          {#each LOOK_GROUPS.filter((g) => lookFilterGroup === "all" || g.id === lookFilterGroup).flatMap((g) => g.presets) as id (id)}
            {@const look = LOOK_PRESETS[id]}
            <button class="lookTile" class:active={activeLook === id} onclick={() => applyLook(id)} title={look.hint}>
              <span
                class="lookImg"
                class:fallback={!lookThumb}
                style={`${lookThumb ? `background-image:url("${lookThumb}");` : ""}filter:${lookFilter(look.values)}`}
              ></span>
              <span class="lookName">{look.label}</span>
            </button>
          {/each}
        </div>
        {#if activeLook}
          <label class="slider">
            <span class="sLabel">Intensity <em>{Math.round(lookIntensity * 100)}%</em></span>
            <input
              type="range"
              min="0"
              max="1.5"
              step="0.01"
              value={lookIntensity}
              oninput={(e) => setLookIntensity(Number((e.currentTarget as HTMLInputElement).value))}
              ondblclick={() => setLookIntensity(1)}
            />
          </label>
        {/if}

        <div class="secHead adjust">
          <span>Adjust</span>
          <button class="linkBtn" onclick={resetColor} title="Reset every adjustment" disabled={neutralLook}>Reset all</button>
        </div>
        <div class="sliders" title="Double-click a slider to reset just that one">
          <label class="slider"><span class="sLabel">Brightness <em>{adjReadout("brightness")}</em></span><input type="range" min="-0.5" max="0.5" step="0.01" bind:value={adjustments.brightness} oninput={detachLook} ondblclick={() => resetAdj("brightness")} /></label>
          <label class="slider"><span class="sLabel">Contrast <em>{adjReadout("contrast")}</em></span><input type="range" min="0.5" max="1.8" step="0.01" bind:value={adjustments.contrast} oninput={detachLook} ondblclick={() => resetAdj("contrast")} /></label>
          <label class="slider"><span class="sLabel">Saturation <em>{adjReadout("saturation")}</em></span><input type="range" min="0" max="2" step="0.01" bind:value={adjustments.saturation} oninput={detachLook} ondblclick={() => resetAdj("saturation")} /></label>
          <label class="slider"><span class="sLabel">Warmth <em>{adjReadout("warmth")}</em></span><input type="range" min="-0.5" max="0.5" step="0.01" bind:value={adjustments.warmth} oninput={detachLook} ondblclick={() => resetAdj("warmth")} /></label>
          <label class="slider"><span class="sLabel">Split tone <em>{adjReadout("splitTone")}</em></span><input type="range" min="0" max="1" step="0.01" bind:value={adjustments.splitTone} oninput={detachLook} ondblclick={() => resetAdj("splitTone")} /></label>
          <label class="slider"><span class="sLabel">Sharpen <em>{adjReadout("sharpen")}</em></span><input type="range" min="0" max="1" step="0.01" bind:value={adjustments.sharpen} oninput={detachLook} ondblclick={() => resetAdj("sharpen")} /></label>
        </div>
        <p class="hint">Double-click a slider to reset just that one. The look applies to the whole edit.</p>
      </div>
    {:else}
      <div class="inspBody">
        {#if selectedClip}
          <div class="clipCard">
            <span class="ccThumb" style={posters[selectedClip.path] ? `background-image:url("${posters[selectedClip.path]}")` : ""}></span>
            <span class="ccText">
              <strong title={selectedClip.path}>{selectedClip.name}</strong>
              <span>V{selectedClip.lane + 1} · {fmt(selectedClip.outS - selectedClip.inS)} of {fmt(selectedClip.duration)}</span>
            </span>
          </div>

          <div class="secHead"><span>Trim</span></div>
          <div class="trimGrid">
            <label class="field">
              <span>In</span>
              <input type="number" min="0" max={selectedClip.outS} step="0.01" value={selectedClip.inS.toFixed(2)} oninput={(e) => updateSelectedClip({ inS: Number((e.currentTarget as HTMLInputElement).value) })} onchange={clampTrim} />
            </label>
            <label class="field">
              <span>Out</span>
              <input type="number" min={selectedClip.inS} max={selectedClip.duration} step="0.01" value={selectedClip.outS.toFixed(2)} oninput={(e) => updateSelectedClip({ outS: Number((e.currentTarget as HTMLInputElement).value) })} onchange={clampTrim} />
            </label>
            <button class="softBtn" onclick={setIn} title="In point at the playhead ([)">Set in <kbd>[</kbd></button>
            <button class="softBtn" onclick={setOut} title="Out point at the playhead (])">Set out <kbd>]</kbd></button>
          </div>

          {#if outPreset.fit !== "original"}
            <div class="secHead"><span>Framing</span></div>
            <p class="hint">Drag the frame on the picture to move it; ⌘/Ctrl + scroll zooms.</p>
            <div class="sliders">
              <label class="slider"><span class="sLabel">Left / right <em>{Math.round(selectedClip.cropX * 100)}%</em></span><input type="range" min="0" max="1" step="0.001" value={selectedClip.cropX} oninput={(e) => updateSelectedClip({ cropX: Number((e.currentTarget as HTMLInputElement).value) })} ondblclick={() => updateSelectedClip({ cropX: 0.5 })} /></label>
              <label class="slider"><span class="sLabel">Up / down <em>{Math.round(selectedClip.cropY * 100)}%</em></span><input type="range" min="0" max="1" step="0.001" value={selectedClip.cropY} oninput={(e) => updateSelectedClip({ cropY: Number((e.currentTarget as HTMLInputElement).value) })} ondblclick={() => updateSelectedClip({ cropY: 0.5 })} /></label>
              <label class="slider"><span class="sLabel">Zoom <em>{selectedClip.zoom.toFixed(2)}×</em></span><input type="range" min="1" max="4" step="0.01" value={selectedClip.zoom} oninput={(e) => updateSelectedClip({ zoom: Number((e.currentTarget as HTMLInputElement).value) })} ondblclick={() => updateSelectedClip({ zoom: 1 })} /></label>
            </div>
          {/if}

          <div class="secHead"><span>Source → output</span></div>
          <dl class="facts">
            <div><dt>Resolution</dt><dd>{probes[selectedClip.path]?.width ?? "–"}×{probes[selectedClip.path]?.height ?? "–"}</dd></div>
            <div><dt>Frame rate</dt><dd>{probes[selectedClip.path]?.fps ? `${Math.round(probes[selectedClip.path]?.fps ?? 0)} fps` : "–"}</dd></div>
            <div><dt>Codec</dt><dd>{probes[selectedClip.path]?.codec ?? "–"}{probes[selectedClip.path]?.hdr ? " · HDR" : ""}</dd></div>
            <div><dt>Output</dt><dd>{outPreset.fit === "original" ? "Original" : `${outPreset.w}×${outPreset.h}`}</dd></div>
          </dl>

          <button class="softBtn danger wide" onclick={() => removeClip(selectedClip.id)}>Remove from the timeline</button>
        {:else if selectedAudio}
          <div class="clipCard audio">
            <span class="ccThumb note"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 18V5.5l10-2V16" /><circle cx="6.5" cy="18" r="2.5" /><circle cx="16.5" cy="16" r="2.5" /></svg></span>
            <span class="ccText">
              <strong title={selectedAudio.path}>{selectedAudio.name}</strong>
              <span>A{selectedAudio.lane + 1} · {fmt(selectedAudio.duration)}</span>
            </span>
          </div>
          <label class="checkRow"><input type="checkbox" bind:checked={preserveSourceAudio} disabled={audioClips.length > 0} /> Keep the clips' own sound</label>
          <button class="softBtn danger wide" onclick={() => removeAudio(selectedAudio.id)}>Remove from the timeline</button>
        {:else}
          <div class="emptyClip">
            <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="3" /><path d="M3 15h18M8 15v4M13 15v4" /></svg>
            <strong>No clip selected</strong>
            <span>Click a clip on the timeline to trim it, frame it or see what it is.</span>
          </div>
        {/if}
      </div>
    {/if}

    {#if exportNote}<p class="note sideNote">{exportNote}</p>{/if}
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

        <p class="igAspectLine">
          {#if outPreset.fit === "original"}Aspect: Original (decided in the edit screen){:else}Aspect: from your edit — <strong>{outPreset.label} · {outPreset.detail}</strong>{/if}
        </p>
        {#if dlgMode === "instagram"}
          <p class="igDisclaim">Instagram-ready settings are pre-filled — everything on the right stays editable. Tip: in the Instagram app, turn on <strong>Settings → Data usage &amp; media quality → Upload at highest quality</strong>, or the app recompresses this on your phone.</p>
        {:else if dlgMode === "lossless"}
          <p class="igDisclaim">
            {#if !willRender}Trim only — stream-copied, no re-encode, no quality loss.{:else if mixedSources}Your clips differ in resolution or codec, so they're joined with a best-quality re-encode (a straight stream copy would produce a broken file).{:else}A crop/adjustment/fps change needs a render, so this re-encodes at best quality.{/if}
          </p>
        {/if}

        <!-- Aligned Source → Output comparison. Each row is one property so the
             current value and the target sit on the same line (was two loosely
             related lists). The Output column stays editable. -->
        <div class="igGrid">
          <div class="igGridHead">
            <span>Property</span>
            <span>Source</span>
            <span>Output</span>
          </div>

          <div class="igGridRow">
            <span class="k">Resolution</span>
            <span class="s">
              {igSourceProbe?.width ?? "?"}×{igSourceProbe?.height ?? "?"}
              {#if igCrop && outPreset.fit !== "original"}<em>crop ≈ {Math.round(igCrop.cropW)}×{Math.round(igCrop.cropH)} px</em>{/if}
            </span>
            <span class="o">
              <select bind:value={dlgRes} disabled={resOptions.length < 2}>
                {#each resOptions as o (o.id)}
                  <option value={o.id}>{o.label}</option>
                {/each}
              </select>
              {#if igCrop && dlgOut.w > 0}
                {#if Math.round(igCrop.cropW) > dlgOut.w + 1}<em class="ok">downscaled — crisp</em>
                {:else if Math.round(igCrop.cropW) < dlgOut.w - 1}<em class="soft">upscaled — slightly soft</em>{/if}
              {/if}
            </span>
          </div>

          <div class="igGridRow">
            <span class="k">Frame rate</span>
            <span class="s">{srcFps ? `${srcFps} fps` : "source fps"}</span>
            <span class="o">
              <select bind:value={dlgFps}>
                {#each fpsOptions as o (o.id)}
                  <option value={o.id}>{o.label}</option>
                {/each}
              </select>
            </span>
          </div>

          <div class="igGridRow">
            <span class="k">Quality</span>
            <span class="s">as recorded</span>
            <span class="o">
              <select bind:value={quality}>
                <option value="best">Best — CRF 16</option>
                <option value="high">High — CRF 18</option>
                <option value="standard">Standard — CRF 20</option>
                <option value="small">Small — CRF 23</option>
              </select>
              <em class="dim">{qualityNote}</em>
            </span>
          </div>

          <div class="igGridRow">
            <span class="k">Format</span>
            <span class="s">{igSourceProbe?.codec ?? "source codec"}</span>
            <span class="o">
              {#if !willRender}stream copy · untouched{:else if keepHdr && igSourceProbe?.hdr && dlgMode === "instagram"}HEVC 10-bit · MP4{:else}H.264 · MP4 · faststart{/if}
            </span>
          </div>

          {#if igSourceProbe?.hdr}
            <div class="igGridRow">
              <span class="k">Dynamic range</span>
              <span class="s warn">HDR (HLG/PQ)</span>
              <span class="o">{keepHdr && dlgMode === "instagram" ? "HDR (HLG kept)" : "SDR (tone-mapped)"}</span>
            </div>
          {/if}

          <div class="igGridRow">
            <span class="k">Length</span>
            <span class="s">{fmt(programSeconds)} · {programClips.length} clip{programClips.length === 1 ? "" : "s"}</span>
            <span class="o">unchanged{#if mixedSources} · <em class="warn">mixed sources conformed</em>{/if}</span>
          </div>
        </div>

        {#if igSourceProbe?.hdr && dlgMode === "instagram"}
          <div class="dlgHdr">
            <span class="igColHead">HDR handling</span>
            <label><input type="radio" name="hdr" checked={!keepHdr} onchange={() => (keepHdr = false)} /> Convert to SDR <em>— recommended, looks consistent on every device</em></label>
            <label><input type="radio" name="hdr" checked={keepHdr} onchange={() => (keepHdr = true)} /> Keep HDR (HLG) <em>— punchier on modern phones; the SDR fallback others see may look flat</em></label>
          </div>
        {/if}
        {#if softCrop}
          <p class="dlgWarn">⚠ Cropping this {igSourceProbe?.width}×{igSourceProbe?.height} clip to vertical upscales past its pixels, so it'll be slightly soft. FoxCull adds a light sharpen; for crisp crops shoot higher-res (2.7K/4K) when you plan to crop.</p>
        {/if}

        <div class="dlgTime">
          <div class="dlgTimeHead">
            <strong><span class="effortDot {effort}"></span>Estimated time ~{fmt(igEstimateSecs)} <em class="effortWord">({effortLabel})</em></strong>
            <span class="dlgInfo" title="Rule-of-thumb breakdown of what drives the time">ⓘ what drives this</span>
          </div>
          {#if willRender && exportCost.steps.length}
            <!-- Stacked bar: each segment's WIDTH is that step's share of the time,
                 so the widest segment is the most taxing operation at a glance. -->
            <div class="costBar" role="img" aria-label="Estimated time split by step">
              {#each exportCost.steps as s (s.label)}
                <span class="costSeg" style="width:{s.pct}%; background:{s.color}" title="{s.label} — ~{s.secs}s ({Math.round(s.pct)}%)"></span>
              {/each}
            </div>
            <ul class="costLegend">
              {#each exportCost.steps as s (s.label)}
                <li>
                  <span class="costKey" style="background:{s.color}"></span>
                  <span class="costName">{s.label}</span>
                  <em>~{s.secs}s · {Math.round(s.pct)}%</em>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="costNone">{exportSteps[0]?.label ?? "Trim (stream copy)"} — {exportSteps[0]?.note ?? "instant, no re-encode"}</p>
          {/if}
          <p class="dlgRerender">{willRender ? "Re-render required" : "No re-render — bit-exact stream copy"}</p>
        </div>

        <div class="dlgName">
          <label class="dlgSet">File name
            <span class="dlgNameRow">
              <input type="text" bind:value={dlgName} spellcheck="false" placeholder={exportName()} />
              <span class="dlgNameExt">.{dlgExt}</span>
            </span>
          </label>
          <p class="dlgLoc">
            Saves next to the source{#if dlgNameTaken} — <em class="taken">name taken, will save as “{dlgName.trim() || exportName()} (2)”</em>{/if}
          </p>
          <p class="dlgNameTip">Rename freely — but keeping the original clip name plus a suffix (e.g. DJI_0679_myedit) keeps the export stacked with its source in the library grid.</p>
        </div>

        <div class="dlgOther">
          <span class="igColHead">Other settings</span>
          <label class="dlgField">Encoder
            <select bind:value={encoder}>
              <option value="auto">Auto (hardware when available)</option>
              <option value="x264">x264 (software)</option>
              <option value="nvenc">NVIDIA NVENC</option>
            </select>
          </label>
          <label class="check"><input type="checkbox" bind:checked={preserveSourceAudio} disabled={audioClips.length > 0} /> Keep source audio</label>
          <div class="music">
            <button class="miniBtn" onclick={() => void pickAudio()}>Choose music…</button>
            {#if audioClips.length}<span class="small">{audioClips[0].name}</span>{/if}
          </div>
        </div>

        <div class="igActions">
          <button class="miniBtn" onclick={() => (exportDlg = false)}>Cancel</button>
          <button class="exportBtn" onclick={runDialogExport} disabled={exporting || !programClips.length}>{exporting ? "Exporting…" : "Export"}</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  /* Edit studio, redesigned 2026-10-04: one quiet chrome (the app's tokens),
     a dark stage for the picture, icon tools with words where they help,
     clips drawn with their own frames. The export dialog's styles are kept as
     they were (further down). */

  /* ── shell ───────────────────────────────────────────────────────────── */
  .editShell {
    --stage: #07080a;
    width: 100%;
    height: 100%;
    display: grid;
    grid-template-columns:
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
  .productionPreviewMode.inspectorCollapsed {
    grid-template-columns: minmax(0, 1fr) 0 0;
    background: #000;
  }
  .workPane {
    grid-column: 1;
    min-width: 0;
    min-height: 0;
    display: grid;
    /* max-content for the top bar (it wraps on narrow panes and must keep its
       height); when height is short the timeline yields first (down to
       120px), then the preview's 180px floor. */
    grid-template-rows:
      max-content
      minmax(180px, 1fr)
      auto
      6px
      minmax(min(120px, var(--timeline-h, 300px)), var(--timeline-h, 300px));
    position: relative;
    overflow: visible;
    z-index: 2;
    container-type: inline-size;
  }
  .timelineCollapsed .workPane {
    grid-template-rows: max-content minmax(180px, 1fr) auto 0 0;
  }
  .productionPreviewMode .workPane {
    grid-template-rows: minmax(0, 1fr);
  }
  .productionPreviewMode .editTop,
  .productionPreviewMode .transport,
  .productionPreviewMode .timelineResize,
  .productionPreviewMode .timeline,
  .productionPreviewMode .panelSplitter,
  .productionPreviewMode .inspector {
    display: none;
  }

  /* Splitters: a hairline that thickens into the accent under the pointer. */
  .panelSplitter,
  .timelineResize {
    position: relative;
    background: transparent;
  }
  .panelSplitter {
    grid-row: 1;
    grid-column: 2;
    min-width: 6px;
    cursor: col-resize;
  }
  .timelineResize {
    min-height: 6px;
    cursor: row-resize;
  }
  .panelSplitter::after,
  .timelineResize::after {
    content: "";
    position: absolute;
    background: var(--border-soft);
    transition: background 120ms ease;
  }
  .panelSplitter::after { top: 0; bottom: 0; left: 2.5px; width: 1px; }
  .timelineResize::after { left: 0; right: 0; top: 2.5px; height: 1px; }
  .panelSplitter:hover::after,
  .panelSplitter:active::after { left: 1.5px; width: 3px; background: var(--accent); }
  .timelineResize:hover::after,
  .timelineResize:active::after { top: 1.5px; height: 3px; background: var(--accent); }
  .inspectorCollapsed .inspectorSplitter,
  .timelineCollapsed .timelineResize {
    display: none;
  }

  /* ── top bar ─────────────────────────────────────────────────────────── */
  .editTop {
    position: relative;
    z-index: 6;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    min-height: 54px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-soft);
    background: var(--bg-panel);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
    margin-right: 4px;
  }
  .brandIco {
    display: grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 9px;
    color: #fff;
    background: linear-gradient(160deg, #f0717f, #d94a5d);
    box-shadow: 0 2px 8px rgba(217, 74, 93, 0.3);
  }
  .brandIco svg { width: 17px; height: 17px; fill: none; stroke: currentColor; stroke-width: 1.9; stroke-linecap: round; stroke-linejoin: round; }
  .brandText { display: flex; flex-direction: column; min-width: 0; line-height: 1.2; }
  .brandText strong { font-family: var(--font-display); font-size: 14px; font-weight: 650; letter-spacing: -0.01em; }
  .brandText span { font-size: 11.5px; color: var(--text-faint); white-space: nowrap; font-variant-numeric: tabular-nums; }

  .aspects {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: 10px;
    border: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg) 75%, transparent);
  }
  .aspects button {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 11px;
    border-radius: 8px;
    color: var(--text-dim);
    font-size: 12px;
    font-weight: 560;
    white-space: nowrap;
  }
  .aspects button:hover:not(.on) { color: var(--text); background: color-mix(in srgb, var(--bg-hover) 60%, transparent); }
  .aspects button.on {
    color: var(--text);
    background: var(--bg-elev);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25), 0 0 0 1px var(--border-soft);
  }
  .ar { display: block; flex: none; border: 1.5px solid currentColor; border-radius: 2.5px; opacity: 0.75; }
  .ar.free { border-style: dashed; }
  .aspects button.on .ar { border-color: var(--accent); opacity: 1; }
  .topGap { flex: 1 1 auto; min-width: 8px; }

  .viewTools { display: inline-flex; gap: 2px; }
  .iconBtn {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    color: var(--text-dim);
  }
  .iconBtn.sm { width: 28px; height: 28px; }
  .iconBtn:hover { color: var(--text); background: var(--bg-hover); }
  .iconBtn.on { color: var(--accent); }
  .iconBtn svg,
  .pillBtn svg,
  .tool svg,
  .exportBtn svg {
    width: 17px;
    height: 17px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .pillBtn {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 32px;
    padding: 0 12px;
    border-radius: 9px;
    border: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg-elev) 80%, transparent);
    color: var(--text);
    font-size: 12.5px;
    font-weight: 560;
  }
  .pillBtn:hover:not(:disabled) { border-color: var(--border-strong); background: var(--bg-hover); }
  .pillBtn.on { border-color: var(--accent); color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, var(--bg-elev)); }
  .topToast {
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 6px 11px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--pick) 45%, transparent);
    background: color-mix(in srgb, var(--pick) 14%, var(--bg-elev));
    color: var(--text);
    font-size: 12px;
  }

  .exportOpts { position: relative; flex: none; }
  .exportGroup { display: inline-flex; }
  .exportBtn {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 32px;
    padding: 0 14px;
    border-radius: 9px;
    background: var(--accent);
    color: var(--accent-on);
    font-size: 12.5px;
    font-weight: 650;
    white-space: nowrap;
  }
  .exportBtn:hover:not(:disabled) { background: var(--accent-hover); }
  .exportBtn.main {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
    box-shadow: 0 4px 14px color-mix(in srgb, var(--accent) 26%, transparent);
  }
  .exportBtn.caret {
    padding: 0 8px;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
    border-left: 1px solid color-mix(in srgb, var(--accent-on) 25%, transparent);
  }
  .exportBtn.caret.on { background: color-mix(in srgb, var(--accent) 82%, #000); }
  .reDot { width: 6px; height: 6px; border-radius: 50%; background: var(--accent-on); opacity: 0.85; }
  /* Exporting: the split button gives way to a progress pill + two-step cancel. */
  .exportProgress { display: flex; align-items: center; gap: 8px; }
  .epBar {
    position: relative;
    width: 130px;
    height: 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text-faint) 22%, transparent);
    overflow: hidden;
  }
  .epBar span { position: absolute; inset: 0 auto 0 0; border-radius: 999px; background: var(--accent); transition: width 0.25s ease; }
  .epPct { min-width: 34px; color: var(--text-dim); font-size: 12px; font-variant-numeric: tabular-nums; text-align: right; }
  .epCancel.armed { color: var(--reject); border-color: color-mix(in srgb, var(--reject) 60%, var(--border)); }
  .exportMenu {
    position: absolute;
    right: 0;
    top: 40px;
    z-index: 240;
    width: 270px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 3px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--bg-elev) 96%, transparent);
    box-shadow: var(--shadow);
    backdrop-filter: blur(20px);
  }
  .exportChoice {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    color: var(--text);
    text-align: left;
  }
  .exportChoice:hover:not(:disabled) { background: var(--bg-hover); }
  .exportChoice strong { font-size: 12.5px; font-weight: 650; }
  .exportChoice span { font-size: 11px; color: var(--text-faint); }
  .exportChoice.sub { color: var(--text-dim); font-size: 12px; }
  .menuSep { height: 1px; margin: 3px 4px; background: var(--border-soft); }

  /* Plain buttons still used by the export dialog and the empty state. */
  .miniBtn {
    padding: 5px 10px;
    border: 1px solid var(--border-soft);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bg-elev) 82%, transparent);
    color: var(--text);
    font-size: 12px;
    white-space: nowrap;
  }
  .miniBtn:hover:not(:disabled) { border-color: var(--border-strong); background: var(--bg-hover); }
  .miniBtn.on { border-color: var(--accent); color: var(--accent); }
  .small { color: var(--text-faint); font-size: 12px; }

  /* ── the stage (preview) ─────────────────────────────────────────────── */
  .preview {
    position: relative;
    z-index: 0;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    background: radial-gradient(ellipse at center, #14171c 0%, var(--stage) 72%);
  }
  .preview video { width: 100%; height: 100%; object-fit: contain; }
  .productionPreviewMode .preview { grid-row: 1; background: #000; }
  .lookFilterDefs { position: absolute; width: 0; height: 0; pointer-events: none; }
  .restoreTab {
    position: absolute;
    z-index: 75;
    padding: 7px 12px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    background: rgba(24, 28, 34, 0.82);
    color: #f3f5f7;
    font-size: 12px;
    font-weight: 600;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.35);
    backdrop-filter: blur(14px);
  }
  .restoreTab:hover { background: rgba(36, 42, 50, 0.92); }
  .restoreLook { right: 12px; top: 12px; }
  .restoreTimeline { left: 50%; bottom: 12px; transform: translateX(-50%); }
  .previewExit {
    position: absolute;
    z-index: 100;
    left: 18px;
    top: 18px;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-height: 34px;
    padding: 7px 12px;
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 999px;
    background: rgba(20, 24, 29, 0.86);
    color: #f6f7f8;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.34);
    backdrop-filter: blur(12px);
    font-size: 12px;
    font-weight: 650;
  }
  .previewExit:hover { background: rgba(34, 40, 47, 0.94); border-color: rgba(255, 255, 255, 0.34); }
  .previewExit svg { width: 15px; height: 15px; }
  .productionFrame { position: absolute; overflow: hidden; background: #000; box-shadow: 0 18px 70px rgba(0, 0, 0, 0.5); }
  .preview video.productionVideo { position: absolute; max-width: none; max-height: none; object-fit: fill; }
  .productionControls {
    position: absolute;
    left: 50%;
    bottom: 18px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 10px;
    width: min(760px, calc(100% - 48px));
    padding: 9px 12px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 14px;
    background: rgba(20, 24, 29, 0.84);
    color: #f3f5f7;
    box-shadow: var(--shadow);
    backdrop-filter: blur(16px);
  }
  .productionControls input { flex: 1; accent-color: var(--accent); }
  .productionControls .play { padding: 5px 12px; border-radius: 8px; background: rgba(255, 255, 255, 0.12); color: inherit; font-size: 12px; font-weight: 600; }
  .productionControls .time { font-size: 12px; color: rgba(255, 255, 255, 0.7); font-variant-numeric: tabular-nums; }
  .previewBusy,
  .trimCaption {
    position: absolute;
    left: 12px;
    padding: 6px 11px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    background: rgba(20, 24, 29, 0.82);
    color: #f3f5f7;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    backdrop-filter: blur(12px);
  }
  .previewBusy { bottom: 12px; }
  .trimCaption { top: 12px; }
  .cropFrame {
    position: absolute;
    padding: 0;
    border: 2px solid rgba(255, 255, 255, 0.95);
    border-radius: 2px;
    background: transparent;
    box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.45), 0 6px 26px rgba(0, 0, 0, 0.45);
    cursor: move;
  }
  .cropFrame span {
    position: absolute;
    inset: 33.333% 0;
    border-top: 1px solid rgba(255, 255, 255, 0.4);
    border-bottom: 1px solid rgba(255, 255, 255, 0.4);
  }
  .cropFrame::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 33.333%;
    width: 33.333%;
    border-left: 1px solid rgba(255, 255, 255, 0.4);
    border-right: 1px solid rgba(255, 255, 255, 0.4);
  }
  .emptyState {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    min-height: 120px;
    color: var(--text-faint);
    font-size: 13px;
  }
  .emptyEdit {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    max-width: 440px;
    height: auto;
    margin: auto;
    padding: 28px 26px;
    border: 1px dashed color-mix(in srgb, var(--text-faint) 34%, transparent);
    border-radius: var(--radius-lg);
    text-align: center;
    line-height: 1.5;
  }
  .emptyEdit strong { font-size: 15px; color: var(--text); }
  .emptyEdit .dim { color: var(--text-faint); font-size: 12px; }
  .emptyEdit kbd {
    padding: 0 5px;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 4px;
    font-size: 11px;
  }

  /* ── transport ───────────────────────────────────────────────────────── */
  .transport {
    display: grid;
    grid-template-columns: auto auto minmax(0, 1fr);
    align-items: center;
    gap: 14px;
    min-height: 54px;
    padding: 8px 18px 8px 16px;
    border-top: 1px solid var(--border-soft);
    background: var(--bg-panel);
  }
  .tc { display: flex; align-items: baseline; gap: 6px; min-width: 136px; font-variant-numeric: tabular-nums; }
  .tcNow { font-size: 17px; font-weight: 620; letter-spacing: -0.01em; color: var(--text); }
  .tcTotal { font-size: 12px; color: var(--text-faint); }
  .tcTotal::before { content: "/ "; }
  .tBtns { display: inline-flex; align-items: center; gap: 2px; }
  .tBtn {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 8px;
    color: var(--text-dim);
  }
  .tBtn:hover:not(:disabled) { color: var(--text); background: var(--bg-hover); }
  .tBtn svg,
  .tPlay svg { width: 17px; height: 17px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .tBtn svg .fill,
  .tPlay svg .fill { fill: currentColor; stroke: none; }
  .tPlay {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    margin: 0 6px;
    border-radius: 50%;
    background: var(--text);
    color: var(--bg);
    box-shadow: 0 3px 12px rgba(0, 0, 0, 0.3);
    transition: transform 90ms ease, background 120ms ease;
  }
  .tPlay:hover:not(:disabled) { transform: scale(1.06); }
  .tPlay:active:not(:disabled) { transform: scale(0.97); }
  .tPlay svg { width: 18px; height: 18px; }
  .progBar { position: relative; display: flex; align-items: center; height: 30px; cursor: pointer; touch-action: none; }
  .progBar.off { cursor: default; opacity: 0.45; }
  .pTrack {
    position: relative;
    flex: 1;
    height: 6px;
    border-radius: 999px;
    overflow: hidden;
    background: color-mix(in srgb, var(--text-faint) 16%, transparent);
    transition: height 120ms ease;
  }
  .progBar:hover .pTrack { height: 8px; }
  .pSeg {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, var(--text-faint) 30%, transparent);
    box-shadow: inset -1px 0 var(--bg-panel);
  }
  .pFill { position: absolute; left: 0; top: 0; bottom: 0; background: var(--accent); }
  .pKnob {
    position: absolute;
    top: 50%;
    width: 14px;
    height: 14px;
    margin: -7px 0 0 -7px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.45), 0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent);
    pointer-events: none;
  }

  /* ── timeline ────────────────────────────────────────────────────────── */
  .timeline {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
  }
  .timelineCollapsed .timeline { overflow: hidden; }
  .timelineCollapsed .timeline > * { display: none; }
  .timelineHead {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    min-height: 46px;
    padding: 6px 10px 6px 14px;
    border-bottom: 1px solid var(--border-soft);
  }
  .tlTitle { display: flex; align-items: baseline; gap: 8px; margin-right: 6px; }
  .tlTitle strong { font-size: 13px; font-weight: 650; }
  .tlTitle span { font-size: 11.5px; color: var(--text-faint); white-space: nowrap; }
  .tlTools { display: inline-flex; gap: 2px; padding-left: 8px; border-left: 1px solid var(--border-soft); }
  .tool {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 9px;
    border-radius: 8px;
    color: var(--text-dim);
    font-size: 12px;
    font-weight: 560;
    white-space: nowrap;
  }
  .tool svg { width: 16px; height: 16px; }
  .tool:hover:not(:disabled) { color: var(--text); background: var(--bg-hover); }
  .tool.on { color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .tool.icon { width: 30px; padding: 0; justify-content: center; }
  .zoomCtl { display: inline-flex; align-items: center; gap: 2px; margin-right: 4px; }
  .zoomCtl input[type="range"] { width: 110px; min-width: 0; accent-color: var(--accent); }
  .spacer { flex: 1 1 auto; min-width: 8px; }
  .snapGuide {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 0;
    border-left: 1px dashed color-mix(in srgb, var(--star) 85%, transparent);
    pointer-events: none;
    z-index: 6;
  }
  .timelineViewport {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
    background: color-mix(in srgb, var(--bg) 90%, black 10%);
  }
  .timelineCanvas { position: relative; min-height: 100%; padding-top: 26px; }
  .ruler {
    position: absolute;
    top: 0;
    left: 44px;
    right: 0;
    height: 26px;
    z-index: 3;
    cursor: pointer;
    border-bottom: 1px solid var(--border-soft);
    background-color: var(--bg-panel);
    background-image: repeating-linear-gradient(90deg, color-mix(in srgb, var(--text-faint) 45%, transparent) 0 1px, transparent 1px var(--minor, 10px));
    background-size: 100% 5px;
    background-repeat: repeat-x;
    background-position: 0 100%;
  }
  .ruler span {
    position: absolute;
    top: 0;
    height: 26px;
    padding: 4px 0 0 4px;
    border-left: 1px solid color-mix(in srgb, var(--text-faint) 65%, transparent);
    color: var(--text-faint);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }
  .track { position: relative; margin-left: 44px; border-bottom: 1px solid var(--border-soft); }
  .videoTrack { height: 46px; }
  .audioTrack { height: 34px; background: color-mix(in srgb, var(--pick) 3%, transparent); }
  .track:hover { background-color: color-mix(in srgb, var(--bg-hover) 28%, transparent); }
  .laneGap { height: 8px; margin-left: 44px; border-bottom: 1px solid var(--border-soft); }
  /* Track names: pinned to the left edge (moved with the scroll position). */
  .trackHeads {
    position: absolute;
    top: 0;
    left: 0;
    bottom: 0;
    width: 44px;
    z-index: 7;
    pointer-events: none;
    background: var(--bg-panel);
    border-right: 1px solid var(--border-soft);
  }
  .thCorner { height: 26px; border-bottom: 1px solid var(--border-soft); }
  .th { display: flex; align-items: center; justify-content: center; border-bottom: 1px solid var(--border-soft); }
  .th.video { height: 46px; }
  .th.audio { height: 34px; }
  .thGap { height: 8px; border-bottom: 1px solid var(--border-soft); }
  .th span {
    padding: 2px 6px;
    border-radius: 5px;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.03em;
    color: var(--text-dim);
    background: color-mix(in srgb, var(--accent) 13%, transparent);
  }
  .th.audio span { background: color-mix(in srgb, var(--pick) 15%, transparent); }

  .timelineClip {
    position: absolute;
    top: 4px;
    bottom: 4px;
    display: block;
    min-width: 36px;
    padding: 0;
    overflow: hidden;
    border-radius: 7px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: color-mix(in srgb, var(--accent) 30%, #1a2027);
    color: #fff;
    text-align: left;
    cursor: grab;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.35);
  }
  .timelineClip:active { cursor: grabbing; }
  .timelineClip.on { z-index: 2; box-shadow: 0 0 0 2px var(--accent), 0 4px 14px rgba(0, 0, 0, 0.35); }
  .thumbs {
    position: absolute;
    inset: 0;
    background-size: auto 100%;
    background-repeat: repeat-x;
    pointer-events: none;
  }
  .thumbs i { position: absolute; top: 0; bottom: 0; background-repeat: no-repeat; box-shadow: inset -1px 0 rgba(0, 0, 0, 0.35); }
  .clipLabel {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px 12px;
    background: linear-gradient(180deg, rgba(0, 0, 0, 0.66), rgba(0, 0, 0, 0));
    font-size: 11px;
    pointer-events: none;
  }
  .clipLabel strong { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6); }
  .clipLabel em { font-style: normal; font-size: 10.5px; opacity: 0.85; font-variant-numeric: tabular-nums; }
  .timelineClip.audio {
    color: var(--text);
    border-color: color-mix(in srgb, var(--pick) 45%, transparent);
    background-color: color-mix(in srgb, var(--pick) 24%, var(--bg-elev));
    background-image: repeating-linear-gradient(90deg, color-mix(in srgb, var(--pick) 55%, transparent) 0 2px, transparent 2px 5px);
    background-size: 100% 36%;
    background-position: 0 78%;
    background-repeat: no-repeat;
  }
  .timelineClip.audio .clipLabel { padding: 3px 8px; background: none; }
  .timelineClip.audio .clipLabel strong { text-shadow: none; }
  .clipLabel .note { width: 13px; height: 13px; flex: none; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; }
  .handle { position: absolute; top: 0; bottom: 0; z-index: 2; width: 9px; cursor: ew-resize; }
  .handle::after {
    content: "";
    position: absolute;
    top: 50%;
    width: 3px;
    height: 16px;
    margin-top: -8px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.9);
    box-shadow: 0 0 3px rgba(0, 0, 0, 0.5);
    opacity: 0;
    transition: opacity 100ms ease;
  }
  .handle.left { left: 0; }
  .handle.left::after { left: 3px; }
  .handle.right { right: 0; }
  .handle.right::after { right: 3px; }
  .handle:hover { background: rgba(255, 255, 255, 0.16); }
  .timelineClip:hover .handle::after,
  .timelineClip.on .handle::after { opacity: 1; }
  /* A clip that differs from the timeline's format (see mismatchOf). */
  .mm {
    flex: none;
    padding: 0 4px;
    border-radius: 4px;
    font-size: 10.5px;
    font-weight: 700;
    color: #1b1300;
    background: var(--star);
  }
  /* Its file wasn't there when the timeline came back (drive unplugged). */
  .timelineClip.gone { border-style: dashed; opacity: 0.55; }
  /* Slim, non-interactive mirror of a video clip's linked source audio. */
  .sourceAudioBar {
    position: absolute;
    top: 9px;
    bottom: 9px;
    border-radius: 4px;
    pointer-events: none;
    background: color-mix(in srgb, var(--pick) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--pick) 26%, transparent);
  }
  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    z-index: 5;
    pointer-events: none;
    background: var(--accent);
    box-shadow: 0 0 6px color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .playhead::before {
    content: "";
    position: absolute;
    top: 3px;
    left: -6px;
    width: 14px;
    height: 13px;
    border-radius: 4px 4px 7px 7px;
    background: var(--accent);
  }

  /* ── inspector ───────────────────────────────────────────────────────── */
  .inspector {
    grid-row: 1;
    grid-column: 3;
    position: relative;
    z-index: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    background: var(--bg-panel);
    border-left: 1px solid var(--border-soft);
  }
  .inspector > * { flex-shrink: 0; }
  .inspectorCollapsed .inspector { border: 0; overflow: hidden; }
  .inspectorCollapsed .inspector > * { display: none; }
  .inspHead {
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 10px 10px 12px;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border-soft);
  }
  .inspTabs {
    flex: 1;
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 9px;
    border: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg) 75%, transparent);
  }
  .inspTabs button { flex: 1; height: 28px; border-radius: 7px; color: var(--text-dim); font-size: 12.5px; font-weight: 600; }
  .inspTabs button:hover:not(.on) { color: var(--text); }
  .inspTabs button.on { color: var(--text); background: var(--bg-elev); box-shadow: 0 1px 2px rgba(0, 0, 0, 0.22), 0 0 0 1px var(--border-soft); }
  .inspBody { display: flex; flex-direction: column; gap: 10px; padding: 12px 14px 18px; }
  .inspBody > * { flex-shrink: 0; }
  .secHead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 4px;
    font-size: 11.5px;
    font-weight: 650;
    color: var(--text-faint);
  }
  .secHead.adjust { margin-top: 8px; padding-top: 14px; border-top: 1px solid var(--border-soft); }
  .linkBtn { padding: 2px 5px; border-radius: 5px; color: var(--accent); font-size: 11.5px; font-weight: 560; }
  .linkBtn:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .linkBtn:disabled { color: var(--text-faint); opacity: 0.6; cursor: default; }
  .lookChips { display: flex; flex-wrap: wrap; gap: 5px; }
  .lookChips button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 25px;
    padding: 0 10px;
    border-radius: 999px;
    border: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg-elev) 70%, transparent);
    color: var(--text-dim);
    font-size: 11.5px;
    font-weight: 560;
  }
  .lookChips button:hover:not(.on) { color: var(--text); border-color: var(--border); }
  .lookChips button.on { border-color: transparent; background: var(--text); color: var(--bg); }
  .chipDot { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .lookGrid { display: grid; grid-template-columns: repeat(auto-fill, minmax(84px, 1fr)); gap: 10px 8px; }
  .lookTile { display: flex; flex-direction: column; gap: 5px; padding: 0; text-align: left; }
  .lookImg {
    display: block;
    aspect-ratio: 4 / 3;
    border-radius: 9px;
    background-color: #20252c;
    background-size: cover;
    background-position: center;
    box-shadow: 0 0 0 1px var(--border-soft);
    transition: box-shadow 120ms ease;
  }
  .lookImg.fallback { background-image: linear-gradient(135deg, #2b6cb0 0%, #38a169 45%, #dd9b34 100%); }
  .lookTile:hover .lookImg { box-shadow: 0 0 0 1px var(--border-strong), 0 4px 12px rgba(0, 0, 0, 0.25); }
  .lookTile.active .lookImg { box-shadow: 0 0 0 2px var(--accent), 0 0 0 5px color-mix(in srgb, var(--accent) 20%, transparent); }
  .lookName { overflow: hidden; color: var(--text-dim); font-size: 11.5px; font-weight: 560; white-space: nowrap; text-overflow: ellipsis; }
  .lookTile.active .lookName { color: var(--text); }
  .sliders { display: flex; flex-direction: column; gap: 2px; }
  label.slider { display: flex; flex-direction: column; gap: 0; color: var(--text-dim); font-size: 12px; }
  .sLabel { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; }
  .sLabel em { font-style: normal; font-size: 11px; color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .slider input[type="range"] { width: 100%; margin: 7px 0 6px; accent-color: var(--accent); }
  .hint { margin: 0; color: var(--text-faint); font-size: 11px; line-height: 1.45; }
  .clipCard {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px;
    border-radius: 11px;
    border: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg-elev) 70%, transparent);
  }
  .ccThumb { flex: none; width: 64px; height: 40px; border-radius: 6px; background: #000 center / cover no-repeat; }
  .ccThumb.note { display: grid; place-items: center; color: var(--pick); background: color-mix(in srgb, var(--pick) 18%, var(--bg-elev)); }
  .ccThumb.note svg { width: 18px; height: 18px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; }
  .ccText { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .ccText strong { overflow: hidden; font-size: 12.5px; font-weight: 600; white-space: nowrap; text-overflow: ellipsis; }
  .ccText span { color: var(--text-faint); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .trimGrid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  label.field { display: flex; flex-direction: column; gap: 4px; color: var(--text-faint); font-size: 11.5px; }
  .field input {
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border-radius: 8px;
    border: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg) 70%, transparent);
    color: var(--text);
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }
  .field input:focus { outline: none; border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent); }
  .softBtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 30px;
    padding: 0 10px;
    border-radius: 8px;
    border: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg-elev) 80%, transparent);
    color: var(--text);
    font-size: 12px;
    font-weight: 560;
  }
  .softBtn:hover:not(:disabled) { border-color: var(--border-strong); background: var(--bg-hover); }
  .softBtn kbd { padding: 0 4px; border: 1px solid var(--border); border-radius: 4px; color: var(--text-faint); font-size: 10.5px; }
  .softBtn.danger { color: var(--reject); }
  .softBtn.danger:hover:not(:disabled) { border-color: color-mix(in srgb, var(--reject) 55%, var(--border)); background: color-mix(in srgb, var(--reject) 10%, var(--bg-elev)); }
  .softBtn.wide { width: 100%; margin-top: 6px; }
  .facts { display: flex; flex-direction: column; margin: 0; overflow: hidden; border-radius: 10px; border: 1px solid var(--border-soft); }
  .facts > div { display: flex; justify-content: space-between; gap: 10px; padding: 7px 10px; font-size: 12px; }
  .facts > div + div { border-top: 1px solid var(--border-soft); }
  .facts dt { color: var(--text-faint); }
  .facts dd { margin: 0; color: var(--text); font-variant-numeric: tabular-nums; text-align: right; }
  label.checkRow { display: flex; align-items: center; gap: 8px; color: var(--text-dim); font-size: 12px; }
  .emptyClip {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 40px 12px;
    color: var(--text-faint);
    font-size: 12px;
    line-height: 1.45;
    text-align: center;
  }
  .emptyClip svg { width: 30px; height: 30px; margin-bottom: 4px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; }
  .emptyClip strong { color: var(--text-dim); font-size: 13px; }
  .note { margin: 0; color: var(--text-faint); font-size: 12px; }
  .sideNote {
    margin: 0 14px 14px;
    padding: 8px 10px;
    border: 1px solid var(--border-soft);
    border-radius: 9px;
    background: var(--bg-elev);
    color: var(--text-dim);
    font-size: 12px;
  }

  /* Form basics the export dialog relies on. */
  input[type="number"],
  select {
    width: 100%;
    padding: 5px 7px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--bg-elev);
    color: var(--text);
  }
  label { display: grid; gap: 4px; color: var(--text-dim); font-size: 12px; }
  .check { display: flex; align-items: center; gap: 7px; }
  .music { display: flex; align-items: center; gap: 7px; min-width: 0; }
  .dim { color: var(--text-faint); }

  /* ── narrow work panes ───────────────────────────────────────────────── */
  @container (max-width: 980px) {
    .brandText span { display: none; }
    .tlTitle span { display: none; }
  }
  @container (max-width: 820px) {
    .brand { display: none; }
    .editTop { gap: 6px; }
    .aspects button { gap: 5px; padding: 0 7px; }
    .pillText { display: none; }
    .pillBtn { padding: 0 9px; }
    .tool span { display: none; }
    .tool { padding: 0 7px; }
    .zoomCtl input[type="range"] { width: 80px; }
    .tc { min-width: 0; }
  }
  @container (max-width: 640px) {
    .aspects { order: 5; width: 100%; overflow-x: auto; }
    .aspects button { flex: 1 0 auto; justify-content: center; }
    .tcTotal { display: none; }
    .tBtns .tBtn:first-child,
    .tBtns .tBtn:last-child { display: none; }
  }

  /* Export dialog */
  /* Rows keep their height and the dialog scrolls: with the default
     flex-shrink the Source → Output table squashed to its header. */
  .igDialog > * {
    flex-shrink: 0;
  }
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
    width: min(560px, 100%);
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
  .igColHead {
    display: block;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
    margin-bottom: 6px;
  }
  /* Aligned Source → Output comparison. A 3-column grid so every property's
     current value and target line up on one row. */
  .igGrid {
    display: grid;
    grid-template-columns: minmax(84px, auto) 1fr 1.15fr;
    gap: 1px;
    border: 1px solid var(--border);
    border-radius: 10px;
    overflow: hidden;
    background: var(--border);
  }
  .igGridHead {
    display: contents;
  }
  .igGridHead span {
    padding: 6px 12px;
    background: var(--bg-elev);
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .igGridHead span:last-child {
    color: color-mix(in srgb, var(--accent) 70%, var(--text-faint));
  }
  .igGridRow {
    display: contents;
  }
  .igGridRow > span {
    padding: 8px 12px;
    background: var(--bg-panel);
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
    display: flex;
    flex-direction: column;
    gap: 2px;
    justify-content: center;
  }
  .igGridRow .k {
    color: var(--text-faint);
    font-weight: 600;
    font-variant-numeric: normal;
  }
  .igGridRow .s {
    color: var(--text-dim);
  }
  .igGridRow .o {
    background: color-mix(in srgb, var(--accent) 8%, var(--bg-panel));
    color: var(--text);
  }
  .igGridRow em {
    font-style: normal;
    font-size: 10.5px;
    color: var(--text-faint);
  }
  .igGridRow .s.warn,
  .igGridRow em.warn {
    color: var(--accent);
  }
  .igGridRow em.ok {
    color: var(--pick);
  }
  .igGridRow em.soft {
    color: #d9a326;
  }
  .igGridRow .o select {
    width: 100%;
    font-size: 12px;
    padding: 4px 6px;
  }
  .igGridRow em.dim {
    line-height: 1.35;
  }
  /* Stacked time-cost bar under the estimate. */
  .costBar {
    display: flex;
    height: 12px;
    margin: 9px 0 8px;
    border-radius: 999px;
    overflow: hidden;
    background: color-mix(in srgb, var(--border) 60%, transparent);
  }
  .costSeg {
    height: 100%;
    min-width: 3px;
  }
  .costSeg + .costSeg {
    border-left: 1px solid var(--bg-elev);
  }
  .costLegend {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
  }
  .costLegend li {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--text-dim);
  }
  .costKey {
    width: 9px;
    height: 9px;
    border-radius: 3px;
    flex: 0 0 auto;
  }
  .costName {
    color: var(--text);
  }
  .costLegend em {
    font-style: normal;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
  }
  .costNone {
    margin: 8px 0 0;
    font-size: 11.5px;
    color: var(--text-dim);
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
  .dlgLoc {
    margin: 0;
    font-size: 11.5px;
    color: var(--text-dim);
  }
  .dlgLoc .taken {
    font-style: normal;
    color: var(--accent);
  }
  .dlgNameTip {
    margin: 3px 0 0;
    font-size: 10.5px;
    line-height: 1.35;
    color: var(--text-faint);
  }
  .dlgSet {
    display: grid;
    gap: 3px;
    font-size: 11px;
    color: var(--text-faint);
  }
  /* Traffic-light effort dot beside the time estimate. */
  .effortDot {
    display: inline-block;
    width: 9px;
    height: 9px;
    margin-right: 6px;
    border-radius: 50%;
    vertical-align: baseline;
  }
  .effortDot.low {
    background: var(--pick);
  }
  .effortDot.medium {
    background: #d9a326;
  }
  .effortDot.high {
    background: var(--reject);
  }
  .effortWord {
    font-style: normal;
    font-weight: 400;
    color: var(--text-faint);
    font-size: 11px;
  }
  .dlgRerender {
    margin: 8px 0 0;
    padding-top: 7px;
    border-top: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
    font-size: 11.5px;
    color: var(--text-dim);
  }
  .dlgName {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .dlgNameRow {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .dlgNameRow input {
    flex: 1;
    min-width: 0;
    background: var(--bg-elev);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 6px 8px;
    font-size: 12.5px;
  }
  .dlgNameExt {
    color: var(--text-faint);
    font-size: 12px;
  }
  .dlgOther {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg-elev);
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
</style>
