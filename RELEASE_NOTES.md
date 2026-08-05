<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

Your feedback on the last nightly, worked through — plus a data bug your
"phantom missing file" report turned out to be hiding.

## Events are now a banner inside the timeline

You were right that the album-style blocks were heavier than you wanted. Events
no longer reorder anything. Your date order stays the spine, and an event paints
as a continuous coloured bar down the left of exactly the rows its photos occupy
— where it ends mid-row, the next photos simply carry on.

- Turn it on at **Arrange ▸ Events ▸ Show event banners**.
- It **disables itself** when you sort by name, size or type. A "continuous" bar
  only means something along a time axis.
- The colour comes from the event's name, so the same trip is the same colour in
  every folder, every session.
- A bar crossing a month header splits into two bands — the separator wins and
  the event resumes underneath.
- The old block view with cover art is still there under **Group ▸ Event** if you
  ever want the album read.

**Events now behave like tags**: remove the last photo from one and it's gone,
from the menu, the filter and the manage list. That's why "Dubai" kept appearing
after you'd emptied it — it was a real record with no photos, and the list could
only ever grow.

## Tags: removing is finally a bulk action

Adding a tag worked on a selection; removing only ever worked on one photo. Now
the bar shows the tags **every** selected photo has, and × removes it from all of
them.

## Clear metadata is one checklist

Instead of six one-shot menu items that each closed the menu, **Clear ▸ Choose
what to clear…** (also on right-click) opens a dialog with tick boxes for stars,
colour labels, pick/reject, tags and events, applied in one pass. Boxes come
pre-ticked for whatever the selection actually has.

## You can now find a missing file

"1 file missing · Re-check" told you a number and nothing else. It's now a button
that lists them with their old paths — click one and it takes you to the folder,
where its "?" tile is waiting with Locate and Forget.

## …which uncovered a real bug

That phantom missing file on E: was never on E:. **The first visit to a new drive
was cloning the previously-open drive's catalog.** Since marks are keyed by path
relative to the drive root, every cloned row described a file that had never
existed there — so a single trim on a video that lives on F: made both D: and E:
permanently claim a missing file. All three drives were also carrying the same
7,254-entry cache.

New drives now start empty. Existing clones on D: and E: are cleared by the hard
reset we discussed.

## Smaller

- The duplicate FoxCull icon and name under the title bar is gone; the sidebar
  header is just actions now.
- That "double border" you asked about is deliberate — the outer ring marks the
  **active** photo (the one keys act on) within a wider selection. I've left it.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet; right-click it → Open on first launch.
