<script lang="ts">
  // Merge videos: a trip's clips joined end to end with no re-encoding, the
  // "dump the Osmo clips into one file for YouTube" workflow. Deliberately not
  // the Edit studio (that's the Instagram crop/trim/grade flow).
  //
  // The owner's rules for this window, which the code follows:
  //   * Nothing is dropped silently. Everything selected is listed, photos
  //     included. The dominant format is worked out from the list, and every
  //     cell that doesn't match it is highlighted with the reason. A night
  //     sequence shot at 30 fps must be visible, not quietly missing.
  //   * The owner removes items: the row's − button, right-click → Remove, or
  //     select rows and press Delete. Merge stays disabled until nothing is
  //     flagged.
  //   * Only real differences are flagged. A variable frame rate (29.73 for a
  //     30) joins cleanly and isn't one; a different frame size is, and the
  //     cell says the size, not a vague "Vertical" (2026-09-29, Meta glasses).
  //   * Clips that differ only in size, rate, codec or audio can still go in
  //     through "Convert to match", which the owner chooses: everything is
  //     re-encoded to one format at a generous bitrate. Different shapes,
  //     HDR mixed with SDR, and photos can't, and stay flagged.
  //   * Chronological (oldest first) by default; rows can be dragged to reorder.
  //   * Clicking a row previews it on the right (photo, or video with hover
  //     scrub and Play). Clicking outside never closes the window.
  import { onDestroy, onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api } from "$lib/api";
  import type { MediaItem, MergeClip, MergeConvert, TreeDir } from "$lib/types";
  import Thumb from "./Thumb.svelte";
  import ContextMenu, { type MenuEntry } from "./ContextMenu.svelte";

  let {
    items,
    sourceDir,
    drives,
    onclose,
    ondone,
  }: {
    /** Everything that was selected, photos included. */
    items: MediaItem[];
    /** Folder the clips live in: the default place to save. */
    sourceDir: string;
    drives: TreeDir[];
    onclose: () => void;
    ondone: (path: string, dir: string) => void;
  } = $props();

  type Phase = "probing" | "ready" | "merging" | "done" | "error";
  let phase = $state<Phase>("probing");
  let byPath = $state<Record<string, MergeClip>>({});
  /** The merge sequence, as paths, in play order. */
  let order = $state<string[]>([]);
  let chronological: string[] = [];
  let sel = $state<Set<string>>(new Set());
  let anchor: string | null = null;
  let previewPath = $state<string | null>(null);
  let playing = $state(false);
  let playFailed = $state(false);
  let photoSrc = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  let name = $state("");
  let destDir = $state("");
  let error = $state("");
  let pct = $state(0);
  let startedAt = 0;
  let now = $state(Date.now());
  let result = $state<{ path: string; bytes: number } | null>(null);
  let listEl = $state<HTMLDivElement | null>(null);

  type Dest = { label: string; path: string; free: number | null };
  let dests = $state<Dest[]>([]);

  const itemFor = (p: string) => items.find((i) => i.path === p);

  // ── what "compatible" means ─────────────────────────────────────────────
  const bitDepth = (c: MergeClip) => (/10|12/.test(c.pix_fmt) ? "10-bit" : "8-bit");
  const codecName = (c: MergeClip) =>
    ({ hevc: "HEVC", h264: "H.264", prores: "ProRes", av1: "AV1", vp9: "VP9" })[c.vcodec] ?? c.vcodec.toUpperCase();
  const hdrName = (color: string) => (color === "hlg" ? "HLG" : color === "pq" ? "HDR10" : "");
  /** "HEVC 10-bit", or "HEVC HLG" for HDR (HDR is always 10-bit). */
  const codecLabel = (c: MergeClip) => `${codecName(c)} ${hdrName(c.color) || bitDepth(c)}`;
  /** Width × height as it plays (a rotated clip is shown upright). */
  const dims = (c: MergeClip): [number, number] => (c.rotation % 180 ? [c.height, c.width] : [c.width, c.height]);
  const frameLabel = (c: MergeClip) => dims(c).join("×");
  const shape = (c: MergeClip) => {
    const [w, h] = dims(c);
    return h > w ? "vertical" : h === w ? "square" : "landscape";
  };
  const aspect = (c: MergeClip) => {
    const [w, h] = dims(c);
    return w / h;
  };
  /** A 29.97 reads "29.97"; a variable-rate 30 that averages 29.73 reads "30". */
  function fpsLabel(c: MergeClip) {
    if (!c.fps) return "—";
    const cls = c.fps_class || Math.round(c.fps);
    if (Math.abs(c.fps - cls / 1.001) < 0.01) return (cls / 1.001).toFixed(2);
    return String(cls);
  }
  function fpsTitle(c: MergeClip) {
    const cls = c.fps_class || Math.round(c.fps);
    if (Math.abs(c.fps - cls) < 0.05 || Math.abs(c.fps - cls / 1.001) < 0.01) return `${fpsLabel(c)} fps`;
    return `Averages ${c.fps.toFixed(2)} fps: a variable frame rate, normal for phones and glasses. It joins like any ${cls} fps clip.`;
  }
  /** A flagged cell's tooltip; in convert mode a fixable one says it's handled. */
  const cellTitle = (why: string | undefined, soft: boolean) =>
    why ? (soft ? `Converted to match. ${why.replace(/ Convert to match can include it\.$/, "")}` : why) : undefined;
  const audioLabel = (c: MergeClip) =>
    c.acodec ? `${c.acodec.toUpperCase()} ${c.arate ? Math.round(c.arate / 1000) + "k" : ""}${c.alayout && c.alayout !== "stereo" ? " " + c.alayout : ""}`.trim() : "None";

  let list = $derived(order.map((p) => byPath[p]).filter(Boolean));

  /** The format most of the running time shares: what the merge will be. */
  let dominant = $derived.by(() => {
    const secs = new Map<string, { clip: MergeClip; secs: number; n: number }>();
    for (const c of list) {
      if (c.kind !== "video" || c.error) continue;
      const g = secs.get(c.signature) ?? { clip: c, secs: 0, n: 0 };
      g.secs += c.duration;
      g.n += 1;
      secs.set(c.signature, g);
    }
    let best: { clip: MergeClip; secs: number; n: number } | null = null;
    for (const g of secs.values()) if (!best || g.secs > best.secs || (g.secs === best.secs && g.n > best.n)) best = g;
    return best?.clip ?? null;
  });

  type Issues = { frame?: string; fps?: string; codec?: string; audio?: string; kind?: string; hard: boolean };
  /** Which attributes of this item stop a lossless join, in words. `hard`:
   *  converting can't fix it either (a photo, another shape, HDR vs SDR). */
  function issuesOf(c: MergeClip): Issues | null {
    if (c.kind === "photo") return { kind: "Photo: only videos can be merged", hard: true };
    if (c.kind !== "video") return { kind: "Not a video", hard: true };
    if (c.error) return { kind: c.error, hard: true };
    const d = dominant;
    if (!d || c.signature === d.signature) return null;
    const out: Issues = { hard: false };
    const [cw, ch] = dims(c);
    const [dw, dh] = dims(d);
    if (cw !== dw || ch !== dh) {
      const sameShape = shape(c) === shape(d) && Math.abs(aspect(c) / aspect(d) - 1) <= 0.03;
      if (sameShape) {
        out.frame = `${cw}×${ch}; the rest are ${dw}×${dh}. A lossless join can't mix frame sizes: the picture breaks after the join. Convert to match can include it.`;
      } else {
        out.frame = `${shape(c)[0].toUpperCase()}${shape(c).slice(1)} ${cw}×${ch}; the rest are ${shape(d)} ${dw}×${dh}. A different shape can't join this video.`;
        out.hard = true;
      }
    }
    if (c.fps_class !== d.fps_class) out.fps = `${fpsLabel(c)} fps; the rest are ${fpsLabel(d)} fps. Convert to match can include it.`;
    if ((c.color || "sdr") !== (d.color || "sdr")) {
      out.codec = `${c.color === "sdr" ? "SDR" : "HDR (" + hdrName(c.color) + ")"}; the rest are ${d.color === "sdr" ? "SDR" : "HDR (" + hdrName(d.color) + ")"}. HDR and SDR can't share one file.`;
      out.hard = true;
    } else if (c.vcodec !== d.vcodec || c.profile !== d.profile || c.pix_fmt !== d.pix_fmt) {
      out.codec = `${codecLabel(c)} (${c.profile || c.vcodec}); the rest are ${codecLabel(d)} (${d.profile || d.vcodec}).`;
    }
    if (c.acodec !== d.acodec || c.arate !== d.arate || c.alayout !== d.alayout) out.audio = `${audioLabel(c)}; the rest are ${audioLabel(d)}.`;
    if (!out.frame && !out.fps && !out.codec && !out.audio) out.codec = "Encoded differently from the rest";
    return out;
  }
  let issues = $derived(Object.fromEntries(list.map((c) => [c.path, issuesOf(c)])) as Record<string, Issues | null>);

  /** "copy" = the lossless join; "convert" = re-encode everything to one format. */
  let mode = $state<"copy" | "convert">("copy");
  /** Clips only a conversion can bring in. */
  let fixable = $derived(list.filter((c) => issues[c.path] && !issues[c.path]!.hard));
  let hardFlagged = $derived(list.filter((c) => issues[c.path]?.hard));
  let converting = $derived(mode === "convert");
  // What blocks the Merge button, and what goes in.
  let flagged = $derived(converting ? hardFlagged : list.filter((c) => issues[c.path]));
  let clean = $derived(list.filter((c) => (converting ? !issues[c.path]?.hard : !issues[c.path])));
  let totalSecs = $derived(clean.reduce((s, c) => s + c.duration, 0));
  // Leave convert mode by itself once there's nothing left it would fix.
  $effect(() => {
    if (mode === "convert" && !fixable.length) mode = "copy";
  });

  /** The format a conversion writes: the dominant clip's rate, colour and bit
   *  depth, at the size of the largest clip of that shape (so nothing is
   *  scaled down), and twice the sharpest source's bitrate for that size, so
   *  a second generation looks the same as the first. */
  let target = $derived.by((): MergeConvert | null => {
    const d = dominant;
    if (!d || !clean.length) return null;
    let big = d;
    for (const c of clean) {
      const [w, h] = dims(c);
      const [bw, bh] = dims(big);
      if (w * h > bw * bh) big = c;
    }
    const [bw, bh] = dims(big);
    const width = bw - (bw % 2);
    const height = bh - (bh % 2);
    const cls = d.fps_class || Math.round(d.fps) || 30;
    const ntsc = Math.abs(d.fps - cls / 1.001) < 0.01;
    const rate = ntsc ? cls / 1.001 : cls;
    let kbps = 0;
    for (const c of clean) {
      const [w, h] = dims(c);
      if (!c.vbitrate || !w || !h) continue;
      kbps = Math.max(kbps, (c.vbitrate * (width * height)) / (w * h) * (rate / (c.fps_class || rate)));
    }
    if (!kbps) kbps = (width * height * rate * 0.1) / 1000; // ~0.1 bit per pixel when ffmpeg didn't say
    return {
      width,
      height,
      fps: ntsc ? `${cls * 1000}/1001` : String(cls),
      tenBit: bitDepth(d) === "10-bit" || d.color !== "sdr",
      color: d.color || "sdr",
      bitrateKbps: Math.min(150_000, Math.max(8_000, Math.round((2 * kbps) / 1000) * 1000)),
    };
  });
  const targetFpsLabel = (t: MergeConvert) => (t.fps.includes("/") ? (Number(t.fps.split("/")[0]) / 1001).toFixed(2) : t.fps);
  const sourceMbps = $derived.by(() => {
    const r = clean.map((c) => c.vbitrate).filter(Boolean);
    if (!r.length) return "";
    const lo = Math.round(Math.min(...r) / 1000);
    const hi = Math.round(Math.max(...r) / 1000);
    return lo === hi ? `${lo}` : `${lo}–${hi}`;
  });

  // DJI clips carry a ~5 Mbps debug track the merge drops, so the plain sum
  // slightly over-estimates: fine for a space check.
  let totalBytes = $derived(
    converting && target ? Math.round((totalSecs * (target.bitrateKbps + 320) * 1000) / 8) : clean.reduce((s, c) => s + c.size, 0),
  );
  /** Free space the job needs: a conversion keeps its converted parts until
   *  they're joined, so it briefly needs the result's size twice. */
  let needBytes = $derived(converting ? totalBytes * 2 : totalBytes);
  let dest = $derived(dests.find((d) => d.path === destDir));
  const MARGIN = 1024 ** 3; // the backend's 1 GB margin
  const fits = (d: Dest | undefined) => !!d && (d.free === null || d.free >= needBytes + MARGIN);

  /** The one thing still in the way of merging, or null when it can run. */
  let blocker = $derived.by(() => {
    if (flagged.length) {
      const n = `${flagged.length} highlighted item${flagged.length === 1 ? "" : "s"}`;
      return !converting && !hardFlagged.length ? `Remove the ${n}, or choose Convert to match` : `Remove the ${n} to merge`;
    }
    if (clean.length < 2) return "Add at least two videos";
    if (!name.trim()) return "Give the file a name";
    if (!destDir) return "Choose where to save it";
    if (dest && dest.free !== null && !fits(dest)) return "Not enough space there: pick another drive";
    return null;
  });

  // ── formatting ────────────────────────────────────────────────────────────
  function fmtDur(s: number) {
    s = Math.round(s);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = String(s % 60).padStart(2, "0");
    return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
  }
  function fmtLong(s: number) {
    const h = Math.floor(s / 3600);
    const m = Math.round((s % 3600) / 60);
    return h ? `${h} h ${m} min` : `${m} min`;
  }
  const gb = (b: number) =>
    b >= 1e12 ? `${(b / 1e12).toFixed(1)} TB` : b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(b / 1e6))} MB`;
  const when = (t: number | null) =>
    t ? new Date(t * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false }) : "—";
  const baseName = (p: string) => p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || p;

  function defaultName(clips: MergeClip[]) {
    const ts = clips.map((c) => c.captured).filter((t): t is number => !!t);
    if (!ts.length) return "Merged video";
    const a = new Date(Math.min(...ts) * 1000);
    const b = new Date(Math.max(...ts) * 1000);
    const mon = (d: Date) => d.toLocaleString(undefined, { month: "short" });
    if (a.toDateString() === b.toDateString()) return `Merged ${a.getDate()} ${mon(a)} ${a.getFullYear()}`;
    if (a.getMonth() === b.getMonth() && a.getFullYear() === b.getFullYear()) return `Merged ${a.getDate()}-${b.getDate()} ${mon(a)} ${a.getFullYear()}`;
    return `Merged ${a.getDate()} ${mon(a)} - ${b.getDate()} ${mon(b)} ${b.getFullYear()}`;
  }

  // ── lifecycle ─────────────────────────────────────────────────────────────
  onMount(async () => {
    let clips: MergeClip[];
    try {
      clips = await api.mergeProbe(items.map((i) => i.path));
    } catch (e) {
      error = String(e);
      phase = "error";
      return;
    }
    byPath = Object.fromEntries(clips.map((c) => [c.path, c]));
    chronological = clips.map((c) => c.path);
    order = [...chronological];
    name = defaultName(clips.filter((c) => c.kind === "video"));
    previewPath = order[0] ?? null;
    phase = "ready";
    void loadDestinations();
  });

  async function loadDestinations() {
    const list: Dest[] = [{ label: `Same folder as the clips (${baseName(sourceDir)})`, path: sourceDir, free: null }];
    const sug = await api.suggestedFolders();
    const movies = sug.find((s) => s.kind === "videos");
    if (movies) list.push({ label: `This computer: ${movies.label}`, path: movies.path, free: null });
    for (const d of drives) {
      if (d.name === "Home" || (await api.isSystemRoot(d.path))) continue;
      if (sourceDir.toLowerCase().startsWith(d.path.toLowerCase())) continue; // the clips' own drive is option 1
      list.push({ label: `${d.name.replace(/[\\/]+$/, "")} (drive)`, path: d.path, free: null });
    }
    dests = list;
    destDir = sourceDir;
    for (const d of list) void refreshFree(d.path);
  }

  async function refreshFree(path: string) {
    try {
      const free = await api.diskFree(path);
      dests = dests.map((x) => (x.path === path ? { ...x, free } : x));
    } catch {
      /* unknown: the backend checks again before writing */
    }
  }

  async function chooseFolder() {
    const picked = await api.pickFolder();
    if (!picked) return;
    if (!dests.some((d) => d.path === picked)) dests = [...dests, { label: picked, path: picked, free: null }];
    destDir = picked;
    void refreshFree(picked);
  }

  // Preview: photos get the sharp Focus-size JPEG; videos show as a large
  // tile (hover to scrub) until Play swaps in a real player.
  $effect(() => {
    const p = previewPath;
    playing = false;
    playFailed = false;
    photoSrc = null;
    const c = p ? byPath[p] : undefined;
    if (c?.kind === "photo") {
      api.loupeSrc(c.path).then(
        (src) => previewPath === p && (photoSrc = api.fileSrc(src)),
        () => {},
      );
    }
  });

  // ── list editing ──────────────────────────────────────────────────────────
  function removePaths(paths: Iterable<string>) {
    const gone = new Set(paths);
    if (!gone.size) return;
    const idx = order.findIndex((p) => gone.has(p));
    order = order.filter((p) => !gone.has(p));
    sel = new Set([...sel].filter((p) => !gone.has(p)));
    if (previewPath && gone.has(previewPath)) {
      previewPath = order[Math.min(Math.max(0, idx), order.length - 1)] ?? null;
      if (previewPath) sel = new Set([previewPath]);
    }
  }

  function clickRow(e: MouseEvent, path: string) {
    if (e.shiftKey && anchor) {
      const a = order.indexOf(anchor);
      const b = order.indexOf(path);
      const range = order.slice(Math.min(a, b), Math.max(a, b) + 1);
      sel = new Set(e.metaKey || e.ctrlKey ? [...sel, ...range] : range);
    } else if (e.metaKey || e.ctrlKey) {
      const next = new Set(sel);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      sel = next;
      anchor = path;
    } else {
      sel = new Set([path]);
      anchor = path;
    }
    previewPath = path;
  }

  function moveSelected(delta: number) {
    if (!sel.size) return;
    const next = [...order];
    const idxs = next.map((p, i) => (sel.has(p) ? i : -1)).filter((i) => i >= 0);
    if (delta < 0 && idxs[0] === 0) return;
    if (delta > 0 && idxs[idxs.length - 1] === next.length - 1) return;
    for (const i of delta < 0 ? idxs : [...idxs].reverse()) {
      [next[i], next[i + delta]] = [next[i + delta], next[i]];
    }
    order = next;
  }

  function moveTo(paths: string[], edge: "top" | "bottom") {
    const moving = order.filter((p) => paths.includes(p));
    const rest = order.filter((p) => !paths.includes(p));
    order = edge === "top" ? [...moving, ...rest] : [...rest, ...moving];
  }

  function rowMenu(e: MouseEvent, path: string) {
    e.preventDefault();
    e.stopPropagation();
    if (!sel.has(path)) {
      sel = new Set([path]);
      anchor = path;
      previewPath = path;
    }
    const targets = order.filter((p) => sel.has(p));
    const n = targets.length > 1 ? ` (${targets.length})` : "";
    menu = {
      x: e.clientX,
      y: e.clientY,
      entries: [
        { label: `Remove from merge${n}`, icon: "−", danger: true, action: () => removePaths(targets) },
        { separator: true },
        { label: `Move to start${n}`, icon: "⤒", action: () => moveTo(targets, "top") },
        { label: `Move to end${n}`, icon: "⤓", action: () => moveTo(targets, "bottom") },
        { separator: true },
        { label: "Show in folder", icon: "⤴", action: () => api.reveal(path) },
      ],
    };
  }

  // Drag to reorder. A row that's part of a multi-selection drags the whole
  // selection with it, keeping their relative order.
  let dragging: string[] = [];
  let dropAt = $state<{ path: string; after: boolean } | null>(null);
  function dragStart(e: DragEvent, path: string) {
    dragging = sel.has(path) ? order.filter((p) => sel.has(p)) : [path];
    e.dataTransfer?.setData("text/x-foxcull-merge-row", path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
    e.stopPropagation();
  }
  function dragOver(e: DragEvent, path: string) {
    if (!dragging.length) return;
    e.preventDefault();
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    dropAt = { path, after: e.clientY > r.top + r.height / 2 };
  }
  function drop(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    const at = dropAt;
    const moving = dragging;
    dragging = [];
    dropAt = null;
    if (!at || !moving.length || moving.includes(at.path)) return;
    const rest = order.filter((p) => !moving.includes(p));
    let i = rest.indexOf(at.path);
    if (at.after) i += 1;
    order = [...rest.slice(0, i), ...moving, ...rest.slice(i)];
  }

  function onkeydown(e: KeyboardEvent) {
    // This window owns the keyboard while it's open (the page ignores keys
    // then), and it never closes on Escape: only Cancel / ✕ close it.
    if (menu) return; // the context menu handles its own keys
    const t = e.target as HTMLElement;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
    if (phase !== "ready") return;
    if (e.key === "Escape") {
      e.preventDefault();
      if (playing) playing = false;
      else sel = new Set();
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      removePaths(sel);
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      sel = new Set(order);
      return;
    }
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const d = e.key === "ArrowUp" ? -1 : 1;
      if (e.altKey) {
        moveSelected(d);
        return;
      }
      const cur = previewPath ? order.indexOf(previewPath) : -1;
      const next = order[Math.max(0, Math.min(order.length - 1, cur + d))];
      if (!next) return;
      if (e.shiftKey) sel = new Set([...sel, next]);
      else {
        sel = new Set([next]);
        anchor = next;
      }
      previewPath = next;
      listEl?.querySelector(`[data-path="${CSS.escape(next)}"]`)?.scrollIntoView({ block: "nearest" });
    }
  }

  // ── merge ─────────────────────────────────────────────────────────────────
  let unlisten: (() => void) | null = null;
  let ticker: ReturnType<typeof setInterval> | null = null;
  async function start() {
    if (blocker) return;
    error = "";
    playing = false;
    phase = "merging";
    pct = 0;
    startedAt = Date.now();
    ticker = setInterval(() => (now = Date.now()), 1000);
    try {
      unlisten = await api.onExportProgress((p) => (pct = p));
    } catch {
      /* no progress events outside the app */
    }
    try {
      result = await api.mergeVideos({ paths: clean.map((c) => c.path), destDir, name, convert: converting ? target : null });
      phase = "done";
      ondone(result.path, destDir);
    } catch (e) {
      const msg = String(e);
      phase = "ready";
      if (!msg.includes("cancelled")) error = msg;
    } finally {
      unlisten?.();
      unlisten = null;
      if (ticker) clearInterval(ticker);
    }
  }

  onDestroy(() => {
    unlisten?.();
    if (ticker) clearInterval(ticker);
  });

  let eta = $derived.by(() => {
    if (phase !== "merging" || pct < 2) return "";
    const elapsed = (now - startedAt) / 1000;
    const left = (elapsed * (100 - pct)) / pct;
    return left > 90 ? `about ${Math.round(left / 60)} min left` : `about ${Math.max(1, Math.round(left))} s left`;
  });

  let preview = $derived(previewPath ? byPath[previewPath] : undefined);
  let previewItem = $derived(previewPath ? itemFor(previewPath) : undefined);
  let reordered = $derived(order.join("\n") !== chronological.filter((p) => order.includes(p)).join("\n"));
</script>

<svelte:window {onkeydown} />

<!-- The backdrop is inert on purpose: a stray click must not throw away a
     half-reviewed list. -->
<div class="backdrop" role="presentation"></div>
<div class="panel" role="dialog" aria-label="Merge videos" aria-modal="true">
  <header>
    <div>
      <h2>Merge videos</h2>
      <p class="sub">
        {#if converting}Every clip re-encoded to one format so they can join. At a high bitrate it looks the same as the originals.{:else}Joined end to end with no re-encoding, so the file keeps the camera's full quality.{/if}
      </p>
    </div>
    <span class="grow"></span>
    {#if phase !== "merging"}<button class="x" onclick={onclose} title="Close" aria-label="Close">✕</button>{/if}
  </header>

  {#if phase === "probing"}
    <div class="center">Checking {items.length} items…</div>
  {:else if phase === "error"}
    <div class="center err">{error}</div>
  {:else}
    <div class="body">
      <!-- ░ left: the merge sequence ░ -->
      <section class="left">
        {#if converting && target && dominant}
          <div class="basis">
            <span class="basisLabel">Converting to</span>
            <span class="fmt">{target.width}×{target.height}</span>
            <span class="fmt">{targetFpsLabel(target)} fps</span>
            <span class="fmt">{codecName(dominant)} {hdrName(target.color) || (target.tenBit ? "10-bit" : "8-bit")}</span>
            <span class="fmt">AAC 48k</span>
            <span class="basisNote">the size of your largest clip, the rest scaled to fit</span>
          </div>
        {:else if dominant}
          <div class="basis">
            <span class="basisLabel">Merging as</span>
            <span class="fmt">{frameLabel(dominant)} {shape(dominant)}</span>
            <span class="fmt">{fpsLabel(dominant)} fps</span>
            <span class="fmt">{codecLabel(dominant)}</span>
            <span class="fmt">{audioLabel(dominant)}</span>
            <span class="basisNote">the format most of this footage shares</span>
          </div>
        {/if}

        <div class="toolbar">
          <span class="count">{list.length} item{list.length === 1 ? "" : "s"}</span>
          {#if flagged.length}
            <span class="flagChip" title="Highlighted cells show why each one can't join the merge">{flagged.length} can't be merged{!converting && !hardFlagged.length ? " losslessly" : ""}</span>
          {:else if list.length && !converting}
            <span class="okChip">All compatible</span>
          {/if}
          {#if converting && fixable.length}
            <span class="convChip" title="Resized or re-timed to match the rest">{fixable.length} adjusted to match</span>
          {/if}
          <span class="grow"></span>
          {#if flagged.length}
            <button class="btn sm" onclick={() => { sel = new Set(flagged.map((c) => c.path)); previewPath = flagged[0].path; }} title="Select every highlighted row, so you can check them and press Delete">Select these</button>
          {/if}
          <button class="btn sm" disabled={!sel.size || phase !== "ready"} onclick={() => removePaths(sel)} title="Remove the selected rows (Delete)">Remove selected</button>
          <button class="btn sm" disabled={!reordered || phase !== "ready"} onclick={() => (order = chronological.filter((p) => order.includes(p)))} title="Put the list back in the order the clips were shot">Sort by time shot</button>
        </div>

        <div class="table" bind:this={listEl} role="listbox" aria-multiselectable="true" aria-label="Merge sequence">
          <div class="row head" aria-hidden="true">
            <span class="c-idx">#</span>
            <span class="c-name">Name</span>
            <span class="c-when">Recorded</span>
            <span class="c-len">Length</span>
            <span class="c-frame">Frame</span>
            <span class="c-fps">FPS</span>
            <span class="c-codec">Video</span>
            <span class="c-audio">Audio</span>
            <span class="c-size">Size</span>
            <span class="c-rm"></span>
          </div>
          {#each list as c, i (c.path)}
            {@const iss = issues[c.path]}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="row"
              class:sel={sel.has(c.path)}
              class:flag={!!iss && (!converting || iss.hard)}
              class:adjusted={converting && !!iss && !iss.hard}
              class:previewing={previewPath === c.path}
              class:dropBefore={dropAt?.path === c.path && !dropAt.after}
              class:dropAfter={dropAt?.path === c.path && dropAt.after}
              role="option"
              aria-selected={sel.has(c.path)}
              tabindex="-1"
              data-path={c.path}
              draggable={phase === "ready"}
              onclick={(e) => clickRow(e, c.path)}
              oncontextmenu={(e) => rowMenu(e, c.path)}
              ondragstart={(e) => dragStart(e, c.path)}
              ondragover={(e) => dragOver(e, c.path)}
              ondrop={drop}
              ondragend={() => { dragging = []; dropAt = null; }}
            >
              <span class="c-idx"><span class="grip" aria-hidden="true">⋮⋮</span>{i + 1}</span>
              <span class="c-name" title={c.path}>
                <svg class="kind" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  {#if c.kind === "video"}<rect x="3" y="6" width="13" height="12" rx="2.5" /><path d="M16 10.5l5-3v9l-5-3" />{:else}<rect x="3" y="4" width="18" height="16" rx="2.5" /><path d="M3 16l5-5 4 4 3-3 6 6" /><circle cx="16" cy="9" r="1.4" />{/if}
                </svg><span class="nmA">{c.name.slice(0, -12)}</span><span class="nmB">{c.name.slice(-12)}</span>
              </span>
              <span class="c-when">{when(c.captured)}</span>
              {#if c.kind !== "video" || c.error}
                <span class="c-span" title={iss?.kind}>
                  <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M6 18L18 6" /></svg>
                  {iss?.kind}
                </span>
              {:else}
                <span class="c-len">{fmtDur(c.duration)}</span>
                {@const soft = converting && !!iss && !iss.hard}
                <span class="c-frame" class:bad={!!iss?.frame && !soft} class:conv={!!iss?.frame && soft} title={cellTitle(iss?.frame, soft) ?? `${frameLabel(c)} ${shape(c)}`}>{frameLabel(c)}</span>
                <span class="c-fps" class:bad={!!iss?.fps && !soft} class:conv={!!iss?.fps && soft} title={cellTitle(iss?.fps, soft) ?? fpsTitle(c)}>{fpsLabel(c)}</span>
                <span class="c-codec" class:bad={!!iss?.codec && !soft} class:conv={!!iss?.codec && soft} title={cellTitle(iss?.codec, soft) ?? `${codecName(c)} ${c.profile} · ${c.pix_fmt}${c.vbitrate ? ` · ${Math.round(c.vbitrate / 1000)} Mbps` : ""}`}>{codecLabel(c)}</span>
                <span class="c-audio" class:bad={!!iss?.audio && !soft} class:conv={!!iss?.audio && soft} title={cellTitle(iss?.audio, soft) ?? audioLabel(c)}>{audioLabel(c)}</span>
              {/if}
              <span class="c-size">{gb(c.size)}</span>
              <span class="c-rm">
                <button class="rm" disabled={phase !== "ready"} onclick={(e) => { e.stopPropagation(); removePaths([c.path]); }} title="Remove from the merge (the file itself isn't touched)" aria-label="Remove {c.name} from the merge">
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 12h12" /></svg>
                </button>
              </span>
            </div>
          {:else}
            <div class="empty">Nothing left in the list. Close this and select some clips.</div>
          {/each}
        </div>
        <p class="keys">Click a row to preview · drag rows to reorder (or ⌥↑ ⌥↓) · Delete removes the selected rows · the files themselves are never touched</p>
      </section>

      <!-- ░ right: preview on top, settings + Merge below ░ -->
      <section class="right">
        <div class="preview">
          {#if preview && previewItem}
            {#if preview.kind === "photo"}
              {#if photoSrc}<img src={photoSrc} alt={preview.name} />{:else}<div class="stage"><Thumb item={previewItem} size={480} /></div>{/if}
            {:else if preview.kind === "video" && playing && !playFailed}
              <!-- svelte-ignore a11y_media_has_caption -->
              <video src={api.fileSrc(preview.path)} controls autoplay onerror={() => (playFailed = true)}></video>
            {:else}
              <div class="stage"><Thumb item={previewItem} size={480} armed /></div>
              {#if preview.kind === "video"}
                <div class="playbar">
                  {#if playFailed}
                    <span class="pf">This clip can't play in here.</span>
                    <button class="btn sm" onclick={() => api.openExternal(preview!.path)}>Open in player</button>
                  {:else}
                    <span class="hint">Hover to scrub</span>
                    <button class="btn sm accent" onclick={() => (playing = true)}>▶ Play</button>
                  {/if}
                </div>
              {/if}
            {/if}
          {:else}
            <div class="noPreview">Click a row to preview it</div>
          {/if}
        </div>
        {#if preview}
          <div class="pcap">
            <span class="pname" title={preview.path}>{preview.name}</span>
            <span class="pmeta">
              {#if preview.kind === "video" && !preview.error}{fmtDur(preview.duration)} · {frameLabel(preview)} · {fpsLabel(preview)} fps · {codecLabel(preview)} · {gb(preview.size)}{:else}{preview.kind === "photo" ? "Photo" : "Not a video"} · {gb(preview.size)}{/if}
            </span>
          </div>
        {/if}

        <div class="settings">
          {#if phase === "done" && result}
            <div class="doneBox">
              <div class="doneIcon">✓</div>
              <h3 title={result.path}>{baseName(result.path)}</h3>
              <p>{gb(result.bytes)} · {fmtLong(totalSecs)} · {clean.length} clips</p>
              <p class="hint2">
                {#if converting && target}Upload this file to YouTube as it is. It's encoded at {Math.round(target.bitrateKbps / 1000)} Mbps, well above what YouTube keeps, so nothing you'd see is lost.{:else}Upload this file to YouTube as it is: that gives YouTube the original quality.{/if}
                Your clips are untouched, so you can delete this file after the upload to get the space back.
              </p>
              <div class="doneActions">
                <button class="btn" onclick={() => api.reveal(result!.path)}>Show in folder</button>
                <button class="btn" onclick={() => openUrl("https://www.youtube.com/upload")}>Open YouTube upload</button>
                <button class="btn accent" onclick={onclose}>Done</button>
              </div>
            </div>
          {:else}
            {#if fixable.length || converting}
              <span class="fl">How to join</span>
              <div class="modes" role="radiogroup" aria-label="How to join">
                <button class="mode" class:on={!converting} role="radio" aria-checked={!converting} disabled={phase === "merging"} onclick={() => (mode = "copy")}>
                  <span class="mt">Lossless</span>
                  <span class="md">Exact camera quality. Only clips in the same format.</span>
                </button>
                <button class="mode" class:on={converting} role="radio" aria-checked={converting} disabled={phase === "merging"} onclick={() => (mode = "convert")}>
                  <span class="mt">Convert to match</span>
                  <span class="md">Brings in the {fixable.length} that differ. Re-encoded; takes a few minutes.</span>
                </button>
              </div>
            {/if}

            <div class="summary">
              <div class="big">{clean.length} clip{clean.length === 1 ? "" : "s"} · {fmtLong(totalSecs)} · about {gb(totalBytes)}</div>
              {#if converting && target}
                <div class="small dim">
                  Re-encoded at {Math.round(target.bitrateKbps / 1000)} Mbps{sourceMbps ? ` (the clips are ${sourceMbps})` : ""}, so it looks the same.
                  Needs {gb(needBytes)} free while it works.
                </div>
              {/if}
              {#if flagged.length}<div class="small warnText">Not counting the {flagged.length} highlighted item{flagged.length === 1 ? "" : "s"}.</div>{/if}
            </div>

            <label class="fl" for="mergeName">File name</label>
            <input id="mergeName" type="text" bind:value={name} spellcheck="false" disabled={phase === "merging"} />

            <span class="fl">Save to</span>
            <div class="dests">
              {#each dests as d (d.path)}
                <button class="dest" class:on={destDir === d.path} class:tight={d.free !== null && !fits(d)} disabled={phase === "merging"} onclick={() => (destDir = d.path)} title={d.path}>
                  <span class="dl">{d.label}</span>
                  <span class="df">{#if d.free === null}checking…{:else if !fits(d)}too small · {gb(d.free)} free{:else}{gb(d.free)} free{/if}</span>
                </button>
              {/each}
              <button class="dest choose" disabled={phase === "merging"} onclick={chooseFolder}>Choose another folder…</button>
            </div>

            {#if error}<p class="warn">{error}</p>{/if}

            {#if phase === "merging"}
              <div class="progress" role="progressbar" aria-valuenow={pct} aria-valuemin="0" aria-valuemax="100">
                <div class="bar"><div class="fill" style="width:{pct}%"></div></div>
                <span>{converting ? "Converting" : "Merging"}… {pct}%{eta ? ` · ${eta}` : ""}</span>
              </div>
            {/if}

            <div class="actions">
              {#if phase === "merging"}
                <span class="grow"></span>
                <button class="btn" onclick={() => api.cancelEditExport()}>Stop</button>
              {:else}
                {#if blocker}<span class="why">{blocker}</span>{/if}
                <span class="grow"></span>
                <button class="btn" onclick={onclose}>Cancel</button>
                <button class="btn accent" disabled={!!blocker} onclick={start}>{converting ? "Convert and merge" : "Merge"} {clean.length} clips</button>
              {/if}
            </div>
          {/if}
        </div>
      </section>
    </div>
  {/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onclose={() => (menu = null)} />
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 100; background: rgba(0, 0, 0, 0.66); backdrop-filter: blur(6px); }
  .panel {
    position: fixed; inset: 16px; z-index: 101; margin: auto;
    max-width: 1400px; max-height: 900px;
    display: flex; flex-direction: column; overflow: hidden;
    background: color-mix(in srgb, var(--bg-panel) 97%, transparent);
    border: 1px solid var(--border-strong); border-radius: var(--radius-xl); box-shadow: var(--shadow);
  }
  header { display: flex; align-items: center; gap: 10px; padding: 12px 18px; border-bottom: 1px solid var(--border-soft); flex-shrink: 0; }
  h2 { margin: 0; font-family: var(--font-display); font-size: 17px; letter-spacing: -0.015em; }
  .sub { margin: 2px 0 0; color: var(--text-dim); font-size: 12.5px; }
  .grow { flex: 1; }
  .x { width: 30px; height: 30px; border-radius: 7px; color: var(--text-dim); font-size: 13px; }
  .x:hover { background: var(--bg-hover); color: var(--text); }
  .center { padding: 60px 18px; text-align: center; color: var(--text-dim); }
  .center.err { color: var(--reject); }

  .body { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) minmax(300px, 420px); }
  @media (max-width: 1150px) {
    .body { grid-template-columns: minmax(0, 1fr) 330px; }
  }
  .left, .right { min-height: 0; display: flex; flex-direction: column; }
  .left { padding: 12px 14px 10px 18px; gap: 8px; border-right: 1px solid var(--border-soft); container-type: inline-size; }
  .left > *, .right > * { flex-shrink: 0; }
  .right { overflow-y: auto; }

  .basis { display: flex; align-items: center; flex-wrap: wrap; gap: 5px; }
  .basisLabel { margin-right: 2px; color: var(--text-faint); font-size: 11px; font-weight: 650; letter-spacing: 0.06em; text-transform: uppercase; }
  .fmt { padding: 2px 8px; border: 1px solid var(--border-soft); border-radius: 999px; background: color-mix(in srgb, var(--bg-elev) 70%, transparent); color: var(--text); font-size: 12px; font-weight: 560; }
  .basisNote { margin-left: 4px; color: var(--text-faint); font-size: 11.5px; }
  .toolbar { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .count { color: var(--text-dim); font-size: 12.5px; font-variant-numeric: tabular-nums; }
  .flagChip, .okChip { padding: 2px 8px; border-radius: 999px; font-size: 11.5px; font-weight: 600; }
  .flagChip { background: color-mix(in srgb, var(--star) 15%, transparent); color: var(--star); }
  .okChip { background: color-mix(in srgb, var(--pick) 15%, transparent); color: var(--pick); }
  .convChip { padding: 2px 8px; border-radius: 999px; background: color-mix(in srgb, var(--accent) 14%, transparent); color: var(--accent); font-size: 11.5px; font-weight: 600; }
  .warnText { color: var(--star); font-weight: 600; }

  /* The sequence table. Plain rows (not <table>) so rows can be dragged. */
  .table { flex: 1 1 auto !important; min-height: 120px; overflow: auto; border: 1px solid var(--border-soft); border-radius: var(--radius-md); background: color-mix(in srgb, var(--bg-elev) 40%, transparent); }
  .row {
    position: relative;
    display: grid;
    grid-template-columns: 44px minmax(150px, 1fr) 84px 50px 86px 54px 88px 70px 62px 30px;
    align-items: center; column-gap: 8px;
    min-height: 30px; padding: 0 6px 0 8px;
    border-bottom: 1px solid var(--border-soft);
    font-size: 12.5px; color: var(--text); cursor: default; user-select: none;
  }
  .row.head { position: sticky; top: 0; z-index: 1; min-height: 28px; background: var(--bg-panel); color: var(--text-faint); font-size: 10.5px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; }
  .row:not(.head):hover { background: color-mix(in srgb, var(--bg-hover) 70%, transparent); }
  .row.flag { background: color-mix(in srgb, var(--star) 3%, transparent); }
  .row.sel, .row.sel:hover { background: color-mix(in srgb, var(--select) 16%, transparent); }
  .row.previewing { box-shadow: inset 2px 0 0 var(--accent); }
  .row.flag { box-shadow: inset 3px 0 0 var(--star); }
  .row.adjusted { box-shadow: inset 3px 0 0 color-mix(in srgb, var(--accent) 70%, transparent); }
  .row.flag.previewing { box-shadow: inset 3px 0 0 var(--star), inset 5px 0 0 var(--accent); }
  .row.dropBefore { box-shadow: inset 0 2px 0 var(--accent); }
  .row.dropAfter { box-shadow: inset 0 -2px 0 var(--accent); }
  .row > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .c-idx { color: var(--text-faint); }
  .grip { margin-right: 4px; color: var(--text-faint); letter-spacing: -2px; cursor: grab; opacity: 0.5; }
  .row:hover .grip { opacity: 1; }
  .kind { flex: none; margin-right: 7px; color: var(--text-faint); }
  /* Middle truncation, as Finder does: camera names differ at the END
     (DJI_20260922000747_0004_D.MP4), so that part must stay visible. */
  .row > .c-name { display: flex; align-items: center; }
  .nmA { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nmB { flex: none; white-space: nowrap; }
  .c-when, .c-len, .c-size { color: var(--text-dim); }
  .c-span { grid-column: 4 / span 5; display: flex; align-items: center; gap: 6px; color: color-mix(in srgb, var(--reject) 80%, var(--text-dim)); font-size: 12px; }
  .c-span svg { flex: none; opacity: 0.85; }
  .conv { justify-self: start; max-width: 100%; padding: 1px 5px; border-radius: 5px; background: color-mix(in srgb, var(--accent) 13%, transparent); color: var(--accent); font-weight: 600; cursor: help; }
  .bad { justify-self: start; max-width: 100%; padding: 1px 5px; border-radius: 5px; background: color-mix(in srgb, var(--star) 17%, transparent); color: var(--star); font-weight: 620; cursor: help; }
  .c-rm { display: flex; justify-content: flex-end; }
  .rm { display: grid; place-items: center; width: 22px; height: 22px; border: 1px solid transparent; border-radius: 6px; color: var(--text-faint); opacity: 0.55; transition: opacity 100ms ease, background 100ms ease, color 100ms ease; }
  .row:hover .rm, .row.sel .rm, .row.flag .rm { opacity: 1; }
  .rm:hover:not(:disabled) { border-color: color-mix(in srgb, var(--reject) 40%, transparent); background: color-mix(in srgb, var(--reject) 12%, transparent); color: var(--reject); }
  .empty { padding: 24px; text-align: center; color: var(--text-faint); font-size: 12.5px; }
  .keys { margin: 0; color: var(--text-faint); font-size: 11px; }

  /* Narrow list: drop the columns you can live without (the row's hover
     titles and the preview caption still carry them), so the name keeps room. */
  @container (max-width: 760px) {
    .row { grid-template-columns: 40px minmax(150px, 1fr) 50px 86px 54px 88px 70px 28px; }
    .c-when, .c-size { display: none; }
    .c-span { grid-column: 3 / span 5; }
  }
  @container (max-width: 600px) {
    .row { grid-template-columns: 40px minmax(140px, 1fr) 46px 84px 52px 86px 28px; }
    .c-audio { display: none; }
    .c-span { grid-column: 3 / span 4; }
  }

  .preview { position: relative; height: 250px; margin: 12px 14px 0; border-radius: var(--radius-md); overflow: hidden; background: #050607; display: flex; align-items: center; justify-content: center; }
  .preview img, .preview video { width: 100%; height: 100%; object-fit: contain; background: #050607; }
  .stage { width: 100%; height: 100%; }
  .playbar { position: absolute; left: 0; right: 0; bottom: 0; display: flex; align-items: center; gap: 8px; padding: 8px 10px; background: linear-gradient(transparent, rgba(0, 0, 0, 0.7)); }
  .playbar .hint { flex: 1; color: rgba(255, 255, 255, 0.7); font-size: 11.5px; }
  .playbar .pf { flex: 1; color: #fff; font-size: 12px; }
  .noPreview { color: var(--text-faint); font-size: 12.5px; }
  .pcap { display: flex; flex-direction: column; gap: 2px; margin: 8px 14px 0; }
  .pname { overflow: hidden; color: var(--text); font-size: 12.5px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .pmeta { color: var(--text-dim); font-size: 11.5px; }

  .settings { display: flex; flex-direction: column; gap: 7px; padding: 12px 14px 14px; margin-top: 10px; border-top: 1px solid var(--border-soft); }
  .settings > * { flex-shrink: 0; }
  .summary { padding: 10px 12px; border: 1px solid var(--border-soft); border-radius: var(--radius-md); background: color-mix(in srgb, var(--accent) 7%, var(--bg-elev)); }
  .big { color: var(--text); font-size: 14.5px; font-weight: 650; font-variant-numeric: tabular-nums; }
  .small { margin-top: 2px; font-size: 12px; }
  .small.dim { color: var(--text-dim); line-height: 1.45; }
  .modes { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .mode { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; padding: 8px 10px; border: 1px solid var(--border-soft); border-radius: 9px; background: color-mix(in srgb, var(--bg-elev) 45%, transparent); text-align: left; }
  .mode:hover:not(:disabled) { border-color: var(--border); }
  .mode.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, var(--bg-elev)); }
  .mt { color: var(--text); font-size: 12.5px; font-weight: 650; }
  .md { color: var(--text-dim); font-size: 11.5px; line-height: 1.35; }
  .fl { margin-top: 4px; color: var(--text-faint); font-size: 10.5px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; }
  .settings input[type="text"] { min-height: var(--control-h); padding: 5px 10px; font-size: 13px; user-select: text; }
  .dests { display: flex; flex-direction: column; gap: 4px; }
  .dest { display: flex; align-items: center; gap: 8px; padding: 7px 10px; border: 1px solid var(--border-soft); border-radius: 8px; background: color-mix(in srgb, var(--bg-elev) 45%, transparent); color: var(--text); font-size: 12.5px; text-align: left; }
  .dest:hover:not(:disabled) { border-color: var(--border); }
  .dest.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, var(--bg-elev)); }
  .dl { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .df { flex: none; color: var(--text-faint); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .dest.tight .df { color: var(--reject); }
  .dest.choose { justify-content: center; color: var(--accent); }
  .warn { margin: 0; color: var(--reject); font-size: 12.5px; line-height: 1.5; }
  .progress { display: flex; flex-direction: column; gap: 6px; color: var(--text-dim); font-size: 12.5px; }
  .bar { height: 6px; border-radius: 999px; background: var(--bg-hover); overflow: hidden; }
  .fill { height: 100%; background: var(--accent); transition: width 300ms ease; }
  /* Always in view, however short the window: the button is the point. */
  .actions {
    position: sticky; bottom: 0; z-index: 1;
    display: flex; flex-wrap: wrap; align-items: center; gap: 8px;
    margin: 6px -14px -14px; padding: 10px 14px 14px;
    border-top: 1px solid var(--border-soft);
    background: var(--bg-panel);
    box-shadow: 0 -8px 16px color-mix(in srgb, var(--bg-panel) 85%, transparent);
  }
  /* What's still in the way, on its own line above the buttons. */
  .actions .why { flex: 1 0 100%; color: var(--star); font-size: 12px; line-height: 1.4; }

  .doneBox { padding: 14px 4px 4px; text-align: center; }
  .doneIcon { display: inline-grid; place-items: center; width: 40px; height: 40px; border-radius: 50%; background: color-mix(in srgb, var(--pick) 18%, transparent); color: var(--pick); font-size: 20px; }
  .doneBox h3 { margin: 10px 0 4px; overflow: hidden; color: var(--text); font-size: 14px; text-overflow: ellipsis; white-space: nowrap; }
  .doneBox p { margin: 0; color: var(--text-dim); font-size: 12.5px; }
  .doneBox .hint2 { margin-top: 10px; line-height: 1.55; }
  .doneActions { display: flex; flex-wrap: wrap; justify-content: center; gap: 6px; margin-top: 14px; }

  /* Short windows (TV size on a laptop): the list and the settings need the
     height more than a big preview does. */
  @media (max-height: 700px) {
    .preview { height: 170px; }
  }
</style>
