# Instagram Export Playbook (Reels & Stories)

_Last researched: July 2026. Devices: Samsung S23 Ultra, DJI Osmo Pocket 3, DJI Mavic Mini 1._

This is the "how do I make my videos look as good on Instagram as they do on my
laptop" reference. It is written for **casual-but-quality** posting: occasional
Reels and Stories, cropped or uncropped, driven off three cameras. It also
sketches an optional **"Export for Instagram" preset** we could build into
FoxCull.

---

## TL;DR — the five things that actually matter

1. **Instagram re-encodes everything to a 1080-px-wide ceiling.** 4K helps you
   *zero*. Feeding it a giant 4K60 file makes its server encoder do a rushed,
   ugly downscale. **Downscale to exactly 1080×1920 yourself, cleanly, first.**
2. **Reels/Stories play at 30 fps.** Instagram converts 60 fps → 30 fps with a
   crude frame-drop that can look juddery. **Deliver 30 fps yourself.**
3. **HDR is the #1 hidden cause of "washed-out / weird colour."** The Osmo
   Pocket 3 (HLG) and the S23 Ultra (HDR10+) can both record HDR. Instagram's
   auto HDR→SDR tone-map is inconsistent and often looks dull. **Tone-map to
   SDR Rec.709 yourself, or record in SDR.**
4. **Turn on the one app setting:** Instagram → Settings → *Data usage and media
   quality* → **Data Saver OFF**, **"Upload at highest quality" ON**. Upload on
   Wi-Fi. Most "why is my reel blurry" cases are just this toggle.
5. **Export once, from the cleanest source, in a single pass.** Every extra
   re-save (especially through WhatsApp/Telegram) recompresses and softens.
   USB-C / Quick Share / Google Photos "original" transfers are lossless — keep
   using those.

If you do nothing else: **produce a 1080×1920, 30 fps, H.264, SDR Rec.709 master
and upload that from the phone with the highest-quality toggle on.**

---

## Your specific question: desktop upload vs phone — is there a quality unlock?

**No.** There is no meaningful quality unlock from uploading via Instagram's
desktop web for Reels or Stories:

- Both desktop and mobile re-encode to the **same 1080p ceiling** with the same
  server-side encoder.
- The **"Upload at highest quality" toggle only exists in the phone app.** That
  toggle matters more than the upload device.
- Stories in particular are a phone-first surface; desktop web is clunky for them.
- Third-party schedulers (Meta Business Suite, Later, Buffer) upload through the
  API, which *also* caps at 1080p and sometimes compresses **harder**, not
  softer. They're for convenience, not quality.

**Verdict:** your instinct is correct. Do your edit on the laptop → export a
proper master → lossless-transfer to the S23 → upload from the phone with the
quality toggle on. The device isn't the lever; **the file + the toggle** are.

---

## Why your Osmo / Mavic clips go blurry but your phone clips mostly don't

Four compounding reasons, in rough order of impact:

| Cause | What happens | Fix |
|---|---|---|
| **Oversized source (4K/2.7K)** | IG's fast server encoder crushes a huge file into 1080p in one rushed pass → soft, blocky | Pre-scale to 1080×1920 with a good scaler (lanczos) before upload |
| **HDR (HLG / HDR10+)** | IG tone-maps to SDR inconsistently → dull, washed-out, off colours | Tone-map to SDR Rec.709 yourself, or shoot SDR |
| **60 fps → 30 fps** | IG decimates frames → motion looks juddery/"weird frame rate" | Deliver 30 fps yourself |
| **Cropping a low-res landscape** | A 9:16 crop out of a *1080p-wide* landscape is only ~607 px wide → upscaled to 1080 → inherently soft | Shoot the **highest** resolution when you plan to crop (see below) |

Your **phone footage is mostly fine** because it's already near 1080p-ish
targets, often already vertical or lightly cropped, and — if HDR10+ is off — SDR.
The Osmo/Mavic clips are big, sometimes HDR, always landscape, and often cropped
— so they hit every trap at once.

---

