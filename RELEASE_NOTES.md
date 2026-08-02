<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## Fast scrolling no longer floods the loader (the last freeze cause)

Live measurement pinned the remaining freeze to thumbnail *loading*: a hard fling
fired a burst of image fetches at the display engine — exactly what you saw as the
"Loading thumbnails" bar racing (and even going backwards) right before it locked
up. Now, while you're flinging fast, FoxCull **holds new thumbnail loads and runs
them the instant you stop.** Tiles already on screen keep their picture; new tiles
fill in the moment you settle (cached ones appear instantly). Gentle, normal
scrolling is unchanged and loads as you go. This is on top of the recycled grid
engine and the two earlier freeze fixes.

## Rebuilt the grid engine — the scroll freeze is fixed at the root

The freeze was never a patch-able glitch — it was how the grid was built. Every
time you scrolled, the app **threw away and rebuilt every visible tile**, and a
fast scroll rebuilt hundreds of them in a burst that overwhelmed the Windows
display engine until it locked up (seconds of black screen, sometimes needing a
relaunch). Earlier builds tried to soften that burst; this one removes it.

The grid, the grouped grid, the Focus filmstrip and the Details list now all use
the same modern **cell-recycling** engine — the standard technique behind
professional data grids. A fixed set of tiles is *reused and repointed* as you
scroll instead of being destroyed and recreated. Nothing is rebuilt in bulk, so:

- **Fast scrolling no longer freezes** — flick as hard as you like, in the grid
  or the filmstrip.
- **No more blank-and-reload flash.** Tiles that stay on screen keep their
  picture; only genuinely new tiles fill in. The placeholder/refresh behavior from
  the last build is gone.
- It's simpler and lighter under the hood — less work per scroll, not more.

Everything you rely on is unchanged: the on-disk thumbnail cache, RAW/HEIC
handling, the click-to-arm hover scrubbing on videos, and folder-switch
cancellation. The idle/on-launch freeze fix from the previous build is included.

## Freeze fixes — two root causes, now measured

A live debugging session with detailed cross-process monitoring pinned down two
*separate* causes behind the freezes, and this build addresses both:

- **Frozen on launch / while idle** (the window opens or sits there dead, but the
  app is actually running and has to be relaunched): this was Windows' WebView2
  wrongly deciding the window was hidden and switching its display off. It's now
  turned off, so the view keeps painting.
- **Frozen after a fast scroll**: a hard fling flooded the display engine faster
  than it could keep up, and it stalled for many seconds (sometimes for good).
  Fast scrolling now shows light placeholders while you're flinging and snaps the
  real thumbnails in the moment you settle — so the display engine is never
  overwhelmed. Gentle, normal scrolling is unchanged and stays fully live. This
  now also covers the bottom filmstrip in Focus, not just the main grid.

This build also records much more detail to `foxcull.log` and can no longer lose
its log when the app is relaunched quickly — so if anything still slips through,
the cause is captured.

## The fast-scroll freeze

If you flung the Grid down quickly — a hard mouse-wheel throw or a held ↓ arrow —
the viewport could go black and the whole app stop responding until you relaunched
it. Gentle scrolling was always fine; only fast motion tripped it. This build
targets that directly.

- While a genuinely fast scroll is in flight, the Grid now shows lightweight
  placeholders and paints the real thumbnails the instant you stop. That keeps
  the burst of image work from overwhelming the display engine, which is what was
  wedging the window. Ordinary monitoring scroll is unchanged — it stays fully
  live, images and all.
- This is the same for both the plain Grid and the month/type-grouped Grid.
- This build also writes extra diagnostics to `foxcull.log` so that, if any freeze
  still slips through, the log now shows conclusively whether the display engine
  stalled while the app itself kept running. If you can reproduce a freeze on this
  build, that log is exactly what pins the remaining cause.

## Interface recovery and smoother browsing

- Fast Grid and filmstrip scrolling no longer promotes every mounted media tile
  to a separate GPU transform layer. Virtualization and asynchronous loading
  are unchanged, but rapid wheel movement cannot exhaust WebView2's compositor
  and leave an otherwise responsive application displaying a black viewport.
