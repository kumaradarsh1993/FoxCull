# Single selector focus handoff

## Intent

Make mouse-to-keyboard navigation behave like Explorer: after clicking a media
item, unmodified arrow keys move one visible selector without leaving a second
highlight on the starting item.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src/routes/+page.svelte` | interaction / accessibility | Release stale DOM focus from a grid, filmstrip or Details media button before the global arrow handler moves the active item. |
| `RELEASE_NOTES.md` | user-facing | Explain the apparent double selection and the fixed behavior. |
| `CLAUDE_CODE_HANDOVER.md` | process / RCA | Record reproduction, cause, scope and native evidence. |
| `docs/PROJECT-LOG.md` | process | Record the selector/focus distinction for future UI work. |

## Behavior changes

- Mouse click followed by an unmodified arrow key shows one selector on the new
  active item.
- Right/left remain one item; down/up remain one grid row in Grid.
- Shift range selection and Ctrl/Cmd toggle selection are unchanged.
- Keyboard focus on non-media controls is unchanged.

## Risks / compatibility

- Media cells use FoxCull's visible active border as their navigation indicator;
  the old DOM focus outline was redundant and stale after global navigation.
- No thumbnail, loader, marks, file-operation, database or Rust behavior changed.

## Verification actually run

- Reproduced on installed nightly.7 in the real 6,825-item library: click a grid
  tile, press Right; old focus outline remained while active border moved.
- Patched native Tauri dev app: click a grid tile, press Right, then Down; exactly
  one border followed the active item after both moves.
- `npm run check`: 0 errors, 0 warnings.
- `npm run build`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `git diff --check`: passed.
