# Handover — 2026-07-22 · Controller shipped and works; **cast is broken and unfixed**

Written at the end of a session that ran out of budget mid-diagnosis. Read this
before touching cast. The controller work is done and confirmed by the owner;
the Chromecast work is **shipped, live, and does not function at all.**

> **2026-07-23 continuation:** the startup-race hypothesis in §3 was confirmed
> directly in current code and fixed. The frontend now logs `cast-ui:` evidence,
> polls from session intent so stale status can self-heal, and no longer gates
> follow on the stale snapshot. The premature `playing_path` assignment was also
> removed. Local checks pass; hardware behavior remains unverified. See the top
> section of `CLAUDE_CODE_HANDOVER.md` and
> `docs/changes/2026-07-23-cast-controller-seek.md`.

---

## 1. State of the world

`main` @ `9c76a1f`, level with `origin/main`. Working tree clean. Four commits
this session:

| Commit | What |
|---|---|
| `6bd1d32` | Controller layout rebuilt for TV culling + **in/out points never saved** bugfix |
| `17de8cf` | Cast transport mirroring (play/pause/seek) + cast logging ← **the broken one** |
| `008419f` | Pairing guide rewritten as two readable cards |
| `9c76a1f` | Release notes |

Two releases published, both prereleases, both green in CI:

- **`v1.2.1-nightly.1`** — from branch `build/no-cast` (branched at `6bd1d32`,
  guide cherry-picked). Deliberate A/B twin **without** any cast changes.
- **`v1.2.1-nightly.2`** — `main`. Everything, including the cast work.

The branch `build/no-cast` still exists on origin. It is the clean bisect
baseline; don't delete it until cast is fixed.

### Confirmed working (owner-verified on hardware)

> "mappings worked, everything from controller point of view is working"

The controller remap, stick-based ratings/labels, touchpad-to-Focus, filmstrip
behaviour. In/out-point persistence was **not** explicitly confirmed by the
owner — he only spoke to the controller. **Ask him to confirm it**; the fix is
believed good (the root cause was found and is unambiguous, see §4).

---

## 2. THE OPEN BUG — cast does nothing

Owner's report, verbatim and complete:

> "I casted, like I was in focus view, I casted, the video started playing from
> beginning, I could not pause play at all, like anything pause or play I was
> doing was happening locally on my laptop but not on the chromecast connected
> device. and my local video was playing independently and the chromecast linked
> device was running independently. changing video did not change it there. after
> the video ended it just stopped and didn't even go back it landed on a blank
> screen"

Four distinct symptoms:

1. Initial LOAD works — the clip plays on the TV. ✅
2. Local play/pause does not reach the TV. ❌ (the new feature)
3. **Changing the selected item does not change the TV.** ❌ — note this is the
   *pre-existing* cast-follow feature from 2026-07-20, claimed as working in the
   v1.2.0 release notes. It is not working.
4. At end of playback the TV lands on a blank receiver screen. ❌

---

## 3. Leading hypothesis — **UNCONFIRMED, do not treat as fact**

The workspace `CLAUDE.md` has a standing warning about asserting inference as
fact (it has cost sessions before). What follows is a code-reading hypothesis
with no runtime evidence behind it. **Confirm it before building on it.**

**Claim: `castStatus.connected` is `false` for the entire session, and that one
boolean disables everything except the initial LOAD.**

The race, in `src-tauri/src/cast.rs`:

- `CastConn::connect()` does the TLS handshake synchronously, creates
  `status` with `connected: false` (~line 625), then `thread::spawn`s
  `run_actor`.
- `run_actor` must send a CONNECT frame **and** a LAUNCH frame over TLS before
  it reaches `status.lock().connected = true` (~line 789).
- Meanwhile `cast_start` (~line 1101-1109) does only `cmd_tx.send(Cmd::Load…)`
  and `conn.status.lock()`, then clones that snapshot and returns it.

Two cheap in-process operations versus OS thread creation plus two network
writes. The main thread should win essentially always, so the frontend receives
`connected: false`.

Every symptom follows from that single value:

| Symptom | Gate | File |
|---|---|---|
| LOAD still works | none — queued on the channel regardless | `cast.rs` |
| Follow dead (symptom 3) | `if (!castStatus.connected …) return` | `+page.svelte:209` |
| Transport dead (symptom 2) | same gate | `+page.svelte:246` |
| Never notices session end (symptom 4) | same gate | `+page.svelte:278` |

And it never self-heals: the only thing that would correct `connected` is the
status poll, which is itself gated on `connected`. Dead lock.

