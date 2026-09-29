<script lang="ts">
  // Merge videos: a trip's clips joined end to end, in shooting order, with no
  // re-encoding: the "dump the Osmo clips into one file for YouTube" workflow.
  // Deliberately NOT the Edit studio: no timeline, no trims, nothing to set up.
  // It lists the clips, keeps the ones that can be joined losslessly (same
  // codec, frame size, frame rate, bit depth, audio), shows how long and how
  // big the result will be, and checks the chosen drive can hold it before
  // anything is written.
  import { onDestroy, onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api } from "$lib/api";
  import type { MergeClip, TreeDir } from "$lib/types";

  let {
    paths,
    skippedPhotos = 0,
    sourceDir,
    drives,
    onclose,
    ondone,
  }: {
    /** The selected videos (any order; the probe returns shooting order). */
    paths: string[];
    /** Photos that were in the selection and were left out. */
    skippedPhotos?: number;
    /** Folder the clips live in: the default place to save. */
    sourceDir: string;
    drives: TreeDir[];
    onclose: () => void;
    ondone: (path: string, dir: string) => void;
  } = $props();

  type Phase = "probing" | "ready" | "merging" | "done" | "error";
  let phase = $state<Phase>("probing");
  let clips = $state<MergeClip[]>([]);
  let chosenSig = $state("");
  let excluded = $state<Set<string>>(new Set());
  let name = $state("");
  let destDir = $state("");
  let error = $state("");
  let pct = $state(0);
  let startedAt = 0;
  let now = $state(Date.now());
  let result = $state<{ path: string; bytes: number } | null>(null);

  type Dest = { label: string; path: string; free: number | null };
  let dests = $state<Dest[]>([]);
  let customDest = $state<Dest | null>(null);

  // ── grouping: clips that can share one lossless file ─────────────────────
  type Group = { sig: string; clips: MergeClip[]; secs: number; label: string };
  const bitDepth = (c: MergeClip) => (/10|12/.test(c.pix_fmt) ? "10-bit" : "8-bit");
  const codecName = (c: MergeClip) => ({ hevc: "HEVC", h264: "H.264", prores: "ProRes", av1: "AV1" })[c.vcodec] ?? c.vcodec.toUpperCase();
  function sizeLabel(c: MergeClip) {
    const w = c.rotation % 180 ? c.height : c.width;
    const h = c.rotation % 180 ? c.width : c.height;
    if (h > w) return `Vertical ${w}×${h}`;
    if (h === w) return `Square ${w}×${h}`;
    if (w >= 3840) return "4K";
    if (w >= 2560) return "2.7K";
    if (w >= 1920) return "1080p";
    return `${w}×${h}`;
  }
  const fpsLabel = (f: number) => `${Number.isInteger(f) ? f : f.toFixed(2)} fps`;
  const groupLabel = (c: MergeClip) => `${sizeLabel(c)} · ${fpsLabel(c.fps)} · ${codecName(c)} ${bitDepth(c)}`;

  let groups = $derived.by(() => {
    const m = new Map<string, Group>();
    for (const c of clips) {
      if (c.error) continue;
      const g = m.get(c.signature) ?? { sig: c.signature, clips: [], secs: 0, label: groupLabel(c) };
      g.clips.push(c);
      g.secs += c.duration;
      m.set(c.signature, g);
    }
    return [...m.values()].sort((a, b) => b.secs - a.secs);
  });
  let chosen = $derived(groups.find((g) => g.sig === chosenSig) ?? groups[0]);

  /** Why a clip can't join the chosen set, in words. */
  function mismatch(c: MergeClip): string | null {
    if (c.error) return c.error;
    const ref = chosen?.clips[0];
    if (!ref || c.signature === ref.signature) return null;
    const why: string[] = [];
    if (sizeLabel(c) !== sizeLabel(ref) || c.width !== ref.width || c.height !== ref.height) why.push(sizeLabel(c));
    if (Math.abs(c.fps - ref.fps) > 0.01) why.push(fpsLabel(c.fps));
    if (c.vcodec !== ref.vcodec) why.push(codecName(c));
    else if (bitDepth(c) !== bitDepth(ref)) why.push(bitDepth(c));
    else if (c.profile !== ref.profile || c.pix_fmt !== ref.pix_fmt) why.push(`${codecName(c)} ${c.profile}`);
    if (c.acodec !== ref.acodec || c.arate !== ref.arate || c.alayout !== ref.alayout) why.push(c.acodec ? "different audio" : "no audio");
    return why.join(" · ") || "shot with different settings";
  }

  let joinable = $derived(clips.filter((c) => !mismatch(c)));
  let included = $derived(joinable.filter((c) => !excluded.has(c.path)));
  let totalSecs = $derived(included.reduce((s, c) => s + c.duration, 0));
  // DJI clips carry a ~5 Mbps debug track the merge drops; estimate with the
  // plain sum and call it approximate rather than under-promise the space.
  let totalBytes = $derived(included.reduce((s, c) => s + c.size, 0));
  let dest = $derived(customDest?.path === destDir ? customDest : dests.find((d) => d.path === destDir));
  const MARGIN = 1024 ** 3; // matches the backend's 1 GB margin
  const fits = (d: Dest | undefined) => !!d && (d.free === null || d.free >= totalBytes + MARGIN);

  // ── formatting ────────────────────────────────────────────────────────────
  function fmtDur(s: number) {
    s = Math.round(s);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = s % 60;
    return h ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${m}:${String(sec).padStart(2, "0")}`;
  }
  function fmtLong(s: number) {
    const h = Math.floor(s / 3600);
    const m = Math.round((s % 3600) / 60);
    return h ? `${h} h ${m} min` : `${m} min`;
  }
  const gb = (b: number) =>
    b >= 1e12 ? `${(b / 1e12).toFixed(1)} TB` : b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${Math.round(b / 1e6)} MB`;
  const when = (t: number | null) =>
    t ? new Date(t * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : "";
  const baseName = (p: string) => p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || p;

  function defaultName(list: MergeClip[]) {
    const ts = list.map((c) => c.captured).filter((t): t is number => !!t);
    if (!ts.length) return "Merged video";
    const a = new Date(Math.min(...ts) * 1000);
    const b = new Date(Math.max(...ts) * 1000);
    const mon = (d: Date) => d.toLocaleString(undefined, { month: "short" });
    if (a.toDateString() === b.toDateString()) return `Merged ${a.getDate()} ${mon(a)} ${a.getFullYear()}`;
    if (a.getMonth() === b.getMonth() && a.getFullYear() === b.getFullYear())
      return `Merged ${a.getDate()}-${b.getDate()} ${mon(a)} ${a.getFullYear()}`;
    return `Merged ${a.getDate()} ${mon(a)} - ${b.getDate()} ${mon(b)} ${b.getFullYear()}`;
  }

  // ── lifecycle ─────────────────────────────────────────────────────────────
  onMount(async () => {
    try {
      clips = await api.mergeProbe(paths);
    } catch (e) {
      error = String(e);
      phase = "error";
      return;
    }
    chosenSig = groups[0]?.sig ?? "";
    name = defaultName(groups[0]?.clips ?? clips);
    phase = "ready";
    await loadDestinations();
  });

  async function loadDestinations() {
    const list: Dest[] = [{ label: `Same folder as the clips (${baseName(sourceDir)})`, path: sourceDir, free: null }];
    const sug = await api.suggestedFolders();
    const movies = sug.find((s) => s.kind === "videos");
    if (movies) list.push({ label: `This computer: ${movies.label}`, path: movies.path, free: null });
    for (const d of drives) {
      if (d.name === "Home" || (await api.isSystemRoot(d.path))) continue;
      if (sourceDir.toLowerCase().startsWith(d.path.toLowerCase())) continue; // same drive as the clips
      list.push({ label: `${d.name.replace(/[\\/]+$/, "")} (drive)`, path: d.path, free: null });
    }
    dests = list;
    destDir = sourceDir;
    for (const d of list) {
      api.diskFree(d.path).then(
        (free) => (dests = dests.map((x) => (x.path === d.path ? { ...x, free } : x))),
        () => {},
      );
    }
  }

  async function chooseFolder() {
    const picked = await api.pickFolder();
    if (!picked) return;
    customDest = { label: picked, path: picked, free: null };
    destDir = picked;
    try {
      const free = await api.diskFree(picked);
      customDest = { ...customDest, free };
    } catch {
      /* unknown: the backend still checks */
    }
  }

  let unlisten: (() => void) | null = null;
  let ticker: ReturnType<typeof setInterval> | null = null;
  async function start() {
    error = "";
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
      result = await api.mergeVideos({ paths: included.map((c) => c.path), destDir, name });
      phase = "done";
      ondone(result.path, destDir);
    } catch (e) {
      const msg = String(e);
      if (msg.includes("cancelled")) phase = "ready";
      else {
        error = msg;
        phase = "ready";
      }
    } finally {
      unlisten?.();
      unlisten = null;
      if (ticker) clearInterval(ticker);
    }
  }

  function cancel() {
    if (phase === "merging") void api.cancelEditExport();
    else onclose();
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

  function toggle(path: string) {
    const next = new Set(excluded);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    excluded = next;
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && phase !== "merging" && onclose()} />

<div class="backdrop" onclick={() => phase !== "merging" && onclose()} role="presentation"></div>
<div class="panel" role="dialog" aria-label="Merge videos">
  <header>
    <div>
      <h2>Merge videos</h2>
      <p class="sub">Joined end to end in the order they were shot. No re-encoding, so the file keeps the camera's full quality.</p>
    </div>
    <span class="grow"></span>
    {#if phase !== "merging"}<button class="x" onclick={onclose} title="Close (Esc)" aria-label="Close">✕</button>{/if}
  </header>

  {#if phase === "probing"}
    <div class="center">Reading {paths.length} clips…</div>
  {:else if phase === "error" && !clips.length}
    <div class="center err">{error}</div>
  {:else if phase === "done" && result}
    <div class="doneBox">
      <div class="doneIcon">✓</div>
      <h3>{baseName(result.path)}</h3>
      <p>{gb(result.bytes)} · {fmtLong(totalSecs)} · {included.length} clips</p>
      <p class="hint">
        To share it, upload this file to YouTube as it is: that gives YouTube the original quality to work from. Your clips are
        untouched, so once the upload finishes you can delete this merged file to get the space back.
      </p>
      <div class="doneActions">
        <button class="btn" onclick={() => api.reveal(result!.path)}>Show in folder</button>
        <button class="btn" onclick={() => openUrl("https://www.youtube.com/upload")}>Open YouTube upload</button>
        <button class="btn accent" onclick={onclose}>Done</button>
      </div>
    </div>
  {:else}
    <div class="scroll">
      <!-- The plan in one line: what you'll get, how long, how big. -->
      <div class="summary">
        <div class="big">{included.length} clips · {fmtLong(totalSecs)}</div>
        <div class="small">{chosen?.label ?? ""} · about {gb(totalBytes)}</div>
      </div>

      {#if groups.length > 1}
        <div class="groups">
          <span class="lbl">These clips were shot with different settings, and only matching clips can be joined without re-encoding. Merge:</span>
          <div class="chips">
            {#each groups as g (g.sig)}
              <button class="chip" class:on={g.sig === chosen?.sig} onclick={() => { chosenSig = g.sig; excluded = new Set(); }}>
                {g.label} <em>{g.clips.length} · {fmtLong(g.secs)}</em>
              </button>
            {/each}
          </div>
        </div>
      {/if}
      {#if skippedPhotos}<p class="note">{skippedPhotos} photo{skippedPhotos === 1 ? " was" : "s were"} in the selection and left out.</p>{/if}

      <ol class="clips">
        {#each clips as c (c.path)}
          {@const why = mismatch(c)}
          <li class:off={!!why || excluded.has(c.path)}>
            <input type="checkbox" checked={!why && !excluded.has(c.path)} disabled={!!why} onchange={() => toggle(c.path)} aria-label="Include {c.name}" />
            <span class="t">{when(c.captured)}</span>
            <span class="n" title={c.path}>{c.name}</span>
            {#if why}<span class="why">{why}</span>{/if}
            <span class="d">{fmtDur(c.duration)}</span>
          </li>
        {/each}
      </ol>

      <div class="field">
        <label for="mergeName">File name</label>
        <input id="mergeName" type="text" bind:value={name} spellcheck="false" />
      </div>

      <div class="field">
        <span class="flabel">Save to</span>
        <div class="dests">
          {#each [...dests, ...(customDest ? [customDest] : [])] as d (d.path)}
            <button class="dest" class:on={destDir === d.path} class:tight={d.free !== null && !fits(d)} onclick={() => (destDir = d.path)} title={d.path}>
              <span class="dl">{d.label}</span>
              <span class="df">
                {#if d.free === null}checking space…{:else if !fits(d)}not enough space · {gb(d.free)} free{:else}{gb(d.free)} free{/if}
              </span>
            </button>
          {/each}
          <button class="dest choose" onclick={chooseFolder}>Choose another folder…</button>
        </div>
        {#if dest && dest.free !== null && !fits(dest)}
          <p class="warn">
            This needs about {gb(totalBytes)} and that drive has {gb(dest.free)} free. Pick an external drive, or merge fewer clips.
          </p>
        {/if}
      </div>

      {#if error}<p class="warn">{error}</p>{/if}
    </div>

    <footer>
      {#if phase === "merging"}
        <div class="progress" role="progressbar" aria-valuenow={pct} aria-valuemin="0" aria-valuemax="100">
          <div class="bar"><div class="fill" style="width:{pct}%"></div></div>
          <span>Merging… {pct}%{eta ? ` · ${eta}` : ""}</span>
        </div>
        <button class="btn" onclick={cancel}>Stop</button>
      {:else}
        <span class="grow"></span>
        <button class="btn" onclick={onclose}>Cancel</button>
        <button class="btn accent" onclick={start} disabled={included.length < 2 || !name.trim() || !destDir || (dest?.free != null && !fits(dest))}>
          Merge {included.length} clips
        </button>
      {/if}
    </footer>
  {/if}
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 100; background: rgba(0, 0, 0, 0.66); backdrop-filter: blur(6px); }
  .panel {
    position: fixed; top: 50%; left: 50%; transform: translate(-50%, -50%); z-index: 101;
    width: min(720px, calc(100vw - 32px)); max-height: calc(100vh - 48px);
    display: flex; flex-direction: column; overflow: hidden;
    background: color-mix(in srgb, var(--bg-panel) 97%, transparent);
    border: 1px solid var(--border-strong); border-radius: var(--radius-xl); box-shadow: var(--shadow);
  }
  header, footer { display: flex; align-items: center; gap: 10px; padding: 14px 18px; flex-shrink: 0; }
  header { border-bottom: 1px solid var(--border-soft); }
  footer { border-top: 1px solid var(--border-soft); }
  h2 { margin: 0; font-family: var(--font-display); font-size: 17px; letter-spacing: -0.015em; }
  .sub { margin: 3px 0 0; color: var(--text-dim); font-size: 12.5px; }
  .grow { flex: 1; }
  .x { width: 30px; height: 30px; border-radius: 7px; color: var(--text-dim); font-size: 13px; }
  .x:hover { background: var(--bg-hover); color: var(--text); }
  .center { padding: 48px 18px; text-align: center; color: var(--text-dim); }
  .center.err { color: var(--reject); }

  .scroll { flex: 1; min-height: 0; overflow-y: auto; padding: 14px 18px 16px; display: flex; flex-direction: column; gap: 14px; }
  .scroll > * { flex-shrink: 0; }
  .summary { padding: 12px 14px; border: 1px solid var(--border-soft); border-radius: var(--radius-md); background: color-mix(in srgb, var(--accent) 7%, var(--bg-elev)); }
  .big { color: var(--text); font-size: 16px; font-weight: 650; font-variant-numeric: tabular-nums; }
  .small { margin-top: 2px; color: var(--text-dim); font-size: 12.5px; }

  .groups .lbl { display: block; margin-bottom: 7px; color: var(--text-dim); font-size: 12.5px; line-height: 1.5; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .chip { padding: 6px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg-elev); color: var(--text); font-size: 12px; text-align: left; }
  .chip em { margin-left: 4px; color: var(--text-faint); font-style: normal; }
  .chip.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 14%, var(--bg-elev)); }
  .chip.on em { color: var(--text-dim); }
  .note { margin: 0; color: var(--text-faint); font-size: 12px; }

  .clips { list-style: none; margin: 0; padding: 4px; max-height: 260px; overflow-y: auto; border: 1px solid var(--border-soft); border-radius: var(--radius-md); background: color-mix(in srgb, var(--bg-elev) 45%, transparent); }
  .clips li { display: flex; align-items: center; gap: 10px; min-height: 30px; padding: 3px 8px; border-radius: 6px; font-size: 12.5px; }
  .clips li:hover { background: var(--bg-hover); }
  .clips li.off { color: var(--text-faint); }
  .clips input { flex: none; accent-color: var(--accent); }
  .t { flex: none; width: 118px; color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .n { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  li.off .n { color: var(--text-faint); }
  .why { flex: none; max-width: 40%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--star); font-size: 11.5px; }
  .d { flex: none; color: var(--text-dim); font-variant-numeric: tabular-nums; }

  .field { display: flex; flex-direction: column; gap: 6px; }
  .field label, .flabel { color: var(--text-faint); font-size: 11px; font-weight: 650; letter-spacing: 0.06em; text-transform: uppercase; }
  .field input[type="text"] { min-height: var(--control-h); padding: 5px 10px; font-size: 13px; user-select: text; }
  .dests { display: flex; flex-direction: column; gap: 5px; }
  .dest { display: flex; align-items: center; gap: 10px; padding: 8px 11px; border: 1px solid var(--border-soft); border-radius: 8px; background: color-mix(in srgb, var(--bg-elev) 45%, transparent); color: var(--text); font-size: 12.5px; text-align: left; }
  .dest:hover { border-color: var(--border); }
  .dest.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, var(--bg-elev)); }
  .dl { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .df { flex: none; color: var(--text-faint); font-size: 12px; font-variant-numeric: tabular-nums; }
  .dest.tight .df { color: var(--reject); }
  .dest.choose { color: var(--accent); justify-content: center; }
  .warn { margin: 0; color: var(--reject); font-size: 12.5px; line-height: 1.5; }

  .progress { flex: 1; display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; color: var(--text-dim); }
  .bar { height: 6px; border-radius: 999px; background: var(--bg-hover); overflow: hidden; }
  .fill { height: 100%; background: var(--accent); transition: width 300ms ease; }

  .doneBox { padding: 28px 24px 22px; text-align: center; overflow-y: auto; }
  .doneIcon { display: inline-grid; place-items: center; width: 44px; height: 44px; border-radius: 50%; background: color-mix(in srgb, var(--pick) 18%, transparent); color: var(--pick); font-size: 22px; }
  .doneBox h3 { margin: 12px 0 4px; color: var(--text); font-size: 15px; word-break: break-all; }
  .doneBox p { margin: 0; color: var(--text-dim); font-size: 12.5px; }
  .doneBox .hint { max-width: 520px; margin: 14px auto 0; line-height: 1.6; }
  .doneActions { display: flex; justify-content: center; flex-wrap: wrap; gap: 8px; margin-top: 18px; }
</style>
