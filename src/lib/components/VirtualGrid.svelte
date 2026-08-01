<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import { api } from "$lib/api";

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

  // Keep each-block context primitive and stable. Rebuilding wrapper objects on
  // every scroll frame made Svelte re-send every visible `item` prop even when
  // the item had not changed, restarting Thumb's image state and causing the
  // entire loaded viewport to blink.
  let visibleIndices = $derived.by(() => {
    const out: number[] = [];
    if (!items.length) return out;
    for (let r = firstRow; r <= lastRow; r++) {
      for (let c = 0; c < cols; c++) {
        const i = r * cols + c;
        if (i >= items.length) break;
        out.push(i);
      }
    }
    return out;
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
  // Reading the native scroll event directly is cheap (only two row bounds
  // change) and always catches the final position after a fast wheel gesture.
  let scrollSettleTimer: ReturnType<typeof setTimeout> | undefined;
  function onScroll() {
    const el = viewport;
    if (!el) return;
    scrollTop = el.scrollTop;
    clearTimeout(scrollSettleTimer);
    scrollSettleTimer = setTimeout(() => {
      // Timer fallback catches a missed/throttled final compositor scroll event.
      if (viewport && scrollTop !== viewport.scrollTop) scrollTop = viewport.scrollTop;
      void api.logNote(
        `grid-scroll top=${Math.round(viewport?.scrollTop ?? 0)} first=${firstRow} last=${lastRow} mounted=${visibleIndices.length}`,
      );
    }, 160);
  }

  $effect(() => () => clearTimeout(scrollSettleTimer));

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
    {#each visibleIndices as i (i)}
      {@const row = Math.floor(i / cols)}
      {@const col = i % cols}
      <div
        class="cellpos"
        class:active={i === activeIndex}
        style="transform:translate({col * (cellW + gap)}px,{row * rowH}px); width:{cellW}px; height:{cellW}px"
      >
        {@render cell(items[i], i)}
      </div>
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
    top: 0;
    left: 0;
    will-change: transform;
  }
</style>
