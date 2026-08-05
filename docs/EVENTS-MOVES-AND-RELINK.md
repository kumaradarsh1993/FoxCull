# Events, moving files, and how metadata survives

How the three 2026-08-04 features actually behave — the happy path, the awkward
paths, and the places they genuinely do not work. Written so you can plan a
workflow around them, and so the agent that reorganises your drive can be given
rules it will not break.

**Status:** Events, moves and relink shipped in `v1.5.0-nightly.1/.2`. The Trash
rework and the agent adapter at the end are **proposals awaiting your sign-off**,
not built yet.

---

## 0. The one fact everything else follows from

FoxCull stores every mark — rating, colour label, pick/reject, tag, event, trim —
in a per-drive SQLite catalog at:

```
<drive>\_FoxCull\catalog.sqlite
```

keyed by the file's **path relative to the drive root**, with forward slashes:

```
All media MASTER/Pics/2026/Munnar trip/DSC_0042.JPG
```

That choice is why a drive carries its own marks between machines, and why it
survives a drive letter changing from `P:` to `E:`. It is also the source of
every problem below: **the key is the path, so moving the file breaks the key**
unless something repairs it.

Two consequences worth internalising:

- **Marks do not cross drives.** Moving a photo from `P:` to `E:` leaves its
  marks in P:'s catalog with nothing to attach to, and E:'s catalog has never
  heard of it. Relink cannot fix this — it only ever searches within one drive.
- **A file with no marks is not tracked.** Move it wherever you like; there is
  nothing to lose and nothing to repair.

---

## 1. Events

### What an event is

A named virtual collection — "Munnar trip", "Rashi's birthday" — stored as
metadata, **not** as a folder. The photos stay exactly where they are filed. An
event spans any number of directories, which is the whole point: shots from your
S23 Ultra, your sister's S22+, your dad's S23 and the Mavic Mini can live in four
different device folders and still come out as one block.

### Happy path

1. Select photos in the grid (they can be from different folders — turn on
   *Include subfolders* and select across them).
2. Right-click → **New event…** → name it.
3. The grid switches to `Arrange ▸ Group ▸ Event` automatically, and you see the
   block.
4. Later, from any other folder: select more, right-click → **Add to "Munnar
   trip"**.
5. Right-click a good shot → **Use as cover of "Munnar trip"** to choose the
   frame that fronts the block.

### Reading it back

- **`Arrange ▸ Group ▸ Event`** — the grid becomes event blocks, each with a
  cover band (cover frame, name, date range, count). Works at any level of the
  hierarchy: open a year folder with subfolders included, and every event inside
  it separates out.
- **`Filters ▸ Event`** — isolate one event across everything currently loaded.
  This is the "virtual collection" read.
- **Order by date / Cover art** — two checkboxes under `Arrange ▸ Events`. Date
  order ranks blocks by each event's *earliest* photo. The existing ↑↓ button
  flips ascending/descending for both name and date order. They are disabled
  outside an event grouping, deliberately, so the option stays discoverable.
- Manage (rename, delete, isolate) from the list under Arrange. **Deleting an
  event never touches a photo** — it removes the grouping only.

### Awkward and adversarial cases

| Case | What happens | Why |
|---|---|---|
| A photo is in **two events** | It renders in the block of the one it joined **first**; both names show on the info bar and in its tooltip | The grid's flat index keys selection by path. Emitting a photo once per event would put two selectable copies of the same file on screen. |
| You **rename** an event | All memberships follow; the filter and the loaded grid update in place | Rename is by event id, not by name. |
| Two events with the **same name** | Impossible — names are unique, case-insensitively. Creating "monar trip" when "Monar Trip" exists returns the existing one | `UNIQUE COLLATE NOCASE`, and `create_event` is idempotent. |
| The cover photo is **deleted or moved away** | The block falls back to its first remaining member | The explicit cover is stored but resolved against what is actually in view. |
| An event's photos are **all outside the current folder** | No block appears | Events are grouped over what is loaded. Open a higher folder with subfolders included to see them. |
| You **undo** after adding to an event | Nothing happens | **Events are not on the undo stack.** So are tags' menu actions. Reverse it from the same right-click menu. This is a decision, not a gap — but it is the one that will surprise you. |
| A photo goes **missing** and is later relinked | Its event membership survives both | Membership moves with the file in `move_media_entries`. |

### Known limits

- Event membership is FoxCull's catalog only. **It is not written into the photo's
  EXIF/XMP**, so Lightroom cannot see it. If you want Lightroom parity you need
  the folder structure as well — which is exactly the double-pass approach you
  are already running with the other agent.
- There is no "event" concept in the filmstrip or Details view beyond the chips;
  grouping is a grid feature.

---

## 2. Moving files

### Inside FoxCull — the safe path

