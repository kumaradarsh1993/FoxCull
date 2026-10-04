<script lang="ts">
  // The job centre, docked at the foot of the sidebar (or floating bottom-left
  // when the sidebar is hidden). One card that always answers three questions:
  // what is FoxCull doing, how far along is it, and when will it be done.
  //
  //   * One job running: its title, the file in flight / bytes / speed, time
  //     left, a progress bar and a Stop button.
  //   * Several: "3 tasks", the combined bar and the longest time left; the
  //     chevron opens every job as its own row.
  //   * Just finished: the card says so for a few seconds with its follow-up
  //     ("Show in folder"), then folds into a one-line "Recent" footer that
  //     opens the list. Failures keep a red count until they're looked at.
  //   * Housekeeping (thumbnails, capture dates) is shown smaller and greyer,
  //     and never kept once it ends.
  import { activity, FRESH_MS, fmtBytes, fmtEta, type Job } from "$lib/activity.svelte";

  let expanded = $state(false);
  let dockEl = $state<HTMLElement | null>(null);
  /** The open list floats above the card, wider than the sidebar (a 230 px
   *  column truncated every label), anchored to the card's left edge. */
  let panelPos = $state("");
  function placePanel() {
    if (!dockEl) return;
    const r = dockEl.getBoundingClientRect();
    const w = Math.min(400, Math.max(r.width, 340), window.innerWidth - r.left - 12);
    panelPos = `left:${Math.max(8, r.left + 6)}px; bottom:${Math.round(window.innerHeight - r.top + 6)}px; width:${w}px;`;
  }
  $effect(() => {
    if (!expanded) return;
    placePanel();
    const onResize = () => placePanel();
    window.addEventListener("resize", onResize);
    // Close on a click anywhere outside the card and the list, like a menu.
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node | null;
      if (t && (dockEl?.contains(t) || (t as Element).closest?.(".jobPanel"))) return;
      expanded = false;
    };
    window.addEventListener("pointerdown", onDown, true);
    return () => {
      window.removeEventListener("resize", onResize);
      window.removeEventListener("pointerdown", onDown, true);
    };
  });
  /** Recent was opened since the last failure arrived. */
  let seenProblems = $state(0);

  let running = $derived(activity.running);
  let fg = $derived(activity.foreground);
  let recent = $derived(activity.recent);
  /** A job that finished in the last few seconds, while nothing the owner started runs. */
  let fresh = $derived.by(() => {
    void activity.tick;
    const j = recent[0];
    return !fg.length && j && Date.now() - (j.ended ?? 0) < FRESH_MS ? j : undefined;
  });
  /** The job the collapsed card headlines: work the owner is waiting on, else
   *  a just-finished result (below), else housekeeping. */
  let lead = $derived(fg[0] ?? (fresh ? undefined : running[0]));
  let unseen = $derived(Math.max(0, activity.problems - seenProblems));
  let visible = $derived(running.length > 0 || recent.length > 0);

  $effect(() => {
    if (expanded) seenProblems = activity.problems;
  });
  $effect(() => {
    if (!visible) expanded = false;
  });

  function frac(j: Job): number {
    if (j.state !== "running") return 1;
    return j.total > 0 ? Math.min(1, Math.max(0, j.done / j.total)) : 0;
  }
  const pct = (j: Job) => Math.round(frac(j) * 100);
  const determinate = (j: Job) => j.total > 0;

  /** Second line for a running job: sizes and speed for copies, n of m for
   *  batches, then the job's own detail (the file in flight). */
  function runningLine(j: Job): string {
    void activity.tick;
    if (j.paused) return `Paused at ${pct(j)}%`;
    const parts: string[] = [];
    if (j.unit === "bytes" && j.total > 0) {
      parts.push(`${fmtBytes(j.done)} of ${fmtBytes(j.total)}`);
      const r = activity.rate(j.id);
      if (r > 0) parts.push(`${fmtBytes(r)}/s`);
    } else if (j.unit === "items" && j.total > 1) {
      parts.push(`${j.done.toLocaleString()} of ${j.total.toLocaleString()}`);
    }
    if (j.detail) parts.push(j.detail);
    return parts.join(" · ");
  }
  function left(j: Job): string {
    void activity.tick;
    const e = activity.eta(j.id);
    return e ? `${e} left` : "";
  }
  /** The longest time left among determinate jobs (when ALL will be done). */
  let allLeft = $derived.by(() => {
    void activity.tick;
    const etas = fg.map((j) => activity.etaSeconds(j.id)).filter((s) => Number.isFinite(s));
    const e = etas.length ? fmtEta(Math.max(...etas)) : "";
    return e ? `${e} left` : "";
  });
  /** Combined progress over the determinate foreground jobs. */
  let combined = $derived.by(() => {
    const d = fg.filter(determinate);
    if (!d.length) return -1;
    return d.reduce((a, j) => a + frac(j), 0) / d.length;
  });
  function ago(t: number | undefined): string {
    void activity.tick;
    if (!t) return "";
    const s = Math.round((Date.now() - t) / 1000);
    if (s < 45) return "just now";
    if (s < 3600) return `${Math.round(s / 60)} min ago`;
    return `${Math.round(s / 3600)} h ago`;
  }
  function stateWord(j: Job): string {
    return j.state === "cancelled" ? "Stopped" : j.state === "error" ? "Failed" : "";
  }
