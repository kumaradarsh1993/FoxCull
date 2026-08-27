# In-app updates

FoxCull updates itself. This file is the shared contract behind that, and
it is **the same in wispr-fox, FoxCull, Fox MD (`md-reader`) and Fox Mark** —
copy any fix into all four.

## Where the user finds it

**Gear -> Version -> About & updates...**, which opens a dialog. The gear row reads "Update available..." when there is one.

A background check runs once per app launch (`primeUpdateCheck()`), so the entry
point can carry a dot when a newer build exists rather than waiting to be
visited. The check is silent on failure: being offline is not news, and a badge
that fires because GitHub was unreachable trains you to ignore it.

## What happens on Install

1. Rust re-resolves the tag against the GitHub releases API — the renderer never
   passes a URL across the IPC boundary, only a tag.
2. The download URL is checked against a small allowlist of GitHub hosts, over
   HTTPS only.
3. The installer streams to `<app cache>/updates/`, emitting `update://progress`
   so the bar reports real bytes rather than a spinner.
4. The finished file must match the byte size the API declared, or it is deleted
   and nothing is launched. This is not a signature check — the artifacts are
   unsigned, exactly as a manual download would be — but it catches a truncated
   transfer before an `.exe` reaches the OS.
5. **Windows:** the NSIS installer runs with `/S /R` — silent, then relaunch —
   and the app exits 1.5s later so its own files can be replaced. This is the
   whole one-click story, and it is why the NSIS `-setup.exe` is preferred over
   the `.msi` (an MSI has no equivalent of `/R`).
   **macOS / Linux:** the `.dmg` or AppImage is downloaded and opened. Neither
   can be a true in-place upgrade without notarisation (mac) or knowing how the
   user installed it (Linux), so the app stays running and says what to do next.

## Two channels, and the rule that stops a downgrade

The panel shows the newest stable and the newest nightly, each compared against
the running build. **A nightly older than the newest stable is hidden entirely**
— offering it would offer a downgrade dressed as an update.

Version comparison is semver-ish and deliberately not a string compare:
`nightly.10` must sort above `nightly.9`, and `3.4.0` must outrank
`3.4.0-nightly.9`. Getting either wrong produces an Install button that never
appears — no error, nothing to notice.

## Two things that will silently break this

- **A nightly published as a DRAFT is invisible.** GitHub does not return draft
  releases to an unauthenticated caller, and a draft has no public download URL,
  so a draft nightly cannot be seen or installed. `release.yml` must publish
  `*-nightly*` tags as **pre-releases**.
- **Renaming a CI artifact can silently disable Install.** The asset picker
  matches by suffix (`x64-setup.exe`, `.dmg`, `.AppImage`, `.deb`), and a miss
  degrades to "No installer for this platform in that release" rather than an
  error. `tools/updates-selftest` in the `md-reader` repo pins every repo's real
  artifact names for this reason.

## Why not `tauri-plugin-updater`

The plugin resolves ONE update per endpoint from a signed `latest.json`, and
wants a keypair whose private half lives in CI secrets. GitHub's
`releases/latest` deliberately excludes pre-releases and there is no "latest
pre-release" equivalent, so the two-channel picker would need a second manifest
published by hand. These apps ship unsigned builds from public repos, so the
signing apparatus buys nothing and costs a key-management story.

## Testing it

The logic cannot be tested where it lives: a Tauri app crate's test harness
links the whole WebView2/tao stack and dies with `STATUS_ENTRYPOINT_NOT_FOUND`
before a test runs, and these repos do not run `cargo test` in CI. Instead:

```
cd md-reader/tools/updates-selftest
D:/Python312/python.exe extract.py && cargo test
```

`extract.py` **slices the real `updates.rs`** rather than restating it, so a
pass is evidence about the shipped code and a drifting module breaks extraction
loudly instead of quietly testing nothing.

## Files

| File | What |
|---|---|
| `src-tauri/src/updates.rs` | Everything above. Identical across the four apps bar three constants at the top. |
| `src/lib/updates.svelte.ts` | Typed IPC contract + the one-per-run background check. |
| `src/lib/components/UpdatePanel.svelte` | The panel. Self-styled from `currentColor`, so it lands correctly in every app's themes without being handed tokens. |