Drag a selection from the grid onto any folder in the left pane. FoxCull moves
the files **and rewrites every catalog key in the same transaction**: ratings,
labels, flags, tags, events, trims, subclips, capture dates and the event cover
all follow. Cached thumbnails for the old path are dropped so nothing stale
lingers.

Supporting pieces:

- **New subfolder…** — the ＋ button at the top of the folder pane, or right-click
  any folder. If you have files staged with Cut, creating the folder moves them
  straight in.
- **Cut / Move here** — right-click a folder → *Move N files here*, for when
  dragging across a long tree is awkward.
- Cross-volume moves fall back to copy-then-delete, and if the delete fails the
  copy is removed so you never end up with a silent duplicate.

### What still breaks

- **Across drives.** Files move fine; **marks do not**. P:'s catalog flags them
  missing, E:'s catalog knows nothing about them. There is no cross-drive metadata
  transfer today. If you plan to consolidate drives, do the culling *after*.
- A move of many thousands of files is one long operation. It no longer freezes
  the window (nightly.2), but there is no progress bar on it yet.

---

## 3. Missing files and relink

This is what catches moves made **outside** FoxCull — by you in Explorer, or by
the other agent.

### When it runs

- **On launch**, after your folder is on screen. `Settings ▸ Check catalog on
  launch`, default on.
- **On demand**: `Settings ▸ Catalog ▸ Check now`, or right-click any folder →
  *Check catalog for moved files*.
- It is **not** a filesystem watcher. Moves made while FoxCull is open are not
  noticed until the next check.

### How it works, in order

**Pass 1 — verify.** For every rel-path carrying marks, does the file exist? This
is the only thing that runs when nothing is wrong, which is why launch does not
get slower. A few hundred stat calls, not a drive scan.

**Pass 2 — hunt (only if something is missing).** The library is walked once,
then matched in two stages:

1. **Folder cohorts.** Absent entries are grouped by the folder they used to live
   in. If **at least half** of that folder's filenames now appear together under
   one new directory, the whole folder is treated as one move and every match is
   relinked. This is what handles "the agent reorganised 2026 by device".
2. **Unique filename fallback.** Leftovers relink only if their filename appears
   **exactly once** in the whole library. Two or more candidates and FoxCull
   declines rather than guess.

**Never** does it adopt a file that already carries its own marks. Fixing one
photo must not overwrite another's.

**Pass 3 — flag, never delete.** Anything unresolved becomes a "?" tile in the
grid, carrying its full ratings, labels, tags and events. Right-click:

- **Locate this file…** — point at it directly.
- **Locate the folder it moved to…** — relinks every "?" from that old folder
  against a folder you pick. Sub-paths are preserved first, then a flat filename
  match.
- **Forget** — deletes the marks. This is the *only* thing in the app that does.

### Where it fails — read this before instructing the agent

| Scenario | Result | Mitigation |
|---|---|---|
| Files **moved AND renamed** in the same pass | ❌ Total loss of the link. Cohort matching and the fallback both key on filename. Everything becomes "?" and only *Locate the folder…* by hand can partly recover it | **Tell the agent to move and rename in two separate passes**, and to run a catalog update between them. Or use the adapter in §5. |
| Duplicate filenames across the library (`IMG_0001.JPG` ×40) | ⚠️ Cohort matching still works. The individual fallback declines — those stay "?" | Rely on whole-folder moves, not scattered single-file moves. |
| A folder **split** across two destinations, near 50/50 | ⚠️ Neither destination wins the cohort vote; falls back to unique-filename matching | Move in whole folders. |
| Move **across drives** | ❌ Not recoverable. Different catalogs | Cull first, consolidate later. |
| A file **replaced in place** (edited, same path) | ⚠️ Not detected as missing — the marks stay attached to a path that is now different content | By design; there is no content hashing. |
| **Case-only rename** on Windows | ✅ Resolves | Path comparison is case-insensitive. |
| A moved file that had **no marks** | ✅ Nothing to do | Untracked by definition. |
| The drive is **offline** at launch | ⚠️ Every tracked file looks missing | The scan flags rather than deletes, so plugging the drive back in and re-checking resolves it. Nothing is lost — but do not hit *Forget* on a disconnected drive. |

The last row is the one that could actually hurt you. **Never use Forget in bulk
without checking the drive is mounted.**

---

## 4. Refresh — what exists today

There are three separate things, and they are not well named. Honest inventory:

| Action | Where | What it actually does |
|---|---|---|
| **Refresh folder** | Right-click a folder | Clears cached folder counts, re-reads the folder from disk |
| **↻** | Top of the folder pane | Re-enumerates physical drives (this is how a just-plugged USB appears), clears **all** cached counts, re-reads the current folder |
| **Check catalog** | Settings, or a folder's right-click menu | The integrity pass in §3 |

What you do **not** need to refresh:

