// Svelte action for floating panels (toolbar menus, the settings popover):
// keep the whole panel inside the window.
//
// Menus are anchored to their button with plain CSS, which is right until the
// button moves. The toolbar wraps to two rows at laptop widths, pushing
// Arrange/Filters to the right edge, so a left-anchored 316px menu hung 135px
// off-screen on a 1280px Mac window. Instead of re-deriving an anchor for every
// breakpoint, the panel measures itself after layout and slides back inside,
// and caps its height to the room below its top edge (scrolling past that).
//
// Pairs with the `[data-keep-in-view] > *` rule in app.css: a height-capped
// flex column would otherwise shrink its rows (the Settings "Theme" row
// overlapped "Interface size" that way).
export function keepInView(node: HTMLElement, margin = 8) {
  node.dataset.keepInView = "";
  let frame = 0;

  const measure = () => {
    node.style.translate = "";
    node.style.maxHeight = "";
    const r = node.getBoundingClientRect();
    const vw = document.documentElement.clientWidth;
    const vh = document.documentElement.clientHeight;
    let dx = 0;
    if (r.right > vw - margin) dx = vw - margin - r.right;
    if (r.left + dx < margin) dx = margin - r.left;
    if (dx) node.style.translate = `${dx}px 0`;
    const room = vh - margin - r.top;
    if (r.height > room) {
      node.style.maxHeight = `${Math.max(120, room)}px`;
      node.style.overflowY = "auto";
    }
  };
  const fit = () => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(measure);
  };

  // First pass synchronously: the action runs once the node is in the DOM but
  // before the browser paints, so the panel never flashes at its unfitted spot.
  measure();
  const ro = new ResizeObserver(fit);
  ro.observe(node);
  window.addEventListener("resize", fit);
  return {
    destroy() {
      cancelAnimationFrame(frame);
      ro.disconnect();
      window.removeEventListener("resize", fit);
    },
  };
}