**Ruled out:** `castDevice` being null (it is set on a successful `startCast`,
and LOAD demonstrably succeeded); and a missing `mediaSessionId` (that would
break transport but *not* follow — follow doesn't need it, and follow is broken).

### If confirmed, the fix

Set `connected: true` in `CastConn::connect()` at the point the status struct is
built. Execution only reaches that line after a successful TLS handshake, which
*is* the connection — inferring it later from the actor thread is what created
the race. `run_actor`'s own assignment then becomes redundant but harmless.

This is a ~1-line change. **Do not stop there** — see §5.

---

## 4. The in/out-point bug (fixed, worth understanding)

Root cause found and fixed in `6bd1d32`. `src/lib/api.ts` passed `in_s`/`out_s`
to `set_trim` and `trim_video`, but Tauri 2 deserializes command args as
**camelCase** by default (verified in
`tauri-macros-2.6.2/src/command/wrapper.rs` — `ArgumentCase::Camel` is the
default; no `rename_all` override exists in `src-tauri/`). Both calls failed
deserialization on every invocation, and `set_trim`'s `.catch(() => {})` threw
the error away — which is why it read as a missing feature for months rather
than a bug. Focus's **Cut** button was broken by the same fault and should now
work; nobody has tested it.

Saved to user memory as `tauri-invoke-camelcase-trap`. Worth grepping the other
Tauri projects in this workspace (`wispr-fox`, `md-reader`) for the same
pattern: snake_case keys inside `invoke(...)` object literals.

---

## 5. What the next agent should do, in order

### Step 1 — get evidence before changing anything

`17de8cf` added cast logging to `foxcull.log`, so the owner's session *should*
have left a trace. The decisive question:

- **`cast: -> PAUSE` lines present** → the frontend reached Rust; the hypothesis
  in §3 is **wrong**, and the fault is in the CASTV2 protocol layer instead.
- **No such lines at all** → the frontend gate blocked before any IPC; §3 is
  very likely right.
- **`cast: transport command dropped (no media session yet)`** → a third case:
  `MEDIA_STATUS` parsing or the app-transport virtual connection is at fault.

### ⚠️ Step 1a — the log I inspected may not be the real one

I found `foxcull.log` at two paths, both **8578 bytes, both stamped
2026-07-22 00:04**, i.e. ~23 hours stale and predating all of this session's
work. So neither reflects the owner's test run.

The two paths were:

```
C:\Users\kadar\AppData\Roaming\com.foxcull.app\foxcull.log
C:\Users\kadar\AppData\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\com.foxcull.app\foxcull.log
```

Identical size and timestamp strongly suggests **these are the same file seen
twice** — the second path is the MSIX/sandbox redirect of the first. Which means
**the agent shell here may be reading a sandboxed view of `%APPDATA%`, not the
real one.** Do not conclude "the owner's session wrote no log" from this; the
run may have logged to a location this shell cannot see. Ask the owner to open
the log himself (or run `explorer %APPDATA%\com.foxcull.app`) and paste the
`cast:` lines. That is the fastest reliable read.

Also consider portable mode: `resolve_data_root()` in `lib.rs` puts everything
beside the exe if a `foxcull-data\` folder exists there, so check the install
directory too.

### Step 2 — fix, then make it self-evidencing

Whatever the cause, the deeper problem this session exposed is that **cast fails
silently and invisibly.** I shipped a feature that did nothing and neither the
UI nor any log told the owner why. Before shipping another cast fix:

- Log on the **frontend** side too (`api.logNote`) — every mirror attempt *and
  every early return with its reason*. The Rust logging I added was useless here
  precisely because the frontend never got far enough to call it.
- Surface cast state in the UI. "Casting to X" is currently driven by the same
  `connected` boolean that is suspected broken, so the owner had no signal.

### Step 3 — symptom 4 (blank screen at end of playback)

Not yet addressed. The Default Media Receiver closes itself when idle; we only
relaunch when there is pending media, so the TV falls to a blank receiver
screen. Proposed (not implemented, not agreed with the owner): on a
`MEDIA_STATUS` of `IDLE`/`FINISHED`, re-LOAD the current item with
`autoplay: false` so the TV settles on its first frame instead of nothing. Loop
risk is nil since an autoplay-false load cannot reach FINISHED again. **Get the
owner's agreement first** — he may prefer it simply hold the last frame.

### Step 4 — verify on hardware before claiming anything

Nothing about cast can be verified from this machine. It needs the owner's Sony
Bravia. `npm run check` and `cargo check` both pass and CI is green — and the
feature still did not work, so **treat green gates as necessary and nowhere near
sufficient for cast.** That mistake is the whole reason this handover exists.

---

## 6. Process note for whoever picks this up

The v1.2.0 release notes told the owner casting was "reliable now" and that "the
TV mirrors whatever you browse". Symptom 3 says that was never true. So the
cast-follow feature has been asserted as working in release notes across at
least two releases without hardware verification. Assume nothing in the cast
path is verified unless the owner has said so about that specific behaviour.

The gates that DO hold on this machine: `npm run check` (svelte-check, 0/0 is
the bar) and `cargo check` from `src-tauri/`. `cargo test` does not link locally
(GNU ld 65k export limit) — it runs in CI. Never `npm run tauri build`.
