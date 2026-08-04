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
