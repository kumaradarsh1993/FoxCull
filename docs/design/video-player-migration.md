# Video player migration — complete brief

**Purpose:** a self-contained record of the attempt to replace FoxCull's video
player, written so that a different agent (or a stronger model) can pick this up
cold and decide what to do next without re-deriving anything. Nothing here
assumes you have read the session it came from.

**Status as of 2026-07-21, 21:30 IST:** two architectures tried and measured.
One works with known costs; one is proven impossible. Decision pending with the
owner. Branch `feat/libmpv-video`, based on the `v1.1.0` stable tag. `main` is
untouched and shipping.

---

## 1. The problem being solved

FoxCull culls large personal libraries — heavily 4K60 HEVC Main 10 from a DJI
Osmo Pocket 3, plus phone video. Playback in the built-in web view is fine.
**Seeking is not:** the `<video>` element tears down and re-buffers on every
seek, so dragging the playhead on a 4K clip stutters and lags the cursor, versus
VLC/mpv which frame-step instantly on the same hardware.

The v1.1.0 workaround is a **sprite sheet**: pre-extract ~40 frames into one JPEG
and paint the frame under the cursor from a CSS sprite. It works, but:

- it costs a build per clip (measured below),
- the frames are low-resolution stand-ins, visible as mush while dragging,
- it needs cache management, progress UI, cancellation, and a settings toggle.

**Owner's goal, in his words:** *"I want the video playback to be butter smooth —
especially for the 4K60 videos from Osmo Pocket 3 — but make sure the experience
is not broken with the kind of overlays etc we just implemented. RAM overhead is
not an issue; good performance and culling experience is."*

---

## 2. Owner's requirements and stated flexibility

Captured from live commentary, because these shape which trade-offs are
acceptable. Paraphrased but faithful.

| Topic | Position |
|---|---|
| Seek smoothness | The whole point. On trying mpv seeking: *"this is the kind of behavior I was expecting. So kudos."* |
| Sprite pre-caching | Should become **unnecessary in Focus view** if the native player works. **Keep it for grid skimming** (a decoder per grid tile is impossible). *"You will also have to tackle the sprite making… that would only be required in the scenario of grid view when I enable it. So retain it there, but remove all of that from this libmpv thing."* |
| In/out point marking | **Not a blocker.** Happy to move the whole in/out workflow to the Edit tab if the library view makes it awkward. *"That's not a hard constraint and no need to solve for it right now."* |
| Overlays over video | Cares about this. Rejected the hide-the-video-on-menu behaviour: *"these trade-offs are just piling up… it just switches frames, it looks odd."* |
| Acceptable degradation | *"It's fine if you pause and the background or the window gets slightly greyed out"* — a **frozen or dimmed** picture is fine; a **changing** picture is not. |
| Willingness to restructure | High. *"I'm super flexible as a user… if it requires to be moved to edit space, then we can move it."* |
| Willingness to abandon | Explicit. *"If I think that this is not worth it, then we'll chuck this entire thing."* |

**EditStudio is deliberately out of scope.** Its live Look preview is CSS/SVG
filters applied to a `<video>` element, algebraically matched to the ffmpeg
export. mpv cannot reproduce that math live, and Edit is single-clip work where
seek cost barely matters. Recommendation: Edit keeps the web player permanently.
This also confines the blast radius — reverting the transplant touches Focus view
only.

---

## 3. What the current (v1.1.0) sprite system costs — measured, not estimated

Benchmarked 2026-07-21 with the bundled ffmpeg against real Osmo 4K60 HEVC Main
10 on the **internal HDD**, 12-core machine, using the exact command line the
sprite builder issues.

| Test | Result | Conclusion |
|---|---|---|
| 6 cold frames, `-hwaccel auto` | 5.92 s | ~0.99 s per frame |
| 6 cold frames, software decode | 6.22 s | hwaccel worth only ~5% for a single keyframe |
| **the SAME 6 frames, OS cache warm** | **5.04 s** | **only ~15% of the time is disk I/O** |
| 1 frame, `-f null` (no scale/encode) | 0.80 s of the 0.99 s | cost is ffmpeg startup + container index parse |
| 12 cold frames at parallel 2 / 4 / 6 | 6.16 / 4.40 / 3.61 s | it parallelises |

