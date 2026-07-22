# 2026-07-22 — Grid decoder never opened; Glimpse becomes a realtime multiple; docs for v1.2.0

## Intent

Pre-stable batch from owner feedback: (1) grid live-scrub did nothing at all,
(2) Glimpse should be a plain speed multiplier, not a fixed-length sweep,
(3) release bodies showed a stale version heading, (4) establish an append-only
project log and refresh the handover before promoting to stable.

## RCA — grid skimming never worked

Two independent defects, both mine, both introduced with the grid-decoder work.

**1. `tilePending` was `$state`.** The opening effect reads it in its guard
(`if (tileEngine || tilePending) return`) and writes it in its body
(`tilePending = true`). That is self-invalidation: the write re-ran the effect,
and the re-run fired the previous run's cleanup — `return () => clearTimeout(timer)`
— which cancelled the `setTimeout` that was about to call `ScrubEngine.open()`.
The timer never fired. **The grid tile decoder never opened, in any build that
shipped it.** Loupe's equivalent flag (`enginePending`) is a plain `let` for
exactly this reason, with a comment saying so; the Thumb copy drifted.

**2. The decode path was gated behind `settings.s.liveScrub`.** That setting is
the opt-in for building *sprite sheets* — minutes of ffmpeg and disk per folder.
Decoding needs none of it. Gating on a default-off toggle whose UI text
describes pre-building made the feature invisible unless the user enabled a
setting for the thing it replaced.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src/lib/components/Thumb.svelte` | logic | `tilePending` → plain `let` (kills the self-cancelling effect). Decode path gated on `liveDecodeScrub` only. NEW `canSkim` derived (`liveDecodeScrub \|\| liveScrub`) drives `updateScrub` and the scrub rail, so the overlay follows whichever path can actually serve frames. |
| `src/lib/components/Loupe.svelte` | UX | Glimpse is now a constant multiple of realtime: `perTick = speed × tick`, no min-sweep cap, no forward keyframe snap. Dropped `GLIMPSE_MIN_SWEEP_S`. |
| `src/lib/settings.svelte.ts` | logic | `glimpseSpeed` default 40 → **5**; NEW exported `GLIMPSE_MIN`/`GLIMPSE_MAX` (2/10); migration clamps out-of-range stored values (an old `40` would now mean 40× realtime). |
| `src/routes/+page.svelte` | UX | Glimpse slider 2–10 step 1 with a rewritten tooltip. "Live Scrub (grid tiles)" renamed **"Sprite fallback (pre-built)"** with honest help text. |
| `RELEASE_NOTES.md` | process | Version heading removed permanently, with a comment explaining why. Rewritten as cumulative user-facing notes for v1.2.0. |
| `docs/PROJECT-LOG.md` | process | **NEW.** Append-only plain-language narrative — asks, decisions, bugs, reasoning. |
| `CLAUDE_CODE_HANDOVER.md` | process | New top section for v1.2.0: current state, the traps this codebase has fallen into, open items. |
| `docs/ROADMAP.md` | process | New "Open after v1.2.0" section: awaiting-owner items, queued work, and the phone-casting product idea. |
| `CLAUDE.md` | process | Doc map gains `PROJECT-LOG.md`; `RELEASE_NOTES.md` row carries the no-version-heading rule. |

## Behavior changes

- **Grid skimming works**: select a clip, hover its tile, frames decode live. No
  setting, no pre-build.
- Glimpse pace is identical on every clip. 5× → 20 s takes 4 s, 10 min takes
  2 min. At low multiples a tick can land inside the keyframe already on screen
  and repeat it: the clock advances at exactly N×, the picture refreshes as
  often as the clip's keyframes allow. That is the accepted trade.
- Anyone with a stored `glimpseSpeed` from an earlier build is reset to 5×.
- Release bodies no longer carry a hand-maintained version.

## Risks / compat

- Removing the min-sweep floor means a 5-second clip at 10× is over in half a
  second. Intended — the owner explicitly chose predictable speed over
  guaranteed duration.
- `canSkim` is true by default now, so the scrub rail appears on hover of an
  armed video tile where previously it required a setting. That is the fix, but
  it is a visible default change.
- The sprite fallback is unchanged and still gated behind its (renamed) setting.

## Verification actually run

- `npm run check` 0 errors / 0 warnings; `npm run build` clean; `cargo check`
  clean (no Rust touched this round beyond earlier commits).
- **Not verified on device.** The grid-decoder fix in particular has never been
  seen working — it is a reasoned fix to a mechanism proven broken by reading,
  not an observed repair. Flagged to the owner as the one unverified item in
  v1.2.0.