## The crop-to-vertical insight (important for you)

You crop landscape → 9:16 about half the time (and prefer it). The **resolution
of the crop** is what decides sharpness, and it depends entirely on the source:

| Source you crop from | Width of a 9:16 crop | Result at 1080×1920 |
|---|---|---|
| Osmo Pocket 3 **4K** (3840×2160) | ~1215 px wide | **Downscaled** to 1080 → sharp ✅ |
| Mavic Mini **2.7K** (2720×1530) | ~860 px wide | Slight **upscale** to 1080 → mildly soft ⚠️ |
| Mavic Mini **1080p** (1920×1080) | ~607 px wide | Big **upscale** to 1080 → clearly soft ❌ |

**Takeaway:** *if you know you might crop to vertical, shoot the highest
resolution the camera offers.* On the Mavic Mini, that means **2.7K, not 1080p**,
whenever a crop is likely. On the Osmo, 4K is already perfect for cropping.
Uncropped (full landscape placed in a 9:16 frame) isn't really your use case, but
if you ever do it, IG pillarboxes with a blurred background — better to export it
letterboxed yourself or just crop.

---

## Step 0 — detect what you actually shot (run this first)

Before deciding whether you need tone-mapping, check the file. Point `ffprobe`
(bundled with FoxCull) at the clip:

```bash
ffprobe -v error -select_streams v:0 \
  -show_entries stream=width,height,r_frame_rate,color_transfer,color_primaries,color_space,codec_name \
  -of default=noprint_wrappers=1 INPUT.mp4
```

Read `color_transfer`:

- `bt709` → **already SDR.** Skip tone-mapping. Use the *SDR recipe*.
- `arib-std-b67` → **HLG HDR** (typical Osmo Pocket 3 HDR). Use the *HDR recipe*.
- `smpte2084` → **PQ / HDR10 / HDR10+** (typical S23 Ultra with HDR10+ on). Use
  the *HDR recipe*.

---

## The master export recipes (single-pass ffmpeg)

All recipes output a **1080×1920, 30 fps, H.264 High, SDR Rec.709, +faststart**
MP4 — the ideal Instagram master. Do the trim, crop, scale, fps, and tone-map in
**one pass** to avoid generational loss.

### Common building blocks

- **Trim:** add `-ss START -to END` (input-seek before `-i` for speed, or
  output-seek after for frame accuracy).
- **Vertical crop from landscape** (centre crop to 9:16, then scale):
  `crop=ih*9/16:ih,scale=1080:1920:flags=lanczos`
- **Already vertical, just normalise size:** `scale=1080:1920:flags=lanczos`
- **Frame rate:** `fps=30`
- **Quality:** `-crf 18 -preset slow` gives an excellent, self-sizing master
  (roughly 10–16 Mbps for 1080p30). If you prefer a hard cap, use
  `-b:v 14M -maxrate 16M -bufsize 24M` instead.
- **Compatibility:** `-pix_fmt yuv420p -profile:v high` (8-bit, universally
  decodable). `-movflags +faststart` puts the index up front for streaming.

### Recipe A — SDR source (Mavic Mini; S23 with HDR10+ OFF; any `bt709` clip)

Cropped vertical from landscape:

```bash
ffmpeg -i INPUT.mp4 \
  -vf "crop=ih*9/16:ih,scale=1080:1920:flags=lanczos,fps=30" \
  -c:v libx264 -preset slow -crf 18 -profile:v high -pix_fmt yuv420p \
  -color_primaries bt709 -color_trc bt709 -colorspace bt709 \
  -c:a aac -b:a 256k -ar 48000 \
  -movflags +faststart OUTPUT_IG.mp4
```

Already-vertical source: swap the filter for
`scale=1080:1920:flags=lanczos,fps=30`.

### Recipe B — HDR source (Osmo Pocket 3 HLG; S23 HDR10+; `arib-std-b67` / `smpte2084`)

Same as A, but with a tone-map chain in front. This uses `hable` (filmic, keeps
highlight + shadow detail). If it looks too dark, swap `tonemap=hable` for
`tonemap=mobius` or `tonemap=reinhard`.

