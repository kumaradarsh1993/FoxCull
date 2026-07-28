# 2026-07-28 — Firewall-independent Chromecast discovery on Windows

## Intent

Fix the XPS-only failure where Chrome/YouTube discovered both Cast receivers
but FoxCull discovered none, without requiring the user to edit Windows
Firewall rules or reinstall with administrator privileges.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src-tauri/src/cast.rs` | discovery architecture / protocol | Retained the normal UDP/5353 mDNS browser and added a parallel ephemeral-port DNS-SD query on every active IPv4 adapter; safely parses PTR/SRV/TXT/A replies, merges by Cast id, and logs each route plus daemon/interface diagnostics. |
| `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` | dependency | Added the already-transitive pure-Rust `if-addrs` crate as a direct dependency so the fallback can query each adapter explicitly. |
| `RELEASE_NOTES.md` | release UX | Explains the Chrome-visible/FoxCull-invisible Windows fix in user language. |
| `CLAUDE_CODE_HANDOVER.md`, `docs/PROJECT-LOG.md`, `BACKLOG.md` | process | Recorded the evidence, firewall model, protocol choice, test status, and remaining Cast hardening scope. |

## Behavior changes

- Pressing Cast starts the standard multicast listener and the new
  ephemeral-port search concurrently, so the menu still completes in roughly
  the same three-second window.
- The fallback sends `_googlecast._tcp.local.` PTR questions from each active
  IPv4 adapter and receives RFC 6762 legacy-unicast replies without needing an
  inbound UDP/5353 exception.
- Standard and fallback results are deduplicated by receiver id.
- A failure in one route no longer hides devices found through the other.
- `foxcull.log` identifies both route counts, queried adapter addresses,
  bind/send/receive failures, daemon errors, and standard mDNS metrics.

## Risks / compatibility

- The fallback is IPv4 because FoxCull's Cast control connection and local
  media-server URL are currently IPv4. The original browser still handles the
  broader cross-platform mDNS behavior.
- DNS packets are untrusted LAN input. The parser bounds packet offsets,
  question/record counts, label traversal, compression pointers, TXT lengths,
  and record lengths before reading them.
- A receiver that ignores RFC 6762 legacy-unicast queries can still be found by
  the original listener.
- The Sony receiver answered the exact fallback probe on the Alienware LAN;
  final XPS verification requires the GitHub-built nightly.

## Verification actually run

- Live LAN probe from an ephemeral IPv4 UDP port to
  `224.0.0.251:5353` — Sony receiver replied immediately with PTR plus three
  additional records.
- `cargo check` — pass.
- `npm run check` — pass, 0 errors and 0 warnings.
- `npm run build` — pass.
- `git diff --check` — pass.
- `cargo fmt --check` — continues to report broad pre-existing formatting
  drift across untouched Rust modules; no bulk format was applied.
- Unit coverage added for a complete synthetic Cast DNS-SD response and
  compressed-name decoding/loop rejection; execution is CI-only on this
  Windows-GNU machine.
- GitHub CI/release gates — pending before publication.
