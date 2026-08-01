<script lang="ts" generics="T">
  import type { Snippet } from "svelte";

  let {
    items,
    activeIndex = 0,
    cellSize = 108,
    gap = 6,
    overscan = 6,
    orientation = "h",
    cell,
  }: {
    items: T[];
    activeIndex?: number;
    cellSize?: number;
    gap?: number;
    overscan?: number;
    orientation?: "h" | "v";
    cell: Snippet<[T, number]>;
  } = $props();

  let viewport = $state<HTMLDivElement | null>(null);
  let scrollPos = $state(0);
  let vpMain = $state(0); // viewport length along the scroll axis

  let step = $derived(cellSize + gap);
  let total = $derived(Math.max(0, items.length * step - gap));
  let first = $derived(Math.max(0, Math.floor(scrollPos / step) - overscan));
  let last = $derived(Math.min(items.length - 1, Math.ceil((scrollPos + vpMain) / step) + overscan));

  // Primitive indices keep retained cells stable during the many small scroll
  // events emitted by eased wheel motion. Fresh wrapper objects made every
  // Thumb receive a redundant prop update on every animation frame.
  let visibleIndices = $derived.by(() => {
    const out: number[] = [];
    for (let i = first; i <= last; i++) {
      if (i < 0 || i >= items.length) continue;
      out.push(i);
    }
    return out;
  });

  function onscroll() {
    if (!viewport) return;
    scrollPos = orientation === "h" ? viewport.scrollLeft : viewport.scrollTop;
  }

  // Measure viewport length along the scroll axis; also wire a non-passive wheel
  // handler so a normal vertical wheel and a Logitech-style thumb wheel both
  // move a horizontal strip. WebView2 reports the thumb wheel with the opposite
  // physical sign, so dedicated deltaX input is intentionally inverted here.
  $effect(() => {
    const el = viewport;
    if (!el) return;
    const measure = () => (vpMain = orientation === "h" ? el.clientWidth : el.clientHeight);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);

    let onWheel: ((e: WheelEvent) => void) | null = null;
    let wheelTarget = el.scrollLeft;
    let lastWheelAt = 0;
    if (orientation === "h") {
      onWheel = (e: WheelEvent) => {
        const horizontal = Math.abs(e.deltaX) > Math.abs(e.deltaY);
        const raw = horizontal ? -e.deltaX : e.deltaY;
        if (raw === 0) return;
        const unit = e.deltaMode === WheelEvent.DOM_DELTA_LINE
          ? 16
          : e.deltaMode === WheelEvent.DOM_DELTA_PAGE
            ? el.clientWidth
            : 1;
        const now = performance.now();
        // A new gesture begins from the live DOM position; events in the same
        // gesture accumulate toward one compositor-owned smooth target.
        if (now - lastWheelAt > 120) wheelTarget = el.scrollLeft;
        lastWheelAt = now;
        const max = Math.max(0, el.scrollWidth - el.clientWidth);
        wheelTarget = Math.max(0, Math.min(max, wheelTarget + raw * unit));
        el.scrollTo({
          left: wheelTarget,
          behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
        });
        e.preventDefault();
      };
      el.addEventListener("wheel", onWheel, { passive: false });
    }
    return () => {
      ro.disconnect();
      if (onWheel) el.removeEventListener("wheel", onWheel);
    };
  });

  // Lightroom-style: only scroll when the active cell isn't comfortably in view,
  // and then just enough to bring it to the near edge — never a hard recenter.
  // Clicking an already-visible tile leaves the strip exactly where it is; the
  // user can also scroll freely without the active cell being yanked back
  // (we key on activeIndex and read the live DOM scroll, NOT the reactive
  // scrollPos, so manual scrolling doesn't re-trigger this).
  $effect(() => {
    const i = activeIndex; // the intended reactive trigger
    const el = viewport;
    if (!el || !vpMain) return;
    void step; // re-run on layout changes (cell size / item count / resize)
    void total;
    const cur = orientation === "h" ? el.scrollLeft : el.scrollTop;
    const cellStart = i * step;
    const cellEnd = cellStart + cellSize;
    const margin = Math.min(step, vpMain / 4); // keep a little context at the edge
    let v = cur;
    if (cellStart < cur + margin) v = cellStart - margin;
    else if (cellEnd > cur + vpMain - margin) v = cellEnd + margin - vpMain;
    v = Math.max(0, Math.min(v, Math.max(0, total - vpMain)));
    if (Math.abs(v - cur) < 1) return; // already in view — don't move
    const behavior = matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
    if (orientation === "h") el.scrollTo({ left: v, behavior });
    else el.scrollTo({ top: v, behavior });
  });
</script>

<div class="strip {orientation}" bind:this={viewport} {onscroll} style="--cell:{cellSize}px">
  <div class="canvas" style={orientation === "h" ? `width:${total}px` : `height:${total}px`}>
    {#each visibleIndices as i (i)}
      <div
        class="cellpos"
        style="transform:{orientation === 'h' ? `translateX(${i * step}px)` : `translateY(${i * step}px)`}; width:var(--cell); height:var(--cell)"
      >
        {@render cell(items[i], i)}
      </div>
    {/each}
  </div>
</div>

<style>
  .strip {
    background: var(--bg-panel);
  }
  .strip.h {
    width: 100%;
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    overscroll-behavior-inline: contain;
  }
  .strip.v {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .canvas {
    position: relative;
    height: 100%;
  }
  .strip.v .canvas {
    width: 100%;
  }
  .cellpos {
    position: absolute;
  }
  .strip.h .cellpos {
    top: calc((100% - var(--cell)) / 2);
    left: 0;
  }
  .strip.v .cellpos {
    left: calc((100% - var(--cell)) / 2);
    top: 0;
  }
</style>
