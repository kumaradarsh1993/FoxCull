# Decisions log (ADR-lite)

Standing technical decisions with their reasoning, newest first. If you're
about to re-litigate one of these, read its entry first, then update it (don't
delete history — strike through and append).

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
