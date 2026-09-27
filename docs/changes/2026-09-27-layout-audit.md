# Layout audit: overlaps across the app, and interface sizes that never scaled

## Intent

The owner, now working on a Mac, reported layout overlaps "across multiple
touchpoints" and asked for a comprehensive audit and fix. The August refit had
been QA'd on Windows only. The Mac's window is shorter (1280×788 content) and
lacks Segoe UI, so the fallback font is wider. Both exposed layouts that only
fit by luck.

Method, so it can be repeated: run the UI in a browser against a dev-only fake
backend, and measure every surface with a DOM probe (text/control
intersections, clipped text, off-screen content, squashed flex rows) at every
window size and interface scale. Details and how to re-run are in
`docs/UX-AUDIT-2026-08.md` → "September 2026 re-audit".

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/keep-in-view.ts` | UX / architecture | New Svelte action: a floating panel measures itself before first paint and on resize, slides back inside the window and caps its height (then scrolls). |
| `src/app.css` | UX | `[data-keep-in-view] > * { flex-shrink: 0 }` (the Settings squash); themed `::-webkit-scrollbar-corner`; removed the dead UI-scale transform and `--ui-scale`. |
| `src/routes/+layout.svelte` | UX / logic | Interface size is now webview page zoom (`getCurrentWebview().setZoom`: 0.9 / 1 / 1.22). |
| `src-tauri/capabilities/default.json` | process | `core:webview:allow-set-webview-zoom`. |
| `src/routes/+page.svelte` | UX | `use:keepInView` on Arrange, Filters, Prepare, Clear, Cast and Settings; grid and filmstrip pass `badge={false}` to `Thumb`; scrolling menu lists don't shrink rows; collapsed filmstrip toggle fits its rail; Settings `max-height` no longer divides by the scale; removed the TV forced two-row toolbar (its premise, that media queries can't see the scaled width, no longer holds). |
| `src/lib/components/EditStudio.svelte` | UX / logic | Side panels share width proportionally (min 230px) to keep the work pane ≥460px, with drags starting from the displayed width; below 932px of studio width one side panel shows at a time; top bar row is `max-content` and the timeline yields height first (min 120px); source list, Look panel and export dialog rows don't shrink; tag chips get their full two rows; timeline Zoom slider can shrink; Look sliders reserve room for the thumb. |
| `src/lib/components/Thumb.svelte` | UX | `badge` prop (default true) for the RAW corner badge. |
| `src/lib/components/ExcludePanel.svelte` | UX | Sizes no longer divide by `--ui-scale`. |
| `src/hooks.client.ts`, `src/lib/dev/mock-ipc.ts`, `src/lib/dev/layout-audit.ts` | process | Dev-only browser harness: fake backend + `__audit` / `__sweep` / `__zoom` / `__setSetting`. Installed only when `import.meta.env.DEV` and no Tauri bridge; confirmed absent from `npm run build` output. |
| `docs/UX-AUDIT-2026-08.md` | process | Responsive contract updated; September re-audit section with the method, matrix and cause table. |

## Behavior changes

- **TV / large and Compact now actually resize the interface.** They were a
  transform on `body > div:first-child`, SvelteKit's `display: contents`
  wrapper, which has no box to transform, so since 2026-08-01 they did nothing
  except force a two-row toolbar. Anyone who had TV selected will see the app
  22% larger after updating.
- Settings: Theme no longer covers Interface size. Arrange/Filters menus stay
  on-screen. The Edit studio's three columns no longer overlap at laptop widths;
  in a narrow studio, opening Look tucks Media away and vice versa.
- RAW tiles show one RAW tag (was two stacked; in the filmstrip the stars were
  drawn on top of one).
- No white square where the Details table's scrollbars meet.

## Risks / compat

- Webview zoom on Windows (WebView2 `SetZoomFactor`) was not exercised: this
  session had only a Mac. The August note about WebView2 not painting
  virtualized images inside a *transformed* scroller does not apply to page
  zoom, but the grid under TV/Compact on Windows deserves a look.
- The Edit studio auto-collapse changes `sourceCollapsed` / `inspectorCollapsed`
  state; widening the window again does not reopen the tucked-away panel.
- `keepInView` writes inline `translate` / `max-height` on the panel; a menu
  that later sets those inline itself would conflict.

## Verification actually run (macOS)

- `__sweep()` at 1024×700, 1049×645, 1280×788, 1280×820, 1366×768, 1422×875,
  1440×900, 1574×885, 1920×1080, plus filmstrip-left and month grouping: no
  findings except the two expected ones (Details header scrolls sideways; the
  Edit "Look" tab floats inside the empty preview).
- Real debug build, isolated `HOME`, `uiScale` set per run, effective viewport
  logged from the webview: Standard 1280×788, TV 1049×645, Compact 1422×875. The
  logging line was temporary and is removed.
- `npm run check` 0/0, `npm run build` OK (grep: no dev tooling in `build/`),
  `cargo build` OK (capability validated), `cargo test --lib` 32/32.
- Not run: Windows, and a Chromecast or controller connected.
