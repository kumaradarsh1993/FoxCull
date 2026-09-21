# Inbox: Osmo Pocket 3 writes a second audio track, and FoxCull should notice

**Filed:** 2026-09-21 · **Status:** thesis only, no implementation proposed yet
**Found during:** the SD-card / S23 backup audit in `photo-consolidation`
**Owner's ask:** research it properly, don't extrapolate; write the thesis, not the code.

---

## 1. What is actually happening — DJI's own words

This is not a quirk or a bug. It is a documented firmware feature called
**Built-in Mic Audio Backup**, added in an Osmo Pocket 3 firmware update. From
DJI's official release notes for the Osmo Pocket 3:

> "Added **Built-in Mic Audio Backup** function in Settings. When enabled, the
> camera will record audio through the built-in microphone and save it as a
> separate in .wav format audio file, which can be used to pick up environment
> sound when connected to a DJI Mic 2 transmitter or an external microphone."

And in a later release:

> "Added support for setting the audio channel in the Built-in Mic Audio Backup
> function. The channel parameters will be set the same as they are for the
> camera audio."

So the owner's recollection was exactly right: he turned on a setting that keeps
the Pocket 3's **own** body microphones recording even while the DJI Mic 2 is
supplying the main audio. The result per clip:

| file | contains | format |
|---|---|---|
| `DJI_<ts>_<seq>_D.MP4` | the **DJI Mic 2** (or other external mic) | AAC in the video container |
| `DJI_<ts>_<seq>_D.WAV` | the **Pocket 3's built-in mics** | uncompressed PCM |

One more documented detail that matters downstream:

> "If stereo is enabled and only one wireless microphone transmitter is
> connected, Osmo Pocket 3 will duplicate the audio to both channels."

…and the Pocket 3 can pair **two** transmitters at once (Mic 2 + Mic Mini).

## 2. What we measured on his actual files — not assumed

26 `.WAV` files from the card, and their paired clips on the master drive:

| property | `.WAV` (built-in) | `.MP4` track (Mic 2) |
|---|---|---|
| codec | **PCM, uncompressed** | AAC (`mp4a`) |
| channels | 2 | 2 |
| sample rate | 48 000 Hz | 48 000 Hz |
| bit depth | 16 | 16 |

**The decisive measurement — durations match to the tenth of a second:**

```
DJI_20260622190543_0628   MP4 546.2s   WAV 546.2s
DJI_20260623172934_0643   MP4 236.4s   WAV 236.4s
DJI_20260623124626_0632   MP4 365.8s   WAV 365.8s
```

**The two tracks are frame-locked and start together.** This is the single most
important fact in this note, because it removes the hard half of the problem:
**there is no sync step.** No clapper, no waveform alignment, no drift
correction. Any mix is a pure gain/blend decision.

L/R correlation across all 26 files runs **0.10 – 0.99** (median ≈ 0.78) — the
built-in capture is **genuinely stereo**, not a duplicated mono channel. Two of
the 26 are near-mono at correlation 1.000, and both peak at full scale (32768),
i.e. **clipped** — wind or a very close loud source.

Per-channel RMS across the set ranges **−15 to −49 dBFS**. The built-in mic
generally runs quiet, and occasionally clips. **A naive normalise would be
wrong on both ends.**

## 3. Why this is a FoxCull problem, and why we already have proof

**We found this because it broke.** During the audit, the master drive turned out
to hold **zero `.wav` files anywhere** — all 26 audio tracks had been orphaned.
21 of them paired with clips that were *already safely backed up*. The MP4s got
copied across; their audio sidecars silently did not.

That is precisely the failure mode FoxCull exists to prevent, and it is the same
shape as the catalog-relink work already in the repo: **a media item that is more
than one file on disk.** FoxCull already reasons about RAW+JPEG pairs. This is
the same relationship with a different pair of extensions.

Note the wrinkle that makes it harder than RAW+JPEG: **the sidecar can be in a
different folder from its clip, on a different drive, at a different time.** In
this exact case the MP4s were on `P:` and the WAVs were still on the card.
Matching has to be by **filename stem across the whole catalog**, not by
"same directory".

## 4. The thesis: what FoxCull should do

### 4a. Detect — cheap and unambiguous

The stem is an exact key: `DJI_20260623172934_0643_D` + `{.MP4, .WAV, .LRF}`.
No heuristics, no content analysis. Three things fall out of detection alone:

1. **Pair them in the catalog** so a cull, a move, or an export carries the WAV
   with the clip. Deleting the clip should offer to delete the WAV.
2. **Flag orphans, in both directions.** A WAV with no clip means the clip was
   culled and the audio was left behind. A clip with no WAV, sitting in a folder
   where sibling clips *do* have WAVs, means the sidecar got lost in a copy —
   which is exactly what happened here.
3. **Surface "this clip has a second audio track"** in the UI at all, which today
   nothing does.

### 4b. The rescue case — probably the highest value, lowest effort

Sometimes the Mic 2 fails: out of range, muted, dead battery, never linked. The
MP4 then carries near-silence or dropouts while the WAV has perfectly good audio.
Detecting that is a level comparison, not DSP: if the MP4 track is below some
floor over a sustained window and the WAV is not, offer **"use the backup
audio instead"** as one click. That alone rescues shoots that are otherwise bin.

