# Merge window, redesigned to the owner's spec

## Intent

The owner reviewed the first merge dialog (2026-09-28) and rejected its core
behaviour: it silently left photos and non-matching clips out. Their rules, in
their words:

- The window must not close on a stray click ("not clicking anywhere should
  not dismiss that").
- Show everything selected, in chronological order, with attribute columns.
  Work out the dominant format, highlight each non-matching attribute with the
  reason, and never drop anything automatically: "say there was a night shot
  sequence… in 4K 30 fps in low light mode… if you silently drop it, then it
  would be a problem".
- Remove items with a − button in the row, right-click → Remove, or select and
  press Delete. The merge button appears only once every issue is resolved.
- Let them change the sequence. Chronological by default.
- List on the left. On the right, a preview on top (photo, or video with
  scrubbing and Play, like a large grid tile) and settings + export below.
- Custom save location and name. Keep the UI "modern, 2026 mature app level".

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/components/MergeDialog.svelte` | UX / logic | Rewritten. Two panes: the sequence table (index + drag grip, name with middle truncation, recorded, length, frame, fps, video, audio, size, −) and, on the right, a preview (photo: Focus-size JPEG; video: armed `Thumb` for hover-scrub, ▶ Play swaps in a native `<video controls>`, fallback "Open in player") over the summary, file name, destinations and a pinned Cancel / Merge bar. The dominant signature is recomputed from the current list (most total running time). Per-cell mismatch highlight with the reason in the tooltip; photo and non-video rows show a calm "Photo: only videos can be merged". Row selection (click / Shift / ⌘), Delete, right-click menu (Remove, Move to start/end, Show in folder), drag to reorder (moves the whole selection), ⌥↑/⌥↓, "Sort by time shot", "Select these" for the flagged rows. Merge is disabled with a stated reason until nothing is flagged, 2+ clips remain, and the destination fits. The backdrop is inert and Escape never closes it. The list drops columns in narrow containers and the right pane slims below 1150px. |
| `src-tauri/src/commands.rs` | logic | `merge_probe` returns every selected item with `kind` (`video` / `photo` / `other`); photos carry their EXIF capture date so they sort into place. `merge_videos` still refuses non-videos (defence in depth). |
| `src/routes/+page.svelte` | logic | The merge window receives the whole selection. The page's key handler returns early while it's open, so its Delete (remove rows) can never act on the grid. |
| `src/lib/types.ts` | logic | `MergeClip.kind`. |
| `src/lib/dev/mock-ipc.ts` | process | The fake probe returns photos and sorts by time. |

## Behavior changes

- Right-click → Merge shows every selected item. Nothing is left out without
  the owner seeing it and removing it.
- The window closes only through Cancel / ✕ (and not during a merge).

## Risks / compat

- Photo EXIF times are local while video `creation_time` is UTC, so a photo can
  sort a few hours off among videos. Photos must be removed before merging
  anyway.
- The native `<video>` preview depends on the webview's codecs. HEVC 10-bit
  plays in WKWebView on Apple Silicon. On Windows it needs the HEVC extension,
  otherwise the preview offers "Open in player" (hover-scrub still works through
  the WebCodecs engine where supported).

## Verification actually run (macOS, browser pane + fake backend)

- 1280×788: 204 items listed chronologically (171 flagged: photos, 29.97 fps,
  vertical). Only the mismatching cells highlighted, with the reasons checked
  in their titles.
- Backdrop click and Escape don't close. Click + Shift-click selected 3 and
  Delete removed them, with the grid selection behind unchanged. Right-click →
  Remove works. ⌥↓ moved a row two places and "Sort by time shot" restored it.
  A synthetic drag moved a row below another.
- "Select these" + Delete: 33 clips, "All compatible". The blocker switched to
  the space problem (the fake SD card is full); choosing the external drive
  enabled "Merge 33 clips", which ran to the done screen.
- `__audit` on the window: clean at 1280×788 and 1440×900. At 1049×645 (TV)
  the only findings are rows scrolling under the pinned action bar (intended).
  `__sweep` over the app: only the expected Details header.
- `npm run check` 0/0, `npm run build` OK (no dev tooling in `build/`),
  `cargo check` clean, `cargo test --lib` 37/37.
- Not run: a real full-size merge from the UI; Windows.
