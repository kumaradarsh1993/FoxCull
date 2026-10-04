<script lang="ts">
  // The Merge window (its own OS window since 2026-10-04). Two screens:
  //   * review: the clip list from the library (MergeDialog), then Merge;
  //   * progress: what the backend's merge is doing, read from merge_status,
  //     so the window can be closed mid-merge and reopened onto it (from the
  //     library's progress panel → "Show merge window").
  // No clip previews while it runs (owner: a waste of resources, and playing
  // a 4K original from a card was slow): a clean progress screen with Pause,
  // Resume and Stop, and plain words when something fails.
  import { onDestroy, onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api } from "$lib/api";
  import { activity, fmtBytes, fmtDuration } from "$lib/activity.svelte";
  import { closeThisWindow } from "$lib/windows";
  import type { MediaItem, MergeInbox, MergeStatus } from "$lib/types";
  import MergeDialog from "./MergeDialog.svelte";

  type View = "loading" | "review" | "progress" | "empty";
  let view = $state<View>("loading");
  let review = $state<{ items: MediaItem[]; sourceDir: string } | null>(null);
  /** Bumped to remount the review list (a new selection from the library). */
  let reviewKey = $state(0);
  let st = $state<MergeStatus | null>(null);
  let stopArmed = $state(false);
  let notice = $state("");
  let busy = $state(false);
  let poll: ReturnType<typeof setInterval> | null = null;

  const running = $derived(st?.state === "running");
  const finished = $derived(st?.state === "done" || st?.state === "error" || st?.state === "cancelled");

  async function refresh() {
    st = await api.mergeStatus().catch(() => st);
  }

  function showProgress() {
    // Never draw the previous (idle) status as if it were this merge.
    if (st?.state === "idle") st = null;
    view = "progress";
    stopArmed = false;
    void refresh();
    if (!poll) poll = setInterval(() => void refresh(), 700);
  }

  async function drainInbox() {
    const items = await api.takeToolInbox<MergeInbox>("merge").catch(() => [] as MergeInbox[]);
    const last = [...items].reverse().find((i) => i?.type === "review") as Extract<MergeInbox, { type: "review" }> | undefined;
    await refresh();
    if (last) {
      // One merge at a time: a new selection while one runs waits its turn.
      if (running) {
        notice = "A merge is already running. When it's done, choose Merge again in the library for the new clips.";
        showProgress();
        return;
      }
      review = { items: last.items, sourceDir: last.sourceDir };
      reviewKey++;
      view = "review";
      if (finished) await api.mergeDismiss();
      return;
    }
    if (view === "loading" || view === "empty") {
      if (running || finished) showProgress();
      else view = review ? "review" : "empty";
    }
  }

  // ── controls ──────────────────────────────────────────────────────────────
  async function pause(p: boolean) {
    busy = true;
    try {
      st = await api.mergePause(p);
    } catch (e) {
      notice = String(e);
    } finally {
      busy = false;
    }
  }
  function stop() {
    if (!stopArmed) {
      stopArmed = true;
      return;
    }
    stopArmed = false;
    void api.cancelJob("merge").then(() => setTimeout(() => void refresh(), 400));
  }
  async function done() {
    await api.mergeDismiss();
    await closeThisWindow();
  }
  async function mergeMore() {
    await api.mergeDismiss();
    st = null;
    view = review ? "review" : "empty";
    reviewKey++;
  }
  async function backToClips() {
    await api.mergeDismiss();
    view = review ? "review" : "empty";
    reviewKey++;
  }

  const elapsed = $derived.by(() => {
    void activity.tick;
    if (!st?.started_ms) return "";
    const end = st.finished_ms || Date.now();
    return fmtDuration(end - st.started_ms);
  });
  const eta = $derived.by(() => {
    void activity.tick;
    return running && !st?.paused ? activity.eta("merge") : "";
  });
  const folderName = (p: string | undefined) => (p ?? "").replace(/[\\/]+$/, "").split(/[\\/]/).pop() || p || "";
  const fmtLen = (s: number) => {
    const t = Math.round(s);
    const h = Math.floor(t / 3600);
    const m = Math.floor((t % 3600) / 60);
    return h ? `${h} h ${m} min` : `${m} min ${t % 60} s`;
  };

  let unlisten: (() => void) | null = null;
  onMount(async () => {
    document.title = "FoxCull Merge";
    await drainInbox();
    unlisten = await api.onToolInbox(() => void drainInbox()).catch(() => null);
  });
  onDestroy(() => {
    unlisten?.();
    if (poll) clearInterval(poll);
  });
