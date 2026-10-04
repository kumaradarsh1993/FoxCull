<script lang="ts">
  // The beat board: step 2 of the Reel window (docs/design/segments-and-
  // reel-mode.md §C). Every piece one below the other as a strip of its
  // frames, with a window over the part that plays. The windows' lengths come
  // from the song's beats (rules in $lib/reel.ts):
  //   * drag a window to choose which part of the clip plays (never its length);
  //   * drag its right edge to make it longer or shorter: it snaps to the
  //     beats (the strong ones first; ⌥ for free), the ticks on the strip show
  //     where they fall, and the pieces after it re-flow (keeping where their
  //     windows sit on their clips);
  //   * a window longer than what's left of its clip shows the overflow in red;
  //   * hover a strip to see that frame in the portrait preview; ▶ plays the
  //     reel (or one piece) with the song.
  import { onDestroy } from "svelte";
  import { api } from "$lib/api";
  import { activity } from "$lib/activity.svelte";
  import { loadVideoFilmstrip } from "$lib/thumbnail-loader";
  import type { BeatInfo, ClipRef, FilmstripInfo, MediaItem, MergeClip } from "$lib/types";
  import { beatGap, fmtS, layout, longestFit, overflowOf, pieceLen, snapEnd, type Piece, type Slot } from "$lib/reel";
  import Thumb from "./Thumb.svelte";

  let {
    pieces,
    info,
    sec,
    song,
    meta,
    refs,
    offsets = $bindable(),
    pins = $bindable(),
    name = $bindable(),
    destDir = $bindable(),
    onback,
    onmove,
    onremove,
    notify,
  }: {
    pieces: Piece[];
    info: BeatInfo;
    sec: [number, number];
    song: string;
    meta: Record<string, MergeClip>;
    refs: Record<string, ClipRef>;
    offsets: Record<string, number>;
    pins: Record<string, number>;
    name: string;
    destDir: string;
    onback: () => void;
    onmove: (key: string, delta: number) => void;
    onremove: (key: string) => void;
    notify: (text: string, warn?: boolean) => void;
  } = $props();

  const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
  const STRIP_H = 54;

  let slots = $derived(layout(pieces, info, sec[0], sec[1], pins));
  let placed = $derived(slots.filter((s): s is Slot => !!s));
  let reelEnd = $derived(placed.length ? placed[placed.length - 1].start + placed[placed.length - 1].len : sec[0]);
  let reelLen = $derived(reelEnd - sec[0]);
  const off = (p: Piece) => offsets[p.key] ?? 0;
  let over = $derived(pieces.map((p, i) => overflowOf(p, off(p), slots[i])));
  let overflowing = $derived(pieces.filter((_, i) => over[i] > 0.02));
  let unplaced = $derived(slots.filter((s) => !s).length);
  let gap = $derived(beatGap(info.beats));

  // ── zoom ──────────────────────────────────────────────────────────────────
  let pps = $state(48);
  let boardEl = $state<HTMLElement | null>(null);
  /** A zoom where the windows are easy to grab (the typical one ~150 px)
   *  and, if that allows, the longest clip fits the row; long clips scroll. */
  function fit() {
    const w = (boardEl?.clientWidth ?? 900) - 240;
    const longest = Math.max(4, ...pieces.map((p, i) => Math.max(pieceLen(p), off(p) + (slots[i]?.len ?? 0))));
    const lens = placed.map((s) => s.len).sort((a, b) => a - b);
    const median = lens.length ? lens[Math.floor(lens.length / 2)] : 3;
    pps = Math.max(12, Math.min(220, Math.floor(Math.max(w / Math.min(longest, 40), 150 / Math.max(0.5, median)))));
  }
  let fitted = false;
  $effect(() => {
    if (!fitted && boardEl && pieces.length) {
      fitted = true;
      fit();
    }
  });

  // ── frame strips ──────────────────────────────────────────────────────────
  let strips = $state<Record<string, FilmstripInfo | null | undefined>>({});
  let queue: Piece[] = [];
  let loading = false;
  $effect(() => {
    for (const p of pieces) {
      if (strips[p.key] === undefined && !queue.some((q) => q.key === p.key)) queue.push(p);
    }
    void pump();
  });
  async function pump() {
    if (loading) return;
    loading = true;
    while (queue.length) {
      const p = queue.shift()!;
      if (strips[p.key] !== undefined) continue;
      strips = { ...strips, [p.key]: null };
      try {
        const f = p.seg == null ? await loadVideoFilmstrip(p.path) : await api.videoRangeStrip(p.path, p.srcIn, p.srcOut);
        if (f) strips = { ...strips, [p.key]: { ...f, src: api.fileSrc(f.src) } };
      } catch {
        /* no strip: the row shows its poster */
      }
    }
    loading = false;
  }

  /** The sprite frame for `t` seconds into the piece, as a CSS background on
   *  a box `w`×`h` (cover: cropped to fill, like the portrait reel). */
  function frameCss(p: Piece, t: number, w: number, h: number): string {
    const f = strips[p.key];
    if (!f || !f.count || !f.tile_w || !f.tile_h) return "";
    const rel = p.seg == null ? p.srcIn + t : t;
    const i = Math.min(f.count - 1, Math.max(0, Math.floor((rel / (f.duration || pieceLen(p) || 1)) * f.count)));
    const s = Math.max(w / f.tile_w, h / f.tile_h);
    const col = i % f.cols;
    const row = Math.floor(i / f.cols);
    const dx = (w - f.tile_w * s) / 2 - col * f.tile_w * s;
    const dy = (h - f.tile_h * s) / 2 - row * f.tile_h * s;
    return `background-image:url("${f.src}");background-size:${f.cols * f.tile_w * s}px ${f.rows * f.tile_h * s}px;background-position:${dx}px ${dy}px;background-repeat:no-repeat`;
  }
  const tileW = (p: Piece) => {
    const f = strips[p.key];
    return f && f.tile_h ? Math.round((STRIP_H * f.tile_w) / f.tile_h) : 96;
  };

  // ── windows: move and resize ──────────────────────────────────────────────
  type Drag = { kind: "move" | "edge"; i: number; x0: number; off0: number; left: number };
  let drag = $state<Drag | null>(null);
  let sel = $state(0);
  let hover = $state<{ i: number; t: number } | null>(null);

  function onWinDown(e: PointerEvent, i: number, kind: "move" | "edge") {
    if (e.button !== 0 || !slots[i]) return;
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const win = (e.currentTarget as HTMLElement).closest(".win") as HTMLElement;
    drag = { kind, i, x0: e.clientX, off0: off(pieces[i]), left: win.getBoundingClientRect().left };
    sel = i;
  }
  function onWinMove(e: PointerEvent) {
    if (!drag) return;
    const p = pieces[drag.i];
    const slot = slots[drag.i];
    if (!p || !slot) return;
    if (drag.kind === "move") {
      const max = Math.max(0, pieceLen(p) - slot.len);
      const o = Math.max(0, Math.min(max, drag.off0 + (e.clientX - drag.x0) / pps));
      offsets = { ...offsets, [p.key]: o };
      hover = { i: drag.i, t: o };
    } else {
      const raw = slot.start + (e.clientX - drag.left) / pps;
      const end = snapEnd(slot.start, raw, info, 14 / pps, e.altKey);
      pins = { ...pins, [p.key]: end - slot.start };
      hover = { i: drag.i, t: off(p) + (end - slot.start) };
    }
  }
  function onWinUp() {
    drag = null;
  }
  function autoLength(p: Piece) {
    const next = { ...pins };
    delete next[p.key];
    pins = next;
  }

  function onStripMove(e: PointerEvent, i: number) {
    if (drag) return;
    const inner = e.currentTarget as HTMLElement;
    const x = e.clientX - inner.getBoundingClientRect().left;
    hover = { i, t: Math.max(0, Math.min(pieceLen(pieces[i]), x / pps)) };
  }

  /** Beats inside a piece's reach, as positions on its strip (seconds into
   *  the piece): where the window's end can snap. */
  function ticksFor(i: number): { x: number; major: boolean }[] {
    const p = pieces[i];
    const slot = slots[i];
    if (!slot) return [];
    const room = Math.max(pieceLen(p) - off(p), slot.len) + gap * 8;
    const majors = new Set(info.major);
    return info.beats
      .filter((b) => b > slot.start + 1e-6 && b - slot.start <= room)
      .map((b) => ({ x: off(p) + (b - slot.start), major: majors.has(b) }));
  }
  const beatsIn = (s: Slot) => info.beats.filter((b) => b > s.start + 1e-6 && b <= s.start + s.len + 1e-6).length;

  /** Shorten or slide overflowing windows so every piece fits its clip:
   *  slide first (keeps the cut), else the longest length that fits. */
  function fixOverflow() {
    let left = 0;
    for (let guard = 0; guard < pieces.length * 2; guard++) {
      const sl = layout(pieces, info, sec[0], sec[1], pins);
      const i = pieces.findIndex((p, k) => overflowOf(p, off(p), sl[k]) > 0.02);
      if (i < 0) break;
      const p = pieces[i];
      const s = sl[i]!;
      if (pieceLen(p) >= s.len) {
        offsets = { ...offsets, [p.key]: Math.max(0, pieceLen(p) - s.len) };
        continue;
      }
      offsets = { ...offsets, [p.key]: 0 };
      const lf = longestFit(p, 0, s, info);
      if (lf == null) {
        left++;
        notify(`${p.name}${p.seg != null ? ` (segment ${p.seg + 1})` : ""} is shorter than one beat: remove it or put a longer clip before it.`, true);
        break;
      }
      pins = { ...pins, [p.key]: lf };
    }
    if (!left) notify("Every window now fits its clip.");
  }

  // ── preview: frames when still, the reel with the song when playing ──────
  let audioEl = $state<HTMLAudioElement | null>(null);
  let phoneW = $state(216);
  let phoneH = $state(384);
  let vA = $state<HTMLVideoElement | null>(null);
  let vB = $state<HTMLVideoElement | null>(null);
  let front = $state(0);
  let playing = $state(false);
  let playT = $state(0);
  let playEnd = 0;
  let cur = $state(-1);
  let raf = 0;
  const vids = () => [vA, vB] as const;

  function prep(v: HTMLVideoElement, i: number, at: number) {
    const p = pieces[i];
    const src = api.fileSrc(p.path);
    if (v.dataset.src !== src) {
      v.src = src;
      v.dataset.src = src;
    }
    const t = p.srcIn + off(p) + at;
    if (Math.abs(v.currentTime - t) > 0.05) v.currentTime = t;
  }
  function playFrom(from: number, to: number) {
    stopPlay();
    if (!audioEl) return;
    playEnd = to;
    cur = -1;
    audioEl.currentTime = from;
    void audioEl.play().catch(() => notify("Couldn't play the song here.", true));
    playing = true;
    raf = requestAnimationFrame(tick);
  }
  function tick() {
    raf = 0;
    if (!audioEl || !playing) return;
    const t = audioEl.currentTime;
    playT = t;
    if (t >= playEnd - 0.02 || audioEl.ended) {
      stopPlay();
      return;
    }
    const i = slots.findIndex((s) => !!s && t >= s.start - 1e-3 && t < s.start + s.len);
    if (i < 0) {
      stopPlay();
      return;
    }
    const [a, b] = vids();
    if (!a || !b) return;
    if (i !== cur) {
      const back = front === 0 ? b : a;
      prep(back, i, t - slots[i]!.start);
      void back.play().catch(() => {});
      (front === 0 ? a : b).pause();
      front = 1 - front;
      cur = i;
      sel = i;
      const n = slots.findIndex((s, k) => k > i && !!s);
      if (n >= 0) prep(front === 0 ? b : a, n, 0);
    } else {
      const v = front === 0 ? a : b;
      const want = pieces[i].srcIn + off(pieces[i]) + (t - slots[i]!.start);
      // A window past its clip's end: the clip has ended; leave its last frame.
      if (want < pieces[i].srcOut && Math.abs(v.currentTime - want) > 0.25) v.currentTime = want;
    }
    raf = requestAnimationFrame(tick);
  }
  function stopPlay() {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
    playing = false;
    audioEl?.pause();
    vA?.pause();
    vB?.pause();
  }
  function togglePlay() {
    if (playing) stopPlay();
    else playFrom(sec[0], reelEnd);
  }
  function playPiece(i: number) {
    const s = slots[i];
    if (s) playFrom(s.start, s.start + s.len);
  }

  // What the still preview shows: the hovered frame, else the selected
  // piece's window start.
  let still = $derived.by(() => {
    if (hover && pieces[hover.i]) return { p: pieces[hover.i], t: hover.t };
    const p = pieces[Math.min(sel, pieces.length - 1)];
    return p ? { p, t: off(p) } : null;
  });

  // ── export ────────────────────────────────────────────────────────────────
  let quality = $state("high");
  let exportErr = $state("");
  let exportedPath = $state<string | null>(null);
  let job = $derived(activity.jobs["reel-export"]);
  let exporting = $derived(job?.state === "running");
  /** The clips' own frame rate (the one most of the reel's time has), ≤ 60. */
  let fps = $derived.by(() => {
    const secs = new Map<number, number>();
    pieces.forEach((p, i) => {
      const c = meta[p.path]?.fps_class;
      if (c && slots[i]) secs.set(c, (secs.get(c) ?? 0) + slots[i]!.len);
    });
    let best = 0;
    let bs = -1;
    for (const [c, s] of secs) if (s > bs) [best, bs] = [c, s];
    return best ? Math.min(60, best) : null;
  });
  let exportBlocker = $derived(
    !placed.length ? "Nothing fits the song section" : overflowing.length ? `${overflowing.length} window${overflowing.length === 1 ? " runs" : "s run"} past the end of ${overflowing.length === 1 ? "its clip" : "their clips"} (red)` : !name.trim() ? "Give the reel a name" : !destDir ? "Choose where to save it" : null,
  );
  async function exportReel() {
    if (exportBlocker || exporting) return;
    stopPlay();
    exportErr = "";
    exportedPath = null;
    // Each cut on a whole frame, counted from the reel's start: per-piece
    // rounding would add up to a visible drift off the beat over a dozen cuts.
    const rate = fps || 30;
    const frame = (t: number) => Math.round((t - sec[0]) * rate);
    const parts = pieces
      .map((p, i) => ({ p, s: slots[i] }))
      .filter((x): x is { p: Piece; s: Slot } => !!x.s)
      .map(({ p, s }) => {
        const len = (frame(s.start + s.len) - frame(s.start)) / rate;
        return { path: p.path, in_s: p.srcIn + off(p), out_s: p.srcIn + off(p) + len };
      });
    try {
      const r = await api.reelExport({ pieces: parts, musicPath: song, musicStartS: sec[0], fps, quality, destDir, name: name.trim() });
      exportedPath = r.path;
    } catch (e) {
      const msg = String(e);
      if (!msg.includes("cancelled")) exportErr = msg;
    }
  }
  async function chooseDest() {
    const p = await api.pickFolder();
    if (p) destDir = p;
  }
  const baseName = (p: string) => p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || p;
  const asItem = (c: ClipRef): MediaItem => ({ name: c.name, path: c.path, rel: "", kind: c.kind, ext: c.ext, mtime: c.mtime, size: c.size, rating: 0, label: null, flag: null, tags: [], events: [], missing: false });

  function onkeydown(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) return;
    if (e.key === " ") {
      e.preventDefault();
      togglePlay();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const d = e.key === "ArrowUp" ? -1 : 1;
      if (e.altKey && pieces[sel]) {
        onmove(pieces[sel].key, d);
        sel = Math.max(0, Math.min(pieces.length - 1, sel + d));
      } else sel = Math.max(0, Math.min(pieces.length - 1, sel + d));
      boardEl?.querySelector(`[data-row="${sel}"]`)?.scrollIntoView({ block: "nearest" });
    }
  }

  onDestroy(stopPlay);
