# Fast-scroll compositor recovery - 2026-08-02

## Field evidence

Installed `v1.3.0-nightly.6` scrolled correctly at low speed but visually froze
as soon as the owner moved quickly. Its added telemetry recorded a fast jump to
`scrollTop=52900`, correct visible rows 286-298 and 117 mounted cells. The UI
heartbeat continued every 20 seconds at 18-19 MB heap, thumbnail work was fully
drained, every FoxCull/WebView process remained responsive, and neither Crashpad
nor Windows recorded an application crash.

## Corrected diagnosis

The DOM and virtual range were healthy; the WebView presentation surface was
not. VirtualGrid and SectionedGrid positioned each mounted cell with a CSS
transform and declared `will-change: transform`. During fast scrolling that
made WebView2 rapidly replace roughly 108-117 image compositor layers. The GPU
subprocess retained about 227 MB privately after the viewport had gone black.

## Fix

- Position VirtualGrid cells with absolute `left` and `top` coordinates.
- Position SectionedGrid headers and cells the same way.
- Position horizontal and vertical VirtualStrip cells without transforms.
- Remove per-cell `will-change: transform` promotion.
- Preserve virtualization bounds, keyed indices, overscan, native scroll
  synchronization and asynchronous thumbnail loading unchanged.

## Local gates

- `npm run check`: 0 errors, 0 warnings.
- `npm run build`: passed.
- `cargo check`: passed.
- `git diff --check`: passed with expected Windows line-ending notices only.