- **Thumbnails and previews.** The cache key is a hash of *(absolute path, mtime,
  size, requested size)*, so editing, replacing or moving a file produces a
  different key automatically. There is no such thing as a stale thumbnail, and
  no cache to clear by hand.
- **Capture dates.** Cached per file and validated against (mtime, size).

The only thing that is *not* auto-invalidated is the folder-count badges, which
is precisely why the ↻ button exists.

**My recommendation, for your sign-off:** rather than adding a Lightroom-style
"hard rescan", collapse these into one **Refresh** with a modifier — plain click
does what ↻ does now, hold Shift for "and check the catalog too". Adding a third
concept would cost you more than it buys, given nothing else goes stale.

---

## 5. The third customer: an external agent (PROPOSAL)

Today the agent that reorganises your drive can only be caught *after the fact*,
by the relink pass — and §3 shows exactly where that fails (move + rename in one
pass loses everything). The agent should be able to tell FoxCull what it did.

Tauri commands are only reachable from inside the app's own window, so an
external process cannot call them. Three options, and my recommendation:

| Option | Verdict |
|---|---|
| Agent writes the SQLite catalog directly | ❌ Rejected. It would have to re-implement the transaction that moves eight tables at once, plus the event-cover and missing-flag updates. One malformed write and the catalog is inconsistent with no error. |
| A localhost HTTP API in the running app | ❌ Rejected for now. Requires FoxCull to be running, adds an open port and an auth story to a local photo tool. |
| **A CLI mode on the same binary** | ✅ **Recommended.** Works whether or not the app is open, goes through the *same* `Catalog` code as the UI, no new surface area, no ports. |

Proposed shape — `foxcull.exe` with a subcommand short-circuits before the window
opens, does the work, prints JSON, exits:

```bash
# after moving files, tell FoxCull where they went
foxcull.exe catalog --drive P: move --from "All media MASTER/Pics/2026/DSC_0042.JPG" \
                                   --to   "All media MASTER/Pics/2026/S23 Ultra/Munnar trip/DSC_0042.JPG"

# or feed a whole batch, one {"from","to"} per line
foxcull.exe catalog --drive P: move --batch moves.jsonl

# events and tags
foxcull.exe catalog --drive P: event add    --name "Munnar trip" --paths paths.txt
foxcull.exe catalog --drive P: event cover  --name "Munnar trip" --path  "…/DSC_0042.JPG"
foxcull.exe catalog --drive P: tag   add    --tag  "people:rashi" --paths paths.txt

# housekeeping
foxcull.exe catalog --drive P: scan --relink     # same pass the app runs
foxcull.exe catalog --drive P: list-events --json
```

**The contract the agent must honour:**

1. Call `move` **for every file it moves or renames**, before or after the
   filesystem operation — the catalog update is idempotent either way.
2. Paths are **relative to the drive root**, forward slashes.
3. If it moves *and* renames, `move` is the only thing that can preserve the
   link. Relink cannot.
4. Cross-drive moves are out of scope; it must not move files between drives if
   their marks matter.
5. Run `scan --relink` at the end as a safety net for anything it forgot.

This also gives you the hook for the **face/pet tagging** idea: whatever produces
those labels — a separate local tool, Immich, anything — writes them in through
`tag add`, and FoxCull's existing tag filter and grouping pick them up with no
new code. I would build the adapter first and treat recognition itself as a
separate program, not something baked into FoxCull. Bundling a model would fight
the "keep it lightweight" goal directly, and a good local face pipeline as of
mid-2026 is a project in its own right, not a feature.

---

## 6. Trash rework (PROPOSAL)

Your complaints, all fair: the Trash is hidden inside `_FoxCull`, shows almost no
information, and you cannot play a video to remember why you deleted it.

Proposed:

- Storage moves from the hidden `<drive>\_FoxCull\recycle` to a **visible
  `<drive>\FoxCull Trash`**, flat, no hierarchy inside. Name collisions get the
  usual ` (2)` suffix.
- It appears in the folder tree like any other folder, so opening it gives you
  the **normal grid**: thumbnails, size, sorting, full playback, Focus view.
  That answers "I want to preview the video before deciding" with no new UI.
- It is excluded from a *parent* folder's recursive scan, so trashed files never
  reappear in the library — but browsing into it directly works.
- Restore and Delete-permanently move into the normal right-click menu when
  you are inside that folder.
- **Provenance gets belt and braces.** Where the file came from is stored in the
  catalog *and* mirrored into a small `_trash-index.json` inside the trash folder.
  That second copy is the direct fix for the ~19 GB of orphans I found: if the
  catalog rows are ever lost again, the index still knows where every file
  belongs.
- The info panel shows **Deleted from: `<original path>`** and the deletion date.

I have not built this yet because it moves your 19 GB of existing trash, and you
said you want to review that content first. Say the word and it is a short job.
