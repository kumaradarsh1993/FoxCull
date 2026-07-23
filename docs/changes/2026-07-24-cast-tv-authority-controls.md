# 2026-07-24 — Chromecast TV-authority playback controls

## Intent

Apply the owner's Sony-TV feedback after nightly.3 proved that cast follow and
playback work: remove doubled local/TV audio, make pause work on the first
keypress with local autoplay off, expose controls in Grid, remove the TV
filename card, and make the active cast session unmistakable.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/cast.rs` | architecture / protocol | Added receiver playback state/time/duration tracking, direct toggle and relative-seek commands, projected receiver time, ordered pre-session transport queuing, and status refreshes. Removed LOAD title metadata. |
| `src-tauri/src/lib.rs` | IPC | Registered `cast_toggle` and `cast_seek_by`. |
| `src/lib/cast.ts` | API | Exposed typed receiver status plus direct toggle/relative-seek wrappers. |
| `src/lib/components/Loupe.svelte` | playback UX | Cast mode disables local autoplay, keeps the local element muted/paused, routes the play button directly to the TV, and displays receiver play state. |
| `src/routes/+page.svelte` | input / UX | Space and Shift+arrows directly control cast video in Grid and Focus; controller play/pause and L2/R2 share the path; added a glowing stateful CASTING badge and one-second status polling. |
| `RELEASE_NOTES.md`, `CLAUDE_CODE_HANDOVER.md`, `docs/PROJECT-LOG.md` | process | Recorded hardware-confirmed nightly.3 behavior, the new authority decision, verification, and nightly.4 test scope. |

## Behavior changes

- While casting a video, only the TV plays audio/video; the laptop remains a
  silent paused reference.
- Space toggles TV playback on its first press regardless of local Video
  Autoplay, from Grid or Focus.
- Shift+Left/Right and DualSense L2/R2 seek the TV by five seconds from its
  current receiver-reported position.
- A command pressed during the short LOAD-to-media-session window is queued for
  the new clip instead of dropped or sent to the prior clip.
- Cast LOAD no longer supplies filename metadata, avoiding the receiver title
  overlay.
- The toolbar displays a glowing CASTING pill with connection/playback state.

## Risks / compatibility

- Chromecast state names and status cadence come from the Default Media
  Receiver. Unknown states conservatively toggle toward PLAY.
- Only explicit timeline seeks are mirrored from the parked local player;
  play/pause never is. This is deliberate single-authority behavior.
- The previous direct play/pause/absolute-seek IPC commands remain registered
  for compatibility, though the library UI now prefers toggle/relative seek.
- Hardware verification is still required for this refinement on nightly.4.

## Verification actually run

- `npm run check` — pass, 0 errors and 0 warnings.
- `npm run build` — pass.
- `cargo check` — pass.
- `cargo fmt --check` — reports broad pre-existing formatting drift across
  untouched Rust modules; no bulk formatting was applied.
- GitHub Actions release/runtime gates — pending the nightly.4 tag.
- Sony-TV verification of the preceding nightly.3 follow/playback fixes — pass
  per owner report; the new control refinements remain pending.
