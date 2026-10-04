# FoxCull project log

**Append-only. Newest entry at the bottom of each dated section; new sections
go at the end.** Never rewrite history here — if something recorded below turned
out to be wrong, add a later entry saying so and why. That correction trail is
the point of the file.

## What this is for

A plain-language narrative of how FoxCull got to where it is: what the owner
asked for, what was built, what broke, what was decided and *why*. Enough that
an agent or person arriving cold can understand the shape of the project without
reading the code or the git history.

**It is not** a commit log (that's git), a per-push technical diff
(`docs/changes/`), or the current state of the world (`CLAUDE_CODE_HANDOVER.md`).
Those three answer "what changed"; this answers "why is it like this".

Related docs: `docs/DECISIONS.md` (ADR-lite, standing technical decisions),
`docs/ROADMAP.md` (what's next), `BACKLOG.md` (prioritized worklist).

---

## Before 2026-07-21 — the shape of the app

FoxCull is a photo/video **culling** tool first and a light editor second, built
for one person's real workflow: pull a shoot off an external SSD, decide fast
what to keep, export a few clips. Tauri 2 + SvelteKit 2 + Svelte 5 + SQLite.
The owner shoots Nikon D5200 RAW/JPEG and, increasingly, **DJI Osmo Pocket 3
4K60 HEVC** video — clips 2 to 15 minutes long, hundreds per folder. That
footage is what most of the hard problems below come from.

Established earlier and still standing:

- **Per-drive data.** Each drive root gets a `_FoxCull/` folder holding its own
  catalog, cache and in-app Trash, so a drive carries its marks between
  machines and a read-only mount still works.
- **Performance doctrine.** Background work must never starve the foreground.
  Small warm pool, shallow queues on USB SSDs, viewport-driven loads, throttled
  ffmpeg fan-out. Written after a "progressively worse / not responding" bug
  that turned out to be memory, not decode speed.
- **Cast is hand-rolled** (CASTV2 over `native-tls`, no `rust_cast`/`rustls`)
  because this machine's toolchain has no C compiler and the obvious crates
  need one. Don't "simplify" it back.
- **Releases are tag-driven.** Pushing `main` ships nothing; `v*-nightly.N`
  builds a prerelease, a bare `v*` builds stable. Promotion to stable only on
  the owner's explicit say-so.

---

## 2026-07-21 — the video player migration

### The problem, in the owner's words

4K60 clips, and *"just caching and doing stuff was a bit inferior experience…
when I was seeking, it would take a lot of time to render."* The objective is
culling: land on a video, and if it's long, **grab the handle and drag** to see
what's in it. That's the whole use case.

### What had been tried and abandoned

An earlier plan replaced the webview player with **libmpv**. Two architectures
were built and both failed on rendering/compositing; one is OS-level impossible
(sibling child HWNDs on Windows do not blend, and the layering API wry needs
isn't exposed). Preserved on branches `archive/libmpv-A` and `archive/libmpv-B`
at the owner's explicit instruction — *"don't touch it, keep it in a committed
state, mark it out very clearly"* — in case a fallback is ever wanted.

### The insight that unblocked it

The owner pushed back on accepting the stall and asked for a ground-up
reconsideration: *"this is July 2026, I'm pretty sure there will be solutions
out there… I'm not sure how other players solve for it, but I'm pretty sure they
solve for it."*

He was right, and the reframing was this: **the goal is a seek *policy*, not a
native decoder.**

Every `video.currentTime = t` in Chromium is a *precise* seek — flush the
pipeline, decode from the previous keyframe, re-sync audio. Thirty of those
while dragging is why it lagged. mpv feels instant because while you drag it
does **keyframe-only** seeks and one precise decode on release. `fastSeek()`
would give us that, but Chromium doesn't implement it.

So: use **WebCodecs**. Keep a persistent hardware `VideoDecoder`, decode the one
frame under the cursor (~16–19 ms), paint it to a canvas over the video, and on
release do a single precise seek underneath and hand back. Called **Architecture
C**; recorded in `docs/design/video-player-migration.md`.

This dissolved the entire overlay/compositing problem that had consumed the
libmpv effort — everything stays inside the webview.

### Proved before building

A probe ran the real pipeline against the owner's actual Osmo file inside the
shipped WebView2: hardware HEVC Main 10 decode confirmed, 16–19 ms/frame, 316 ms
to index a multi-GB file. Only then was it built. This ordering was deliberate —
the project had already been burned three times by asserting inference as fact.

### The owner's constraints, recorded verbatim in spirit

- Nearest-keyframe seeking while dragging is fine: *"that seems reasonable, and
  I have absolutely no problem with it."*
- Keep the GitHub release pages clean.
- *"Build incrementally towards the stable release and mark a tag over it."*

### Bugs found by a self-test, not by a human

Because no one can drive a mouse in CI, a test was written that drives the real
engine through a simulated drag. It caught two things that would otherwise have
shipped:

1. **The picture froze during a drag** — the coalescer skipped painting any
   frame superseded by a newer request, which during a real drag is *always*.
   1 of 24 frames painted.
2. **A 4-second stall on release** — holding all decoded frames of a GOP starved
   the hardware decoder's output-texture pool and `flush()` deadlocked. Fixed by
   keeping only the best-matching frame alive.

Lesson worth keeping: **a test that exercises the real component finds things
reasoning does not.**

---

## 2026-07-22 — feedback, fixes, and the road to stable

A long session driven entirely by the owner testing installed nightly builds.

### Fresh start

At his request, every FoxCull-created cache, catalog and settings file was wiped
across all drives so the retest wasn't biased by old renders. The in-app Trash
(9 culled videos, 18.6 GB) was **kept** on his explicit choice; culling marks
were wiped on his explicit choice.

### Glimpse

New feature, his idea: *"imagine I'm in an editor's position… it just quickly
jumps through the video and I get a very rough idea of what it's all about."*
Mapped to Ctrl+Space and a button beside play.

**Built wrong the first time.** v1 compressed every clip into a fixed-length
sweep (a 4-second floor), so short and long clips ran at wildly different
apparent speeds. He rejected it and was right: *"if I'm moving at 5x, it should
be moving at 5x"* — like YouTube, so the pace is learnable and consistent
between clips. Rebuilt as a **plain realtime multiple**, 2x–10x, default 5x.

### The blip

He reported *"one frame is loaded and then it blips out and… loads again."* Real:
the still shown while a clip opened was extracted at **1 second** while the video
starts at **0** — two different moments of the same clip, swapping. Focus posters
now come from t=0. Grid thumbnails keep the 1-second frame because frame zero is
often black.

### Sprites retired

He asked whether grid tiles could skim the same way Focus does, and get rid of
pre-caching entirely. **Yes** — and the reason previously given for keeping
sprites in the grid ("a decoder per tile isn't viable") never applied, because
the existing *armed* rule already guarantees exactly one skimming tile at a time.
Video pre-caching was removed everywhere. Sprites survive only as an on-demand
fallback for codecs the decoder rejects.

### The freeze — full RCA, because he asked for one

Symptom: clicking the timeline during playback left the picture stuck while the
audio played on; intermittent, and once it happened on a clip it kept happening
on that clip.

Cause: releasing the playhead starts two async things — decoding the exact frame
(~150 ms) and the element's own seek. Whichever finished second was supposed to
hand the picture back to live video. When the **seek** won, it cleared the still
*and cancelled the 1500 ms safety timer*; the late-arriving decoded frame then
put itself back on screen with nothing left alive that could ever remove it.

Intermittent because it was a race. Sticky per clip because which side wins is a
property of that file's seek latency. Audio kept playing because the video was
never broken — it was playing correctly underneath an opaque canvas.

Fixed by tagging each decode with the hand-off it belongs to, plus a four-times-
a-second invariant check that a playing video is never covered. Verified by
transcribing the state machine out of the component and driving both orderings:
the pre-fix logic reproduces the freeze in exactly the "seek wins" ordering.

### Cast — three bugs, one per symptom

He reported three different misbehaviours and they turned out to be three
distinct defects:

1. **The Default Media Receiver closes itself when idle**, and `LAUNCH` was sent
   exactly once at connect. After it closed, every later load sat queued forever
   while the connection stayed healthy and the UI still said "Casting". → the
   clip vanishing from the TV, permanently, for that session.
2. **`playing_path` was set when a load was queued, not sent** — so the backend
   reported files the TV had never been told about and the follow logic
   considered itself done. → the previous clip continuing on the TV.
3. **No sequencing on load requests** — for RAW/HEIC a preview is generated
   first (seconds), so two fast navigations could land out of order.

The relaunch decision was extracted into a pure function specifically so CI
could test it; four tests cover the exact pre-fix hole.

### Edit mode — ~8 GB on a 229-clip folder

He hit 92% of system memory opening Edit on 229 Osmo 4K60 clips, with every item
stuck on "Reading details...".

Two agents were dispatched to investigate in parallel; **both died immediately on
an account spend limit**, so the RCA was done directly.

Findings:
- **EditStudio's source pane is not virtualized** (the library grid is). It
  mounted one tile per file — 229 at once — each firing an ffmpeg poster
  extraction plus two IPC calls on mount.
- **The probe sweep did `slice(0, 80)`.** Items 81–229 were therefore *never*
  probed: "Reading details..." was permanent, not slow. And the 80 that did fire
  competed with the 229 poster extractions for the same disk.

Fixed by gating all tile fetching on real visibility and probing on scroll-into-
view. **Measured result: 7,940 MB → ~519 MB.**

Notably, JS heap was only ever ~10–20 MB of a 4,192 MB limit — so the renderer
gigabytes were never JavaScript objects. Instrumentation (`edit-mem` log lines)
was added *before* claiming a fix, and it then caught a second real defect: a
close/open pair 1 ms apart proving the source list recomputed and handed all 229
tiles a new object identity, re-running every load effect.

### Grid skimming didn't work at all — two of my own bugs

He turned Live Scrub on, tried skimming in the grid, and nothing happened.

1. **`tilePending` was `$state`.** The opening effect both read it in its guard
   and wrote it in its body — self-invalidation. The re-run fired the previous
   run's cleanup, which cleared the `setTimeout` that was about to open the
   decoder. **The grid decoder therefore never opened, ever.** Loupe's equivalent
   flag is a plain `let` for exactly this reason; this one drifted.
2. **The decode path was gated behind the `liveScrub` setting** — the opt-in for
   building *sprites*, which decoding doesn't need. Default off, and described
   in the UI as a pre-build, so the feature was invisible.

Both fixed. Skimming now needs no setting and no pre-building. `liveScrub` was
renamed in the UI to **"Sprite fallback (pre-built)"**, which is what it actually
is.

### Release-notes process fix

He noticed nightly.5 and .6 both announced themselves as "nightly.4" in the
release body. Cause: `release.yml` pastes `RELEASE_NOTES.md` in verbatim, and the
file carried a hand-written version heading that went stale whenever it wasn't
bumped. **Rule now: no version heading in `RELEASE_NOTES.md`** — the tag is the
only version source.

### Answered along the way

- **Will this hold up on the XPS 13?** Unknown, and deliberately not guessed at.
  Chrome ships no software HEVC decoder, so it depends on that machine's iGPU
  generation (Intel gained 10-bit HEVC decode at Kaby Lake / 7th gen), which no
  doc records. The app now logs `scrub-engine OK|FALLBACK` per clip so the
  machine answers it itself.
- **Confirmed working on the main machine** from its own log: HEVC Main 10
  (`hvc1.2.4`), 3840x2160 **and** 1728x3072 portrait, 88–290 ms to index, zero
  fallbacks across ~24 clips.

---

## 2026-07-23 — why cast loaded once and then ignored everything

The previous night ended with a telling hardware report: the first Chromecast
video loaded, but laptop play/pause, seeking, and moving to another item did
nothing. The code confirmed the handover's race hypothesis. TCP and TLS had
already connected successfully, but the status object was initialized as
disconnected until a newly spawned actor thread sent its first two frames. The
main thread normally returned that false snapshot first. Unfortunately every
feature that could correct or use the session — follow, transport, and even the
status poll — was gated on the same false value.

The connection now becomes true when its synchronous TLS handshake succeeds,
and the frontend's recovery path follows the user's session intent rather than
making recovery conditional on the stale value it must recover. Frontend cast
decisions are also written to `foxcull.log`; before this, the absence of Rust
transport lines could not distinguish a frontend early return from an
inaccessible/stale log file.

The same pass revisited the owner's report that DualSense L2/R2 seeking felt
glitchy. The analog curve fired while a trigger was only 35% depressed, so an
initial skip was roughly 2.4 seconds before abruptly becoming 5-second seeks at
an eight-per-second repeat rate. That was mechanically inconsistent, not just a
matter of taste. Trigger behavior is now discrete and learnable: one pull skips
five seconds; a deliberate hold repeats after a half-second grace period.

Both changes pass local type, frontend production-build, and Rust compile gates.
Chromecast remains a hardware feature: the Sony TV test, not compilation, is
the point at which it can be called fixed.

### The local installer that compiled but could not start

At the owner's request, a one-off local Windows build was attempted despite the
usual CI-only release rule. Compilation and NSIS packaging both returned
success, but the installed app immediately failed because
`WebView2Loader.dll` was missing. The machine uses Windows-GNU; FoxCull's
supported GitHub Windows build uses MSVC. Tauri placed the loader beside the
raw GNU executable but omitted it from the installer, proving that compile
success and even a build-directory dependency check are not distributable
artifact tests.

The response is deliberately categorical: Windows-GNU artifacts are never
handed off. The release workflow now launch-smokes the MSVC executable before
publishing, requires FFmpeg, and verifies the portable ZIP contains it. This
also exposed that earlier portable ZIPs had copied only FoxCull's small
executable and omitted the 140 MB FFmpeg sidecar; that package path is corrected
in the same nightly.

---

## 2026-07-24 — the TV became the player, not a mirror of the laptop

The Sony-TV test of nightly.3 finally closed the original cast failure: moving
through Grid or Focus changed the TV, photos followed, and videos played. The
remaining flaw came from the control model. FoxCull still treated the laptop's
`<video>` as the authority and mirrored its events to Chromecast. With local
autoplay off, the first Space therefore started the laptop and only the second
Space generated the pause the already-playing TV needed. With autoplay on, both
screens played audio.

Cast mode now has one authority: **the receiver**. The laptop stays paused and
muted; Space asks the TV to toggle based on the TV's own reported player state.
Relative seeking is calculated from receiver time, not a parked local playhead.
Those controls are intercepted before Grid navigation, so Space and
Shift+Left/Right work without entering Focus, and controller controls share the
same path.

There was one narrow race worth solving rather than documenting: immediately
after navigating, the receiver has not yet minted the new clip's media-session
id. Commands in that gap are queued and delivered in order once its first status
arrives. The old session is invalidated as soon as LOAD is received, preventing
a fast pause from accidentally controlling the previous clip.

The TV's two-second filename card was self-inflicted optional metadata, so that
metadata is no longer sent. A glowing CASTING pill now makes the session and its
Live/Loading/Paused state unmistakable in the laptop UI.

---

## 2026-07-28 — when Chrome found the TV but FoxCull did not

The owner's two Windows laptops made the problem unusually clean. FoxCull
always found both Cast receivers on the Alienware and found neither on the XPS,
even with the laptops side by side. YouTube in Chrome on the XPS immediately
found both. The Wi-Fi, dual-band SSID, TV, and soundbar were therefore not the
missing piece; FoxCull's own discovery route was.

FoxCull had exactly one route: open the standard mDNS listener on UDP/5353 and
wait for multicast replies. Windows can permit that inbound traffic for Chrome
while dropping it for a different unsigned executable. Because the discovery
library initializes its network sockets on a background thread, FoxCull also
could not distinguish a blocked listener from a real empty network and simply
said no devices were found.

The app now asks twice in parallel. It keeps the standard listener, and also
sends the same Cast DNS question from a temporary outbound port on every active
IPv4 adapter. Cast receivers return that form of question directly to the
temporary port, which Windows treats as a reply to outbound traffic instead of
unsolicited inbound multicast. A real probe to the Sony receiver confirmed
that it supports this route. Results from both searches are merged, and the log
now says which route worked and which adapters or sockets failed.

---

## 2026-08-01 — the interface became one designed product

The owner made the scope unusually explicit: functionality was already nailed;
the new job was to audit every surface, menu, control, container, state and
workflow, then make FoxCull look like professionally made software in August
2026. The same app also needed to remain useful on the Alienware, the smaller
XPS, and a 65-inch TV viewed from six feet away over HDMI.

The important design decision was not to turn this into a different workflow.
The Library / Focus / Edit model, keyboard language, filmstrip docks, culling
marks, cast behavior and file operations have all been proven in real use. The
visual refit therefore builds a system under them: semantic surfaces, quiet
neutral stage, consistent type and iconography, coherent selected/active/pick/
reject states, and one depth language for every floating menu and modal.

Four themes now have jobs rather than just colours: Studio is neutral graphite,
Midnight is a cooler deep-black room, Amber is for low-blue late work, and
Daylight is for bright rooms. The actual media stage remains neutral and dark in
all four, because the surrounding chrome must not tint colour judgement.

The large-display requirement became a real product feature rather than a note
about Windows scaling. Interface size is persisted as Compact, Standard or
TV-large. It scales the whole workstation proportionally, and TV-large forces
the command bar into its two-row contract so zooming cannot push Settings or
destructive actions off the right edge. Narrow windows use the same contract.

The old first-run screen said what to do; the new one feels like the beginning of
a product. The folder tree carries FoxCull's identity and active context. The
grid, Details, Focus video overlay, Edit panes/timeline/inspector, culling footer,
settings, Trash, controller guide, progress and menus now look like parts of the
same tool. The full workflow/permutation/surface map lives in
`docs/UX-AUDIT-2026-08.md` so future polish does not begin from screenshots and
memory again.

Before this design line began, the exact latest nightly commit was promoted as
`v1.2.1` stable at the owner's instruction. Fresh Windows/macOS/Linux artifacts
passed CI; the visual work starts after that immutable stable point.

---

## 2026-08-01 - the polish pass got a recovery pass

The first visual nightly exposed the sort of small regressions that make a
finished product feel unfinished: an old native icon beside the new identity,
version noise in the title, a home-made refresh glyph, menus slipping under
tiles, and Edit modes with no visible route back. Those are now treated as one
system rather than isolated screenshot fixes. Native and in-app branding share
one icon; floating surfaces own an explicit stacking layer; every collapsible
or temporary Edit state has a visible recovery action; and the laptop toolbar
wraps before the folder tree can squeeze important controls away.

The performance work stayed narrow. FoxCull already virtualized the grid and
generated thumbnails asynchronously, but six simultaneous video-poster jobs
could still contend for disk and decoder time. Heavy jobs now have their own
two-slot budget while lighter images continue through the shared queue. The
filmstrip also converts wheel input into a clamped, eased target instead of
applying every device delta as an abrupt jump.

---

## 2026-08-01 - the fox was the identity, not the artifact

The first recovery pass misunderstood a two-icon screenshot. The native orange
fox with its film strip and green/red dots was the mark the owner wanted to
keep; the adjacent square/screen symbol was the obsolete artifact. Because Git
retained the exact original master, the established fox could be restored
without approximation. It now generates every native platform icon and the
in-app favicon, making the intended identity consistent everywhere.

---

## 2026-08-01 - a restore control hidden in plain sight

The folder explorer had not lost its restore button or state transition. The
button was simply below the command bar in the stacking order after the menu
layering repair made that bar opaque and isolated. That is a useful distinction:
checking that a control exists is not enough when it can still be visually
unreachable. The restore button now owns a layer above the bar, and the bar
reserves its footprint when the tree is collapsed. The same existence,
stacking and reachability audit was applied to every other collapsible surface.

---

## 2026-08-01 - the loader was idle because painting had failed

The black Grid initially looked like another thumbnail backlog, but the live
log showed the opposite: the UI heartbeat was healthy, memory was flat, and the
thumbnail queue had no pending or running work. The URLs already existed. That
evidence shifted the investigation from Rust and decoding to WebView painting.

The design pass had unknowingly nested two whole-application scale transforms,
including two nominal `scale(1)` compositor layers in Standard mode. The next
performance patch placed each transformed virtual cell in its own paint
containment boundary. WebView2 could flash those layers while scrolling and
eventually stop painting them, while JavaScript and the backend continued
normally underneath.

FoxCull now leaves Standard mode entirely untransformed and uses only one root
transform for the two opt-in interface sizes. Cell paint containment was
removed. Virtual scrollers also retain primitive cell identities, and thumbnail
state listens to a stable path/size key rather than every parent scroll update.
Visibility observation remains only where it adds value: large unvirtualized
lists, never a grid that has already virtualized its own mounted cells.

---

## 2026-08-02 - scrolling stopped waiting for the next frame

The first render recovery removed invalid compositor pressure, but the owner
could still strand the Grid after two quick pages. The remaining weak link was
older than the visual pass: virtual-range updates were gated by one outstanding
animation-frame callback. WebView2 could continue compositor scrolling while
that callback remained pending, leaving the DOM populated with cells for an old
position. Timers and the backend stayed alive, which is why the memory heartbeat
looked healthy while the screen stayed blank.

Virtual ranges now follow native scroll events synchronously and verify the
settled DOM position on a short timer. Smooth filmstrip movement is handed to
the browser's scroll compositor rather than driven by a JavaScript frame loop.
The apparently frozen thumbnail progress had its own smaller cause: an
all-canceled queue was drained but never marked done because it had zero
completed items. A drained queue now always closes its activity.

A 6,825-cell stress harness exercised the shipped virtual components through a
full 120-step descent and twelve alternating end-to-end traversals. It retained
the correct populated range at both extremes every time, then was removed from
the product tree.

---

## 2026-08-02 - the range was correct but the GPU surface was not

Nightly.6 proved that fast scrolling could still make the installed application
look crashed even when the virtual Grid had moved to exactly the right rows.
The new range log reported the correct cells after a large jump; the JavaScript
heartbeat, thumbnail queues and processes all stayed alive. This corrected the
previous diagnosis: a stale range was no longer the active failure.

The remaining pressure was in presentation. Each mounted virtual tile was
positioned with a transform and explicitly marked `will-change: transform`, so
rapid scrolling continuously created and discarded more than a hundred image
compositor layers. Those transforms never animated and did not need promotion.
Grid, grouped Grid and filmstrip tiles now use normal absolute coordinates.
Virtualization still limits DOM work and thumbnail loading remains asynchronous,
but WebView2 no longer has to churn a GPU layer per visible media item.

---

## 2026-08-03 - the freeze was JavaScript backpressure, not the GPU

The owner supplied the observation that finally separated the layers. During a
freeze, native scrolling, CSS hover highlights and tooltips still worked, while
clicks, selection movement, tile population and the loading chip did not. Those
first behaviors do not require JavaScript; the second group does. The main
thread was blocked and the compositor was healthy. That also reinterpreted the
rAF gap correctly: both rAF and timers stop during a long JavaScript task, then
resume later.

The v1.2 loader had started publishing reactive progress for every thumbnail
entering, leaving or finishing the queue. Later attempts to protect fast scroll
deferred those requests, but the settle path recursively released every cached
hit in one turn. The result was a compound burst of progress re-renders, promise
callbacks and image assignments — plus the downstream memory/handle spike that
had been mistaken for the cause.

The loader is now paced like a renderer. It cancels by key in constant time,
settles every dropped request, caps live work, and releases at most one grid row
of cached assignments per paint. The loading chip is an indeterminate start/end
signal instead of a percentage whose denominator changes while scrolling.

On the real 6,825-item library, 800 rapid three-row jumps produced no paint gap;
the queue remained bounded, drained to zero, tiles populated, the activity chip
cleared, and selection responded immediately. Grid and Focus live scrubbing
were preserved throughout.

---

## 2026-08-03 - one selector across mouse and keyboard

A mouse-clicked media button kept DOM focus after FoxCull moved its active item.
The first arrow press caused the global keyboard focus outline to appear on that
old button while the real active border moved, creating two visual highlights
even though the selection set already contained only the new item.

Arrow navigation now releases stale focus only from grid, filmstrip and Details
media cells before moving. Native click, Right and Down testing showed a single
selector following the active item; intentional modifier-based multi-selection
and ordinary control focus remain unchanged.

---

## 2026-08-04 - v1.4.0 stable, and the catalog stops losing photos

The owner confirmed from real work that the 1.4.0 nightlies had resolved the
fast-scroll freeze, and asked for the latest nightly to be promoted. v1.4.0
shipped as stable with **no code change over nightly.8** - the tagged commit
carries only the stable release notes and the base-version bump. That is
deliberate: the build he tested is the build that shipped.

He then asked for two features, both of which are really about the same thing -
metadata that survives contact with the real world.

### Moving photos used to quietly destroy their marks

FoxCull keys every rating, label, flag, tag and trim by the file's path relative
to the drive. That is what makes a drive's catalog portable between machines,
and it is also what made a move in Explorer catastrophic: the row stayed, the
path stopped resolving, and the marks were effectively gone with no signal that
anything had happened. The owner named the model he wanted, and it was the right
one - Lightroom's. Flag what you cannot find with a "?", never delete it, work
out for yourself when a whole folder has moved, and let me remap the rest by
hand. He was explicit that he is happy to wait ten or fifteen seconds at launch
for this, the way he already does in Lightroom.

The shape that fell out of that:

- **Verify, then hunt - in that order, because the order is the cost model.**
  Pass one only asks the filesystem whether each metadata-carrying path still
  exists. If nothing is missing - the overwhelmingly common case - the command
  returns without ever walking the library, and the promised ten seconds cost
  a few hundred stats instead. The walk is the expensive thing, so it is the
  thing that has to be conditional.
- **Folders move, not files.** Matching filename-by-filename would have been the
  obvious implementation and the wrong one: it is ambiguous exactly where people
  have duplicates. So absent entries are grouped by the folder they used to live
  in, and a folder is considered moved when at least half its filenames turn up
  together under one new directory. Only the leftovers fall back to an
  individual match, and only when that match is unique.
- **Never adopt a file that already has metadata.** Reconnecting one photo must
  not overwrite another photo's marks to do it. This is the guard that makes
  automatic relinking safe enough to run unattended on launch.
- **A scan never deletes.** Anything unresolved is flagged and rendered as a "?"
  tile that still carries its full marks. Exactly one path in the app deletes
  metadata for a missing file, and it is the explicit "Forget" action behind a
  confirmation.

The other half of the ask was the everyday path: he wanted to move photos *inside*
FoxCull so this problem does not arise. Dragging a selection onto a folder in the
left pane already moved the files and their metadata together - that was built
earlier and worked. What was missing was somewhere to drop them, so the tree can
now create subfolders, and creating one with files already staged moves them
straight in.

### Events

The second feature is a virtual collection sitting at the same level as tags -
"Monar trip", "Rashi's birthday" - and the important word in the request was
*virtual*. It is emphatically not a folder. The photos stay wherever they are
filed; the event is metadata, so a trip whose shots are scattered across ten
subfolders still renders as one block.

Grouping by event was therefore the natural home for it: the grid already
sections by folder, type and date, and an event is just another section key. Two
things needed care. Blocks needed to be orderable by name *or* by date in both
directions, but the grouped sort compares section keys directly and never applies
the sort direction to them - so the direction is baked into a computed rank
instead of applied at compare time. And unassigned shots needed to reliably trail
every real block, which under a numeric collator means parking them at a rank no
real event can reach.

Visually he asked for something closer to how Google Photos fronts an album than
to a text header, and he was right that it changes how a long recursive feed
reads: each event gets a cover band with the frame, the name, the date range and
the count. The cover is whichever member was chosen with "Use as cover", or the
first one otherwise.

Events were deliberately left out of the undo stack, like tags' own menu actions.
The stack snapshots marks, every event action is one menu click to reverse, and
widening the snapshot shape for this would have been a lot of machinery for very
little.

---

## 2026-08-04 (evening) - the freeze that was a different freeze

The owner clicked `D:\` - by accident, he only meant to expand it - and the whole
window went "Not Responding" for minutes. Coming days after the 1.4.0 scroll-freeze
work, the obvious read was "it's back". It wasn't, and the thing that proved it
took one command: the native `foxcull.exe` was `Responding=False` while every
WebView2 child process was `Responding=True`. The 1.4.0 freeze had exactly the
opposite signature - a jammed JavaScript thread with a healthy native process.
Same word from the user, different bug, and the fingerprint separates them in
seconds. That is worth more than any amount of reasoning about what "froze" means.

The cause was a threading model, not an algorithm. A synchronous Tauri command
runs on the main thread, which is also the thread that pumps the window's
messages, and the folder walk was synchronous. On any real folder that is
invisible - 8,403 files on `F:\` in 390 ms. On `D:\` it descended into
`node_modules`, a shared cargo target directory and a Steam library, and took the
window with it. Nothing was wrong with the walk; it was on the wrong thread, and
only a pathological folder ever revealed that. Three other commands had the same
latent defect and were converted at the same time.

Beyond just moving it off the thread, the owner's framing was the useful part: he
did not ask for it to be fast, he asked for it not to trap him. So the scan is
now abandonable - click another folder and the old walk is dropped - it reports a
live file count instead of sitting mute, the activity bar lifts and highlights
itself while it works, and the scanning screen says in plain words that you can
go elsewhere. Slow is acceptable when it is honest and you are not stuck in it.

### The audit that should not have found anything

He also asked for a fresh start: delete the `_FoxCull` folder on every drive,
since he had done no culling and his Trash was empty. He added "unless you think
something is important". Checking was meant to be a formality.

Two drives were holding real media in their recycle folders with no row in the
`trash` table: an 18 GB merged Dubai-trip clip on E:, and 22 files on P: of which
8 were DJI Mavic Mini clips totalling about a gigabyte. None of the originals were
back on disk. The Trash panel showed nothing because it lists the table, not the
folder - so the files were invisible, unreachable by Restore, and about to be
deleted with his blessing, based on a screen that was telling him the truth about
a database and nothing at all about his disk.

The immediate fix is that the Trash now reconciles the folder against the table
and adopts whatever it finds, reconstructing where each file came from. But the
lesson generalises past this panel, and it is the same one behind the catalog
integrity work earlier the same day: when a view renders a catalog and the user
reads it as the filesystem, the two have to be reconciled or the view is quietly
lying. How the rows went missing in the first place is still unproven.

---

## 2026-08-04 (late) - the washed-out still, and a theory killed before it shipped

The owner reported that a video in Focus view looked "very washed out" until he
pressed play, at which point the colour snapped back - and, tellingly, did not go
washed out again when he paused. He wondered whether it was a half-built paused
state.

That last detail is the whole diagnosis. An HTML video poster is shown only until
the first frame arrives and never returns, so "correct after play, never washed
out again on pause" locates the bug in the still image and nowhere else. Not
playback, not CSS, not a paused state. The symptom was more precise than any
amount of code reading would have been.

The first theory was still wrong. Washed-out video stills are almost always a
limited-versus-full range mismatch, and that is where the investigation started.
Testing it took one command: adding the range conversion produced a byte-identical
file, because swscale already handles range on the hop from yuv420p to yuvj420p.
The theory died in about a minute rather than becoming a plausible-looking commit.

The real cause was a label. His footage is BT.709; JPEG means BT.601; and ffmpeg's
mjpeg encoder tags its output BT.601 without converting the coefficients. FoxCull
was handing the webview BT.709 pixels wearing a BT.601 label, and the webview
believed the label. Asking swscale to genuinely convert instead measured 41.19 dB
against an RGB ground-truth decode where the old path managed 38.36. The grid
tiles had been wrong all along too - a saturation shift just doesn't read at
176 pixels.

What was deliberately not done is worth recording: HDR clips extracted without
tone-mapping will still look flat, and worse than this did. Every clip reachable
on this machine was SDR, so there was nothing to verify a tone-map chain against,
and shipping an unverified filter chain to fix an unobserved case is how you
acquire a bug you cannot reproduce.

He also asked, reasonably, for a written explanation of the three features that
had just landed - events, moving files, relinking - because he wanted to plan his
workflow around them rather than discover their edges by losing something. Writing
that document was more useful than writing it was comfortable: it forced an
explicit list of what does not work. Move and rename in the same pass is
unrecoverable. Marks do not cross drives, because catalogs are per drive. And
"Forget" on a disconnected drive would cheerfully delete the marks for every file
that is merely unplugged. None of those are new bugs; all three were invisible
until someone had to write them down for a user.

## 2026-09-27 — The Mac that scanned itself

The owner got FoxCull running on a Mac and asked two sensible things: a real
welcome screen for the first open, and a guarantee that clicking the system drive
would not crawl and cache the operating system. Then, mid-session, a better
version of the second one: a proper exclude list in Settings, with system folders
pre-ticked and the user's own additions kept on the machine.

The Mac's log had already told the story. A month earlier, one click on Macintosh
HD had walked 241,138 files in 71 seconds and left 234 MB of cache behind: a
catalog full of capture dates for app icons, and not a single rating. On the
morning of this session, the app relaunched into the same drive and started the
walk again. The build he had installed was tagged one commit before the Mac
skip-list fix, so it had no defence at all.

The interesting decision was where a rule applies, not which names go on the list.
The earlier fix skipped `library`, `system` and `windows` wherever they appeared,
which is right for `C:\Windows` and wrong for a folder of photos of windows, or
an SSD whose shoots live in `Library`. The rules are now tied to a place: OS
folders only at the top of the system drive, `Library` only inside a home folder,
and only the unmistakably machine-made names (`node_modules`, `AppData`, `.app`
bundles, game libraries) everywhere. The same Mac drive now walks in about nine
seconds. What's left is almost entirely his coursework image datasets. Those are
real photos, and deciding to hide them is his call, which is what the exclude
list is for.

Two smaller truths surfaced along the way. On macOS every external drive had
been quietly filing its catalog and thumbnails on the Mac instead of on itself,
because the lookup took the first drive that matched and `/` matches everything.
And the release notes for the last two nightlies had shipped with the text from
the one before, so nobody reading the release page had been told about the
update checker or the signing fix.

## 2026-09-27 (later) — Measuring the overlaps instead of looking for them

"The layout overlaps across multiple touchpoints" is a hard bug report to close
by eye. A screenshot catches what you already suspect. So the audit started by
making FoxCull run in a plain browser with a pretend library behind it, then
wrote a probe that measures every piece of text and every control on screen and
reports any two that collide. It opens each view, menu and dialog in turn, at
every window size the owner's machines produce.

Most of what it found came from a handful of causes rather than dozens of
separate bugs. The one behind the Settings overlap the owner had seen is a
browser layout rule: a scrolling list is allowed to squash its rows. On the
Windows laptop the Settings panel fit, so nothing was squashed. On the Mac's
shorter window it didn't fit, and the two-line Theme row was crushed onto the
line below. The same rule was crushing the clip list in the Edit studio. Menus
anchored to the left of their buttons ran off the screen once the toolbar
wrapped. The Edit studio's fixed side panels left the preview a sliver, and its
toolbar painted over its neighbour.

The surprise was underneath all of that. The TV and Compact interface sizes, a
pillar of the August redesign, had never worked. They scaled a wrapper element
the framework renders with no box at all, so there was nothing to scale. They
now use the window's own zoom, the same thing a browser does on Cmd-plus, and a
real build on the Mac confirmed the numbers: TV on a 1280-pixel window lays out
at exactly 1049.

What stayed out of scope is written down too. None of it was run on Windows,
where the zoom goes through a different engine. And the controller pairing
guide still tells a Mac user to open Windows Bluetooth settings: that's a
copy problem, not a layout one.

## 2026-09-28 — The YouTube workflow gets its own door

The owner described FoxCull as two apps living in one. The Edit studio was built
for Instagram: crop a landscape clip to portrait, trim it, grade it, add music,
export small and sharp. The other job is simpler and bigger: come home from a
trip with an hour and a half of Osmo Pocket 3 footage, join it into one file,
and put it on YouTube for the family, without re-encoding, because there's no
time and the files are enormous. That second job had been squeezed through the
timeline, and the timeline is the wrong tool for it.

Before designing anything, the actual clips were examined, and they changed the
design. One Seattle folder held five different kinds of video: 59.94 fps
10-bit, 29.97 fps 8-bit, a 23.98 fps clip, vertical, and square. A lossless
join only works when every clip matches, and the Edit export's existing check
looked only at frame size and codec, so it would have happily glued 60 and 30
fps footage into a file that stutters. So the merge dialog reads each clip's
full signature, groups the matching ones, and leaves the rest out with the
reason spelled out, the way LosslessCut does.

The owner then added the part that matters on a 512 GB laptop holding 97 GB of
footage: tell me the size before you start, and let me send it to an external
drive. The dialog shows the estimated length and size up front, lists every
drive with its free space, and won't start where the file can't fit. In the
same spirit, selecting a batch of clips now adds up their count, length and
size in the bottom bar, like a spreadsheet's status line. Tiles also show each
video's length, read from the file header in a fraction of a millisecond, so
fifty-seven clips cost twenty milliseconds.

One wish stayed a wish: uploading straight to YouTube without writing the file
locally. It can be done, but it means a Google developer project, sign-in
screens and a daily upload quota. That's a project of its own, and saving to an
external drive solves the space problem today.

## 2026-09-30 — clips that don't match: glasses footage and "Convert to match"

The owner came back from a day in Seattle with twelve clips from Meta's new
glasses and the merge window wouldn't take seven of them. Their questions were
fair: is that ffmpeg, or just FoxCull? And why is "Vertical" highlighted when
every clip is vertical?

Both halves had an answer. The frame-rate flags were FoxCull being too strict.
Glasses and phones record a variable frame rate, dropping frames when the light
is low, so a 30 fps clip averages 29.73 or 29.94. A lossless join carries every
frame's own timestamp, so those join cleanly, and a test join proved it. The
"Vertical" flag was right but badly worded. The glasses crop every clip a little
differently, from 1376×1824 to 1488×1984, and a lossless join of different sizes
really does break: after the join the picture turns to green garbage. The column
said "Vertical" when it should have said the size.

So the window now shows real sizes and compares frame rates the way a person
would. For clips that can't be joined losslessly, the owner can choose
"Convert to match": every clip is re-encoded to one size at twice the camera's
bitrate, which on a Mac's hardware encoder takes a few minutes and looks the
same. The obvious way to build that, one ffmpeg run fed all twelve clips, was
tried first on the real footage. It produced a file with three minutes of frozen
picture spread across six of the joins. What shipped encodes each clip on its
own and then joins the results with the same lossless join the app already
trusts, after checking that every part came out of the encoder with identical
settings.

## 2026-10-04 — a progress panel worth the name, and drags that cross drives

The owner listed a batch of fixes to ship before reworking the Edit module.
Dragging a selection onto a folder floated the whole selection under the
pointer; they wanted a small stack with a count, and when the drop meant
copying to another drive, progress with a time estimate inside FoxCull. That
led to the bigger ask: the little progress strip at the foot of the sidebar
should become a real panel for everything FoxCull does in the background,
several jobs at once, clear about what each is doing and when it will finish,
and the merge window should be able to get out of the way while it works.

Looking at moves turned up something worse than a missing progress bar. A
move from the external SSD to the Mac was refused outright, and a move the
other way "worked" but filed the photos' stars and labels in the Mac's own
catalog, because on a Mac every external drive's path starts with `/`. Moves
now pick the destination's catalog by drive, copy with progress, check each
copy before deleting the original, and carry the marks across. That was
tested on a real second volume, not just in the browser.

The owner also asked why merges took a minute when LosslessCut "does it in
five or ten seconds". Measured on their own Osmo clips from the SSD, FoxCull
was the faster of the two (LosslessCut rewrites the whole file a second time by
default). Five seconds is what a few gigabytes on the internal disk takes; their
merges were 15 to 70 GB. The panel now shows the speed, so the slow drive is
visible instead of the app looking slow.

Smaller asks in the same batch: missing files cleared per folder or per
selection rather than one right-click at a time, Lightroom's "find one, find
the rest" when relinking, grouping by day for trips, and a green or red tab on
picked and rejected tiles. Asked whether Prepare was still needed, the answer
was mostly no: Focus already readies the next few photos as you move, so the
button went from the toolbar to the folder menu, where it still helps on slow
cards.

The same day the owner described the Edit rework: Edit and Merge as their own
windows beside the library, the library as the only media picker, in/out
points set there and dragged onto the timeline. That is the next nightly; this
one ships fixes only.

## 2026-10-04 (later) — Edit and Merge leave the library

With the fixes shipped, the owner turned to editing. Edit used to take over
the library window, with its own media list down the side that "just messes
things up". Now it is a window of its own beside the library: the library is
where clips are found, rated and trimmed, and they go across by dragging, by
pressing E, or by copy and paste, with any in/out ranges marked in Focus
arriving as separate pieces. Merge, the YouTube workflow the owner relies on,
got its own window too, and the thing they asked for most: start a merge,
close the window, keep working, pause it or stop it from the progress panel.

Two decisions shaped it. The merge now belongs to the app's backend, not to a
window, so a window can come and go without the merge noticing; and quitting
FoxCull asks first and then cleans up, because a merge's ffmpeg used to keep
writing after the app had gone. The same day the owner described what comes
next: segments in Merge, a cleaner way to mark them, and a third, music-synced
reel mode (`docs/design/segments-and-reel-mode.md`).

## 2026-10-04 (evening) — segments, and reels cut to the beat

The owner's "major item" came as one long voice note, and it rests on one
idea: the library is where clips are chosen and trimmed, for all three ways
of making something (Merge for YouTube, Edit for anything careful, and a new
Reel mode for Instagram). So trimming had to get good first. In Focus, `[`
and `]` now build several pieces of a clip, the way the owner described
pressing them while a clip plays; the pieces can't overlap, they show on the
scrub bar, and their edges drag a frame at a time.

Merge then learned to use those pieces: tick a clip and only its marked
parts go into the YouTube file, untick it and the whole clip does, since
pieces are sometimes marked for an Instagram cut instead. Doing that without
re-encoding took some measuring: two obvious ways of cutting gave files whose
timestamps ran backwards at the joins, and a third (cutting each piece to a
transport stream first) joined cleanly on a real Osmo clip. The catch, which
the window states, is that a lossless cut starts on a keyframe, up to half a
second early on the Osmo; Convert cuts exactly.

The Reel window is the new part. The owner wanted Instagram's "sync to
music" without its habit of cutting a ten-second clip to two seconds because
a beat happened to land there. FoxCull finds a song's beats and bar starts
itself, spreads the clips over the chosen part of the song cut on bar starts,
and shows every clip as a strip of frames with a window the owner can slide
or stretch to another beat, the rest of the reel re-flowing behind it. A clip
too short for its window shows the gap in red. The beat finder was written
here rather than pulled in, and is tested on drum-machine tracks; how it does
on the owner's real songs is the next thing to learn. Cropping landscape
clips by hand is left for later, as the owner suggested.

## 2026-10-04 (night) — a timeline that played one clip forever

Testing the new nightly, the owner laid clips out on alternating tracks in
Edit and pressed Play: the first clip played, then looped, and the playhead
never moved on. The cause was small and general. A clip's length comes from
the file's container, and phone clips often stop a few milliseconds before
it. The player waited for an end it never reached, and asking an ended video
to play starts it over. An ended clip now simply counts as finished. Snapping
had a related problem: it reached a fixed sixth of a second, which on screen
was a few pixels, so clips never seemed to snap; it now reaches ten pixels at
any zoom and works from either end of a clip.

## 2026-10-04 (night) — Settings, as one place

The settings popover had grown one row at a time to twenty-two, past the
bottom of a laptop screen, with what each row did hidden in hover text. The
owner asked for it to be rethought as a whole, and for Prepare to come back
to it. Settings is now one sheet with a sidebar, like the operating system's
own: six sections, cards of rows that each say what they do, switches for
on/off, a search that finds a setting by the word you'd use, and the panels
it used to open on top of itself (excluded folders, the controller, updates)
as pages inside it. Prepare sits in "Speed & storage" with its progress and
the size of the drive's preview cache beside it, so the question "is it worth
it here?" has its answer next to the button.

## 2026-10-04 (late) — the filmstrip on a Mac

The owner found the filmstrip's scrolling odd on the Mac. It had been tuned
for a Windows mouse with a thumb wheel, whose sideways direction comes in
reversed, and it smoothed every wheel event into a glide. On a trackpad that
reversed a sideways swipe and made the strip trail the fingers. On a Mac the
strip now leaves sideways swipes to the system and maps up/down swipes 1:1.
