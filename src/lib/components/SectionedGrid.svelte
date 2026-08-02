<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import { untrack } from "svelte";
  import { api } from "$lib/api";
  import { setScrolling } from "$lib/thumbnail-loader";

  type GridSection = { label: string; count: number; level?: 1 | 2; cellCount?: number };

  // A virtualized grid split into labeled sections (month headers), sharing the
  // same flat `items` array + global index space as VirtualGrid so keyboard
  // navigation and active-cell highlighting stay identical. `groups` gives the
  // section labels and their item counts, IN the same order as `items`.
  let {
    items,
    groups,
    cellMin = 176,
    gap = 6,
    headerH = 38,
    overscanRows = 3,
    activeIndex = 0,
    cell,
  }: {
    items: T[];
    groups: GridSection[];
    cellMin?: number;
    gap?: number;
    headerH?: number;
    overscanRows?: number;
    activeIndex?: number;
    cell: Snippet<[T, number]>;
  } = $props();

  let viewport = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let vpWidth = $state(0);
  let vpHeight = $state(0);

  let cols = $derived(Math.max(1, Math.floor((vpWidth + gap) / (cellMin + gap))));
  let cellW = $derived(cols > 0 ? (vpWidth - gap * (cols - 1)) / cols : cellMin);
  let rowH = $derived(cellW + gap);

  type Row =
    | { type: "header"; key: string; label: string; count: number; level: 1 | 2; y: number; h: number }
    | { type: "cells"; key: string; idxs: number[]; y: number; h: number };
  type HeaderRow = Extract<Row, { type: "header" }>;

  // Flatten the sections into a list of positioned rows (one header row + N cell
  // rows per group). Recomputed only when layout inputs change, not on scroll.
  let rows = $derived.by(() => {
    const out: Row[] = [];
    let y = 0;
    let gi = 0; // running global item index
    for (let g = 0; g < groups.length; g++) {
      const grp = groups[g];
      const level = grp.level ?? 1;
      const h = level === 2 ? Math.max(28, headerH - 8) : headerH;
      out.push({ type: "header", key: `h${g}`, label: grp.label, count: grp.count, level, y, h });
      y += h;
      let remaining = grp.cellCount ?? grp.count;
      while (remaining > 0) {
        const n = Math.min(cols, remaining);
        const idxs: number[] = [];
        for (let k = 0; k < n; k++) idxs.push(gi + k);
        out.push({ type: "cells", key: `c${gi}`, idxs, y, h: rowH });
        y += rowH;
        gi += n;
        remaining -= n;
      }
    }
    return out;
  });

  let totalH = $derived(rows.length ? rows[rows.length - 1].y + rows[rows.length - 1].h : 0);

  let visible = $derived.by(() => {
    const pad = overscanRows * rowH;
    const lo = scrollTop - pad;
    const hi = scrollTop + vpHeight + pad;
    const out: Row[] = [];
    for (const r of rows) {
      if (r.y + r.h < lo) continue;
      if (r.y > hi) break;
      out.push(r);
    }
    return out;
  });

  // Headers are few, cheap and image-free — render them directly (keyed by their
  // stable key). Cells are the expensive part and are RECYCLED (see below).
  let visibleHeaders = $derived(visible.filter((r): r is HeaderRow => r.type === "header"));

  // Flatten the visible cell rows into positioned cells (global index + x/y).
  let visibleCells = $derived.by(() => {
    const out: { gi: number; x: number; y: number }[] = [];
    for (const r of visible) {
      if (r.type !== "cells") continue;
      for (let c = 0; c < r.idxs.length; c++) {
        out.push({ gi: r.idxs[c], x: c * (cellW + gap), y: r.y });
      }
    }
    return out;
  });

  // ── Cell recycling ─────────────────────────────────────────────────────────
  // Same technique as VirtualGrid: a grow-only pool of slots, cell `gi` rendered
  // in slot `gi % poolSize`. A window shift updates the existing Thumb's `item`
  // prop in place instead of destroying/recreating cells. This reduces routine
  // churn; loader pacing is the main-thread freeze fix. Headers render outside
  // the pool.
  let poolSize = $state(0);
  $effect(() => {
    const need = visibleCells.length + cols * 2 + 2;
    if (need > untrack(() => poolSize)) poolSize = need;
  });

  let slotCell = $derived.by(() => {
    const map: ({ gi: number; x: number; y: number } | null)[] = new Array(poolSize).fill(null);
    if (poolSize <= 0) return map;
    for (const vc of visibleCells) map[vc.gi % poolSize] = vc;
    return map;
  });

  $effect(() => {
    const el = viewport;
    if (!el) return;
    const measure = () => {
      vpWidth = el.clientWidth;
      vpHeight = el.clientHeight;
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });

  // Read scroll directly (never via rAF — WebView2 can pause an outstanding rAF
  // mid-gesture and strand the range), with a settle-timer safety net.
  let lastTop = 0;
  let lastScrollAt = 0;
  let recentMove = 0;
  let scrollSettleTimer: ReturnType<typeof setTimeout> | undefined;
  function onScroll() {
    const el = viewport;
    if (!el) return;
    const top = el.scrollTop;
    const now = performance.now();
    const delta = Math.abs(top - lastTop);
    recentMove = now - lastScrollAt < 120 ? recentMove + delta : delta;
    lastScrollAt = now;
    lastTop = top;
    scrollTop = top;
    // Defer image fetches during a fast fling — see VirtualGrid.
    if (delta > rowH * 1.5 || recentMove > rowH * 2.5) setScrolling(true);
    clearTimeout(scrollSettleTimer);
    scrollSettleTimer = setTimeout(() => {
      if (viewport && scrollTop !== viewport.scrollTop) scrollTop = viewport.scrollTop;
      setScrolling(false);
      recentMove = 0;
      void api.logNote(
        `sectioned-grid-scroll top=${Math.round(viewport?.scrollTop ?? 0)} cells=${visibleCells.length} pool=${poolSize}`,
      );
    }, 160);
  }

  $effect(() => () => {
    clearTimeout(scrollSettleTimer);
    setScrolling(false); // never leave the loader paused if we unmount mid-fling
  });

  /** Bring a global item index into view (used by keyboard navigation). With
   *  `center`, place it mid-viewport (restores position on return from Focus). */
  export function scrollToIndex(i: number, center = false) {
    const el = viewport;
    if (!el) return;
    const r = rows.find(
      (rw) => rw.type === "cells" && rw.idxs.length > 0 && i >= rw.idxs[0] && i <= rw.idxs[rw.idxs.length - 1],
    );
    if (!r) return;
    if (center) {
      el.scrollTop = Math.max(0, r.y - (vpHeight - rowH) / 2);
      return;
    }
    // Reveal the month header too when scrolling up to a section's first row.
    if (r.y - headerH < el.scrollTop) el.scrollTop = Math.max(0, r.y - headerH);
    else if (r.y + rowH > el.scrollTop + vpHeight) el.scrollTop = r.y + rowH - vpHeight;
  }

  export function columnCount() {
    return cols;
  }
</script>

<div class="vp" bind:this={viewport} onscroll={onScroll}>
  <div class="canvas" style="height:{totalH}px">
    {#each visibleHeaders as r (r.key)}
      <div class="hdr level-{r.level}" style="top:{r.y}px; height:{r.h}px">
        <strong>{r.label}</strong>
        <span>{r.count}</span>
      </div>
    {/each}
    {#each slotCell as sc, s (s)}
      {#if sc}
        <div
          class="cellpos"
          class:active={sc.gi === activeIndex}
          style="left:{sc.x}px; top:{sc.y}px; width:{cellW}px; height:{cellW}px"
        >
          {@render cell(items[sc.gi], sc.gi)}
        </div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .vp {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .canvas {
    position: relative;
    width: 100%;
  }
  .hdr {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    font-family: var(--font-display);
    font-size: 15px;
    font-weight: 680;
    letter-spacing: -0.01em;
    color: var(--text);
    border-top: 1px solid var(--border-soft);
    border-bottom: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg-panel) 78%, transparent);
    padding: 0 14px 4px;
    backdrop-filter: blur(12px);
  }
  .hdr.level-2 {
    left: 18px;
    right: 0;
    font-size: 12px;
    font-weight: 700;
    color: var(--text-dim);
    border-top-style: dashed;
    background: color-mix(in srgb, var(--bg-panel) 68%, transparent);
    padding-left: 12px;
  }
  .hdr span {
    min-width: 24px;
    padding: 2px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-elev) 82%, transparent);
    color: var(--text-faint);
    font-size: 11px;
    text-align: center;
  }
  .cellpos {
    position: absolute;
    /* Coordinates are deliberate: per-tile transform promotion overwhelmed
       WebView2's compositor during fast Grid traversal. */
  }
</style>