</script>

{#snippet icon(j: Job, size = 15)}
  <span class="ji" class:err={j.state === "error"} class:ok={j.state === "done"} class:stop={j.state === "cancelled"} class:quiet={j.quiet} style="--s:{size}px">
    {#if j.state === "error"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3.5 2.8 19.5h18.4L12 3.5Z" /><path d="M12 10v4.2M12 17.2v.1" /></svg>
    {:else if j.state === "done"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="m7.8 12.3 2.8 2.8 5.6-5.8" /></svg>
    {:else if j.state === "cancelled"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M8.5 15.5l7-7" /></svg>
    {:else if j.kind === "move" || j.kind === "copy"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3.5 7.5c0-1.1.9-2 2-2h3.8l2 2h7.2c1.1 0 2 .9 2 2v7.5c0 1.1-.9 2-2 2h-13c-1.1 0-2-.9-2-2Z" /><path d="M9.5 13.2h6M13.2 10.8l2.4 2.4-2.4 2.4" /></svg>
    {:else if j.kind === "merge"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="6" width="7" height="12" rx="1.6" /><rect x="14" y="6" width="7" height="12" rx="1.6" /><path d="M10 12h4" /></svg>
    {:else if j.kind === "export"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15V4M8 8l4-4 4 4" /><path d="M5 13v5c0 1.1.9 2 2 2h10c1.1 0 2-.9 2-2v-5" /></svg>
    {:else if j.kind === "thumbs" || j.kind === "prepare"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="5" width="17" height="14" rx="2" /><path d="m3.5 16 4.6-4.4 3.6 3.4 2.6-2.4 6.2 5.4" /><circle cx="15.5" cy="9.3" r="1.4" /></svg>
    {:else if j.kind === "dates"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></svg>
    {:else if j.kind === "trash"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M9 7V4.8c0-.4.4-.8.8-.8h4.4c.4 0 .8.4.8.8V7M6 7l1 12.2c.1.9.8 1.8 1.8 1.8h6.4c1 0 1.7-.9 1.8-1.8L18 7" /></svg>
    {:else if j.kind === "scan" || j.kind === "relink"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6" /><path d="m15 15 5 5" /></svg>
    {:else}
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M12 11v5M12 8v.1" /></svg>
    {/if}
  </span>
{/snippet}

{#snippet bar(j: Job | null, f: number)}
  <span class="bar" class:thin={j?.quiet} class:paused={j?.paused}>
    <span
      class="fill"
      class:indet={f < 0}
      class:err={j?.state === "error"}
      class:ok={j?.state === "done"}
      style="width:{f < 0 ? 38 : Math.max(2, f * 100)}%"
    ></span>
  </span>
{/snippet}

{#snippet row(j: Job)}
  <div class="row" class:done={j.state !== "running"} class:quiet={j.quiet} class:err={j.state === "error"}>
    {@render icon(j)}
    <div class="rbody">
      <div class="rtop">
        <span class="rl" title={j.label}>{#if stateWord(j)}<b>{stateWord(j)}:</b> {/if}{j.label}</span>
        {#if j.state === "running" && determinate(j)}<span class="rp">{pct(j)}%</span>{/if}
      </div>
      {#if j.state === "running"}
        {#if runningLine(j) || left(j)}
          <div class="rd"><span class="rdt" title={runningLine(j)}>{runningLine(j)}</span>{#if left(j)}<span class="rl2">{left(j)}</span>{/if}</div>
        {/if}
        {#if !j.queued}{@render bar(j, determinate(j) ? frac(j) : -1)}{/if}
        {#if activity.actionsOf(j).length}
          <div class="acts">
            {#each activity.actionsOf(j) as a (a.label)}<button class="act" onclick={a.run}>{a.label}</button>{/each}
          </div>
        {/if}
      {:else}
        <div class="rd">
          {#if j.detail}<span class="rdt wrap" title={j.detail}>{j.detail}</span>{/if}
          <span class="age">{ago(j.ended)}</span>
        </div>
        {#if activity.actionsOf(j).length}
          <div class="acts">
            {#each activity.actionsOf(j) as a (a.label)}<button class="act" onclick={a.run}>{a.label}</button>{/each}
          </div>
        {/if}
      {/if}
    </div>
    {#if j.state === "running" && j.cancellable}
      <button class="x" title="Stop" aria-label={`Stop ${j.label}`} onclick={() => activity.cancel(j.id)}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1.5" /></svg>
      </button>
    {:else if j.state !== "running"}
      <button class="x" title="Dismiss" aria-label="Dismiss" onclick={() => activity.dismiss(j.id)}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 7 10 10M17 7 7 17" /></svg>
      </button>
    {/if}
  </div>
{/snippet}

{#if visible}
  <section class="dock" class:busy={fg.length > 0} class:open={expanded} aria-label="Background tasks" bind:this={dockEl}>
    {#if expanded}
      <div class="panel jobPanel" style={panelPos} role="dialog" aria-label="Tasks">
        {#if running.length}
          <div class="sect">
            <span>In progress</span>
          </div>
          {#each running as j (j.id)}{@render row(j)}{/each}
        {/if}
        {#if recent.length}
          <div class="sect">
            <span>Recent</span>
            <button class="clear" onclick={() => activity.clearFinished()}>Clear</button>
          </div>
          {#each recent as j (j.id)}{@render row(j)}{/each}
        {/if}
      </div>
    {/if}

    <!-- The headline. Always at the bottom so it doesn't jump when the list opens. -->
    <div class="head" role="status" aria-live="polite">
      {#if fg.length > 1}
        <button class="main" onclick={() => (expanded = !expanded)} title="Show every task">
          <span class="stackIco" aria-hidden="true">{fg.length}</span>
          <span class="mt">
            <span class="ml">{fg.length} tasks running</span>
            <span class="md">{fg.map((j) => j.label.split(" → ")[0]).join(" · ")}</span>
          </span>
          {#if allLeft}<span class="eta">{allLeft}</span>{/if}
          <span class="chev" class:up={!expanded} aria-hidden="true"></span>
        </button>
        <div class="barRow">{@render bar(null, combined)}</div>
      {:else if lead}
        <div class="one" class:quiet={lead.quiet}>
          <button class="main" onclick={() => (expanded = !expanded)} title={expanded ? "Hide details" : "Show details"}>
            {@render icon(lead, lead.quiet ? 13 : 15)}
            <span class="mt">
              <span class="ml" title={lead.label}>{lead.label}</span>
              {#if runningLine(lead) || left(lead)}
                <span class="md">{runningLine(lead)}{#if runningLine(lead) && left(lead)} · {/if}{#if left(lead)}<b>{left(lead)}</b>{/if}</span>
              {/if}
            </span>
            {#if determinate(lead)}<span class="pc">{pct(lead)}%</span>{/if}
          </button>
          {#if lead.cancellable}
            <button class="x" title="Stop" aria-label={`Stop ${lead.label}`} onclick={() => activity.cancel(lead.id)}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1.5" /></svg>
            </button>
          {/if}
        </div>
        <div class="barRow">{@render bar(lead, determinate(lead) ? frac(lead) : -1)}</div>
        {#if activity.actionsOf(lead).length}
          <div class="acts">
            {#each activity.actionsOf(lead) as a (a.label)}<button class="act" onclick={a.run}>{a.label}</button>{/each}
          </div>
        {/if}
      {:else if fresh}
        <div class="one fresh" class:err={fresh.state === "error"}>
          <button class="main" onclick={() => (expanded = !expanded)} title="Show recent tasks">
            {@render icon(fresh)}
            <span class="mt">
              <span class="ml" title={fresh.label}>{#if stateWord(fresh)}<b class="sw">{stateWord(fresh)}:</b>{/if}{fresh.label}</span>
              {#if fresh.detail}<span class="md" title={fresh.detail}>{fresh.detail}</span>{/if}
            </span>
          </button>
          {#if activity.actionsOf(fresh).length}<button class="act sm" onclick={activity.actionsOf(fresh)[0].run}>{activity.actionsOf(fresh)[0].label}</button>{/if}
        </div>
      {:else}
        <button class="foot" onclick={() => (expanded = !expanded)} title="Recent tasks">
          {#if unseen}<span class="prob">{unseen} problem{unseen === 1 ? "" : "s"}</span>{/if}
          <span class="fl">{recent.length} recent task{recent.length === 1 ? "" : "s"}</span>
          <span class="chev" class:up={!expanded} aria-hidden="true"></span>
        </button>
      {/if}
      {#if running.length && unseen && !expanded}
        <button class="probLine" onclick={() => (expanded = true)}>{unseen} problem{unseen === 1 ? "" : "s"} — show</button>
      {/if}
    </div>
  </section>
{/if}

<style>
  .dock {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg-panel) 96%, transparent);
    transition: background 240ms ease, border-color 240ms ease;
    max-height: 62vh;
    min-width: 0;
  }
  /* Work the owner is waiting on lifts the card; housekeeping doesn't. */
  .dock.busy {
    border-top-color: color-mix(in srgb, var(--accent) 45%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, var(--bg-panel));
  }

  .panel {
    position: fixed;
    z-index: 120;
    max-height: min(62vh, 560px);
    overflow-y: auto;
    padding: 4px 6px 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--bg-elev) 97%, transparent);
    box-shadow: var(--shadow);
    backdrop-filter: blur(20px) saturate(1.1);
  }
  .sect {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 6px 2px;
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-faint);
    flex-shrink: 0;
  }
  .clear {
    font-size: var(--fs-xs);
    text-transform: none;
    letter-spacing: 0;
    color: var(--accent);
    padding: 1px 4px;
    border-radius: var(--radius-xs);
  }
  .clear:hover { background: var(--bg-hover); }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 7px 6px;
    border-radius: var(--radius-sm);
    flex-shrink: 0;
  }
  .row:hover { background: color-mix(in srgb, var(--bg-hover) 70%, transparent); }
  .rbody { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .rtop { display: flex; align-items: baseline; gap: 6px; min-width: 0; }
  .rl {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-sm);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row.quiet .rl { color: var(--text-dim); font-size: var(--fs-sm); }
  .rl b { margin-right: 4px; font-weight: var(--fw-semibold); }
  .row.err .rl b { color: var(--reject); }
  .rp { flex: 0 0 auto; font-size: var(--fs-xs); color: var(--text-dim); font-variant-numeric: tabular-nums; }
  .rd { display: flex; align-items: baseline; gap: 6px; min-width: 0; font-size: var(--fs-xs); color: var(--text-faint); }
  .rdt { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .rdt.wrap { white-space: normal; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; }
  .rl2 { flex: 0 0 auto; color: var(--text-dim); font-variant-numeric: tabular-nums; }
  .age { flex: 0 0 auto; margin-left: auto; }
  .acts { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 2px; }

  .act {
    font-size: var(--fs-xs);
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--border));
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 6%, transparent);
    white-space: nowrap;
  }
  .act:hover { background: color-mix(in srgb, var(--accent) 14%, transparent); }
  .act.sm { flex: 0 0 auto; align-self: center; }

  .x {
    flex: 0 0 auto;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border-radius: var(--radius-xs);
    color: var(--text-faint);
  }
  .x:hover { background: var(--bg-hover); color: var(--text); }
  .x svg { display: block; width: 13px; height: 13px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; }
  .x rect { fill: currentColor; stroke: none; }

  /* ── icons ── */
  .ji {
    flex: 0 0 auto;
    width: var(--s);
    height: var(--s);
    margin-top: 1px;
    color: var(--accent);
  }
  .ji svg { display: block; width: 100%; height: 100%; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .ji.quiet { color: var(--text-faint); }
  .ji.ok { color: var(--pick); }
  .ji.err { color: var(--reject); }
  .ji.stop { color: var(--text-faint); }

  /* ── the headline ── */
  .head { padding: 8px 10px 9px; display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  /* A column of rows that must never be squeezed (see the layout audit). */
  .head > *, .panel > * { flex-shrink: 0; }
  .one { display: flex; align-items: center; gap: 4px; min-width: 0; }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 9px;
    text-align: left;
    padding: 0;
    border-radius: var(--radius-xs);
  }
  .mt { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .ml {
    font-size: var(--fs-sm);
    font-weight: var(--fw-semibold);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .md {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .md b { font-weight: var(--fw-medium); color: var(--text-dim); }
  .ml .sw { margin-right: 4px; }
  .one.quiet .ml { font-weight: var(--fw-medium); color: var(--text-dim); font-size: var(--fs-sm); }
  .one.fresh .ml { font-weight: var(--fw-semibold); }
  .one.fresh.err .ml { color: var(--reject); }
  .pc { flex: 0 0 auto; font-size: var(--fs-sm); font-weight: var(--fw-semibold); color: var(--text-dim); font-variant-numeric: tabular-nums; }
  .eta { flex: 0 0 auto; font-size: var(--fs-xs); color: var(--text-dim); font-variant-numeric: tabular-nums; }
  .stackIco {
    flex: 0 0 auto;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border-radius: var(--radius-xs);
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    color: var(--accent-on);
    background: var(--accent);
    box-shadow: 2px -2px 0 -0.5px color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .chev {
    flex: 0 0 auto;
    width: 7px;
    height: 7px;
    margin: 0 3px 3px 2px;
    border-right: 1.6px solid var(--text-faint);
    border-bottom: 1.6px solid var(--text-faint);
    transform: rotate(45deg);
    transition: transform 160ms ease;
  }
  .chev.up { transform: rotate(-135deg); margin-bottom: -2px; }

  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 0;
    font-size: var(--fs-xs);
    color: var(--text-faint);
    text-align: left;
  }
  .foot:hover .fl { color: var(--text-dim); }
  .fl { flex: 1; }
  .prob, .probLine {
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    color: var(--reject);
  }
  .prob {
    padding: 1px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--reject) 13%, transparent);
  }
  .probLine { text-align: left; padding: 0; }

  /* ── bars ── */
  .barRow { min-width: 0; }
  .bar {
    display: block;
    height: 4px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text-faint) 18%, transparent);
    overflow: hidden;
  }
  .bar.thin { height: 3px; }
  .fill {
    display: block;
    height: 100%;
    border-radius: 999px;
    background: linear-gradient(90deg, var(--accent), var(--accent-hover));
    transition: width 0.3s ease;
  }
  .fill.ok { background: var(--pick); }
  .bar.paused .fill { background: var(--star); opacity: 0.7; }
  .fill.err { background: var(--reject); }
  .bar.thin .fill { background: color-mix(in srgb, var(--text-faint) 70%, transparent); }
  /* Indeterminate: a segment sweeping back and forth. */
  .fill.indet { animation: sweep 1.25s ease-in-out infinite alternate; }
  @keyframes sweep {
    from { transform: translateX(-30%); }
    to { transform: translateX(200%); }
  }
  @media (prefers-reduced-motion: reduce) {
    .fill.indet { animation: none; opacity: 0.6; }
  }
</style>
