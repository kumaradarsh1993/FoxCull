<script lang="ts">
  // The Reel window (owner, 2026-10-04; docs/design/segments-and-reel-mode.md
  // §C): clips cut to the beat of a song, for an Instagram reel. Its own
  // window and workflow, not the Edit timeline.
  //
  //   Step 1: the clips (reorderable; a clip with marked segments gives one
  //           item per segment, or the whole clip when unticked) and the song
  //           (dropped or picked), whose beats are found automatically; choose
  //           the section of the song to use.
  //   Step 2: the beat board (ReelBoard.svelte).
  //
  // Clips arrive from the library (right-click → Create a reel synced to
  // music…, or ⌘C there and ⌘V here). A song is dropped from Finder/Explorer
  // (this window keeps the native file drop for that; see tool_windows.rs) or
  // chosen. The work is kept in localStorage, so closing the window loses
  // nothing.
  import { onDestroy, onMount } from "svelte";
  import { api } from "$lib/api";
  import type { BeatInfo, ClipRef, MediaItem, MergeClip, ReelInbox } from "$lib/types";
  import { normalize } from "$lib/segments";
  import { defaultSection, fmtS, pieceLen, piecesFor, snapToMajor, type Piece } from "$lib/reel";
  import Thumb from "./Thumb.svelte";
  import ReelBoard from "./ReelBoard.svelte";

  const SAVE_KEY = "foxcull-reel-v1";
  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
  const AUDIO_RE = /\.(mp3|m4a|aac|wav|flac|ogg|opus|aiff?)$/i;

  let clips = $state<ClipRef[]>([]);
  let meta = $state<Record<string, MergeClip>>({});
  /** Per clip: use its marked segments (default) or the whole clip. */
  let segUse = $state<Record<string, boolean>>({});
  /** Pieces taken out of the reel (a segment the owner removed). */
  let removed = $state<string[]>([]);
  /** Piece keys in reel order. */
  let order = $state<string[]>([]);
  let song = $state<string | null>(null);
  let info = $state<BeatInfo | null>(null);
  let analyzing = $state(false);
  let songErr = $state("");
  let sec = $state<[number, number] | null>(null);
  let offsets = $state<Record<string, number>>({});
  let pins = $state<Record<string, number>>({});
  let step = $state<1 | 2>(1);
  let name = $state("");
  let destDir = $state("");
  let probing = $state(false);
  let restored = false;

  type Notice = { id: number; text: string; warn: boolean };
  let notices = $state<Notice[]>([]);
  let seq = 0;
  function notify(text: string, warn = false) {
    const id = ++seq;
    notices = [...notices, { id, text, warn }].slice(-4);
    if (!warn) setTimeout(() => (notices = notices.filter((n) => n.id !== id)), 6000);
  }

  // ── pieces ────────────────────────────────────────────────────────────────
  const segsOf = (c: ClipRef) => normalize(c.ranges ?? [], meta[c.path]?.duration || Infinity);
  const usable = (c: ClipRef) => !meta[c.path] || (meta[c.path].kind === "video" && !meta[c.path].error);
  let byKey = $derived.by(() => {
    const m: Record<string, Piece> = {};
    for (const c of clips) {
      if (!usable(c)) continue;
      for (const p of piecesFor(c.path, c.name, meta[c.path]?.duration ?? 0, segsOf(c), segUse[c.path] ?? true)) m[p.key] = p;
    }
    return m;
  });
  let pieces = $derived(order.map((k) => byKey[k]).filter((p): p is Piece => !!p && !removed.includes(p.key)));
  let refByPath = $derived(Object.fromEntries(clips.map((c) => [c.path, c])) as Record<string, ClipRef>);
  let segClips = $derived(clips.filter((c) => usable(c) && segsOf(c).length));
  let segHead = $derived<"all" | "some" | "none">(
    !segClips.length ? "none" : segClips.every((c) => segUse[c.path] ?? true) ? "all" : segClips.some((c) => segUse[c.path] ?? true) ? "some" : "none",
  );
  let totalLen = $derived(pieces.reduce((s, p) => s + pieceLen(p), 0));

  /** Keep the order when pieces change: a clip switched between segments and
   *  whole takes the place of its first piece; new clips go at the end. */
  function rebuildOrder() {
    const want = new Map<string, string[]>();
    for (const p of Object.values(byKey)) {
      const list = want.get(p.path) ?? [];
      list.push(p.key);
      want.set(p.path, list);
    }
    const next: string[] = [];
    const replaced = new Set<string>();
    for (const k of order) {
      const path = k.slice(0, k.lastIndexOf("#"));
      const keys = want.get(path);
      if (!keys) continue;
      if (keys.includes(k)) {
        if (!next.includes(k)) next.push(k);
        continue;
      }
      if (!replaced.has(path)) for (const nk of keys) if (!order.includes(nk) && !next.includes(nk)) next.push(nk);
      replaced.add(path);
    }
    for (const c of clips) for (const k of want.get(c.path) ?? []) if (!next.includes(k)) next.push(k);
    order = next;
  }

  function setSegUse(paths: string[], on: boolean) {
    const next = { ...segUse };
    for (const p of paths) next[p] = on;
    segUse = next;
    rebuildOrder();
  }

  // ── bringing clips in ─────────────────────────────────────────────────────
  async function addClips(refs: ClipRef[]) {
    const vids = refs.filter((r) => r.kind === "video" && !r.missing);
    const left = refs.length - vids.length;
    const fresh = vids.filter((r) => !clips.some((c) => c.path === r.path));
    // Already here: take the segments as they're marked now.
    clips = clips.map((c) => vids.find((r) => r.path === c.path) ?? c);
    if (fresh.length) clips = [...clips, ...fresh];
    removed = removed.filter((k) => !vids.some((r) => k.startsWith(`${r.path}#`)));
    rebuildOrder();
    if (fresh.length) await probe(fresh.map((r) => r.path));
    if (!destDir && clips[0]) destDir = parentOf(clips[0].path);
    if (!name) name = defaultName();
    if (fresh.length) notify(`Added ${fresh.length} clip${fresh.length === 1 ? "" : "s"}.`);
    else if (vids.length) notify("Those clips are already in the reel (their segments are updated).");
    if (left) notify(`${left} item${left === 1 ? "" : "s"} left out: a reel takes videos.`, true);
  }

  async function probe(paths: string[]) {
    probing = true;
    try {
      const res = await api.mergeProbe(paths);
      const next = { ...meta };
      for (const m of res) next[m.path] = m;
      meta = next;
      const bad = res.filter((m) => m.kind !== "video" || m.error);
      if (bad.length) notify(`${bad.map((b) => b.name).join(", ")}: can't be read, left out.`, true);
    } catch (e) {
      notify(`Couldn't read the clips: ${e}`, true);
    } finally {
      probing = false;
    }
    await refreshRanges(paths);
  }

  /** Segments as the catalog has them now (marked or changed in the library
   *  since the clips were sent), read from each clip's own drive. */
  async function refreshRanges(paths = clips.map((c) => c.path)) {
    if (!paths.length) return;
    const got = await api.videoRanges(paths).catch(() => null);
    if (!got) return;
    let changed = false;
    clips = clips.map((c) => {
      const r = got[c.path];
      if (!r || JSON.stringify(r) === JSON.stringify(c.ranges ?? [])) return c;
      changed = true;
      return { ...c, ranges: r };
    });
    if (changed) rebuildOrder();
  }

  async function drainInbox() {
    const items = await api.takeToolInbox<ReelInbox>("reel").catch(() => [] as ReelInbox[]);
    for (const it of items) if (it?.type === "add") await addClips(it.clips);
  }

  async function paste() {
    const refs = await api.stashGet<ClipRef[]>("clips");
    if (!refs?.length) {
      notify(`Nothing copied yet: select clips in the library and press ${isMac ? "⌘" : "Ctrl+"}C, then paste here.`, true);
      return;
    }
    await addClips(refs);
  }

  function removePiece(key: string) {
    const p = byKey[key];
    if (!p) return;
    if (p.seg == null) {
      clips = clips.filter((c) => c.path !== p.path);
      rebuildOrder();
    } else {
      removed = [...removed, key];
    }
  }

  function movePiece(key: string, delta: number) {
    const vis = pieces.map((p) => p.key);
    const i = vis.indexOf(key);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= vis.length) return;
    [vis[i], vis[j]] = [vis[j], vis[i]];
    order = [...vis, ...order.filter((k) => !vis.includes(k))];
  }

  // ── the song ──────────────────────────────────────────────────────────────
  async function setSong(path: string, keepSection = false) {
    if (!AUDIO_RE.test(path)) {
      songErr = "That isn't an audio file. Drop an MP3, M4A, AAC, WAV, FLAC, OGG, Opus or AIFF.";
      return;
    }
    stopAudio();
    songErr = "";
    analyzing = true;
    const prev = song;
    song = path;
    info = null;
    try {
      const got = await api.analyzeBeats(path);
      if (song !== path) return;
      info = got;
      const fits = keepSection && sec && sec[1] <= got.duration + 0.01;
      if (!fits || prev !== path) sec = keepSection && fits ? sec : defaultSection(got, got.duration, pieces);
      if (got.beats.length < 4) notify("No steady beat found in this song, so the cuts will be spaced evenly. A song with a clearer rhythm cuts better.", true);
    } catch (e) {
      songErr = String(e);
      song = null;
    } finally {
      analyzing = false;
    }
  }

  async function chooseSong() {
    const p = await api.pickAudio();
    if (p) await setSong(p);
  }

  // Native drop (a song from Finder/Explorer, with its path).
  let dropHot = $state(false);
  let unDrop: (() => void) | null = null;
  async function listenNativeDrop() {
    try {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      unDrop = await getCurrentWebview().onDragDropEvent((e) => {
        const t = e.payload.type;
        if (t === "enter" || t === "over") dropHot = true;
        else if (t === "leave") dropHot = false;
        else if (t === "drop") {
          dropHot = false;
          void onDroppedPaths(e.payload.paths);
        }
      });
    } catch {
      /* the browser harness: HTML5 drop below */
    }
  }
  async function onDroppedPaths(paths: string[]) {
    const audio = paths.find((p) => AUDIO_RE.test(p));
    if (audio) {
      await setSong(audio);
      return;
    }
    if (paths.length) notify(`Drop a song here. To add clips, select them in the library and press ${isMac ? "⌘" : "Ctrl+"}C, then ${isMac ? "⌘" : "Ctrl+"}V here.`, true);
  }
  // Browser harness only: HTML5 drops carry no paths, so make one up.
  function onHtmlDrop(e: DragEvent) {
    dropHot = false;
    const f = e.dataTransfer?.files?.[0];
    if (!f || !import.meta.env.DEV) return;
    e.preventDefault();
    void onDroppedPaths([`/Users/demo/Music/${f.name}`]);
  }

  // ── the song section, on the waveform ─────────────────────────────────────
  let waveEl = $state<HTMLDivElement | null>(null);
  let audioEl = $state<HTMLAudioElement | null>(null);
  let audioT = $state<number | null>(null);
  let audioEnd = 0;
  let audioRaf = 0;
  const W = 1000;
  /** The waveform's view: the whole song, or zoomed around the section. */
  let zoomed = $state(false);
  let view = $derived.by((): [number, number] => {
    if (!info) return [0, 1];
    if (!zoomed || !sec) return [0, info.duration];
    const pad = Math.max(4, (sec[1] - sec[0]) * 0.35);
    return [Math.max(0, sec[0] - pad), Math.min(info.duration, sec[1] + pad)];
  });
  // Frozen while dragging, so the view doesn't run away under the pointer.
  let viewHold = $state<[number, number] | null>(null);
  let vw = $derived(viewHold ?? view);
  let waveCols = $derived.by(() => {
    if (!info?.wave.length) return "";
    const n = info.wave.length;
    const [v0, v1] = vw;
    const i0 = (v0 / info.duration) * n;
    const span = ((v1 - v0) / info.duration) * n;
    let d = "";
    for (let x = 0; x < W; x += 2) {
      const a = Math.floor(i0 + (x / W) * span);
      const b = Math.max(a + 1, Math.floor(i0 + ((x + 2) / W) * span));
      let m = 0;
      for (let i = a; i < b && i < n; i++) m = Math.max(m, info.wave[i]);
      const h = Math.max(1, m * 44);
      d += `M${x + 1},${50 - h}V${50 + h}`;
    }
    return d;
  });
  const xOf = (t: number) => ((t - vw[0]) / Math.max(1e-6, vw[1] - vw[0])) * 100;
  let showMinor = $derived(!!info && ((waveEl?.clientWidth ?? 600) * (60 / Math.max(info.bpm || 120, 1))) / (vw[1] - vw[0]) > 5);
  const inView = (t: number) => t >= vw[0] && t <= vw[1];

  type WDrag = { mode: "l" | "r" | "move" | "new"; t0: number; s0: [number, number]; x0: number; moved: boolean };
  let wdrag: WDrag | null = null;
  function tAt(clientX: number) {
    const r = waveEl!.getBoundingClientRect();
    return vw[0] + Math.max(0, Math.min(1, (clientX - r.left) / r.width)) * (vw[1] - vw[0]);
  }
  function snapT(t: number, free: boolean) {
    if (!info || free) return t;
    const r = waveEl!.getBoundingClientRect();
    return snapToMajor(t, info, (8 / r.width) * (vw[1] - vw[0]));
  }
  function onWaveDown(e: PointerEvent) {
    if (!info || !sec || e.button !== 0) return;
    e.preventDefault();
    waveEl!.setPointerCapture(e.pointerId);
    const r = waveEl!.getBoundingClientRect();
    const px = (t: number) => r.left + ((t - vw[0]) / (vw[1] - vw[0])) * r.width;
    viewHold = [vw[0], vw[1]];
    const t = tAt(e.clientX);
    const mode = Math.abs(e.clientX - px(sec[0])) <= 7 ? "l" : Math.abs(e.clientX - px(sec[1])) <= 7 ? "r" : t > sec[0] && t < sec[1] ? "move" : "new";
    wdrag = { mode, t0: t, s0: [sec[0], sec[1]], x0: e.clientX, moved: false };
  }
  function onWaveMove(e: PointerEvent) {
    if (!wdrag || !info || !sec) return;
    if (Math.abs(e.clientX - wdrag.x0) > 3) wdrag.moved = true;
    if (!wdrag.moved) return;
    const t = tAt(e.clientX);
    const dur = info.duration;
    const [a, b] = wdrag.s0;
    if (wdrag.mode === "l") sec = [Math.min(snapT(t, e.altKey), b - 1), b];
    else if (wdrag.mode === "r") sec = [a, Math.max(snapT(t, e.altKey), a + 1)];
    else if (wdrag.mode === "move") {
      const len = b - a;
      let na = Math.max(0, Math.min(dur - len, a + (t - wdrag.t0)));
      na = snapT(na, e.altKey);
      na = Math.max(0, Math.min(dur - len, na));
      sec = [na, na + len];
    } else {
      // Drawing a new section from where the drag began.
      const na = snapT(Math.min(wdrag.t0, t), e.altKey);
      let nb = snapT(Math.max(wdrag.t0, t), e.altKey);
      if (nb - na < 1) nb = Math.min(dur, na + 1);
      sec = [na, nb];
    }
  }
  function onWaveUp(e: PointerEvent) {
    if (!wdrag) return;
    const click = !wdrag.moved;
    wdrag = null;
    viewHold = null;
    // A click auditions the song from there (to the section's end, or 8 s).
    if (click && info && sec) {
      const t = tAt(e.clientX);
      playAudio(t, t < sec[1] && t >= sec[0] ? sec[1] : Math.min(info.duration, t + 8));
    }
  }

  function playAudio(from: number, to: number) {
    if (!audioEl || !song) return;
    stopAudio();
    audioEnd = to;
    audioEl.currentTime = from;
    void audioEl.play().catch(() => notify("Couldn't play the song here.", true));
    const tick = () => {
      if (!audioEl || audioEl.paused) {
        audioT = null;
        return;
      }
      audioT = audioEl.currentTime;
      if (audioEl.currentTime >= audioEnd) {
        stopAudio();
        return;
      }
      audioRaf = requestAnimationFrame(tick);
    };
    audioRaf = requestAnimationFrame(tick);
  }
  function stopAudio() {
    if (audioRaf) cancelAnimationFrame(audioRaf);
    audioRaf = 0;
    audioEl?.pause();
    audioT = null;
  }

  // ── step 1 list: drag to reorder (pointer, not HTML5: the native file drop
  //    owns HTML5 drags in this window) ──────────────────────────────────────
  let listEl = $state<HTMLDivElement | null>(null);
  let rowDrag = $state<{ key: string; y0: number; over: number | null } | null>(null);
  function onGripDown(e: PointerEvent, key: string) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    rowDrag = { key, y0: e.clientY, over: null };
  }
  function onGripMove(e: PointerEvent) {
    if (!rowDrag || !listEl) return;
    const rows = [...listEl.querySelectorAll<HTMLElement>(".prow")];
    let over = rows.length;
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i].getBoundingClientRect();
      if (e.clientY < r.top + r.height / 2) {
        over = i;
        break;
      }
    }
    rowDrag = { ...rowDrag, over };
  }
  function onGripUp() {
    if (!rowDrag) return;
    const { key, over } = rowDrag;
    rowDrag = null;
    if (over == null) return;
    const vis = pieces.map((p) => p.key);
    const from = vis.indexOf(key);
    if (from < 0) return;
    vis.splice(from, 1);
    vis.splice(over > from ? over - 1 : over, 0, key);
    order = [...vis, ...order.filter((k) => !vis.includes(k))];
  }

  // ── helpers ───────────────────────────────────────────────────────────────
  const parentOf = (p: string) => p.replace(/[\\/][^\\/]*$/, "");
  const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;
  function defaultName() {
    const t = clips.map((c) => c.mtime).filter(Boolean);
    const d = new Date((t.length ? Math.min(...t) : Date.now() / 1000) * 1000);
    return `Reel ${d.getDate()} ${d.toLocaleString(undefined, { month: "short" })} ${d.getFullYear()}`;
  }
  const asItem = (c: ClipRef): MediaItem => ({
    name: c.name,
    path: c.path,
    rel: "",
    kind: c.kind,
    ext: c.ext,
    mtime: c.mtime,
    size: c.size,
    rating: 0,
    label: null,
    flag: null,
    tags: [],
    events: [],
    missing: false,
  });
  const shapeOf = (path: string) => {
    const m = meta[path];
    if (!m) return null;
    const [w, h] = m.rotation % 180 ? [m.height, m.width] : [m.width, m.height];
    return h > w ? "portrait" : h === w ? "square" : "landscape";
  };

  let blocker = $derived(
    !pieces.length
      ? "Add clips from the library first"
      : probing
        ? "Reading the clips…"
        : !song
          ? "Drop a song (or choose one)"
          : analyzing
            ? "Finding the beats…"
            : !info || !sec
              ? "Choose a song"
              : null,
  );

  // ── keeping the work ──────────────────────────────────────────────────────
  type Saved = {
    v: 1;
    clips: ClipRef[];
    segUse: Record<string, boolean>;
    removed: string[];
    order: string[];
    song: string | null;
    sec: [number, number] | null;
    offsets: Record<string, number>;
    pins: Record<string, number>;
    step: 1 | 2;
    name: string;
    destDir: string;
  };
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const st: Saved = {
      v: 1,
      clips: $state.snapshot(clips) as ClipRef[],
      segUse: { ...segUse },
      removed: [...removed],
      order: [...order],
      song,
      sec: sec ? [sec[0], sec[1]] : null,
      offsets: { ...offsets },
      pins: { ...pins },
      step,
      name,
      destDir,
    };
    if (!restored) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      try {
        if (st.clips.length || st.song) localStorage.setItem(SAVE_KEY, JSON.stringify(st));
        else localStorage.removeItem(SAVE_KEY);
      } catch {
        /* storage blocked: the reel just won't survive a close */
      }
    }, 400);
  });

  async function restore() {
    let st: Saved | null = null;
    try {
      st = JSON.parse(localStorage.getItem(SAVE_KEY) || "null");
    } catch {
      st = null;
    }
    if (st?.v === 1) {
      clips = st.clips ?? [];
      segUse = st.segUse ?? {};
      removed = st.removed ?? [];
      order = st.order ?? [];
      offsets = st.offsets ?? {};
      pins = st.pins ?? {};
      name = st.name ?? "";
      destDir = st.destDir ?? "";
      sec = st.sec;
      if (clips.length) await probe(clips.map((c) => c.path));
      rebuildOrder();
      const gone = clips.filter((c) => meta[c.path]?.error);
      if (st.song) await setSong(st.song, true);
      if (st.step === 2 && pieces.length && info && sec) step = 2;
      if (clips.length) notify(`Your reel from last time is back (${pieces.length} clip${pieces.length === 1 ? "" : "s"}). Start over clears it.`);
      if (gone.length) notify(`Not available right now: ${gone.map((g) => g.name).join(", ")}. Is that drive plugged in?`, true);
    }
    restored = true;
  }

  function startOver() {
    stopAudio();
    clips = [];
    meta = {};
    segUse = {};
    removed = [];
    order = [];
    song = null;
    info = null;
    sec = null;
    offsets = {};
    pins = {};
    step = 1;
    name = "";
    songErr = "";
  }

  function onkeydown(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "v") {
      e.preventDefault();
      void paste();
      return;
    }
    if (step === 1 && e.key === " " && song) {
      e.preventDefault();
      if (audioT != null) stopAudio();
      else if (sec) playAudio(sec[0], sec[1]);
    }
  }

  let unInbox: (() => void) | null = null;
  onMount(async () => {
    document.title = "FoxCull Reel";
    await restore();
    await drainInbox();
    unInbox = await api.onToolInbox(() => void drainInbox()).catch(() => null);
    void listenNativeDrop();
  });
  onDestroy(() => {
    unInbox?.();
    unDrop?.();
    stopAudio();
    if (saveTimer) clearTimeout(saveTimer);
  });
