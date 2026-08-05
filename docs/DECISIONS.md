# Decisions log (ADR-lite)

Standing technical decisions with their reasoning, newest first. If you're
about to re-litigate one of these, read its entry first, then update it (don't
delete history — strike through and append).

---

## 2026-07-22 · The grid will never be shown over Chromecast. HDMI is the answer.

**Question (owner):** "Is there a way I can see that grid on my TV too? If it's
feasible without sacrificing image quality, that would make things much easier."

**Decision:** no Chromecast grid. Casting stays a single-item, full-screen
surface. For grid-on-TV, plug in HDMI and go fullscreen.

**Reasoning:**

1. **The Default Media Receiver (`CC1AD845`) renders exactly one image or video
   URL.** It is not a page we can push a DOM to. This is a protocol fact, not a
   limitation of our implementation.
2. **The only real route is a custom Cast receiver app** — an HTML page hosted
   on a *public* HTTPS origin, registered in the Google Cast Developer Console,
   with devices registered for testing, plus a second UI to build and keep in
   sync with the real one. Our hand-rolled sender could technically drive it
   (`LAUNCH` takes any appId; custom namespaces are just more frames), so this
   is a cost problem, not an architecture problem. The cost is weeks and an
   ongoing hosting + sync burden for a screen that is already reachable by
   cable.
3. **The cheap version is worse than nothing.** Rendering the grid to a JPEG and
   casting it as a still means a full `LOAD` per navigation — receiver flicker
   and ~0.5–1.5 s of lag on every d-pad press. The whole point of grid-on-TV is
   fast browsing.
