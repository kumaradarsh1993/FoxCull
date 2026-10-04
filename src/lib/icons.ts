// One line-icon set for menus (2026-10-04). Menu entries still name their
// icon with the glyph they always used ("⧉", "⤴"…), so call sites read the
// same; ContextMenu swaps each glyph for its drawing here. 24×24, stroked
// with currentColor at 1.75 px, rounded caps: they sit with the SVG icons in
// the toolbars and look the same on every OS (emoji didn't).
const FOLDER = '<path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v7.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/>';
const X = '<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>';
const PLUS = '<path d="M12 5v14M5 12h14"/>';

export const GLYPH_ICONS: Record<string, string> = {
  "⧉": '<rect x="8.5" y="8.5" width="11.5" height="11.5" rx="2"/><path d="M15.5 8.5V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7.5a2 2 0 0 0 2 2h2.5"/>',
  "⌫": '<path d="M20 6H9.2a1 1 0 0 0-.76.35L4 12l4.44 5.65a1 1 0 0 0 .76.35H20a1 1 0 0 0 1-1V7a1 1 0 0 0-1-1z"/><path d="m12 9.5 5 5M17 9.5l-5 5"/>',
  "⤴": FOLDER + '<path d="M12 16v-5M9.5 13.5 12 11l2.5 2.5"/>',
  "↗": '<path d="M7 17 17 7M9 7h8v8"/>',
  "▣": '<rect x="4" y="4" width="16" height="16" rx="3"/><path d="m8.5 12.2 2.4 2.4 4.6-5"/>',
  "▦": '<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>',
  "⤓": '<path d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19.5h14"/>',
  "⤒": '<path d="M5 4.5h14M12 20V9M7.5 13.5 12 9l4.5 4.5"/>',
  "⇩": '<path d="M12 4v11M7.5 10.5 12 15l4.5-4.5"/><path d="M4.5 15.5v2a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-2"/>',
  "⇥": '<path d="M4 12h11.5M11.5 7.5 16 12l-4.5 4.5M20 5v14"/>',
  "▶": '<path d="M8 5.8v12.4a.8.8 0 0 0 1.2.7l9.6-6.2a.8.8 0 0 0 0-1.4L9.2 5.1A.8.8 0 0 0 8 5.8z"/>',
  "✕": X,
  "×": X,
  "✓": '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  "🔎": '<circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/>',
  "＋": PLUS,
  "+": PLUS,
  "−": '<path d="M5 12h14"/>',
  "✂": '<circle cx="6.5" cy="6.5" r="2.5"/><circle cx="6.5" cy="17.5" r="2.5"/><path d="M8.6 8 20 18.5M8.6 16 20 5.5"/>',
  "↻": '<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3"/><path d="M19.5 4.5v4h-4"/>',
  "⟲": '<path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3"/><path d="M4.5 4.5v4h4"/>',
  "📁": FOLDER,
  "✦": '<path d="M12 3.5 13.8 10.2 20.5 12l-6.7 1.8L12 20.5l-1.8-6.7L3.5 12l6.7-1.8z"/>',
  "✎": '<path d="M15.5 4.5 19.5 8.5 9 19H5v-4z"/><path d="m13.5 6.5 4 4"/>',
  "⚡": '<path d="M13 3.5 5.5 13.5H12l-1 7 7.5-10H12z"/>',
  "♫": '<path d="M9 18V5.5l10-2V16"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/>',
  "★": '<path d="m12 4 2.4 5 5.4.7-4 3.7 1 5.4L12 16.2l-4.8 2.6 1-5.4-4-3.7 5.4-.7z"/>',
  "◎": '<circle cx="12" cy="12" r="7.5"/><circle cx="12" cy="12" r="2.5"/>',
  "▭": '<rect x="3.5" y="7" width="17" height="10" rx="2"/>',
  "⊞": '<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M12 8v8M8 12h8"/>',
  "⊘": '<circle cx="12" cy="12" r="8"/><path d="m6.5 17.5 11-11"/>',
  "↩": '<path d="M9 14 4.5 9.5 9 5"/><path d="M4.5 9.5H14a5.5 5.5 0 0 1 0 11h-3"/>',
  "→": '<path d="M5 12h14M13 6l6 6-6 6"/>',
  "←": '<path d="M19 12H5M11 6l-6 6 6 6"/>',
  "🗑": '<path d="M4 7h16M9 7V5h6v2M6.5 7l1 12.5h9l1-12.5"/>',
  "🎮": '<rect x="2.5" y="7" width="19" height="11" rx="5"/><path d="M7 11v3M5.5 12.5h3"/><circle cx="16" cy="11.5" r="1"/><circle cx="18" cy="13.5" r="1"/>',
  "⌨": '<rect x="2.5" y="6.5" width="19" height="11" rx="3"/><path d="M6.5 10h1M10 10h1M13.5 10h1M17 10h.5M8 14h8"/>',
};

/** The icon as an SVG string, or null when the glyph has no drawing. */
export function glyphSvg(glyph: string | undefined, size = 16): string | null {
  const d = glyph ? GLYPH_ICONS[glyph] : undefined;
  if (!d) return null;
  return `<svg viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;
}
