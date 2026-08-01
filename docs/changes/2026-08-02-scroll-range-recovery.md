# Scroll-range recovery - 2026-08-02

## Reproduction and evidence

Installed `v1.3.0-nightly.5` permanently blanked after roughly two quick Grid
pages. Its live log continued 20-second UI heartbeats at about 18-19 MB heap and
reported `pending=0 queue=0 inflight=0` with 180 memoized thumbnails. The visible
Loading thumbnails activity also stopped progressing.

## Root causes

1. VirtualGrid and SectionedGrid coalesced native scroll events behind one
   outstanding `requestAnimationFrame`. WebView2 could keep compositor scrolling
   while that callback remained pending, permanently desynchronizing the DOM
   cell range from the real `scrollTop`.
2. The custom smooth horizontal strip used another JavaScript rAF loop.
3. If scrolling canceled every queued thumbnail before one completed, the queue
   reached zero with `jobDone === 0`; the activity job was left running forever.

## Fix

- Update Grid/sectioned virtual ranges directly on native scroll events.
- Re-read and repair the final DOM scroll position after 160 ms.
- Log settled scroll position, visible bounds and mounted-cell count for future
  field diagnosis.
- Use compositor-native smooth scrolling for the horizontal strip; retain the
  Logitech delta-direction correction and reduced-motion behavior.
- End every drained thumbnail activity batch, including all-canceled batches.

## Stress verification

A temporary local route mounted the production VirtualGrid and VirtualStrip
components with 6,825 synthetic media cells. It completed:

- A 120-increment top-to-bottom Grid scroll.
- Twelve complete alternating bottom-to-top/top-to-bottom traversals.
- Correct settled mounts on every cycle: indices 0-80 at the top and 6759-6824
  at the end, never an empty range.
- Repeated native smooth strip repositioning with populated virtual ranges.

The harness route, browser tab and local server were removed afterward.

## Release gates

- `npm run check`: 0 errors, 0 warnings.
- `cargo check`: passed.
- `npm run build`: passed.
- `git diff --check`: passed with expected Windows line-ending notices only.
