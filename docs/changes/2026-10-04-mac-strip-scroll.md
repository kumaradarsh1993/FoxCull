# Filmstrip scrolling on a Mac

Nightly `v1.5.2-nightly.8` (one fix). Owner, 2026-10-04: the scrolling of the
bottom bar (the filmstrip) "felt a little non intuitive" in the library on
the Mac.

## Cause

`VirtualStrip`'s wheel handler was written for Windows: it inverts `deltaX`
(WebView2 reports a Logitech thumb wheel with the opposite sign) and turns
every wheel event into a smooth `scrollTo` towards an accumulated target. On a
Mac trackpad or Magic Mouse that meant:

- a sideways two-finger swipe moved the strip the wrong way;
- every event (dozens a second, with the system's own momentum) was animated
  again, so the strip trailed behind the fingers and floated after they
  lifted.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/components/VirtualStrip.svelte` | UX | On macOS, a sideways swipe is left to the browser's native scrolling (natural direction, momentum, edge bounce), and a vertical one moves the strip by the same amount at once (`scrollLeft += deltaY`). Windows keeps its path unchanged. |

## Behavior changes

- Mac: the filmstrip follows two-finger swipes 1:1, in both directions, in
  every view (grid, details, Focus). Windows: unchanged.

## Risks / compat

- Platform test is `navigator.platform` (as in `drag.svelte.ts`).
- A Mac with a plain notched wheel mouse now moves the strip in the wheel's
  own steps instead of a smoothed glide.

## Verification actually run

- Harness on this Mac (Chromium, `MacIntel`): a vertical wheel event of 120
  moved the strip 120 px immediately and was consumed; a horizontal one was
  not consumed (native scrolling takes it).
- Not tried on the real app's WebKit with a trackpad; the owner's hands are
  the test.