</script>

<svelte:window {onkeydown} onfocus={() => step === 1 && !probing && void refreshRanges()} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="reel"
  class:dropHot
  ondragover={(e) => {
    if (e.dataTransfer?.types.includes("Files")) {
      e.preventDefault();
      dropHot = true;
    }
  }}
  ondragleave={(e) => {
    if (e.relatedTarget === null) dropHot = false;
  }}
  ondrop={onHtmlDrop}
>
  <header>
    <h2>Reel</h2>
    <ol class="steps" aria-label="Steps">
      <li class:on={step === 1}><button onclick={() => (step = 1)}>1 · Clips and song</button></li>
      <li class:on={step === 2}><button disabled={!!blocker} onclick={() => (step = 2)}>2 · Beat board</button></li>
    </ol>
    <span class="grow"></span>
    {#if clips.length || song}<button class="btn sm" onclick={startOver} title="Clear the clips, the song and the cuts">Start over</button>{/if}
  </header>

  {#if step === 1}
    <div class="s1">
      <!-- ░ the clips ░ -->
      <section class="clips">
        <div class="bar">
          <span class="count">{pieces.length} {pieces.length === 1 ? "piece" : "pieces"}{#if pieces.length}{" · "}{fmtS(totalLen)} of footage{/if}</span>
          {#if segClips.length}
            <label class="segAll" title="Clips with in/out segments marked in the library: use each segment as its own piece (ticked), or the whole clip">
              <input type="checkbox" checked={segHead === "all"} indeterminate={segHead === "some"} onchange={() => setSegUse(segClips.map((c) => c.path), segHead !== "all")} />
              Use marked segments
            </label>
          {/if}
          <span class="grow"></span>
          <button class="btn sm" onclick={() => void paste()} title="Paste clips copied in the library ({isMac ? '⌘' : 'Ctrl+'}V)">Paste clips</button>
        </div>
        <div class="list" bind:this={listEl}>
          {#each pieces as p, i (p.key)}
            {@const c = refByPath[p.path]}
            {@const shape = shapeOf(p.path)}
            <div class="prow" class:dragging={rowDrag?.key === p.key} class:dropBefore={rowDrag?.over === i && rowDrag.key !== p.key} class:dropAfter={rowDrag?.over === pieces.length && i === pieces.length - 1}>
              <button class="grip" aria-label="Drag to reorder" title="Drag to reorder (or ⌥↑ ⌥↓ on the board)" onpointerdown={(e) => onGripDown(e, p.key)} onpointermove={onGripMove} onpointerup={onGripUp} onpointercancel={() => (rowDrag = null)}>⋮⋮</button>
              <span class="idx">{i + 1}</span>
              <span class="th">{#if c}<Thumb item={asItem(c)} size={160} />{/if}</span>
              <span class="pname">
                <span class="n" title={p.path}>{p.name}</span>
                <span class="sub">
                  {#if p.seg != null}Segment {p.seg + 1} of {p.segCount} · {fmtS(p.srcIn)}–{fmtS(p.srcOut)}{:else}Whole clip{#if p.segCount} (has {p.segCount} segment{p.segCount === 1 ? "" : "s"}){/if}{/if}
                  {#if shape === "landscape"}<span class="crop" title="Landscape: cropped to the middle of the frame for a portrait reel. Choosing the crop comes later.">· cropped to 9:16</span>{/if}
                </span>
              </span>
              <span class="len">{meta[p.path] ? fmtS(pieceLen(p)) : "…"}</span>
              {#if c && segsOf(c).length}
                <label class="useSeg" title="Use this clip's marked segments as separate pieces (unticked: the whole clip)">
                  <input type="checkbox" checked={segUse[p.path] ?? true} onchange={(e) => setSegUse([p.path], (e.currentTarget as HTMLInputElement).checked)} />✂
                </label>
              {:else}<span></span>{/if}
              <button class="rm" onclick={() => removePiece(p.key)} title={p.seg != null ? "Take this segment out of the reel" : "Take this clip out of the reel"} aria-label="Remove">−</button>
            </div>
          {:else}
            <div class="empty">
              <p><b>No clips yet.</b></p>
              <p>In the library, select clips and right-click → <b>Create a reel synced to music…</b></p>
              <p>or copy them ({isMac ? "⌘" : "Ctrl+"}C) and paste here ({isMac ? "⌘" : "Ctrl+"}V).</p>
            </div>
          {/each}
        </div>
      </section>

      <!-- ░ the song ░ -->
      <section class="song">
        {#if !song && !analyzing}
          <div class="drop" class:hot={dropHot}>
            <div class="dIcon" aria-hidden="true">♫</div>
            <p class="dT">Drop a song here</p>
            <p class="dS">Any MP3, M4A, WAV, FLAC… The beats are found automatically.</p>
            <button class="btn accent" onclick={() => void chooseSong()}>Choose a song…</button>
            {#if songErr}<p class="err">{songErr}</p>{/if}
          </div>
        {:else}
          <div class="songHead">
            <div class="sname">
              <span class="n" title={song ?? ""}>{song ? baseName(song) : ""}</span>
              <span class="sub">{#if analyzing}Finding the beats…{:else if info}{fmtS(info.duration)} · {info.bpm ? `${Math.round(info.bpm)} BPM` : "no steady beat"} · {info.beats.length} beats, {info.major.length} strong{/if}</span>
            </div>
            <button class="btn sm" onclick={() => void chooseSong()}>Change…</button>
          </div>
          {#if info && sec}
            <p class="hint">Drag the highlighted section to choose the part of the song; drag its edges to resize (they snap to the strong beats; hold ⌥ for free). Click anywhere to listen from there.</p>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="wave" bind:this={waveEl} onpointerdown={onWaveDown} onpointermove={onWaveMove} onpointerup={onWaveUp} onpointercancel={() => { wdrag = null; viewHold = null; }}>
              <svg viewBox="0 0 {W} 100" preserveAspectRatio="none" aria-hidden="true">
                <path d={waveCols} class="wpath" />
              </svg>
              {#each info.major.filter(inView) as m (m)}<span class="tick maj" style:left="{xOf(m)}%"></span>{/each}
              {#if showMinor}{#each info.beats.filter(inView) as b (b)}<span class="tick" style:left="{xOf(b)}%"></span>{/each}{/if}
              <div class="sel" style:left="{xOf(sec[0])}%" style:width="{xOf(sec[1]) - xOf(sec[0])}%">
                <span class="edge l"></span><span class="edge r"></span>
              </div>
              {#if audioT != null}<span class="ph" style:left="{xOf(audioT)}%"></span>{/if}
            </div>
            <div class="secInfo">
              <span><b>{fmtS(sec[0])} – {fmtS(sec[1])}</b> · {fmtS(sec[1] - sec[0])} of song{#if info.major.length}{" · "}{info.major.filter((m) => m >= sec![0] - 0.01 && m < sec![1] - 0.01).length} bars{/if}</span>
              <span class="grow"></span>
              <button class="btn sm" onclick={() => (zoomed = !zoomed)} title={zoomed ? "Show the whole song" : "Zoom in around the section, to place its edges on the beats you want"}>{zoomed ? "Whole song" : "Zoom to section"}</button>
              <button class="btn sm" onclick={() => (audioT != null ? stopAudio() : playAudio(sec![0], sec![1]))}>{audioT != null ? "Stop" : "▶ Play section"}</button>
            </div>
            <p class="fitNote">
              {#if pieces.length}
                {pieces.length} pieces over {fmtS(sec[1] - sec[0])}: about {fmtS((sec[1] - sec[0]) / pieces.length)} each, cut on the beats. You can stretch or move each one on the next step.
                {#if totalLen < sec[1] - sec[0]}<span class="warnT"> The clips add up to only {fmtS(totalLen)}: the reel will end early (or choose a shorter section).</span>{/if}
              {/if}
            </p>
          {:else if analyzing}
            <div class="analyzing"><span class="spin" aria-hidden="true"></span> Listening for the beats…</div>
          {/if}
          {#if songErr}<p class="err">{songErr}</p>{/if}
        {/if}
        {#if song}<audio bind:this={audioEl} src={api.fileSrc(song)} preload="auto"></audio>{/if}
      </section>
    </div>
    <footer>
      {#if blocker}<span class="why">{blocker}</span>{/if}
      <span class="grow"></span>
      <button class="btn accent" disabled={!!blocker} onclick={() => { stopAudio(); step = 2; }}>Next: beat board →</button>
    </footer>
  {:else if info && sec && song}
    <ReelBoard
      {pieces}
      {info}
      {sec}
      {song}
      {meta}
      refs={refByPath}
      bind:offsets
      bind:pins
      bind:name
      bind:destDir
      onback={() => (step = 1)}
      onmove={movePiece}
      onremove={removePiece}
      {notify}
    />
  {/if}

  {#if dropHot && step === 1}
    <div class="dropHint" aria-hidden="true">Drop the song to use it</div>
  {/if}
  {#if notices.length}
    <div class="notices" role="status" aria-live="polite">
      {#each notices as n (n.id)}
        <div class="notice" class:warn={n.warn}>
          <span>{n.text}</span>
          <button onclick={() => (notices = notices.filter((x) => x.id !== n.id))} aria-label="Dismiss">×</button>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .reel { position: fixed; inset: 0; display: flex; flex-direction: column; background: var(--bg-panel); color: var(--text); }
  .reel.dropHot { outline: 2px dashed var(--accent); outline-offset: -6px; }
  header { display: flex; align-items: center; gap: 14px; padding: 10px 16px; border-bottom: 1px solid var(--border-soft); flex: none; }
  h2 { margin: 0; font-family: var(--font-display); font-size: 17px; letter-spacing: -0.015em; }
  .steps { display: flex; gap: 4px; margin: 0; padding: 0; list-style: none; }
  .steps button { padding: 4px 10px; border-radius: 999px; color: var(--text-dim); font-size: 12.5px; font-weight: 600; }
  .steps li.on button { background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent); }
  .steps button:disabled { opacity: 0.45; }
  .grow { flex: 1; }

  .s1 { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(340px, 1fr) minmax(360px, 1.1fr); }
  @media (max-width: 980px) {
    .s1 { grid-template-columns: 1fr; grid-template-rows: minmax(300px, 1fr) auto; overflow-y: auto; }
    .clips { border-right: 0; border-bottom: 1px solid var(--border-soft); }
  }
  .clips { min-height: 0; display: flex; flex-direction: column; gap: 8px; padding: 12px 14px 12px 16px; border-right: 1px solid var(--border-soft); }
  .bar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; flex: none; }
  .count { color: var(--text-dim); font-size: 12.5px; font-variant-numeric: tabular-nums; }
  .segAll { display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--text); cursor: pointer; }
  .segAll input, .useSeg input { accent-color: var(--accent); }
  .list { flex: 1; min-height: 120px; overflow-y: auto; border: 1px solid var(--border-soft); border-radius: var(--radius-md); background: color-mix(in srgb, var(--bg-elev) 40%, transparent); }
  .prow { position: relative; display: grid; grid-template-columns: 22px 22px 64px minmax(0, 1fr) 54px 34px 26px; align-items: center; gap: 8px; min-height: 48px; padding: 4px 8px; border-bottom: 1px solid var(--border-soft); font-size: 12.5px; }
  .prow.dragging { opacity: 0.5; }
  .prow.dropBefore { box-shadow: inset 0 2px 0 var(--accent); }
  .prow.dropAfter { box-shadow: inset 0 -2px 0 var(--accent); }
  .grip { color: var(--text-faint); cursor: grab; letter-spacing: -2px; touch-action: none; }
  .idx { color: var(--text-faint); font-variant-numeric: tabular-nums; text-align: right; }
  .th { width: 64px; height: 40px; border-radius: 5px; overflow: hidden; background: #050607; }
  .pname { display: flex; flex-direction: column; min-width: 0; }
  .pname .n, .sname .n { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .sub { color: var(--text-dim); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .crop { color: var(--text-faint); }
  .len { color: var(--text-dim); text-align: right; font-variant-numeric: tabular-nums; }
  .useSeg { display: flex; align-items: center; gap: 2px; color: var(--text-dim); cursor: pointer; }
  .rm { width: 22px; height: 22px; border-radius: 6px; color: var(--text-faint); }
  .rm:hover { background: color-mix(in srgb, var(--reject) 12%, transparent); color: var(--reject); }
  .empty { padding: 28px 18px; color: var(--text-dim); font-size: 12.5px; text-align: center; line-height: 1.5; }
  .empty p { margin: 2px 0; }

  .song { min-height: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 10px; padding: 12px 16px; }
  .drop { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; min-height: 260px; padding: 24px; border: 2px dashed var(--border); border-radius: 14px; text-align: center; }
  .drop.hot { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 8%, transparent); }
  .dIcon { font-size: 34px; color: var(--accent); }
  .dT { margin: 0; font-size: 15px; font-weight: 650; }
  .dS { margin: 0 0 8px; color: var(--text-dim); font-size: 12.5px; }
  .songHead { display: flex; align-items: center; gap: 10px; }
  .sname { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .hint, .fitNote { margin: 0; color: var(--text-dim); font-size: 12px; line-height: 1.45; }
  .warnT { color: var(--star); }
  .wave { position: relative; height: 120px; border-radius: 10px; background: #0b0d10; overflow: hidden; cursor: crosshair; touch-action: none; user-select: none; }
  .wave svg { position: absolute; inset: 0; width: 100%; height: 100%; }
  .wpath { stroke: color-mix(in srgb, var(--accent) 55%, #9aa4b2); stroke-width: 1.4; vector-effect: non-scaling-stroke; }
  .tick { position: absolute; bottom: 0; width: 1px; height: 14%; background: rgba(255, 255, 255, 0.22); pointer-events: none; }
  .tick.maj { height: 30%; background: rgba(255, 255, 255, 0.5); }
  .sel { position: absolute; top: 0; bottom: 0; border: 2px solid var(--accent); border-radius: 6px; background: color-mix(in srgb, var(--accent) 16%, transparent); cursor: grab; }
  .edge { position: absolute; top: 0; bottom: 0; width: 8px; cursor: ew-resize; }
  .edge.l { left: -5px; }
  .edge.r { right: -5px; }
  .ph { position: absolute; top: 0; bottom: 0; width: 2px; margin-left: -1px; background: #fff; pointer-events: none; }
  .secInfo { display: flex; align-items: center; gap: 10px; font-size: 12.5px; font-variant-numeric: tabular-nums; }
  .analyzing { display: flex; align-items: center; gap: 10px; padding: 30px 0; color: var(--text-dim); }
  .spin { width: 16px; height: 16px; border: 2px solid var(--border); border-top-color: var(--accent); border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .err { margin: 0; color: var(--reject); font-size: 12.5px; }

  footer { display: flex; align-items: center; gap: 10px; padding: 10px 16px; border-top: 1px solid var(--border-soft); flex: none; }
  .why { color: var(--text-dim); font-size: 12.5px; }

  .dropHint { position: absolute; left: 50%; top: 60px; transform: translateX(-50%); z-index: 60; padding: 7px 14px; border-radius: 999px; background: var(--accent); color: var(--accent-on); font-size: 12px; font-weight: 600; box-shadow: var(--shadow); pointer-events: none; }
  .notices { position: absolute; right: 16px; bottom: 64px; z-index: 70; display: flex; flex-direction: column; gap: 8px; width: min(420px, calc(100vw - 32px)); }
  .notice { display: flex; align-items: flex-start; gap: 10px; padding: 10px 12px; border: 1px solid var(--border-strong); border-radius: 10px; background: color-mix(in srgb, var(--bg-elev) 96%, transparent); box-shadow: var(--shadow); font-size: 12.5px; line-height: 1.45; backdrop-filter: blur(16px); }
  .notice.warn { border-color: color-mix(in srgb, var(--star) 55%, var(--border)); box-shadow: inset 3px 0 0 var(--star), var(--shadow); }
  .notice span { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .notice button { width: 20px; height: 20px; border-radius: 5px; color: var(--text-faint); }
</style>