### 4c. The mix — and why the obvious answer is wrong

The owner's scenario, in his words: Mic 2 on his girlfriend, standing some
distance away and vlogging; Pocket 3 in his hand while he talks and walks. So:

- **MP4 track**: her clear, him faint
- **WAV**: him clear, her faint

Both tracks contain both people, at inverted ratios. **The tempting move — sum
them at some ratio — is the wrong default**, and the reason is physics rather
than taste. Her voice reaches the Mic 2 immediately but reaches the Pocket 3's
body mic later, by roughly 3 ms per metre. At 5 m that is ~15 ms. Summing two
copies of the same voice offset by 15 ms is **comb filtering** — the hollow,
phasey, "recorded in a pipe" sound. It will be audibly worse than either track
alone, and it gets worse the more equally you blend them.

One delay cannot fix it, because each track needs a *different* correction for
each speaker.

So the thesis is a ladder, and the default must be the safe rung:

**Tier 1 — the default, zero controls.** Mic 2 stays the voice. The WAV is mixed
underneath as an **ambience bed at roughly −15 to −20 dB**. This is what DJI
itself describes the feature as being for ("pick up environment sound"). At that
level the comb filtering is inaudible, and you gain room, traffic, sea, crowd —
the things a lavalier on someone's chest never hears. Almost always better than
the Mic 2 alone; essentially never worse.

**Tier 2 — one slider.** "Whose voice matters here?", from *her* to *both* to
*him*. A static crossfade, no analysis. Honest and predictable. The UI should
discourage the middle by showing it as the phasey option, not hide that it exists.

**Tier 3 — the actually-correct answer: an automixer.** This is a solved problem
in live sound, and the solution is the **Dugan-style automixer**: rather than a
fixed blend, give each channel a gain proportional to its share of the *total*
energy at that instant, so the sum of gains stays constant. Whoever is currently
loudest on their own mic owns the mix; the other channel drops. Because only one
channel is ever dominant, **the two copies of a voice never sit at comparable
level, so the comb filtering never forms.** That is the property we want, and it
is why an automixer beats a blend rather than merely automating it.

It is also cheap to compute — a per-block energy ratio with smoothing — and it
needs no machine learning, no speaker ID, no transcription.

**A real constraint on all three tiers**, from the measurements above: the source
runs between −15 and −49 dBFS and sometimes clips. Whatever we build needs
loudness normalisation with true-peak limiting, not a fixed gain, or quiet clips
will vanish under the bed and the clipped ones will crunch.

### 4d. Keep it no-code, which is the owner's actual requirement

Nothing above needs to be exposed as DSP. The whole surface can be:

- a badge on the clip saying it has backup audio
- a three-way preset: **Mic only · Mic + ambience (default) · Auto-balance**
- one "use backup audio instead" button for the rescue case

Everything else is internal.

## 5. Secondary finding worth its own line: `.LRF` is a free proxy

Alongside each clip DJI also writes `DJI_<stem>_D.LRF`. We checked: it is a
genuine ISO-BMFF MP4 (`ftypisom`), and for one sample clip it is **64.8 MB
against the original's 1,018 MB — about 16× smaller**.

That is a ready-made proxy, shipped by the camera, that FoxCull currently ignores
and that this backup audit was about to throw away as junk. For a culling app
whose whole job is fast scrubbing through footage, **playing the LRF instead of
decoding 4K/10-bit is close to free performance.** Worth evaluating on its own,
independent of the audio work.

## 6. What we do NOT know yet

Stated plainly so nobody builds on a guess:

- Whether the WAV's stereo image is fixed to the body or **rotates with the
  gimbal**. It matters for the ambience bed. Untested.
- What happens with **two transmitters** paired (Mic 2 + Mic Mini) — whether the
  MP4 carries them as L/R and what the WAV then holds. We have no two-transmitter
  footage to inspect.
- Whether the MP4's AAC track is dual-mono in his single-transmitter clips. The
  release notes say it should be, but we have no AAC decoder on this machine, so
  **this is unverified**. If it is dual-mono, the mix can collapse to mono for
  free and halve the work.
- Whether the WAV is gain-matched to the MP4 track at all, or independently AGC'd.

The first three are answerable with one short test shoot and an `ffmpeg` install.

---

## Sources

- [DJI Osmo Pocket 3 Release Notes (official, dji.com CDN)](https://dl.djicdn.com/downloads/DJI_Osmo_Pocket_3/RN/20241126/DJI_Osmo_Pocket_3_Release_Notes_en.pdf) — the two quoted release notes
- [Understanding Backup Audio Options with DJI Osmo Pocket 3 — DJI Forum](https://forum.dji.com/thread-308453-1-1.html)
- [The Pocket 3 is now a four-channel audio recorder with video! — DJI Forum](https://forum.dji.com/thread-302503-1-1.html)
- Everything in §2 and §5 is measured from the owner's own files on `G:\DCIM\DJI_001` and `P:\All media MASTER\Pics\2026\Osmo Pocket 3`, not cited.
