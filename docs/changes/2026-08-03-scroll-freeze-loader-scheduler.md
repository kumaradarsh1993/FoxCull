# Scroll freeze: frame-paced loader scheduler

## Intent

Close the v1.2.0 fast-scroll regression decisively after the owner symptom split
proved the JavaScript main thread — not WebView2's compositor/GPU — was blocked.
Preserve grid and Focus live-decode scrubbing.

## Modules touched

| File | Level | Change |
|---|---|---|
| `src/lib/thumbnail-loader.ts` | architecture / logic | Replaced the scan/splice queue and recursive pump with a keyed live queue, compacted LIFO priority log, O(1) cancellation, settled dropped promises, and a 12-dispatch-per-frame pump. |
| `src/lib/thumbnail-loader.ts` | UX / performance | Changed on-demand thumbnail activity from a changing percentage to an indeterminate start/end signal; removed per-tile reactive writes. |
| `src/routes/+page.svelte` | diagnostics | Added live/storage queue and total/heavy inflight counts to the existing memory heartbeat. |
| `src/lib/components/VirtualGrid.svelte` | documentation | Corrected the stale claim that recycling fixed the freeze; retained recycling for lower churn/flicker. |
| `src/lib/components/SectionedGrid.svelte` | documentation | Corrected the same RCA comment. |
| `src/lib/components/VirtualStrip.svelte` | documentation | Corrected the same RCA comment. |
| `docs/design/FREEZE-HANDOVER-2026-08-03.md` | process / evidence | Recorded the final RCA, structural bounds, and native stress results. |
| `docs/design/precache-policy.md` | architecture | Synchronized queue order, cancellation, dispatch pace, fast-scroll and activity policy. |
| `docs/design/AUDIT-2026-08-02.md` | process | Retracted the claim that asset-protocol cost corroborated the freeze RCA; kept it as a separate measured optimization. |
| `CLAUDE_CODE_HANDOVER.md` | process | Added the resolved state at the top. |
| `RELEASE_NOTES.md` | user-facing | Replaced the "still open" note with the shipped behavior and native evidence. |

## Behavior changes

- A fast fling still defers pass-through thumbnail work.
- Settling releases at most 12 cached assignments per animation frame rather
  than recursively draining up to 240 in one main-thread turn.
- Queued cancellation is O(1), always resolves its waiter, and never leaves a
  stale live request in the priority log.
- Backend concurrency remains 6 total and 2 heavy.
- "Loading thumbnails" is a spinner because a moving viewport has no honest
  fixed denominator; it appears only after 700 ms and clears on full drain.
- Grid and Focus live-decode scrubbing are unchanged.

## Risks / compatibility

- Cached tiles can take several paints to fill after a hard fling. This is
  intentional backpressure; the current viewport is LIFO-prioritized.
- The priority log may temporarily contain tombstones, but compacts at 480
  stored entries and the authoritative live set is capped at 240.
- No Rust, cache format, database, settings, file-operation or export behavior
  changed.

## Verification actually run

- `npm run check`: 0 errors, 0 warnings.
- Native Tauri dev app opened the real `F:\Note 10+ DCIM backup Feb 2023_bak`
  library (6,825 items).
- Repeated large wheel jumps: populated normally; no paint-resume gap.
- Temporary dev-only stress probe (removed after testing): 800 three-row jumps
  at a 4 ms cadence.
  - no `PAINT-RESUME` log;
  - peak post-traversal loader state: 43 queued + 6 in flight;
  - drained to 0 queued / 0 in flight;
  - JS heap: 92 MB peak → 47 MB settled;
  - FoxCull WebView2 tree: ~332 MB private, ~3,270 handles settled;
  - activity indicator cleared and all visible tiles populated;
  - Right-arrow moved selection from item 312 to 313 immediately afterward.
- `npm run build`: passed (static production bundle written successfully).
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `git diff --check`: passed.