```bash
ffmpeg -i INPUT.mp4 \
  -vf "crop=ih*9/16:ih,\
zscale=transfer=linear:npl=100,format=gbrpf32le,\
zscale=primaries=bt709,\
tonemap=tonemap=hable:desat=0,\
zscale=transfer=bt709:matrix=bt709:range=tv,\
format=yuv420p,\
scale=1080:1920:flags=lanczos,fps=30" \
  -c:v libx264 -preset slow -crf 18 -profile:v high -pix_fmt yuv420p \
  -color_primaries bt709 -color_trc bt709 -colorspace bt709 \
  -c:a aac -b:a 256k -ar 48000 \
  -movflags +faststart OUTPUT_IG.mp4
```

For an already-vertical HDR clip, drop the leading `crop=...,` and keep the rest.

**Verify the output is SDR:**

```bash
ffprobe -v error -select_streams v:0 -show_entries stream=color_transfer \
  -of default=nw=1:nk=1 OUTPUT_IG.mp4   # should print: bt709
```

> GPU alternative for tone-mapping (faster, if a build has libplacebo):
> `-vf "libplacebo=tonemapping=bt.2390:colorspace=bt709:color_primaries=bt709:color_trc=bt709:format=yuv420p,crop=...,scale=1080:1920,fps=30"`.
> The bundled martin-riedl ffmpeg has libx264; libplacebo availability varies, so
> the zscale CPU path above is the portable default.

---

## Per-device cheat sheet

### Samsung S23 Ultra (≈50% of clips)
- **Best single change:** Camera → Settings → *Advanced recording options* →
  **HDR10+ video OFF.** Then the phone records SDR and your "mostly fine"
  becomes "always fine" — no tone-map needed.
- Shoot **FHD 30** for Reels/Stories (FHD 60 is fine too but gets halved to 30).
  4K is pointless for Instagram.
- If you leave HDR10+ on, run **Recipe B** on export.

### DJI Osmo Pocket 3 (≈30% of clips)
- Check the clip with Step 0. Factory **Normal** colour = SDR (`bt709`) → Recipe
  A. If you've enabled **HLG**, it's HDR (`arib-std-b67`) → Recipe B.
- If you want the simplest life, set the Pocket 3 to **Normal (SDR)** colour for
  IG-bound shooting and skip tone-mapping entirely.
- **4K is great for cropping** — a 9:16 crop is still >1080 wide. Downscale to
  1080×1920, 60→30 fps.

### DJI Mavic Mini 1 (≈20% of clips)
- 8-bit **SDR**, no HDR — the simplest camera. Always **Recipe A**.
- **Shoot 2.7K (not 1080p) whenever you might crop vertically** — a 1080p
  landscape crop is only ~607 px wide and will look soft no matter what. This is
  almost certainly why your cropped Mavic reels have looked blurry.
- 1080p60 uncropped is fine; just deliver 30 fps.

---

## Instagram app checklist (do every time)

- [ ] Settings → *Data usage and media quality* → **Data Saver: OFF**
- [ ] Same screen → **Upload at highest quality: ON**
- [ ] Upload on **Wi-Fi**, not cellular (cellular can trigger the low-quality path)
- [ ] Post the file **directly from the gallery** — never re-share it through
      WhatsApp/Telegram/DM first (that recompresses)
- [ ] Reel is exactly **1080×1920**, full-frame 9:16 (no accidental letterbox)
- [ ] Length **3–90 s** for a standard Reel; Stories auto-split at 60 s segments
- [ ] Preview on the **phone** before posting (desktop preview hides edge-crop
      and text-cutoff issues)

---

## Quick spec reference (Instagram, 2026)