</script>

{#if view === "review" && review}
  {#key reviewKey}
    <MergeDialog items={review.items} sourceDir={review.sourceDir} onclose={() => void closeThisWindow()} onstarted={showProgress} />
  {/key}
{:else if view === "progress" && st && st.state !== "idle"}
  <main class="prog" aria-live="polite">
    <header>
      <h2>Merge videos</h2>
      <span class="grow"></span>
      <button class="x" onclick={() => void closeThisWindow()} title={running ? "Close the window; the merge keeps going" : "Close"} aria-label="Close">✕</button>
    </header>
    <section class="card" class:paused={st.paused} class:ok={st.state === "done"} class:bad={st.state === "error"}>
      <div class="icon" aria-hidden="true">
        {#if st.state === "done"}✓{:else if st.state === "error"}!{:else if st.state === "cancelled"}■{:else}
          <svg viewBox="0 0 24 24"><rect x="3" y="6" width="7" height="12" rx="1.6" /><rect x="14" y="6" width="7" height="12" rx="1.6" /><path d="M10 12h4" /></svg>
        {/if}
      </div>
      <h3 title={st.out_path}>
        {#if st.state === "done"}Merged {st.clips} clips{:else if st.state === "error"}The merge didn't finish{:else if st.state === "cancelled"}Merge stopped{:else if st.paused}Paused{:else}{st.convert ? "Converting and merging" : "Merging"} {st.clips} clips{/if}
      </h3>
      <p class="file" title={st.out_path}>{st.name}</p>
      <p class="where">in <button class="link" onclick={() => void api.reveal(st!.state === "done" ? st!.out_path : st!.dest_dir)}>{folderName(st.dest_dir)}</button> · {fmtLen(st.total_s)} of video</p>

      {#if running}
        <div class="bar" role="progressbar" aria-valuenow={st.pct} aria-valuemin="0" aria-valuemax="100">
          <span class="fill" style="width:{Math.max(1, st.pct)}%"></span>
        </div>
        <div class="stats">
          <span class="pct">{st.pct}%</span>
          {#if st.paused}<span>Paused — nothing is being written; Resume carries on from here</span>
          {:else}
            {#if st.detail}<span>{st.detail}</span>{/if}
            {#if eta}<b>{eta} left</b>{/if}
          {/if}
          <span class="grow"></span>
          <span class="dim">{elapsed}</span>
        </div>
        <div class="actions">
          {#if st.paused}
            <button class="btn accent" disabled={busy} onclick={() => void pause(false)}>Resume</button>
          {:else}
            <button class="btn" disabled={busy} onclick={() => void pause(true)}>Pause</button>
          {/if}
          {#if stopArmed}
            <button class="btn danger" onclick={stop}>Stop and delete the partial file</button>
            <button class="btn" onclick={() => (stopArmed = false)}>Keep merging</button>
          {:else}
            <button class="btn" onclick={stop}>Stop…</button>
          {/if}
          <span class="grow"></span>
          <button class="btn" onclick={() => void closeThisWindow()}>Close window</button>
        </div>
        <p class="hint">Closing this window doesn't stop the merge. Follow it in the library's progress panel (bottom left), where you can also pause it or open this window again.</p>
      {:else if st.state === "done"}
        <p class="stats done">{fmtBytes(st.out_bytes)} · took {elapsed}</p>
        <p class="hint">Upload this file to YouTube as it is{st.convert ? ": it's encoded well above what YouTube keeps" : ": that gives YouTube the camera's original quality"}. Your clips are untouched, so you can delete this file after the upload to get the space back.</p>
        <div class="actions">
          <button class="btn" onclick={() => void api.reveal(st!.out_path)}>Show in folder</button>
          <button class="btn" onclick={() => void openUrl("https://www.youtube.com/upload")}>Open YouTube upload</button>
          <span class="grow"></span>
          {#if review}<button class="btn" onclick={() => void mergeMore()}>Merge other clips</button>{/if}
          <button class="btn accent" onclick={() => void done()}>Done</button>
        </div>
      {:else}
        <p class="err">{st.state === "cancelled" ? "Nothing was saved: the partial file was deleted." : (st.error ?? "Something went wrong.")}</p>
        <div class="actions">
          {#if review}<button class="btn accent" onclick={() => void backToClips()}>Back to the clips</button>{/if}
          <span class="grow"></span>
          <button class="btn" onclick={() => void done()}>Close</button>
        </div>
      {/if}
      {#if notice}<p class="notice">{notice}</p>{/if}
    </section>
  </main>
{:else if view === "empty"}
  <main class="prog">
    <header><h2>Merge videos</h2><span class="grow"></span><button class="x" onclick={() => void closeThisWindow()} aria-label="Close">✕</button></header>
    <section class="card">
      <h3>Nothing to merge yet</h3>
      <p class="hint">Select the clips in the library, right-click and choose <b>Merge N videos into one…</b></p>
      <div class="actions"><span class="grow"></span><button class="btn" onclick={() => void closeThisWindow()}>Close</button></div>
    </section>
  </main>
{:else}
  <main class="prog"><p class="loading">Loading…</p></main>
{/if}

<style>
  .prog {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    color: var(--text);
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 18px;
    border-bottom: 1px solid var(--border-soft);
  }
  h2 { margin: 0; font-size: 17px; letter-spacing: -0.015em; }
  .grow { flex: 1; }
  .x { width: 30px; height: 30px; border-radius: 7px; color: var(--text-dim); font-size: 13px; }
  .x:hover { background: var(--bg-hover); color: var(--text); }
  .loading { margin: auto; color: var(--text-faint); }
  .card {
    width: min(620px, calc(100% - 48px));
    margin: auto;
    padding: 28px 30px 24px;
    border: 1px solid var(--border-soft);
    border-radius: 16px;
    background: color-mix(in srgb, var(--bg-elev) 55%, transparent);
    box-shadow: var(--shadow-soft);
    text-align: center;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 50%;
    font-size: 22px;
    font-weight: 700;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .icon svg { width: 24px; height: 24px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .card.ok .icon { color: var(--pick); background: color-mix(in srgb, var(--pick) 16%, transparent); }
  .card.bad .icon { color: var(--reject); background: color-mix(in srgb, var(--reject) 14%, transparent); }
  .card.paused .icon { color: var(--star); background: color-mix(in srgb, var(--star) 14%, transparent); }
  h3 { margin: 12px 0 4px; font-size: 17px; }
  .file { margin: 0; font-size: 13.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .where { margin: 4px 0 18px; color: var(--text-dim); font-size: 12.5px; }
  .link { color: var(--accent); padding: 0; font-size: inherit; }
  .link:hover { text-decoration: underline; }
  .bar { height: 8px; border-radius: 999px; background: color-mix(in srgb, var(--text-faint) 20%, transparent); overflow: hidden; }
  .fill { display: block; height: 100%; border-radius: 999px; background: linear-gradient(90deg, var(--accent), var(--accent-hover)); transition: width 0.4s ease; }
  .card.paused .fill { background: var(--star); opacity: 0.7; }
  .stats {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px 12px;
    margin: 10px 0 18px;
    color: var(--text-dim);
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
    text-align: left;
  }
  .stats.done { justify-content: center; margin: 0 0 10px; }
  .stats .pct { font-weight: 700; color: var(--text); }
  .stats b { font-weight: 600; color: var(--text); }
  .dim { color: var(--text-faint); }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-top: 16px; }
  .hint { margin: 14px 0 0; color: var(--text-faint); font-size: 12px; line-height: 1.55; }
  .err { margin: 6px 0 0; color: var(--reject); font-size: 13px; line-height: 1.55; }
  .notice { margin: 14px 0 0; padding: 8px 10px; border-radius: 8px; color: var(--text); background: color-mix(in srgb, var(--star) 14%, transparent); font-size: 12.5px; }
</style>
