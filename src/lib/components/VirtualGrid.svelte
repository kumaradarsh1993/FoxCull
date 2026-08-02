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
  //
  // Fast-scroll blanking guard (measured, 2026-08-02). A fast scroll makes
  // WebView2's compositor choke on the burst of tile mount/unmount + image work:
  // ALL painting stalls for 10+ seconds (rAF stops firing), WebView2's handle
  // count spikes ~18k as frames pile up behind the stalled compositor, RAM jumps
  // ~900 MB, then it drains — while the GPU stays idle (it is a pipeline stall,
  // not a GPU/TDR one) and the process stays responsive. Sometimes it doesn't
  // drain and the app must be relaunched. While motion is fast we render
  // image-free placeholder cells so NO per-tile image fetch, decode or observer
  // is created mid-scroll, then mount the real tiles once motion settles.
  //
  // The trigger keys on how far the viewport moves, because that is what
  // distinguishes a fling/held-key from a gentle scan: a big single jump (one
  // large wheel step or a keyboard page) OR fast cumulative motion (a held arrow
  // key) blanks the tiles; a gentle single notch (well under a row) never does,
  // so ordinary monitoring scroll stays fully live. (The previous heuristic keyed
  // on rapid event *bursts* and never fired on real flings, which arrive as one
  // big jump — that is why nightly.8 did nothing.) Positioning is untouched, so
  // this never depends on rAF for correctness.
  let isScrolling = $state(false);
  let lastScrollAt = 0;
  let lastTop = 0;
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
    const fast = delta > rowH * 1.5 || recentMove > rowH * 2.5;
    if (fast && !isScrolling)
      void api.logNote(`grid-gate ON delta=${Math.round(delta)} recent=${Math.round(recentMove)} rowH=${Math.round(rowH)}`);
    if (fast) isScrolling = true;
    clearTimeout(scrollSettleTimer);
    scrollSettleTimer = setTimeout(() => {
      // Timer fallback catches a missed/throttled final compositor scroll event.
      if (viewport && scrollTop !== viewport.scrollTop) scrollTop = viewport.scrollTop;
      isScrolling = false;
      recentMove = 0;
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
        style="left:{col * (cellW + gap)}px; top:{row * rowH}px; width:{cellW}px; height:{cellW}px"
      >
        {#if isScrolling}
          <div class="cellskeleton"></div>
        {:else}
          {@render cell(items[i], i)}
        {/if}
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
    /* Keep media tiles out of individual GPU layers. Transform positioning plus
       will-change can black out WebView2's surface during rapid scroll churn. */
  }
  /* Shown in place of a real tile only while a fast scroll is in flight. Matches
     Thumb's idle background so swapping the real tile back in never colour-pops.
     Deliberately image-free: this is what keeps the GPU texture churn off the
     compositor during a fling. */
  .cellskeleton {
    width: 100%;
    height: 100%;
    background: color-mix(in srgb, var(--text-faint) 12%, var(--viewport-bg));
  }
</style>
