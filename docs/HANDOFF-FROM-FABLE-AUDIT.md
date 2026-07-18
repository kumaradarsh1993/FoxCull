# Handoff: Fable Independent Audit → main-machine agent

**Date:** 2026-07-18 · **Reviewer:** an independent Claude Code session (model
Fable, run remotely — not the main development machine) · **Branch:**
`claude/fox-cull-audit-kc8iwu` · **PR:** #1

This file is the short brief for whoever/whatever picks the repo up next. The
full analysis lives in [`AUDIT-2026-07.md`](AUDIT-2026-07.md); the prioritized
worklist is [`../BACKLOG.md`](../BACKLOG.md); the running project narrative is
[`../CLAUDE_CODE_HANDOVER.md`](../CLAUDE_CODE_HANDOVER.md) (new dated section at
the top).

## What this was

A one-off holistic audit — architecture, performance (tuned for the Alienware
15 R4 / GTX 1070 and the XPS 13), memory, code quality, security, and macOS
compatibility. Independent of the main line of development; all work isolated
on a branch off `main` at commit `b3ac7ff` (nothing on `main` was touched).

## Key findings still open (top of the backlog)

- **P0** — Preview-cache keys hash the *absolute* path, so the cache does not
  actually travel between machines sharing a drive despite that being the
  design intent. Move to library-relative-path keys + a stable hash + GC.
- **P0** — Filmstrip/scrub-strip generation fully software-decodes clips; add
  `-hwaccel auto` + keyframe-only sampling. Biggest single perf win.
- **P1** — Keep-HDR export is CPU-only libx265; try `hevc_nvenc` on the 1070.
- **P2 (Mac)** — `window.confirm()` on the JPEG-export path is unreliable in
  WKWebView; replace with an in-app dialog.
- Everything else, with reasoning and priority, is in `BACKLOG.md`.

## What was fixed (the 4 commits on this branch)

- CI stamps the release version into `Cargo.toml` too (window title showed the
  stale version on tagged builds).
- `warm_thumbnails` opt-in `heavy` flag — Prepare now actually pre-builds RAW
  previews and video posters (was silently a no-op on RAW/video folders).
- Edit-mode probe throttle (max 4 concurrent ffmpeg processes, was unbounded).
- `buildRelatedIndex` de-quadratified (rebuilds on every mark keystroke).
- `move_media_files` cross-volume fallback can't leave a duplicate on failure.
- All folder walkers skip macOS `._*` AppleDouble dotfiles.
- New `.github/workflows/check.yml`: svelte-check + build + cargo check/test on
  every push/PR (the Rust tests had never run anywhere before).
- `@types/node` added; svelte-check is 0 errors / 0 warnings.

Validated: `cargo check`, `cargo test` (6/6), `svelte-check` (clean),
`vite build` — all passing.

## Next actions for the main-machine agent

1. Merge PR #1 into `main` so this is the new stable state; `git pull` locally.
2. Repo hygiene: this repo should hold *everything* about FoxCull — design
   notes, internal discussion, handover docs, roadmap, playbooks, and a root
   `CLAUDE.md` (none exists yet — create one). Scan the local working folder
   for FoxCull docs/notes not yet committed and commit them. Keep only secrets
   out of git (`.env*` is already ignored; verify nothing sensitive is swept
   in before committing).