- Fast Grid scrolling now synchronizes the virtual range directly from native
  scroll events, with a timer-backed final-position check instead of depending
  on an animation-frame callback. The thumbnail activity chip also closes when
  a fast scroll cancels an entire queued batch.
- Grid and filmstrip rendering no longer pass through nested whole-app
  compositor transforms or per-cell paint containment. Loaded images remain
  stable during smooth scrolling, and virtualized cells load directly instead
  of depending on a second visibility observer that could leave the view black.
- Collapsing the folder explorer now leaves a clearly visible restore button
  above the command bar, with space reserved so it cannot cover View controls.
- FoxCull's established orange fox, film strip and green/red status dots are
  restored as the single identity across the Windows title bar, executable,
  installer and in-app chrome. The unrelated square/screen mark is gone.
- The native title is simply **FoxCull**, without build metadata competing with
  the product name.
- Arrange and Filters reliably float above the media grid again.
- Edit always exposes a **Media** recovery control after its picker is hidden,
  and Preview now has a visible **Back to edit** action instead of relying on
  Escape.
- The refresh action uses the same balanced SVG language as the rest of the
  command bar.
- Horizontal filmstrip input is direction-corrected for precision mouse wheels
  and eased to remove abrupt stepping.
- Grid thumbnail work is throttled so video poster extraction cannot saturate
  every loader slot while scrolling; virtualization and asynchronous loading
  remain intact.
- The responsive command bar switches to its two-row layout earlier, keeping
  Library, Edit and Settings reachable on laptop-width workspaces.

## A complete visual refit

FoxCull now looks and behaves like one coherent professional media workstation.
The Library, Focus player, Edit workspace, folder tree, details table, filmstrip,
menus, settings, dialogs, Trash and controller setup share the same surface
hierarchy, spacing, type, corners, shadows and interaction states.

The media stage stays spectrally neutral in every theme. Chrome is layered with
quiet luminance and depth instead of colour that could bias a photo or video
judgement.

## Four purpose-built themes

- **Studio** is the refined neutral graphite default.
- **Midnight** is a deeper, cooler black workspace.
- **Amber** reduces blue light for late-night sessions.
- **Daylight** is a crisp light workspace for bright rooms.

Each theme now carries the whole semantic palette: selection, focus, hover,
pick, reject, ratings, labels, stacks, borders, overlays and floating surfaces.

## Built for the XPS, Alienware and the TV

Settings now has three persistent interface sizes:

- **Compact** recovers canvas room on the XPS 13.
- **Standard** is tuned for normal laptop and monitor use.
- **TV / large** enlarges the entire interface for a distant display or a
  65-inch TV over HDMI.

The command bar no longer clips its right-hand controls. It compacts at laptop
widths and becomes a calm two-row bar in narrow windows or TV-large mode, keeping
Settings, casting and destructive actions reachable.

## Library polish

- The folder panel has a real FoxCull identity and shows the active folder.
- Grid, Details and Focus use one SVG icon language.
- Arrange and Filters are easier to scan; active filter counts are separate,
  high-contrast badges.
- Media tiles feel like real objects: cleaner focus/selection, better stack
  treatment, neutral image wells and less visual noise.
- The bottom culling bar has a stronger filename/metadata hierarchy, a contained
  rating control, quieter tags and explicit pick/reject totals.
- The first-run screen is now a purposeful launch surface with an Open Folder
  action and the four keys that matter first.

## Focus and Edit polish

- Focus keeps a true near-black reference surround. Video transport and clip
  tools use a legible glass overlay over any frame, with a cleaner scrub track
  and metadata card.
- Edit has clearer separation between Source, preview, Timeline and Look. Source
  cards, tracks, clips, preset groups, export menus, restore tabs and the export
  dialog now belong to the same design system as Library.
- Trash and controller setup use full-workspace modal surfaces with clearer
  headers, cards and selected states.

No culling, playback, export, cast or file-management behavior was intentionally
changed in this pass. The work is a visual and responsive refit over the proven
workflow model.

## Verification

- Visual QA: 1440×900, 1366×768 and 1024×768, plus TV-large scaling.
- `svelte-check`: 0 errors, 0 warnings.
- Production frontend build: passed.
- Rust `cargo check`: passed.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use “More info” → “Run anyway”.
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