4. **Screen mirroring** (Chrome's "Cast desktop") is a different protocol stack
   — a mirroring receiver plus a real-time encoder — and is not reachable from
   this codebase at any sane cost.
5. **The quality premise is backwards.** The owner worried HDMI would cost image
   quality; it is the opposite. The Default Media Receiver **downscales stills
   to roughly 720–1080p** (documented at the top of `cast.rs`). HDMI is native
   resolution, zero latency, filmstrip and all, and gives the TV the audio too.
   Casting is already the lossy path.

**Consequence:** cast keeps improving as a *single-item* surface — it follows
the active item in every view including grid (2026-07-20), and mirrors the
laptop's transport (2026-07-22). Grid-on-TV is an HDMI workflow.

---

## 2026-07-20 · HEIC/HEIF stills: bundled ffmpeg stays the decoder; no OS-codec path, no setting

**Question (owner):** Windows 11 has paid HEVC/HEIF Store extensions — shouldn't
the OS be the first line of defense, with our bundled decode as fallback (auto
or via a setting)? Which is more performance-efficient?

**Decision:** keep the bundled ffmpeg as the *only* still-image HEIC path.
No OS-first mode, no setting.

**Reasoning:**

1. **The webview can't use the OS codecs for stills anyway.** FoxCull's UI is
   Chromium (WebView2), and Chromium does not render HEIC in `<img>` no matter
   what codecs Windows has. Any "OS-level" path would mean *us* calling
   Windows WIC/Media Foundation from Rust to produce the same cached JPEG that
   ffmpeg produces today — a second, Windows-only decode pipeline, not a free
   ride.
2. **The decode is one-time, then cached.** Every HEIC is decoded once into
   the `_FoxCull/thumbs` JPEG cache and reused forever after (and across
   machines via the SSD). Steady-state, both approaches cost identically:
   ~zero. The OS path could only speed up the *first* decode (WIC can use the
   GPU for the HEVC tiles) — and if bulk-import speed ever matters, ffmpeg can
   get the same GPU boost portably via `-hwaccel auto`, no OS dependency.
3. **The OS path is exactly what just failed the owner.** After the Windows
   reset, the HEVC extension shows "installed" but doesn't work. A codepath
   (or setting) depending on Store-extension health reintroduces the precise
   failure mode this app just spent a session diagnosing. Bundled ffmpeg is
   deterministic on every machine, including the future Mac.

**Nuance — video playback is the opposite, by design:** in-player video runs
through the webview's `<video>` → Media Foundation → OS HEVC codec *first*,
and FoxCull's H.264 proxy ("Convert & play here") is the fallback when the OS
can't. That IS the owner's requested "OS first, ours as callback" — it has been
the architecture all along, and it stays. Repairing the Store HEVC extension
will restore direct HEVC playback; the app never *requires* it.

---

## 2026-07-20 · Cast quality: originals for video, receiver-bounded stills, custom receiver as the 4K-photos path

**Question (owner):** casting must be feature-parity (follow browsing across
photos AND videos) and maximum quality — what's the best way, especially for
4K60 clips?

**Decided/state:**

- **Videos: already maximal.** The local Range server streams the *original
  bytes*, untranscoded; the Bravia's own decoder plays 4K60 HEVC natively.
  No pre-crunching exists that would raise quality above "the original file".
- **Stills: bounded by the receiver, not by us.** We send full-resolution
  originals (JPEG/PNG/WebP), but Google's **Default Media Receiver** renders
  images on a ~720p–1080p canvas on most devices. The only route to a true 4K
  photo canvas is a **custom Web Receiver** (one-time $5 Cast developer
  registration, a small hosted receiver page declaring 4K support) —
  **BACKLOG P2** now. Until then, differences beyond ~1080p are invisible on
  the TV regardless of what we send.
- **HEIC/RAW stills** cast their cached 1920 px loupe JPEG (the receiver's
  Chromium can't decode HEIC/NEF at all — raw bytes rendered nothing). 1920 px
  ≥ the DMR canvas, so no quality is lost vs. any other approach.
- **Follow-mode** (2026-07-20): one cast session, TV mirrors the active item
  as you browse; LOADs are debounced and reuse the live CASTV2 connection.
- **Fire TV Stick** speaks its own protocols (DIAL/AirPlay-ish), not Google
  Cast — out of scope; the Bravia's Chromecast built-in is the target.

---

## 2026-08-04 · Catalog integrity: flag-and-relink, never delete; verify before you walk

**Question (owner):** the catalog is keyed by path, so moving photos on the drive
loses their metadata. Make it behave like Lightroom — show a "?" for what can't
be found, work out folder moves automatically, let me remap the rest. A ten- to
fifteen-second launch cost is acceptable.

**Decided:**

- **A scan flags; only an explicit action deletes.** `catalog_scan` writes
  unresolved rel-paths into a `missing` table and touches nothing else. The one
  path in the app that removes marks for an absent file is `forget_missing`,
  behind a confirmation. Rationale: metadata is the expensive thing the user
  created; a file is cheap to find again. Anything that can silently discard the
  former in response to a filesystem event is the wrong trade, no matter how
  confident the heuristic looks.
- **Verify before you walk, and only walk on demand.** Pass one is an existence
  check over metadata-carrying rel-paths; the library is walked ONLY if something
  is missing. The owner offered ten seconds; that budget should be spent when
  there is something to find, not on every launch. Do not collapse this into a
  single unconditional walk "for simplicity" — the cost model *is* the design.
- **Cohort matching before filename matching.** Absent entries are grouped by
  their old folder and a folder is treated as moved when ≥50% of its filenames
  appear together under one new directory. Only leftovers fall back to an
  individual match, and only when that match is unique. Filename-first matching
  is the obvious implementation and is ambiguous exactly where real libraries
  have duplicates.
- **A relink never adopts a file that already carries metadata.** Guarded by the
  `owned` set in `catalog_scan`. Without it, automatic relinking could overwrite
  photo B's marks in the course of "fixing" photo A — which would make running
  the scan unattended on launch indefensible.
- **What counts as "tracked" is narrow on purpose.** Marks, tags, trims, segments
  and event membership. NOT default-valued decision rows, and not the
  `captures`/`dir_counts` caches. Merely opening a folder must never manufacture
  a missing-photo report the owner then has to resolve.

---

## 2026-08-04 · Events are metadata, not folders — and ordering is a rank, not a comparator

**Question (owner):** events ("Monar trip", "Rashi's birthday") sitting at the
same level as tags, working across folder hierarchies, rendered as a visually
distinct block with a feature photo.

**Decided:**

- **Stored as `events` + `event_members`, keyed by rel-path like every other
  mark.** Deliberately not a folder and not a tag-name convention: an event needs
  a cover, a stable identity across renames, and a count, none of which a string
  prefix on the tags table gives you cleanly.
- **Grouping is the primary read; filtering is the secondary one.** The grid
  already sections by folder/type/date, so an event is just another section key —
  which is what makes a trip scattered across ten subfolders come out as one
  block for free. `Filters ▸ Event` covers the "show me only this trip" case.
- **An item may join several events; the first one it joined is primary** and
  decides which block it renders in. Emitting an item once per event would
  duplicate it in the grid's flat index space, where selection is keyed by path —
  two highlighted copies of one photo. Not worth it for a rare case.
- **Block order is encoded as a rank, not applied at compare time.** The grouped
  sort compares section keys directly and never multiplies by `sortDir` (true for
  every grouping, not just events). So `eventRank` bakes ascending/descending in
  itself, and `NO_EVENT_KEY = "999999"` parks unassigned shots after every real
  block under the numeric collator. If you ever make section order direction-aware
  globally, delete the rank — do not leave both.
- **Events stay out of the undo stack**, like tags' own add/remove menu entries.
  The stack's snapshot shape covers marks only, and every event action is one
  menu click to reverse.

---

## 2026-08-04 · A Tauri command that touches the filesystem is `async`, always

**Question:** why did clicking `D:\` hang the whole window for minutes, when the
same code opened an 8,403-file folder on `F:\` in 390 ms?

**Decided:** because a *synchronous* Tauri command runs on the **main thread**,
which is the thread that pumps the native window's messages. The walk and the
window are the same thread, so a slow walk is a dead window. Nothing about the
code was wrong on a normal folder; the threading model only shows up at the tail.

- Every command whose worst case is unbounded — a recursive walk, N file moves,
  anything on a removable drive — is `async` + `spawn_blocking`. The bounded ones
  (a single `read_dir`, a catalog query) may stay synchronous.
- Long work must also be **cancellable and superseded-safe**. `warm_gen` is
  bumped on every folder open and now doubles as the walk's cancellation token;
  the frontend additionally guards on an `openGen` counter so a late reply from
  an abandoned scan can never overwrite the folder the user actually landed on.
- **Diagnostic worth keeping:** `foxcull.exe Responding=False` while every
  `msedgewebview2.exe` child is `Responding=True` means the NATIVE pump is
  blocked. The 1.4.0 freeze had the opposite signature (a blocked JS main thread,
  native process healthy). Two different bugs with the same user-facing word.

---

## 2026-08-04 · The in-app Trash adopts orphans instead of ignoring them

**Question:** the owner asked to delete every `_FoxCull` folder for a fresh
start, having checked the Trash and found it empty. Was that safe?

**No.** ~19 GB of his media was sitting in the per-drive recycle folders with no
`trash` row — an 18 GB merged Dubai clip on E:, and 8 DJI Mavic Mini clips on P:.
The Trash panel showed nothing because it lists the table, not the folder, so the
files were invisible, unrestorable, and about to be deleted on his say-so.

**Decided:** `list_trash` reconciles the folder against the table and **adopts**
anything it finds, rather than treating the table as the sole truth. The recycle
layout mirrors the original rel-path (`stored == orig` for every row FoxCull
writes), so an orphan's position under `recycle/` reconstructs its restore target
exactly. Adoption never deletes; it only makes a file accountable.

The general principle, which is the same one behind `catalog_scan`: **where the
filesystem and the catalog disagree, surface the discrepancy — never let the
catalog's silence stand in for the filesystem's contents.** A panel that renders
a table is telling you about the table; if the user reads it as "what is on
disk", the two had better be reconciled.

Root cause of the orphaning is unproven (a catalog reset is the leading theory).
Until it is, adoption is the safety net rather than the fix.