</script>

<svelte:window {onkeydown} />

<div class="rb">
  <!-- ░ the board ░ -->
  <section class="board" bind:this={boardEl}>
    <div class="bhead">
      <button class="btn sm" onclick={() => { stopPlay(); onback(); }}>← Clips and song</button>
      <span class="stat"><b>{fmtS(reelLen)}</b> reel · {Math.round(info.bpm)} BPM · section {fmtS(sec[0])}–{fmtS(sec[1])}</span>
      <span class="grow"></span>
      <label class="zoom" title="Zoom the strips">
        <span>Zoom</span>
        <input type="range" min="12" max="220" step="1" bind:value={pps} />
      </label>
      <button class="btn sm" onclick={fit}>Fit</button>
    </div>
    {#if overflowing.length || unplaced}
      <div class="alerts">
        {#if overflowing.length}
          <span class="aRed">{overflowing.length} window{overflowing.length === 1 ? " is" : "s are"} longer than what's left of {overflowing.length === 1 ? "its clip" : "their clips"} (red): slide {overflowing.length === 1 ? "it" : "them"} left, shorten the one before, or reorder.</span>
          <button class="btn sm" onclick={fixOverflow}>Fix for me</button>
        {/if}
        {#if unplaced}
          <span class="aDim">{unplaced} piece{unplaced === 1 ? "" : "s"} past the end of the song section: not in the reel. Shorten earlier windows or choose a longer section.</span>
        {/if}
      </div>
    {/if}
    <p class="bhint">Drag a window to choose the part of the clip · drag its right edge to change the cut (snaps to the beats; ⌥ for free; double-click it for automatic) · hover to preview · Space plays the reel · ⌥↑ ⌥↓ move the selected clip</p>

    <div class="rows">
      {#each pieces as p, i (p.key)}
        {@const s = slots[i]}
        {@const o = off(p)}
        {@const len = pieceLen(p)}
        {@const ov = over[i]}
        {@const innerW = Math.max(len, s ? o + s.len : 0) * pps + 40}
        {@const tw = tileW(p)}
        <div class="row" class:sel={sel === i} class:nofit={!s} class:now={playing && cur === i} data-row={i}>
          <div class="gut">
            <span class="ix">{i + 1}</span>
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="startF" style={frameCss(p, o, 40, 70)} onclick={() => (sel = i)} title="The window's first frame">
              {#if !strips[p.key] && refs[p.path]}<Thumb item={asItem(refs[p.path])} size={160} />{/if}
            </div>
            <div class="gtxt">
              <span class="gn" title={p.path}>{p.name}</span>
              <span class="gs">{#if p.seg != null}Seg {p.seg + 1}/{p.segCount} · {/if}{fmtS(len)} clip</span>
              {#if s}
                <span class="gl" class:pinned={pins[p.key] != null}>
                  {fmtS(s.len)} · {beatsIn(s)} beat{beatsIn(s) === 1 ? "" : "s"}{#if pins[p.key] != null}<button class="auto" onclick={() => autoLength(p)} title="Back to the automatic length">auto</button>{/if}
                </span>
              {:else}<span class="gl dim">not in the reel</span>{/if}
            </div>
            <div class="gbtns">
              <button onclick={() => playPiece(i)} disabled={!s} title="Play this piece with the song" aria-label="Play this piece">▶</button>
              <button onclick={() => onmove(p.key, -1)} disabled={i === 0} title="Move up (⌥↑)" aria-label="Move up">↑</button>
              <button onclick={() => onmove(p.key, 1)} disabled={i === pieces.length - 1} title="Move down (⌥↓)" aria-label="Move down">↓</button>
              <button onclick={() => onremove(p.key)} title="Take out of the reel" aria-label="Remove">−</button>
            </div>
          </div>
          <div class="strip">
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="inner" style:width="{innerW}px" onpointermove={(e) => onStripMove(e, i)} onpointerleave={() => !drag && (hover = null)} onpointerdown={() => (sel = i)}>
              <div class="film" style:width="{len * pps}px">
                {#if strips[p.key]}
                  {#each Array.from({ length: Math.ceil((len * pps) / tw) }) as _, k (k)}
                    <span class="tile" style:left="{k * tw}px" style:width="{tw}px" style={frameCss(p, Math.min(len, ((k + 0.5) * tw) / pps), tw, STRIP_H)}></span>
                  {/each}
                {:else}
                  <span class="noFrames">{strips[p.key] === null ? "Getting frames…" : ""}</span>
                {/if}
              </div>
              {#if s}
                {#each ticksFor(i) as tk (tk.x)}
                  <span class="bt" class:maj={tk.major} style:left="{tk.x * pps}px"></span>
                {/each}
                {#if ov > 0.02}
                  <span class="ovf" style:left="{len * pps}px" style:width="{ov * pps}px" title="{fmtS(ov)} past the end of the clip"></span>
                {/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class="win" class:ovr={ov > 0.02} class:dragging={drag?.i === i} style:left="{o * pps}px" style:width="{s.len * pps}px" onpointerdown={(e) => onWinDown(e, i, "move")} onpointermove={onWinMove} onpointerup={onWinUp} onpointercancel={onWinUp}>
                  <span class="wlab">{fmtS(s.len)}</span>
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span class="redge" title="Drag to change where this cut falls; double-click for automatic" onpointerdown={(e) => onWinDown(e, i, "edge")} onpointermove={onWinMove} onpointerup={onWinUp} onpointercancel={onWinUp} ondblclick={() => autoLength(p)}></span>
                </div>
                <span class="shadeL" style:width="{o * pps}px"></span>
                <span class="shadeR" style:left="{(o + s.len) * pps}px" style:width="{Math.max(0, len - o - s.len) * pps}px"></span>
              {/if}
              {#if hover?.i === i && !drag}<span class="hv" style:left="{hover.t * pps}px"></span>{/if}
            </div>
          </div>
        </div>
      {/each}
    </div>
  </section>

  <!-- ░ preview and export ░ -->
  <aside class="side">
    <div class="phone" bind:clientWidth={phoneW} bind:clientHeight={phoneH}>
      <!-- svelte-ignore a11y_media_has_caption -->
      <video bind:this={vA} class:show={playing && front === 0} muted playsinline preload="auto"></video>
      <!-- svelte-ignore a11y_media_has_caption -->
      <video bind:this={vB} class:show={playing && front === 1} muted playsinline preload="auto"></video>
      {#if !playing && still}
        <div class="stillF" style={frameCss(still.p, still.t, phoneW || 216, phoneH || 384)}>
          {#if !strips[still.p.key] && refs[still.p.path]}<Thumb item={asItem(refs[still.p.path])} size={480} />{/if}
        </div>
      {/if}
      <div class="pbar"><span class="pfill" style:width="{reelLen > 0 ? ((playing ? playT - sec[0] : 0) / reelLen) * 100 : 0}%"></span></div>
    </div>
    <div class="pctl">
      <button class="btn" onclick={togglePlay} disabled={!placed.length}>{playing ? "■ Stop" : "▶ Play the reel"}</button>
      <span class="ptime">{fmtS(playing ? playT - sec[0] : 0)} / {fmtS(reelLen)}</span>
    </div>
    <p class="pnote">Portrait 1080×1920. Landscape clips fill the frame from their middle (choosing the crop is a later step).</p>

    <div class="exp">
      <label class="fl" for="reelName">Name</label>
      <input id="reelName" type="text" bind:value={name} spellcheck="false" disabled={exporting} />
      <span class="fl">Save to</span>
      <div class="dest">
        <span class="dl" title={destDir}>{destDir ? baseName(destDir) : "—"}</span>
        <button class="btn sm" onclick={() => void chooseDest()} disabled={exporting}>Choose…</button>
      </div>
      <label class="fl" for="reelQ">Quality</label>
      <select id="reelQ" bind:value={quality} disabled={exporting}>
        <option value="best">Best (bigger file)</option>
        <option value="high">High (recommended)</option>
        <option value="standard">Standard</option>
      </select>
      <p class="small">{fps ? `${fps} fps` : "The clips' frame rate"} · SDR · AAC 192k · the song fades out over the last second</p>
      {#if job && (exporting || job.state === "error")}
        <div class="prog">
          <div class="pl">{job.label}{#if job.detail}{" · "}{job.detail}{/if}</div>
          {#if exporting}
            <div class="track"><span style:width="{job.total ? (job.done / job.total) * 100 : 0}%"></span></div>
            <button class="btn sm" onclick={() => activity.cancel("reel-export")}>Stop</button>
          {/if}
        </div>
      {/if}
      {#if exportErr}<p class="err">{exportErr}</p>{/if}
      {#if exportedPath && !exporting}
        <div class="done">Saved <b>{baseName(exportedPath)}</b> <button class="btn sm" onclick={() => api.reveal(exportedPath!)}>Show in folder</button></div>
      {/if}
      <div class="eact">
        {#if exportBlocker}<span class="why">{exportBlocker}</span>{/if}
        <button class="btn accent" disabled={!!exportBlocker || exporting} onclick={() => void exportReel()}>{exporting ? "Exporting…" : "Export reel"}</button>
      </div>
    </div>
  </aside>
  <audio bind:this={audioEl} src={api.fileSrc(song)} preload="auto"></audio>
</div>

<style>
  .rb { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) 300px; }
  .board { min-width: 0; min-height: 0; display: flex; flex-direction: column; gap: 6px; padding: 10px 12px 10px 16px; border-right: 1px solid var(--border-soft); }
  .bhead { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; flex: none; }
  .stat { color: var(--text-dim); font-size: var(--fs-md); font-variant-numeric: tabular-nums; }
  .stat b { color: var(--text); }
  .grow { flex: 1; }
  .zoom { display: flex; align-items: center; gap: 6px; color: var(--text-dim); font-size: var(--fs-sm); }
  .zoom input { width: 110px; accent-color: var(--accent); }
  .alerts { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 10px; flex: none; font-size: var(--fs-sm); line-height: 1.4; }
  .aRed { color: var(--reject); }
  .aDim { color: var(--text-dim); }
  .bhint { margin: 0; flex: none; color: var(--text-faint); font-size: var(--fs-sm); }
  .rows { flex: 1; min-height: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 6px; padding-bottom: 8px; }
  .row { flex: none; display: grid; grid-template-columns: 214px minmax(0, 1fr); align-items: stretch; min-height: 70px; border: 1px solid var(--border-soft); border-radius: var(--radius-md); background: color-mix(in srgb, var(--bg-elev) 45%, transparent); }
  .row.sel { border-color: color-mix(in srgb, var(--accent) 60%, var(--border)); }
  .row.now { box-shadow: inset 3px 0 0 var(--accent); }
  .row.nofit { opacity: 0.5; }
  .gut { display: grid; grid-template-columns: 16px 40px minmax(0, 1fr); grid-template-rows: 1fr auto; gap: 2px 7px; padding: 6px 8px; }
  .ix { grid-row: 1 / span 2; align-self: center; color: var(--text-faint); font-size: var(--fs-sm); text-align: right; font-variant-numeric: tabular-nums; }
  .startF { grid-row: 1 / span 2; width: 40px; height: 58px; align-self: center; border-radius: var(--radius-xs); overflow: hidden; background-color: #050607; cursor: pointer; }
  .gtxt { display: flex; flex-direction: column; min-width: 0; }
  .gn { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-sm); font-weight: var(--fw-semibold); }
  .gs { color: var(--text-faint); font-size: var(--fs-xs); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .gl { display: flex; align-items: center; gap: 5px; color: var(--accent); font-size: var(--fs-sm); font-weight: var(--fw-semibold); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .gl.dim { color: var(--text-faint); font-weight: var(--fw-medium); }
  .auto { padding: 0 5px; border-radius: var(--radius-xs); background: color-mix(in srgb, var(--accent) 15%, transparent); color: var(--accent); font-size: var(--fs-xs); font-weight: var(--fw-semibold); }
  .gbtns { display: flex; gap: 2px; }
  .gbtns button { width: 22px; height: 20px; border-radius: var(--radius-xs); color: var(--text-dim); font-size: var(--fs-xs); }
  .gbtns button:hover:not(:disabled) { background: var(--bg-hover); color: var(--text); }
  .gbtns button:disabled { opacity: 0.35; }
  .strip { position: relative; min-width: 0; overflow-x: auto; overflow-y: hidden; padding: 7px 0; scrollbar-width: thin; }
  .inner { position: relative; height: 54px; margin-left: 4px; touch-action: none; }
  .film { position: absolute; left: 0; top: 0; height: 54px; border-radius: var(--radius-xs); overflow: hidden; background: #15181d; }
  .tile { position: absolute; top: 0; height: 54px; border-right: 1px solid rgba(0, 0, 0, 0.35); }
  .noFrames { position: absolute; left: 8px; top: 18px; color: var(--text-faint); font-size: var(--fs-xs); }
  .shadeL, .shadeR { position: absolute; top: 0; height: 54px; background: rgba(5, 6, 8, 0.58); pointer-events: none; }
  .shadeL { left: 0; border-radius: 4px 0 0 4px; }
  .bt { position: absolute; bottom: -5px; width: 1px; height: 7px; margin-left: -0.5px; background: color-mix(in srgb, var(--text-faint) 80%, transparent); pointer-events: none; z-index: 3; }
  .bt.maj { height: 64px; bottom: -5px; background: color-mix(in srgb, var(--star) 55%, transparent); }
  .ovf { position: absolute; top: 0; height: 54px; border-radius: 0 4px 4px 0; background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--reject) 55%, transparent) 0 6px, color-mix(in srgb, var(--reject) 25%, transparent) 6px 12px); pointer-events: none; z-index: 2; }
  .win { position: absolute; top: -3px; height: 60px; border: 2px solid var(--accent); border-radius: var(--radius-xs); box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5), 0 2px 10px rgba(0, 0, 0, 0.35); cursor: grab; z-index: 4; touch-action: none; }
  .win.dragging { cursor: grabbing; }
  .win.ovr { border-color: var(--reject); }
  .wlab { position: absolute; left: 4px; top: 2px; padding: 0 4px; border-radius: var(--radius-xs); background: rgba(0, 0, 0, 0.65); color: #fff; font-size: var(--fs-xs); font-weight: var(--fw-semibold); font-variant-numeric: tabular-nums; pointer-events: none; }
  .redge { position: absolute; right: -6px; top: 6px; bottom: 6px; width: 10px; border-radius: var(--radius-xs); background: var(--accent); cursor: ew-resize; box-shadow: 0 0 0 2px rgba(0, 0, 0, 0.4); }
  .win.ovr .redge { background: var(--reject); }
  .hv { position: absolute; top: -2px; height: 58px; width: 2px; margin-left: -1px; background: #fff; box-shadow: 0 0 3px rgba(0, 0, 0, 0.8); pointer-events: none; z-index: 5; }

  .side { min-height: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 8px; padding: 12px 14px; }
  .side > * { flex-shrink: 0; }
  /* 9:16, as tall as the window comfortably allows. */
  .phone { position: relative; height: clamp(220px, calc(100vh - 330px), 384px); aspect-ratio: 9 / 16; align-self: center; border-radius: var(--radius-xl); overflow: hidden; background: #050607; box-shadow: 0 0 0 1px var(--border-soft), 0 8px 24px rgba(0, 0, 0, 0.35); }
  .phone video { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; opacity: 0; }
  .phone video.show { opacity: 1; }
  .stillF { position: absolute; inset: 0; }
  .pbar { position: absolute; left: 0; right: 0; bottom: 0; height: 3px; background: rgba(255, 255, 255, 0.15); }
  .pfill { display: block; height: 100%; background: var(--accent); }
  .pctl { display: flex; align-items: center; justify-content: center; gap: 10px; }
  .ptime { color: var(--text-dim); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  .pnote { margin: 0; color: var(--text-faint); font-size: var(--fs-xs); line-height: 1.4; text-align: center; }
  .exp { display: flex; flex-direction: column; gap: 6px; padding-top: 10px; border-top: 1px solid var(--border-soft); }
  .fl { margin-top: 2px; color: var(--text-faint); font-size: var(--fs-xs); font-weight: var(--fw-semibold); letter-spacing: 0.01em; text-transform: none; }
  .exp input[type="text"], .exp select { min-height: var(--control-h); padding: 5px 10px; font-size: var(--fs-md); }
  .dest { display: flex; align-items: center; gap: 8px; }
  .dl { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-md); }
  .small { margin: 0; color: var(--text-dim); font-size: var(--fs-sm); }
  .prog { display: flex; flex-direction: column; gap: 5px; padding: 8px 10px; border: 1px solid var(--border-soft); border-radius: var(--radius-sm); }
  .pl { font-size: var(--fs-sm); }
  .track { height: 5px; border-radius: var(--radius-xs); background: var(--border-soft); overflow: hidden; }
  .track span { display: block; height: 100%; background: var(--accent); }
  .err { margin: 0; color: var(--reject); font-size: var(--fs-sm); }
  .done { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: var(--fs-md); }
  .eact { display: flex; flex-direction: column; align-items: stretch; gap: 6px; margin-top: 4px; }
  .why { color: var(--star); font-size: var(--fs-sm); line-height: 1.4; }
</style>
