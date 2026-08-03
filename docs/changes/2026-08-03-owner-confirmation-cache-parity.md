# Owner confirmation and cache-parity handover

## Intent

Record the owner's limited field confirmation that nightly.7 appears to have
largely resolved the scroll freeze, and hand the next agent a correctly scoped
question about photo thumbnail caching versus Explorer/Finder behavior.

## Modules touched

| File | Level | Change |
|---|---|---|
| `CLAUDE_CODE_HANDOVER.md` | process / architecture | Added owner confirmation, recurrence guardrails, cache-parity framing, re-ranked P2, and flagged stale Prepare copy. |
| `docs/design/precache-policy.md` | architecture / documentation | Clarified that persistent thumbnails are standard while automatic 600-image warming is the measure-first question; corrected stale sprite/Prepare policy left from intermediate migrations. |

## Behavior changes

None. This is documentation-only; no runtime or settings code changed.

## Risks / compatibility

- “Largely resolved” records limited owner testing and is deliberately not an
  all-folders/all-codecs guarantee.
- The automatic folder-open warm remains enabled until an A/B measurement
  demonstrates whether removing or narrowing it improves the experience.
- The stale Prepare tooltip/activity wording is documented for follow-up but is
  not changed in this documentation-only handoff.

## Verification actually run

- Diff reviewed against `+page.svelte`, `Thumb.svelte`, `Loupe.svelte`,
  `settings.svelte.ts`, `thumbnail-loader.ts`, and Rust `warm_thumbnails`.
- Microsoft Learn documentation confirms Windows uses a shared, multi-tier
  thumbnail cache with cache-first and lower-priority extraction paths.
- Apple Developer documentation confirms Quick Look provides asynchronous icon,
  cached low-quality and full thumbnail representations.
- `git diff --check`: passed.
