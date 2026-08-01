# Restore the original FoxCull identity - 2026-08-01

## Correction

The orange fox above a film strip, with green and red dots, is the intentional
FoxCull logo. The separate square/screen icon visible inside the owner's
screenshot was the obsolete artifact. Nightly.2 incorrectly replaced both.

## Change

- Restored the exact pre-design-pass `src-tauri/icons/icon.png` from commit
  `65b1052`.
- Preserved it as `src-tauri/icons/app-icon-source.png`.
- Regenerated all Tauri Windows, macOS, Linux, iOS and Android icons from that
  master.
- Regenerated `static/favicon.png` from the same identity so the square/screen
  artifact is no longer used as FoxCull branding.

## Verification

- Visually inspected the recovered source at its original resolution.
- `npm run check`: 0 errors, 0 warnings.
- `cargo check`: passed.
- `git diff --check`: passed with expected Windows line-ending notices only.
