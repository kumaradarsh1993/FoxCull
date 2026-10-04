# Design foundations: themes, type, shape, icons, Mac title bar

Nightly `v1.5.2-nightly.10`, the first of three redesign nightlies the owner
asked for (one each, so each can be rolled back on its own; reviewed after
all three). Audit: `docs/UX-AUDIT-2026-10.md`.

## Intent

Make every later visual change cheap and consistent: one set of tokens for
colour, type, shape and motion, themes that pass contrast, icons that look
the same on every OS, and a Mac window that looks like a 2026 Mac app.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/app.css` | UX / architecture | Six themes (graphite, studio, midnight, amber, daylight, paper) on `data-theme`, each AA for every text role; accents on `data-tone` × `data-accent`; `data-surround` for `--viewport-bg`; tokens `--fs-*` (11/12/13/15/20), `--fw-*` (400/560/650), `--radius-*` (6/8/12/16), control heights, motion; platform font stack; Mac padding for the traffic lights. |
| `src/lib/settings.svelte.ts` | logic | `Theme` (new ids + "system"), `Accent`, `Surround`; migration of the old ids; appearance syncs across windows (`onKeyChange`). |
| `src/routes/+layout.svelte` | architecture | Resolves "system" live from `prefers-color-scheme`; stamps `data-theme`, `data-tone`, `data-accent`, `data-surround`, `data-platform`. |
| `src/lib/components/SettingsSheet.svelte` | UX | Six theme tiles + Match system, Accent colour row, Behind the pictures row. |
| every `.svelte` style block | UX | 372 font sizes, 143 weights, 173 radii moved onto the tokens (a scripted sweep: ≤11 px → 11, ≤12 → 12, ≤13.75 → 13, ≤16 → 15, ≤22 → 20; 500–580 → 560, 600–900 → 650; radii 3–6 → 6, 7–9 → 8, 10–13 → 12, 14–20 → 16). `UpdatePanel.svelte` left alone: it's byte-identical across the four apps. |
| `src/lib/icons.ts` (new), `ContextMenu.svelte` | UX | One SVG line-icon set; menus swap their glyph for the drawing (call sites unchanged). |
| `src/routes/+page.svelte` | UX | Shortcut guide shows ⌘ ⇧ ⌃ on the Mac; emoji in guides/toasts replaced by icons. |
| `src-tauri/tauri.conf.json`, `tool_windows.rs`, `capabilities/default.json` | architecture | macOS: `titleBarStyle: Overlay`, hidden title, traffic lights at (18, 21) in every window; top bars are drag regions (`data-tauri-drag-region`, `allow-start-dragging`, `allow-toggle-maximize`). Windows keeps its native title bar. |
| `src/lib/dev/layout-audit.ts` | process | `__sweep` audits every Settings page instead of the old popover and modals. |

## Behavior changes

- The owner's saved theme becomes its new counterpart (Studio → Graphite).
- Text is never smaller than 11 px; most UI text is 12–13 px.
- Colour labels draw as dots.
- On the Mac the title bar is gone: the traffic lights sit in each window's
  top bar, which drags the window (double-click zooms).

## Risks / compat

- **The Mac title bar change is only visible in the real app.** If the
  traffic lights overlap a control, the padding is in `app.css`
  (`:root[data-platform="mac"] …`).
- Materials (vibrancy/Mica) were not done: they need a transparent window,
  which is a bigger change. Noted in the audit.
- The font-size sweep made the smallest text 1–2 px larger; `__sweep` found
  nothing new overlapping at 1440×900 and 1024×700 (the two findings it
  reports were there before, checked by stashing these changes).

## Verification actually run

- `npm run check` 0/0, `cargo check`.
- Harness: Graphite applied and migrated; `__sweep` at 1440×900 and
  1024×700: only the two pre-existing findings (Details column off screen,
  Focus 14 px squeeze); every Settings page, menu, dialog and the Edit
  window clean.
- Palettes: every text role ≥ 4.5:1 on panel and raised card (script).
