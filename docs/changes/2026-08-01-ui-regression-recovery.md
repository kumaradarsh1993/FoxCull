# UI regression recovery - 2026-08-01

## Scope

Follow-up to the visual-system nightly based on owner testing in the Windows
desktop build. This change intentionally preserves the Library / Focus / Edit
workflow and concentrates on recovery affordances, layering, branding and
scroll responsiveness.

## Changes

- Generated a new fox/aperture master icon and regenerated the Tauri Windows,
  macOS, Linux, Android and iOS icon set; the web favicon uses the same source.
- Simplified the native window title to `FoxCull` while leaving package and
  release version metadata intact.
- Replaced the refresh CSS construction with a proportionate SVG.
- Raised and isolated the command-bar menu layer so Arrange and Filters render
  above virtualized grid cells.
- Added persistent Media and Back to edit recovery actions in Edit.
- Moved laptop layouts to the two-row command-bar contract at 1400 px and added
  a container-responsive preset row in Edit.
- Normalized and eased horizontal filmstrip wheel input, including the reported
  Logitech precision-wheel direction.
- Limited concurrent heavy video thumbnail work, reduced offscreen lookahead,
  and added cell containment without removing asynchronous loading or grid
  virtualization.

## Verification

- `npm run check`: 0 errors, 0 warnings.
- `cargo check`: passed.
- `npm run build`: passed.
- `git diff --check`: passed, apart from expected Windows line-ending notices.
- Native spot checks before extended automation was stopped: new title/icon,
  Arrange/Filters stacking, responsive toolbar and collapsed-Media recovery.
- Owner QA requested after installation: Logitech horizontal wheel feel, Edit
  Preview return action, and long video-folder grid scrolling.
