# 2026-07-22 — Cast: why the TV stopped following, and a decoder-availability log

## Intent

Owner report against v1.2.0-nightly.3: casting quality is excellent, but the
session is unreliable when navigating. Three distinct observed behaviours, all
on a Sony Bravia:

1. Press next → the new clip plays locally and **vanishes from the TV**.
2. Press next → **the previous clip keeps playing on the TV** while the app
   moves on.
3. Sometimes it simply works.

Separately: will the live-decode scrub work on the XPS 13 (4-core U-series,
Intel iGPU) as it does on the Alienware (GTX 1070)?

## RCA — cast

Three defects, and each of the three observed behaviours falls out of them.

**1. The receiver app was launched exactly once, and never again.**
`run_actor` sent `LAUNCH` immediately after connecting and nowhere else. But the
Default Media Receiver is not permanent — it closes itself whenever it goes idle
(a clip ends, a still has been up a while, someone presses Home on the TV).
When it closed, `RECEIVER_STATUS` no longer listed our appId, the handler
correctly set `transport_id = None` … and then every subsequent `LOAD` sat in
`pending_load` waiting for a transport id **that nothing would ever produce
again**. The TCP/TLS connection stayed alive and healthy, so `is_alive()` was
true, `cast_status` said connected, and the UI kept saying "Casting to …".
That is symptom 1 (and, once in this state, the session is dead for good, which
matches "it snapped out of it").

**2. `playing_path` was set when the LOAD was *queued*, not when it was *sent*.**
`cast_start` wrote `st.playing_path = Some(path)` right after pushing the
command onto the channel. Combined with defect 1, the backend reported a file
the TV had never been told about. The frontend's follow effect then compared the
active item against that value, concluded its job was done, and stopped trying.
That is symptom 2 exactly: TV on the old clip, app on the new one, nothing
attempting to reconcile them.

**3. `castTo` awaited `castablePath` with no sequencing.** For RAW/HEIC that
call *generates a JPEG preview*, which can take seconds; for a cached JPEG it is
instant. Two quick presses of → could therefore issue their LOADs out of order
and leave the TV on the earlier shot. A third route to symptom 2, and the reason
the whole thing looked nondeterministic.

**Plus:** nothing ever called `cast_status`. A session that died for any reason
(TV off, network blip, another sender taking the device) showed as connected
until the app was restarted.

### Fixes

- `load_action(has_pending, has_transport, since_launch)` — extracted from
  `run_actor` **so it can be tested** — returns `Idle` / `Relaunch` / `Send`.
  Queued media with no transport now relaunches the receiver (rate-limited to
  once per 3 s), keeping `pending_load` so the media goes out the moment the new
  transport id arrives. A `GET_STATUS` accompanies the `LAUNCH` so we don't
  depend on an unsolicited status.
- `playing_path` is assigned only after the `LOAD` frame is successfully sent.
- The follow effect gates on a new non-reactive `castWantedPath` (what we last
  *asked* for) rather than on the backend's report, and `castTo` carries a
  sequence number so a superseded request can't land.
- A 2.5 s status poll while a session is live folds the session up honestly when
  the backend says it is gone.

## RCA — the XPS 13 question

Not answerable from here, and I will not assert it. What is *knowable*: Chrome
has no software HEVC decoder (licensing), so `VideoDecoder` accepts HEVC only
where the GPU does. Intel iGPUs decode HEVC 8-bit from Skylake and **HEVC Main
10 from Kaby Lake (7th gen) onward**; Osmo Pocket 3 4K60 is Main 10. So the
answer depends on that machine's CPU generation, which no document in this repo
records.

Rather than guess, the app now says so itself: `api.logNote` (new `log_note`
command) writes one line per clip into `foxcull.log` — `scrub-engine OK codec=…
open=…ms` or `scrub-engine FALLBACK … — codec unsupported: …`. Opening a few
clips on the XPS and reading the log answers it definitively.

The degradation path if it *is* unsupported was checked and is intact: the
engine rejects, `engineReady` stays false, Glimpse's button disables itself with
an explanatory tooltip, and scrubbing falls back to `<video>` seeking (or the
sprite strip if Live Scrub is on).

## Modules touched

| File | Level | Change |
|---|---|---|
| `src-tauri/src/cast.rs` | architecture | NEW `LoadAction` + `load_action()` (extracted for testability) and `RELAUNCH_EVERY`; `run_actor` step 2 rewritten around it so a closed receiver app is relaunched instead of stranding the queue. `playing_path` moved from command-queue time to post-send. NEW `mod tests` — 4 cases over the relaunch decision. |
| `src-tauri/src/commands.rs` | process | NEW `log_note` command. |
| `src-tauri/src/lib.rs` | process | Register `log_note`. |
| `src/lib/api.ts` | process | `logNote` wrapper (never throws). |
| `src/routes/+page.svelte` | logic | NEW `castWantedPath` (non-reactive follow guard) + `castSeq` request sequencing; follow effect and `startCast`/`stopCast` updated; NEW 2.5 s `cast_status` poll that ends a dead session. |
| `src/lib/components/Loupe.svelte` | process | Log the live-decoder verdict per clip (OK with codec/size/keyframes/open-ms, or FALLBACK with the reason). |

## Behavior changes

- Navigating while casting keeps the TV in step, including after the receiver
  app has idled out — FoxCull relaunches it and delivers the queued media.
- The cast button stops claiming a session that has ended; "Cast session ended"
  appears in the activity chip instead.
- `foxcull.log` gains one `UI scrub-engine …` line per video opened.

## Risks / compat

- Relaunch is bounded by time (once per 3 s) but not by attempt count: a TV that
  is on the network yet refusing to launch would be re-asked every 3 s for as
  long as a session is nominally live. The status poll ends the session when the
  connection itself dies, which is the common case; a live-but-refusing receiver
  would retry indefinitely. Acceptable for now, worth an attempt cap if seen.
- If the user closes the receiver on the TV *while nothing is queued*, we do not
  relaunch (deliberate — `Idle` for `has_pending == false`, covered by a test).
  The next navigation will bring it back.
- `MediaServer::register` still mints a token per cast and never evicts old
  ones. Unbounded but tiny (a map entry per item cast in a session), and
  pre-existing; not touched here.

## Verification actually run

- `npm run check` 0/0. `npm run build` clean. `cargo check` and
  `cargo check --tests` clean, no warnings.
- **4 unit tests over `load_action`**, asserting the exact pre-fix hole:
  queued media + no transport ⇒ `Relaunch` (pre-fix: nothing, forever), the
  rate limit, transport-beats-backoff, and that an idle session never relaunches
  under the user. These run in CI (`cargo test --lib`); local `cargo test`
  cannot link on this machine's GNU toolchain.
- **Not verified on device:** the actual TV. The relaunch path in particular
  depends on the Bravia's real `RECEIVER_STATUS` behaviour after an idle close,
  which cannot be simulated here — the decision logic is tested, the wire
  exchange is not.