**Headline: sprite building is CPU/process-bound, not disk-bound.** Moving the
library to an SSD barely helps — which matches the owner's own observation. The
fixed cost is paid once per frame because each frame is a separate ffmpeg process
re-opening a multi-gigabyte container.

Acted on: `SPRITE_PARALLEL` 2 → `(cores/3).clamp(2,4)`.
Not attempted: one ffmpeg per *slice* of the timeline emitting several frames
each — the obvious remaining lever, worth ~2-3× more. Filed in `BACKLOG.md`.

---

## 4. Architecture A — mpv child window IN FRONT of the webview

`HWND_TOP`. Implemented, working, commit `4ae90ae` (+ fixes in `e338e3f`).

**Result: this works.** 3840×2160 HEVC Main 10, `hwdec=d3d11va` on a GTX 1070,
D3D11 flip-model presentation, seeking instant and frame-accurate. Owner
confirmed the seek feel was what he wanted.

**The cost:** a window in front of the page cannot be drawn over by anything *in*
the page. Every HTML overlay landing on the video is invisible — Settings,
Prepare, Filters, Arrange, context menus, the Info panel, dialogs, and the
transport bar itself. Mitigations built:

- transport moved into a **reserved strip** below the video (worked well);
- video **hidden** while any overlay is open (rejected by the owner — the
  hide/show produced a visible frame change).

**Unresolved in this architecture:** the frame-switch objection. A candidate fix
that was designed but *not implemented*: on overlay-open, pause mpv, capture its
current frame (`screenshot-to-file`), display that still as an HTML `<img>`, hide
the surface. The pixels are identical, so there is no visible switch — which
matches the owner's stated tolerance ("fine if you pause… shouldn't switch
frames"). **This is the most promising unexplored path.**

---

## 5. Architecture B — mpv BEHIND a transparent webview

`HWND_BOTTOM` + `transparent: true` + a CSS "hole". Implemented fully, commit
`e338e3f`.

**Result: PROVEN IMPOSSIBLE. Do not retry this.**

The idea: make the window transparent, let the page paint its own opaque chrome,
leave the video stage transparent, and put mpv underneath — so overlays become
ordinary HTML again and semi-transparent ones alpha-blend over the video.

Everything on our side worked. The CSS hole opened correctly (visibly black
rather than the normal viewport grey). mpv reported `visible=true`, correct
`screen_rect`, `vo-configured=yes`, 1920×1080, `hwdec=d3d11va`, clock advancing,
"first video frame after restart shown". Audio played.

**The video was still invisible — solid black.**

**Why, and why it is not fixable by tuning:** sibling child windows on Windows do
not blend with one another. Whichever child is topmost owns those pixels
outright. A transparent WebView2 does not reveal a sibling behind it; its
transparent region composites the *top-level window* against the **desktop**, and
the parent's background there is black. Child HWNDs are mutually exclusive
rectangles, not a composited layer stack.

The only way to get true layering between native video and web content is to put
both into **one DirectComposition visual tree** — WebView2's
`CoreWebView2CompositionController` (visual hosting) with mpv's swapchain as a
sibling visual. Tauri/wry create the webview with the *windowed* controller and
expose no way to switch. That would mean forking wry. Judged out of proportion.

---

## 6. Bugs found and fixed along the way (all real, all worth keeping)

1. **`loadfile` rejected every Windows path.** `mpv_command_string` runs mpv's
   own parser and **backslash is an escape character inside a quoted argument**,
   so `loadfile "P:\All media MASTER\…"` parsed as `\A`, `\M`, `\P` and returned
   error -4. mpv had a working window and had been handed nothing to play. This
   single bug caused the original "no picture" result, which was then
   misdiagnosed as a compositing failure. **Fix: `mpv_command`'s argv form,
   which parses nothing.**
2. **Diagnostics sampled too early.** Reading `vo-configured` 1.2 s after start
   always said `no`; D3D11 context creation + first-run shader compilation takes
   ~6 s. Not a bug in the player — a bug in the instrument.
3. **`SWP_NOZORDER` on every reposition** preserved whatever order existed, so a
   webview recreation could put the video permanently on the wrong side. Z-order
   is now restated on every move.
4. **A failed `loadfile` leaked** the child window and mpv context (early return
   past cleanup). Fixed by constructing the player before loading so Drop runs.
