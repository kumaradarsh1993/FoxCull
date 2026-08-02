<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import { untrack } from "svelte";
  import { api } from "$lib/api";
  import { setScrolling } from "$lib/thumbnail-loader";

  let {
    items,
    cellMin = 176,
    gap = 6,
    overscanRows = 3,
    activeIndex = 0,
    cell,
  }: {
    items: T[];
    cellMin?: number;
    gap?: number;
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
  let rowCount = $derived(Math.ceil(items.length / cols));
  let totalH = $derived(Math.max(0, rowCount * rowH - gap));

  let firstRow = $derived(Math.max(0, Math.floor(scrollTop / rowH) - overscanRows));
  let lastRow = $derived(
    Math.min(rowCount - 1, Math.ceil((scrollTop + vpHeight) / rowH) + overscanRows),
  );

  // ── Cell recycling ─────────────────────────────────────────────────────────
  // This is the fix for the fast-scroll freeze. The old design keyed the {#each}
  // by item index, so every window shift DESTROYED and RECREATED all ~108 visible
  // cells — each a full Thumb (image fetch + ResizeObserver + live-decode). A fast
  // scroll produced a burst of teardowns/rebuilds that choked WebView2's
  // compositor: ALL painting stalled for 10+ seconds while handles piled up ~18k
  // (GPU idle throughout — a pipeline stall, not a GPU one). Placeholder-gating
  // that burst (nightly.8/.9) only relocated it and added a blank/reload flicker;
  // it did not stop the freeze.
  //
  // Recycling removes the burst entirely. We keep a stable, grow-only pool of cell
  // slots and map each visible item to slot `index % poolSize`. Because the pool
  // is larger than the visible window, at most one visible item maps to a slot, so
  // item `i` always lives in slot `i % poolSize`. Scrolling changes WHICH item a
  // slot shows (Svelte updates the existing Thumb's `item` prop in place), never
  // how many slots exist — nothing mounts or unmounts mid-scroll, so there is no
  // burst to choke on, and tiles that stay visible keep their pixels (no flicker).
  let poolSize = $state(0);
  $effect(() => {
    const rowsInView = vpHeight > 0 && rowH > 0 ? Math.ceil(vpHeight / rowH) : 0;
    const need = (rowsInView + overscanRows * 2 + 2) * cols;
    // Grow-only; untrack the read so this effect doesn't depend on its own write.
    if (need > untrack(() => poolSize)) poolSize = need;
  });

  // slot -> global item index it should render (or -1 when this slot has no
  // visible item — only ever at the very top/bottom edges).
  let slotItem = $derived.by(() => {
    const map: number[] = new Array(poolSize).fill(-1);
    if (!items.length || poolSize <= 0) return map;
    const lo = firstRow * cols;
    const hi = Math.min(items.length - 1, (lastRow + 1) * cols - 1);
    for (let i = lo; i <= hi; i++) map[i % poolSize] = i;
    return map;
  });

  // Measure the viewport (and react to window/pane resizes).
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

  // Correctness cannot depend on requestAnimationFrame here. WebView2 can keep
  // compositor scrolling while pausing an outstanding rAF; the stale scrollTop
  // then leaves every virtual cell positioned outside the actual viewport.
  // Reading the native scroll event directly is cheap and always catches the
  // final position after a fast wheel gesture. With recycling there is no per-
  // event churn cost, so no throttle/placeholder gate is needed.
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
    // Fast fling (a big single jump OR fast cumulative motion) → defer image
    // fetches until settle so the fetch/decode burst can't choke the compositor.
    // A gentle monitoring scroll never trips this and stays fully live; recycling
    // keeps already-loaded tiles painted throughout either way.
    if (delta > rowH * 1.5 || recentMove > rowH * 2.5) setScrolling(true);
    clearTimeout(scrollSettleTimer);
    scrollSettleTimer = setTimeout(() => {
      if (viewport && scrollTop !== viewport.scrollTop) scrollTop = viewport.scrollTop;
      setScrolling(false);
      recentMove = 0;
      void api.logNote(
        `grid-scroll top=${Math.round(viewport?.scrollTop ?? 0)} first=${firstRow} last=${lastRow} pool=${poolSize}`,
      );
    }, 160);
  }

  $effect(() => () => {
    clearTimeout(scrollSettleTimer);
    setScrolling(false); // never leave the loader paused if we unmount mid-fling
  });

  /** Keep a given index visible — used by keyboard navigation. With `center`,
   *  place it mid-viewport (used to restore position when returning from Focus). */
  export function scrollToIndex(i: number, center = false) {
    const el = viewport;
    if (!el || cols <= 0) return;
    const row = Math.floor(i / cols);
    const y = row * rowH;
    if (center) {
      el.scrollTop = Math.max(0, y - (vpHeight - cellW) / 2);
      return;
    }
    if (y < el.scrollTop) el.scrollTop = y;
    else if (y + cellW > el.scrollTop + vpHeight) el.scrollTop = y + cellW - vpHeight;
  }

  export function columnCount() {
    return cols;
  }
</script>

<div class="vp" bind:this={viewport} onscroll={onScroll}>
  <div class="canvas" style="height:{totalH}px">
    {#each slotItem as idx, s (s)}
      {#if idx >= 0}
        {@const row = Math.floor(idx / cols)}
        {@const col = idx % cols}
        <div
          class="cellpos"
          class:active={idx === activeIndex}
          style="left:{col * (cellW + gap)}px; top:{row * rowH}px; width:{cellW}px; height:{cellW}px"
        >
          {@render cell(items[idx], idx)}
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
  .cellpos {
    position: absolute;
    /* Keep media tiles out of individual GPU layers. Transform positioning plus
       will-change can black out WebView2's surface during rapid scroll churn. */
  }
</style>
