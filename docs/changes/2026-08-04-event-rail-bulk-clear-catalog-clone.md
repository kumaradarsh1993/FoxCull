# Event banners, bulk tag removal, Clear-metadata, and the catalog clone bug

## Intent

Owner feedback after using nightly.3. Seven items, one of which turned out to be
a data-integrity bug he had reported only as a confusing symptom.

## 1. The phantom "1 file could not be found" — a catalog clone

He was in `E:\Dubai trip`, the Filters panel claimed one file was missing, and
nothing in the folder was marked "?". Reading the catalogs directly explained it:

```
D:  MISSING: Movies _ Final Exports_bak/Chaos Day 1 Drone shots.mp4   (never existed on D:)
E:  MISSING: Movies _ Final Exports_bak/Chaos Day 1 Drone shots.mp4   (never existed on E:)
F:  trims:   Movies _ Final Exports_bak/Chaos Day 1 Drone shots.mp4   exists=True
```

`set_library_root` seeded a new per-drive catalog by copying **whatever catalog
was already open**. That branch existed for the legacy single-catalog migration,
but it fires on the first visit to *any* new drive. Since keys are paths relative
to the drive root, every cloned row describes a file that does not exist there.
D:, E: and F: also all carried an identical 7,254-row capture cache — the same
clone showing through.

**Fix:** only a drive's own legacy `<drive>/fox-cull.catalog` is ever adopted. A
drive FoxCull has not seen before starts empty.

## 2. Events reworked: a banner inside the timeline, not a grouping

The owner's revised design, and it is lighter than what shipped: events should
not reorder the grid at all. The timeline stays the spine and an event paints as
a continuous coloured bar down the left of the rows it occupies — Google Photos
showing an album inside a date feed.

- New `EventRail.svelte` — the bar, with the name running up it, and a colour
  derived from the event name (deterministic, nothing stored, same trip is the
  same colour everywhere).
- `eventRuns` are maximal runs of consecutive items sharing a primary event,
  computed once over the already-sorted view in `+page.svelte`.
- `VirtualGrid` and `SectionedGrid` reserve a 30 px gutter **for the whole grid**
  whenever any event is in view — never per-row, because a gutter that appeared
  and vanished mid-scroll would reflow every tile.
- In `SectionedGrid` a run crossing a section header is emitted as two bands: the
  month separator wins and the event resumes underneath.
- **Self-disables under name/size/type ordering**, where a "continuous" run is
  not a meaningful claim.
- The old `Group ▸ Event` block view with cover art is kept for when an album
  view is actually wanted.

## 3. Events now behave like tags (lifecycle)

Removing the last member deletes the event. Previously "Dubai" and "dubaisecond"
survived with zero members and stayed in the Add-to menu forever — a list that
could only grow. `remove_from_event` prunes in the same transaction,
`prune_empty_events` sweeps before every listing, and `list_events` filters
memberless rows defensively.

## 4. Bulk tag removal + intersection semantics

Adding a tag was bulk; removing was active-item-only, so "tag these forty, then
untag them" was forty clicks. The info bar now shows the **intersection** of the
selection's tags — showing the union would offer to remove a tag from photos that
never had it — and × removes from every selected item.

## 5. Clear metadata as a checklist

Six one-shot menu items, each dismissing the popover, became one dialog with
checkboxes for stars / colour labels / pick-reject / tags / events, applied in a
single pass. Boxes are pre-ticked from what the selection actually carries. Also
on the right-click menu, where a bulk selection actually lives.

## 6. Missing files are reachable

`Filters ▸ Catalog` counted missing files and offered only "Re-check" — it named
a number and gave no way to find out which file, and the "?" tile only appears if
you open the folder the file used to be in. The count is now a button opening a
list of old paths; clicking a row navigates to that folder. A *Forget all* escape
hatch is there too, with an explicit warning that an unplugged drive makes every
file on it look missing.

## 7. Sidebar identity block removed

The second FoxCull icon + name + folder name under the title bar repeated what
the window title already says and spent a third of the sidebar header on it.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src-tauri/src/commands.rs` | **correctness** | `set_library_root` no longer clones another drive's catalog. `list_events` prunes first. |
| `src-tauri/src/catalog.rs` | logic | `prune_empty_events`; `remove_from_event` deletes the event when it empties; `list_events` skips memberless rows. |
| `src/lib/components/EventRail.svelte` | UX (new) | The banner + `RAIL_W` + name→hue. |
| `src/lib/components/VirtualGrid.svelte` · `SectionedGrid.svelte` | UX / layout | `eventRuns` prop, reserved gutter, rail bands. |
| `src/lib/settings.svelte.ts` | logic | `eventRail`. |
| `src/routes/+page.svelte` | UX / logic | `eventRuns`, rail toggle, bulk untag + intersection, Clear dialog, missing-files list, header cleanup. |
| `docs/EVENTS-MOVES-AND-RELINK.md` | docs | Rewritten Events section, event lifecycle, finding missing files, the clone defect. |

## Risks / compatibility

- **The grid layout math changed** — the most fragile code in this repo. The only
  change is subtracting the gutter from the width used for column fitting and
  offsetting cell `left`; recycling, scroll handling, overscan and the loader
  gate are untouched. Verified against a throwaway harness (below).
- Existing D:/E: catalogs still contain clones from the old behaviour; the fix
  prevents new ones but does not retro-clean. A hard reset clears them.
- `remove_from_event` deleting the event is a behaviour change: an event kept
  deliberately empty as a placeholder is no longer possible. That matches the
  owner's explicit "just like tags, auto delete".

## Verification actually run

- `cargo check`: passed. `npm run check`: 0 errors, 0 warnings. `npm run build`:
  passed.
- **Rail geometry proved on a throwaway `/rail-check` route** (deleted after),
  reading back computed positions from the DOM. With viewport 890 px, `RAIL_W`
  30, `cellMin` 120: 6 columns, cells offset to `left: 30`, and
  - run `[4..22]` → `top 0px, height 578px` = rows 0–3 ✓
  - run `[31..34]` → `top 730px, height 140px` = row 5 ✓
- **A pre-existing quirk was isolated, not introduced:** cells overflow the
  viewport by ~10 px because `vpWidth` is measured before the scrollbar appears
  and the ResizeObserver does not re-fire for a scrollbar-only change. Re-tested
  with `eventRuns: []` (gutter 0) and the identical 10 px overflow appeared, so
  it predates this work. Left alone deliberately; noted here so the next person
  does not attribute it to the rail.
- **Catalog clone bug** diagnosed by reading all four drive catalogs directly,
  not inferred.
- Device QA pending for the rail's appearance and the new dialogs.
