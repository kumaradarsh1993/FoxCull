# 2026-07-19 — video preview rework · controller culling · UX pass · Looks overhaul (retro entry)

**Author:** remote cloud agent (Fable) · **Tags:** v1.1.0-nightly.3, v1.1.0-nightly.4
**Retro note:** written after the fact (2026-07-20) when the change-ledger
convention was adopted, condensed from commits `213902b`, `3a8bbcb`, `0fc997a`,
`a86ce31`. For full detail read those commit bodies and
`CLAUDE_CODE_HANDOVER.md`'s 2026-07-19 section.

## Modules touched (condensed)

| Area | Level | What changed |
|---|---|---|
| `src-tauri/src/video.rs` | **architecture** | Sprite extraction rebuilt: per-frame keyframe seeks (`-ss` + `-skip_frame nokey`, one ffmpeg process per frame, 2 parallel, serialized process-wide) replace the single full-decode `fps=` pass. Cancellation tokens (`cancel_sprite`/`cancel_all_sprites`), per-frame progress, proxy builds got `-hwaccel auto`. |
| `src-tauri/src/commands.rs` | **logic** | Prepare pre-builds video posters + hover scrub strips; sprite token registry; activity ETAs computed from observed rate. |
| `src/lib/components/Loupe.svelte` | **logic (⚠ regression)** | Focus player: poster paints instantly; dense filmstrip now builds **unconditionally on Focus open** — the pre-rework `if (liveScrub)` gate was removed. This is the subject of `.rca-live-scrub.md` (2026-07-20); fix pending owner approval. Drag-scrub paints sprite frames full-canvas; optimistic throttled seeks. |
| `src/lib/gamepad.svelte.ts`, `ControllerPanel.svelte` | **new feature** | Gamepad-API culling: DualSense defaults, press-to-bind remapper, button-guide overlay. |
| `src/routes/+page.svelte`, popovers | **UX** | Light-dismiss on all toolbar popovers, `?` shortcut guide, Details-list context menu, empty-state Clear-filters. |
| `EditStudio.svelte` + `commands.rs` filters | **logic** | Look panel: 12 grouped presets, warmth/split-tone strengthened with preview↔export parity kept. |
| `.github/workflows/release.yml` | **process** | Workflow-dispatch can mint the tag; RELEASE_NOTES.md prepended to release bodies. |

## Known issues introduced (status as of 2026-07-20)

- Focus filmstrip ignores the Live Scrub toggle → heavy unwanted builds on
  HDD libraries (RCA done, fix designed, awaiting approval).
- Always-on builds hold big files open → contributed to the delete freeze
  (delete side fixed in `2026-07-20-heic-delete-cast.md`).
