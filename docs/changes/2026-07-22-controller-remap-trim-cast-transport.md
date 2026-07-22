# 2026-07-22 — Controller layout rebuilt · in/out points actually save · cast transport mirroring

**Author:** base-machine agent (Claude) · **Tag:** rolls into the next v1.2.0 nightly
**Scope note:** Chromecast *grid* display was investigated and deliberately NOT
built — see `docs/DECISIONS.md` for the feasibility finding and the HDMI
recommendation.

## Intent

Owner brief: the pad layout had drifted out of step with the features added
since it was written, and he wants a full TV-distance review session driven by
the DualSense alone — navigate, mark, rate, label, mark video in/out, enter and
leave Focus, toggle the filmstrip, go fullscreen. Three things blocked that:
too many actions for the buttons available, in/out markers that "reset when I
move to the next video and come back", and a cast session that showed the
right item but ignored the laptop's play/pause/scrub.

## Modules touched

| File | Level | What changed |
|---|---|---|
| `src/lib/api.ts` → `setTrim` / `trimVideo` | **logic (bugfix, root cause)** | Argument keys `in_s`/`out_s` → `inS`/`outS`. Tauri 2 deserializes command args as **camelCase** by default (`tauri-macros` `WrapperAttributes` defaults to `ArgumentCase::Camel`; no `rename_all` override exists anywhere in `src-tauri/`), so both calls failed deserialization on **every** invocation since they were written. `set_trim` swallowed it in `.catch(() => {})`, which is why it read as a UX gap rather than a bug; the `.catch` is gone. `trim_video` has the same defect, meaning **Focus's Cut button could never have worked** either. The SQLite `trims` table and all three Rust commands were always correct — nothing had ever reached them. |
| `src/lib/gamepad.svelte.ts` | **architecture + UX** | Analog sticks become eight bindable synthetic buttons (indices 100–107, `AXIS_BUTTONS`) flowing through the same edge detector as real buttons, with press/release hysteresis (0.6 / 0.4) so a resting or worn stick can't machine-gun. `prev`/`holdSince`/`lastFire` moved from arrays to `Map`s to carry the sparse high indices. New actions: `label1`–`label5`, `markIn`, `markOut`, `toggleView`, `toggleFilmstrip`. New `pressedNow` state feeding the panel's button tester. `DEFAULT_BINDINGS` rebuilt (table below). |
| `src/lib/settings.svelte.ts` | **architecture (migration)** | New `padBindingsVersion` (=2) + reset-on-bump in `init()`. `pad.bind()` writes the **whole merged map**, so any user who had ever rebound one button had that day's defaults frozen in their settings file and would never have seen the new layout. New `stripShow: Record<view, boolean>`. |
| `src/lib/components/ControllerPanel.svelte` | **UX** | Live **button tester** — press anything and it names the index. Added because whether Chromium exposes the PS button (16) and touchpad click (17) depends on OS + pad + engine, and a binding that appears dead is otherwise indistinguishable from a bug. New `Label` group; mouse-button choices gained `toggleView` / `toggleFilmstrip`. |
| `src/routes/+page.svelte` → `handlePadAction` | **logic** | Cases for `label1`–`label5` (mapped through `LABELS`, so stick order == keyboard 6/7/8/9/0), `markIn`/`markOut` (toast when not on a video in Focus — a silent no-op is unreadable from across a room), `toggleView`, `toggleFilmstrip`. |
| `src/routes/+page.svelte` → filmstrip block | **UX** | Strip follows the view: hidden in Grid (where it only repeats the grid), shown in Focus (where it is the only sense of position). `stripViewApplied` guards the effect to view *changes* only — it writes `filmstripPos`, which it also reads, so without the guard a manual toggle would be undone on the next run. The Arrange popover's chips now also record intent per view. |
| `src/lib/components/Loupe.svelte` | **architecture** | New `ontransport` prop fired from the `<video>` element's own `play`/`pause`/`seeked` events. Hooked to the element rather than to each call site deliberately: the transport bar, keyboard, controller and scrub drag all converge there, so one hook covers four paths. `persist()` now guards `end > inS` and surfaces a failed write instead of discarding it. |
| `src-tauri/src/cast.rs` | **architecture + logic** | `Cmd::Play` / `Pause` / `Seek(f64)` + `cast_play` / `cast_pause` / `cast_seek` commands. Captures `mediaSessionId` from `MEDIA_STATUS` (distinct from the receiver `sessionId` already tracked) and clears it on each LOAD, since the receiver mints a fresh one per item. Logging added across discovery, connect, LOAD, relaunch, receiver-closed and every transport frame. |
| `src/routes/+page.svelte` → `onLoupeTransport` | **logic** | Throttles seeks to one per 450 ms with a trailing send, and only mirrors when `castWantedPath === active.path` — during the 350 ms follow debounce the TV is still on the previous item and seeking that would be visible nonsense. |

