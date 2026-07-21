# 2026-07-21 — scrub module audit, delete diagnosis, undo-restore, left filmstrip

## Intent

Owner feedback after installing nightly.6, in his words: *"the scrub feature is
still broken… I think a few things are breaking together, you might want to look
into it properly"*, plus a delete that fails on a folder of large HEVC clips
with an unreadable error, an undo that can't bring deleted files back, no
left-hand filmstrip dock, and a Prepare button that only ever covers the whole
folder. Treated as one pass over the video-preview subsystem rather than four
point fixes, and documented the caching rules so the next audit starts from a
written policy instead of five files.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/components/Thumb.svelte` | **logic (bug)** | Strip build is now driven by an `$effect` on `(armed && hovering)` instead of the `pointerenter` handler. Clicking a tile arms it while the pointer is *already inside*, so `pointerenter` had fired before arming and never fired again — the selected tile was the one tile that never got a build. Added `hovering`/`building` state. |
| `src/lib/components/Thumb.svelte` | **UX (bug)** | Pointer→time now maps across the whole cell, not the letterboxed picture. A 9:16 clip paints ~30% of a landscape cell, so the timeline was crammed into that sliver (hypersensitive) with dead travel either side. |
| `src/lib/components/Thumb.svelte` | logic | Pointer-leave no longer cancels the build of an **armed** tile; only unarmed tiles cancel. Disarming still cancels. Stops a 10 s extraction being thrown away and restarted because the pointer drifted. |
| `src/lib/components/Thumb.svelte` | UX | Build chip shows while extracting, not only while the cursor is producing a scrub position. |
| `src/lib/components/Loupe.svelte` | **UX** | With Live Scrub ON the dense filmstrip builds on clip **open**, not on first pointer contact with the seek bar. New `.stripbuild` chip (top-left) reports progress. Live Scrub OFF still builds nothing. |
| `src/routes/+page.svelte` | UX | Filmstrip can dock **left**, between the folder tree and the viewport; resize handle direction inverted for that dock. |
| `src/routes/+page.svelte` | UX | Prepare is a split button: primary = whole folder (unchanged), ▾ = selection / videos only / photos & RAW only, each with a live count. |
| `src/routes/+page.svelte` | **logic** | Undo stack is now a discriminated union (`marks` \| `delete`). A dispose into the in-app Trash pushes a `delete` entry; undoing it confirms first, then restores. Delete entries are never redoable. |
| `src/routes/+page.svelte` | UX | New confirm/notice modal (`ask`), used for the restore confirmation and for delete failures whose real reason doesn't fit the activity chip. Swallows keys while open. |
| `src/routes/+page.svelte` | logic | Opt-in neighbour scrub prefetch (±3 clips, 900 ms settle) while a video is open in Focus. |
| `src-tauri/src/commands.rs` | **logic (bug)** | `move_into_recycle` no longer reports every failure as "file is in use". Classifies `ERROR_SHARING_VIOLATION`/`ERROR_LOCK_VIOLATION` vs `PermissionDenied`, clears a read-only attribute and retries once, and names the file in the message. |
| `src-tauri/src/commands.rs` | architecture | `TrashOutcome` gained `trashed: Vec<String>` — the trash keys of this batch, the handle Undo needs. |
| `src/lib/settings.svelte.ts` | architecture | `FilmstripPos` gained `"left"`; new `scrubPrefetch` setting (default off). |
| `src/lib/types.ts` | architecture | `TrashOutcome.trashed`. |
| `docs/design/precache-policy.md` | **process (new)** | Authoritative, human+machine-readable record of every cached artifact, its key, its build triggers, the concurrency doctrine, and an honest gaps list. Owner-requested so the policy can be audited against the code by a person or a model. |

## Behavior changes

- Grid skimming works on the clip you click, which is what "click to arm" was
  supposed to mean since nightly.6. Sweeping across other tiles still does
  nothing.
- Portrait clips skim at the same sensitivity as landscape ones.
- Opening a video with Live Scrub ON starts the filmstrip build immediately and
  says so. Previously the build began only when the pointer entered the seek
  bar, with no indicator anywhere.
- A failed delete now explains itself: a lock and a permissions denial get
  different text, and the full reason appears in a modal.
- Ctrl+Z after a delete offers to restore the batch from the in-app Trash
  (confirmation required, no redo).
- Filmstrip has a third dock position.
- Prepare can be scoped.

## Risks / compat

- **Settings migrate silently.** `scrubPrefetch` defaults false and
  `filmstripPos` gains a value; existing stores load through `{...DEFAULTS,
  ...loaded}` so old configs are unaffected.
- **`TrashOutcome` gained a field.** Additive; no existing caller breaks.
- **The delete-undo entry holds trash keys, not paths.** If the user purges the
  Trash before undoing, the restore reports failures rather than doing anything
  destructive.
- **Neighbour prefetch is opt-in and gated on Live Scrub**, so the default
  install does no new background work. With both on it can queue up to 6 strip
  builds — the backend still serializes them and a folder switch cancels.
- The Focus-open filmstrip build is a deliberate reversal of a nightly.5
  decision. It is safe only because it stays gated on Live Scrub; if that gate
  is ever removed, the nightly.3 "a minute of ffmpeg per clip on an HDD" bug
  comes back. Stated in the policy doc.

## Verification actually run

- `npm run check` — 274 files, **0 errors, 0 warnings**.
- `cargo check` — clean (2m38s).
- Not run: `cargo test` (does not link on this machine's GNU toolchain — CI
  only), `npm run tauri build` (CI builds installers on tag).
- Runtime behaviour is verified by the owner against the tagged nightly build;
  the two scrub bugs above were both diagnosed from his reproduction rather
  than guessed.

## Not addressed here

The Windows "You'll need to provide administrator permission to delete this
file" dialog the owner also hits **in Explorer** on `E:` is an OS-level ACL
issue from the Windows reinstall, not an app bug — FoxCull now reports it
accurately instead of blaming a phantom lock, but fixing it means taking
ownership of those folders once, outside the app.
