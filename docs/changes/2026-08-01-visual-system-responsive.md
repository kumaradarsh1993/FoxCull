# August 2026 visual system and responsive refit

## Intent

Give every FoxCull surface one professional visual language while preserving the
proven culling, playback, editing, casting and file-management workflows. Make
the same UI work at laptop density, a narrow window and TV viewing distance.

## Modules touched

| Module | Level | Change |
|---|---|---|
| `src/app.css` | architecture / UX | Rebuilt theme tokens, typography, controls, scrollbars, focus, motion and global scale contract. |
| `src/lib/settings.svelte.ts` | logic / UX | Added persisted Compact / Standard / TV-large interface size. |
| `src/routes/+layout.svelte` | architecture | Applies theme and scale attributes at the document root. |
| `src/routes/+page.svelte` | UX | Refit sidebar, toolbar, settings, first-run, grid states, culling footer, popovers, dialogs and responsive behavior. |
| `ActivityBar.svelte` | UX | Stronger dock and progress treatment. |
| `ContextMenu.svelte` | UX | Larger, layered floating menu with clearer interaction targets. |
| `TreeNode.svelte` | UX | Contained active row and accent rail. |
| `DetailsView.svelte` | UX | Consistent table/header/menu/thumbnail hierarchy. |
| `SectionedGrid.svelte` | UX | Refined group headers and count pills. |
| `Loupe.svelte` | UX | True-neutral stage plus glass transport, clip tools and info overlay. |
| `EditStudio.svelte` | UX | Unified Source, Preview, Timeline, Look, export menu and dialog surfaces. |
| `TrashPanel.svelte` | UX | Refined modal workspace and media cards. |
| `ControllerPanel.svelte` | UX | Refined modal, setup cards and tester surface. |
| `RELEASE_NOTES.md` | process | User-facing notes for the visual nightly. |
| `docs/UX-AUDIT-2026-08.md` | process / design | Workflow, permutation, surface and responsive audit. |
| `BACKLOG.md` | process | Closed the responsive-toolbar item. |
| `CLAUDE_CODE_HANDOVER.md` / `docs/PROJECT-LOG.md` | process | Recorded current state and design reasoning. |

## Behavior changes

- Settings persists `uiScale`: compact (90%), comfortable (100%) or distance
  (122%). Existing settings migrate to Standard.
- The toolbar compacts at laptop widths and uses two rows at narrow widths or in
  TV-large mode; controls are no longer clipped.
- The first-run screen now opens the folder picker directly.
- Theme labels are Studio, Midnight, Amber and Daylight; persisted theme values
  remain compatible.

## Risks / compatibility

- CSS `transform: scale()` is applied to the app shell with an inverse layout
  size. Browser QA confirmed exact viewport fit in Standard and TV-large modes.
- Popovers use `backdrop-filter`; unsupported platforms fall back to the same
  opaque-enough surface colour.
- No backend, catalog schema, media pipeline or filesystem behavior changed.

## Verification actually run

- Visual: 1440×900, 1366×768, 1024×768, TV-large at 1440×900.
- Native Tauri dev: 1280×850 with real drives, restored folder, persisted Amber
  theme and Settings popover.
- `npm run check` — 0 errors, 0 warnings.
- `npm run build` — passed.
- `cargo check` — passed.
- `git diff --check` — passed; only expected CRLF notices.