## New default controller layout

| Button | Action | Was |
|---|---|---|
| ✕ Cross | Reject | Pick |
| ○ Circle | Clear all marks | Reject |
| □ Square | Play / pause | (same) |
| △ Triangle | Pick | Clear marks |
| L1 / R1 | Mark In / Mark Out | Grid view / Focus view |
| L2 / R2 | Shuttle back / forward | (same) |
| Create / Share | Show / hide filmstrip | Button guide |
| Options | Button guide | Fullscreen |
| L3 / R3 | ★★★★★ / Label Yellow | Info overlay / — |
| PS | Play mode (fullscreen) | — |
| Touchpad click | Open / close Focus | — |
| Left stick ↑→↓← | ★ ★★ ★★★ ★★★★ | — |
| Right stick ↑→↓← | Blue Purple Red Green | — |

## Behavior changes visible to the user

- In/out points **persist** — per clip, in the drive catalog, whether or not
  they were committed, and they come back when you return to the clip.
- Stored controller bindings reset **once** to pick up the new layout.
- The filmstrip hides itself in Grid and reappears in Focus.
- With a cast session live, pausing or scrubbing on the laptop does the same on
  the TV.
- `foxcull.log` now carries a full cast trace.

## Risks / compatibility

- **The binding reset is a one-time loss of custom bindings.** Unavoidable
  without it, the new defaults would be invisible to exactly the users who
  care most. The `Reset to PS5 defaults` button remains.
- **PS (16) and touchpad (17) are unverified on this owner's hardware.**
  Chromium's DualSense/DS4 mapper does expose both (`BUTTON_INDEX_META` = 16,
  touchpad = 17, 18 buttons total) and WebView2 is Chromium, but the OS may
  intercept the PS button. The button tester makes this a ten-second check and
  both are remappable if not.
- **The left stick can no longer navigate** — it is the rating pad now. D-pad
  navigates. `info` overlay ends up unbound by default (no button left) and is
  assignable in the panel.
- Cast transport commands are dropped, with a log line, between a LOAD and the
  receiver's first `MEDIA_STATUS`. Correct rather than lossy: media arrives
  autoplaying from 0 in that window.
- `stripShow` is additive; an absent key falls back to `v === "loupe"`.

## Verification actually run

- `npm run check` — **279 files, 0 errors, 0 warnings** (run twice: after the
  controller/trim work and again after the cast work).
- `cargo check` — **clean**, full dependency rebuild, 2m21s.
- **Not verified on hardware, and it needs to be.** Nothing here could be
  exercised on this machine: the pad layout needs a DualSense, and the cast
  transport path needs the Sony Bravia. `cargo test` does not link on this
  machine's GNU toolchain (65k export limit) — the `cast.rs` `load_action`
  unit tests are untouched and run in CI. Owner test list:
  1. Controller panel → button tester → press **PS** and the **touchpad**;
     confirm both report.
  2. Grid → touchpad click opens Focus, again returns. Filmstrip gone in Grid,
     present in Focus.
  3. Video in Focus → L1/R1 mark in/out → arrow to the next clip → arrow back;
     markers must still be there.
  4. Cast to the TV, open a video, pause and scrub on the laptop; the TV should
     follow within ~half a second. Then check `foxcull.log` for the `cast:`
     lines.