| Spec | Reels / Stories value |
|---|---|
| Resolution ceiling | **1080 px wide** (1080×1920 for 9:16) — no 4K playback |
| Aspect ratio | 9:16 (0.5625); Stories 9:16, Feed video 4:5 (1080×1350) |
| Frame rate | 30 fps delivered (24 & 60 accepted but 60 → 30 on server) |
| Codec | **H.264 High**, `yuv420p` 8-bit (H.265/HEVC also accepted but H.264 is safest) |
| Container | MP4 (or MOV) |
| Bitrate | IG's floor ~3.5 Mbps; deliver a **~10–16 Mbps** master so its recompress starts from clean data |
| Colour | **SDR Rec.709** for consistency (HDR accepted but tone-maps unreliably) |
| Audio | AAC, 48 kHz, up to 256 kbps |
| Max file size | ~1 GB (some paths up to 4 GB); a 30–90 s 1080p master is ~40–150 MB |
| Reel length | 3 s – 3 min (90 s is the classic sweet spot) |

Rule of thumb on bitrate: **don't** upload an enormous 50 Mbps file thinking
"more is better" — very high bitrates can trigger *harsher* re-compression. A
clean ~10–16 Mbps 1080p30 master is the sweet spot.

---

## Should we build this into FoxCull? (build spec — not yet implemented)

**Yes, and it's a natural fit, not bloat.** FoxCull already bundles ffmpeg and
already does trim / crop / merge. An **"Export for Instagram"** path is basically
one more export button with a fixed, smart filtergraph. Recommended scope:

**Feature: "Export → For Instagram (Reel/Story)"**

- **Vertical crop picker** (reuse existing crop UI): centre 9:16 by default with
  a draggable horizontal position, since the user crops into landscape often.
- **Auto HDR handling:** run `ffprobe` on the source `color_transfer`; if HLG/PQ,
  transparently insert the `zscale…tonemap=hable…` chain (Recipe B); if `bt709`,
  skip it (Recipe A). No user decision required — just a small "HDR → SDR
  (tone-mapped)" badge so it's visible.
- **Fixed output:** 1080×1920, `fps=30`, libx264 High, `-crf 18` (or a
  bitrate-cap toggle ~14 Mbps), `yuv420p`, Rec.709 tags, AAC 256k,
  `+faststart`.
- **Single pass:** fold the existing trim + crop + this normalisation into **one**
  ffmpeg invocation → no generational loss. FoxCull already does native trim, so
  this is the same command with a richer `-vf` and encode settings.
- **Output naming:** `<name>_IG.mp4` alongside the source, folder opened on
  finish (mirrors the existing "Export as JPEG" behaviour).
- **Optional presets:** *Reel/Story (9:16)*, *Feed (4:5, 1080×1350)*,
  *Square (1:1)* — same pipeline, different crop + scale target.
- **Activity chip:** reuse the existing job/activity system for progress
  (`-progress` parse, same as the HEVC proxy path).

**Effort:** modest — one new export command + one filtergraph builder + a small
dialog. It reuses crop UI, the ffmpeg sidecar, the activity chip, and the
"export then reveal in folder" pattern already shipped. **It does not bloat the
app**; it turns the trim/crop work you already do into something that survives
Instagram intact, which is the whole point.

**Deliberately out of scope:** direct desktop→Instagram publishing. There is no
quality benefit (see above), the Graph API needs a Business/Creator account +
app review, and it adds real maintenance surface. Transfer-to-phone stays the
recommended path.

---

## One-paragraph workflow summary

Edit on the laptop in FoxCull (trim + crop) → export a **1080×1920, 30 fps,
H.264, SDR Rec.709** master in a single pass (tone-map first if the source is
HDR) → lossless-transfer to the S23 (USB-C or Quick Share, never a chat app) →
in Instagram turn **Data Saver off / Upload at highest quality on** → post the
master from the gallery as a Reel or Story over Wi-Fi. That's the whole game.

---

_Sources: Instagram Help (Reel size & aspect ratios); Meta Engineering "Bringing
HDR video to Reels" (2023); StayAbundant, Mallary, Taja, SocialPilot, Sendcove
2026 spec guides; TechWiser & Semiocast quality/blur guides; DEV Community
FFmpeg HDR→SDR tone-mapping (2026). Verified July 2026._
