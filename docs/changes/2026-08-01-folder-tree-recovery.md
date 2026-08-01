# Folder-tree recovery - 2026-08-01

## Regression

Collapsing the folder explorer removed the visible route to expand it. The
restore button was still rendered, but nightly.2 had raised the opaque command
bar from the normal document layer to an isolated `z-index: 100`; the restore
button remained at `z-index: 80` underneath it.

## Fix

- Expose `treeCollapsed` on the app root.
- Raise the restore control above the command bar and give it an opaque elevated
  surface.
- Reserve 48 px in the command bar while collapsed so the recovery button does
  not overlap View controls.

## Collapse/recovery audit

- Folder explorer: visible top-left restore control.
- Bottom/side filmstrip: persistent bottom restore rail/button.
- Edit Media, Timeline and Look: toolbar toggles plus floating restore controls.
- Edit production Preview: persistent Back to edit control.
- Related-item stacks: representative tile retains its expand control.
- Fullscreen/lights-out: Escape and existing keyboard cycle remain deliberate.

## Verification

- `npm run check`: 0 errors, 0 warnings.
- `cargo check`: passed.
- `npm run build`: passed.
- `git diff --check`: passed with expected Windows line-ending notices only.
