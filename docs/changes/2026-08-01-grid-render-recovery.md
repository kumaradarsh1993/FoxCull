# Grid render recovery - 2026-08-01

## Report

In installed `v1.3.0-nightly.4`, scrolling a normal photo Grid a few rows could
leave new tiles blank, then leave the entire viewport black after scrolling
back. The condition persisted for more than five minutes. Grid and bottom-strip
images also blinked at every smooth-scroll increment.

## Live evidence

The installed process remained responsive and its 20-second heartbeat
continued. JS heap stayed around 18-19 MB. At the same time the loader reported
`pending=0 queue=0 inflight=0` with 135 memoized thumbnail URLs. This excluded a
scan, decoder, memory or thumbnail-queue deadlock: WebView2 had stopped painting
or observing virtual cells while the application remained alive.

## Fix

- Removed the duplicate `.app` scale transform. Standard mode now has no
  transformed ancestor; Compact and TV-large have one root transform only.
- Reverted per-cell paint containment and overflow-anchor changes added in
  nightly.2.
- Changed VirtualGrid and VirtualStrip visible sets to stable primitive indices
  rather than fresh wrapper objects on every scroll frame.
- Keyed Thumb loading to stable media path/kind/size identity so redundant
  virtualization updates cannot reset loaded image state.
- Removed the opacity transition that made remounted cached cells visibly flash.
- Bypassed IntersectionObserver for already-virtualized Grid, filmstrip and
  Details cells. Visibility deferral remains opt-in for unvirtualized Edit and
  Trash lists.
- Restored a 400 px image lookahead; heavy video concurrency remains separately
  capped at two jobs.

## Verification

- Live log diagnosis against the installed frozen nightly.4 process.
- `npm run check`: 0 errors, 0 warnings.
- `cargo check`: passed.
- `npm run build`: passed.
- `git diff --check`: passed with expected Windows line-ending notices only.