5. **mpv's OSD seek bar** (a white bar) appeared over the picture on every drag.
   `osc=no` does **not** cover it — `osd-bar=no` and `osd-level=0` are separate.
6. **Autoplay setting ignored** — the probe hard-coded "play" and it was left in.

### Method note, stated because it cost real time

Three times a cause was asserted from *absence of output* — no picture must mean
compositing; `vo-configured=no` must mean the window class is wrong. All three
were wrong. Every actual answer came from instrumenting: mpv's own verbose log
(`%APPDATA%/com.foxcull.app/mpv.log`, enabled while the flag is on) and a
`native_video_diagnostics` command that dumps window state plus mpv's properties.
**Instrument before theorising.**

### Dev-environment trap

`tauri dev` on this machine had a **72 KB stub `ffmpeg.exe`** beside the built exe
(the placeholder that satisfies `cargo check`), not the real 144 MB binary. Every
ffmpeg operation therefore failed silently in dev — video posters, scrub sprites,
HEIC decode, proxies — and was misreported as thumbnail and live-scrub
regressions. Videos still *played*, because mpv brings its own decoders. **Copy
the real binary to `src-tauri/binaries/ffmpeg-<triple>.exe` (gitignored) before
trusting any media behaviour in a dev run.**

---

## 7. Where the code is

Branch `feat/libmpv-video`, on top of `v1.1.0`.

| File | What it holds |
|---|---|
| `src-tauri/src/mpv.rs` | All FFI. `libloading` (dlopen) — **no link-time dependency**, so a missing dll just disables the feature. `VIDEO_Z` is the one constant that flips architecture A ↔ B. |
| `src-tauri/src/commands.rs` | `native_video_probe / start / set_rect / command / state / set_visible / diagnostics / stop`. |
| `src/lib/components/Loupe.svelte` | Surface lifecycle, rect, transport wiring to mpv, `nativeHole` class. |
| `src/app.css`, `src/routes/+page.svelte` | The `.nativeHole` rules (architecture B only). |
| `docs/design/libmpv-transplant.md` | Original design doc + milestone history. |
| `docs/design/precache-policy.md` | The sprite/cache system this is meant to replace in Focus. |

Settings flag: `experimentalNativeVideo` (default **false**). libmpv-2.dll is
resolved at runtime from the exe dir, `%LOCALAPPDATA%/FoxCull`, or PATH — CI does
**not** yet bundle it, so a shipped build silently falls back to `<video>`.

---

## 8. The decision, laid out

**Option 1 — Architecture A + freeze-frame.** Revert to `VIDEO_Z = HWND_TOP`,
`transparent: false`, keep the reserved transport strip, and implement the
screenshot-freeze for overlays. Delivers the seek performance the owner wants.
Costs: the transport can't float (minimal-bar setting inert in Focus), and
overlay handling is a real mechanism rather than a one-liner. **Recommended if
the transplant continues.**

**Option 2 — Abandon the transplant.** Keep v1.1.0's web player; spend the effort
on the measured sprite lever (one ffmpeg per timeline slice rather than per
frame, worth ~2-3×). Known ceiling — it will never match mpv — but zero
architectural cost and the branch simply gets deleted.

**Option 3 — Visual hosting.** Technically correct, no compromises, requires
forking wry to use `CoreWebView2CompositionController`. Large, and couples
FoxCull to a patched webview stack. Not recommended without a much stronger
reason.

**What is NOT on the table:** architecture B. It is an OS-level rule, not a bug.

---

## 9. Open items unrelated to the layering decision

- **In/out points don't take effect** in the native path. Keys are wired and the
  functions are exported; cause not diagnosed. Owner has said this workflow may
  move to Edit entirely, which would moot it.
- **Fullscreen (F)** positioning of the native surface is untested.
- **Grid strip hover triggers scrub while in Focus view** — owner noted skimming
  the bottom filmstrip shouldn't build sprites. Backlog.
- **Prepare has no cancel button.** Owner request, unrelated.
- **macOS** embedding is a different API entirely (NSView pointer via `--wid`).
  Untouched; the feature is Windows-only and gated as such.
- **CI does not bundle `libmpv-2.dll`.** Required before this can ship to anyone.
