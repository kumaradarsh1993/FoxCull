// The WebCodecs scrub engine — "Architecture C".
//
// WHY THIS EXISTS (full story: docs/design/video-player-migration.md §10-11):
// dragging the playhead on a 4K60 HEVC clip was unusable because every
// `video.currentTime = t` in Chromium is a *precise* seek — pipeline flush,
// decode from the previous keyframe to the exact frame, audio re-sync. Thirty
// of those per drag lags the cursor. `fastSeek()` (the cheap keyframe-only
// seek) is NOT implemented in Chromium, so the element can never scrub the way
// mpv does.
//
// mpv's trick isn't privileged hardware access — it's a *policy*: keyframe-only
// seeks on a decoder that stays hot while you drag, and one precise decode when
// you let go. WebCodecs lets us run exactly that policy inside the page, where
// the pixels are ordinary DOM pixels and every overlay/menu/fullscreen concern
// simply doesn't arise (that is what sank the libmpv transplant — see §5).
//
// Measured on the dev machine, real Osmo 4K60 HEVC Main 10 off a spinning HDD:
// moov index 316 ms / 4.9 MB, hot keyframe decode 16-19 ms, keyframe every
// ~0.5 s. That is 20-30 full-resolution scrub frames per second with ZERO
// pre-caching — versus ~1 s per frame to build the low-res sprite sheet it
// replaces (§3).
//
// Scope: Focus view only. Grid tiles keep the sprite sheet (a decoder per tile
// is not a thing) — the owner scoped it that way deliberately.

import * as MP4Box from "mp4box";
import { api } from "./api";

/** Bytes per metadata read while hunting for the moov box. */
const PARSE_CHUNK = 2 * 1024 * 1024;
/** Safety rail on the metadata hunt (8 GB of 2 MB chunks). */
const PARSE_MAX_CHUNKS = 4096;
/** Give up on a single decode after this long and drop the request. */
const DECODE_TIMEOUT_MS = 4000;

export interface ScrubSample {
  offset: number;
  size: number;
  /** Presentation time, seconds. */
  cts: number;
  isSync: boolean;
}

export interface ScrubIndex {
  codec: string;
  codedWidth: number;
  codedHeight: number;
  description?: Uint8Array;
  /** Container rotation (0/90/180/270). WebCodecs does NOT apply it; the
   *  `<video>` element does, so we must, or portrait phone clips scrub
   *  sideways. */
  rotation: number;
  durationS: number;
  /** All samples, decode order. */
  samples: ScrubSample[];
  /** Indices into `samples` that are keyframes, ascending by cts. */
  syncIdx: number[];
}

async function readRange(path: string, offset: number, len: number): Promise<Uint8Array> {
  return new Uint8Array(await api.readFileRange(path, offset, len));
}

/** Feed mp4box until the moov is parsed.
 *
 *  Camera files put the moov AFTER a multi-gigabyte mdat. `appendBuffer`
 *  returns the next byte offset it actually wants, which jumps clean over the
 *  mdat — so this reads a few MB total no matter how big the file is. That is
 *  the whole reason indexing is free and "Prepare" is unnecessary in Focus. */
export async function parseMoov(
  path: string,
  isStale?: () => boolean,
): Promise<{ info: any; file: any }> {
  const file = MP4Box.createFile();
  let info: any = null;
  let err: unknown = null;
  file.onReady = (i: any) => (info = i);
  file.onError = (e: unknown) => (err = e);

  let pos = 0;
  for (let i = 0; i < PARSE_MAX_CHUNKS && !info && !err; i++) {
    if (isStale?.()) throw new Error("stale");
    const bytes = await readRange(path, pos, PARSE_CHUNK);
    if (bytes.length === 0) break; // EOF
    const ab = bytes.buffer as ArrayBuffer & { fileStart: number };
    ab.fileStart = pos;
    const next = file.appendBuffer(ab);
    if (bytes.length < PARSE_CHUNK) break; // short read = EOF
    pos = typeof next === "number" ? next : pos + bytes.length;
  }
  if (err) throw new Error(`mp4box: ${String(err)}`);
  if (!info) throw new Error("no moov (not an MP4/MOV?)");
  return { info, file };
}

/** The codec-config box (hvcC/avcC/…) minus its 8-byte header — the
 *  `description` VideoDecoder needs to read length-prefixed samples. */
export function codecDescription(file: any, trackId: number): Uint8Array | undefined {
  const entries = file.getTrackById(trackId)?.mdia?.minf?.stbl?.stsd?.entries ?? [];
  for (const e of entries) {
    const box = e.hvcC ?? e.avcC ?? e.vpcC ?? e.av1C;
    if (!box) continue;
    const stream = new MP4Box.DataStream(undefined, 0, MP4Box.DataStream.BIG_ENDIAN);
    box.write(stream);
    return new Uint8Array(stream.buffer, 8, stream.getPosition() - 8);
  }
  return undefined; // Annex-B / raw streams need no description
}

/** Rotation from the track matrix (16.16 fixed point a,b,_,c,d,…). */
function rotationFromMatrix(m: number[] | undefined): number {
  if (!m || m.length < 5) return 0;
  const a = m[0] / 65536, b = m[1] / 65536, c = m[3] / 65536, d = m[4] / 65536;
  if (a === 0 && d === 0) {
    if (b === 1 && c === -1) return 90;
    if (b === -1 && c === 1) return 270;
  }
  if (a === -1 && d === -1) return 180;
  const deg = Math.round((Math.atan2(b, a) * 180) / Math.PI);
  return ((deg % 360) + 360) % 360;
}

/**
 * One clip's scrub session: an index, a hot decoder, and a latest-wins request
 * queue. Create with `ScrubEngine.open()`, throw it away with `close()`.
 */
export class ScrubEngine {
  readonly index: ScrubIndex;
  private path: string;
  private decoder: VideoDecoder | null = null;
  /** Latest requested time; a newer request replaces an unstarted older one.
   *  Without this the decode backlog makes the picture lag the cursor — the
   *  exact failure mode we are here to remove. */
  private wanted: { t: number; exact: boolean; cb: (f: VideoFrame) => void } | null = null;
  private running = false;
  private closed = false;

  private constructor(path: string, index: ScrubIndex) {
    this.path = path;
    this.index = index;
  }

  /** Parse + validate a clip. Throws if this file can't be scrubbed this way;
   *  every caller must be ready to fall back to the sprite path. */
  static async open(path: string, isStale?: () => boolean): Promise<ScrubEngine> {
    if (typeof VideoDecoder === "undefined") throw new Error("no WebCodecs");
    const { info, file } = await parseMoov(path, isStale);
    const track = info.videoTracks?.[0];
    if (!track) throw new Error("no video track");

    const trak = file.getTrackById(track.id);
    const raw: any[] = trak?.samples ?? [];
    if (raw.length === 0) throw new Error("empty sample table");

    const samples: ScrubSample[] = raw.map((s) => ({
      offset: s.offset,
      size: s.size,
      cts: s.cts / s.timescale,
      isSync: !!s.is_sync,
    }));
    const syncIdx: number[] = [];
    for (let i = 0; i < samples.length; i++) if (samples[i].isSync) syncIdx.push(i);
    // A file with no sync flags at all (rare, some MOVs) means every sample is
    // a keyframe by convention — but we can't seek cheaply without knowing, so
    // treat it as unsupported rather than guess.
    if (syncIdx.length === 0) throw new Error("no sync samples");

    const index: ScrubIndex = {
      codec: track.codec,
      codedWidth: track.video?.width ?? track.track_width ?? 0,
      codedHeight: track.video?.height ?? track.track_height ?? 0,
      description: codecDescription(file, track.id),
      rotation: rotationFromMatrix(trak?.tkhd?.matrix),
      durationS: info.duration && info.timescale ? info.duration / info.timescale : 0,
      samples,
      syncIdx,
    };

    const cfg: VideoDecoderConfig = {
      codec: index.codec,
      codedWidth: index.codedWidth,
      codedHeight: index.codedHeight,
      description: index.description,
      // This decoder exists to answer "one frame, NOW" — never to keep a
      // pipeline full. That is precisely what this hint is for.
      optimizeForLatency: true,
    };
    const hw = await VideoDecoder.isConfigSupported({
      ...cfg,
      hardwareAcceleration: "prefer-hardware",
    }).catch(() => ({ supported: false }) as VideoDecoderSupport);
    const supported =
      hw.supported ||
      (await VideoDecoder.isConfigSupported(cfg).catch(() => ({ supported: false }) as VideoDecoderSupport))
        .supported;
    if (!supported) throw new Error(`codec unsupported: ${index.codec}`);

    const engine = new ScrubEngine(path, index);
    engine.configure(cfg, !!hw.supported);
    return engine;
  }

  private configure(cfg: VideoDecoderConfig, hardware: boolean) {
    this.decoder = new VideoDecoder({
      output: (f) => this.onOutput(f),
      error: () => this.onDecoderError(),
    });
    this.decoder.configure({
      ...cfg,
      hardwareAcceleration: hardware ? "prefer-hardware" : "no-preference",
    });
    this.cfg = cfg;
    this.hardware = hardware;
  }
  private cfg: VideoDecoderConfig | null = null;
  private hardware = false;

  // ── Output plumbing ────────────────────────────────────────────────────
  // Only the frame closest to what we asked for is kept alive; every other
  // output is closed the instant it arrives.
  //
  // This is NOT just tidiness. Collecting all of a GOP's frames and choosing
  // afterwards deadlocked the decoder: a hardware VideoDecoder emits into a
  // small pool of GPU textures, and holding a dozen 4K frames leaves it with
  // nothing to decode into, so `flush()` never resolves. Measured: 1 sample
  // flushed in 13 ms, 12 samples hit the 4 s timeout — with the reads (112 ms)
  // ruling out I/O. Holding ≤2 frames also caps this path at ~24 MB instead of
  // ~144 MB.
  private best: VideoFrame | null = null;
  private wantTs = 0;
  private onOutput(f: VideoFrame) {
    const b = this.best;
    if (!b) {
      this.best = f;
      return;
    }
    if (Math.abs(f.timestamp - this.wantTs) < Math.abs(b.timestamp - this.wantTs)) {
      b.close();
      this.best = f;
    } else {
      f.close();
    }
  }
  private dropBest() {
    this.best?.close();
    this.best = null;
  }
  private onDecoderError() {
    // A decoder that has errored is unusable; rebuild it lazily so a single bad
    // sample doesn't kill scrubbing for the rest of the clip.
    try {
      this.decoder?.close();
    } catch {}
    this.decoder = null;
  }
  private ensureDecoder() {
    if (!this.decoder && this.cfg && !this.closed) this.configure(this.cfg, this.hardware);
    return this.decoder;
  }

  /** Nearest keyframe at or before `t` (index into `samples`). */
  private keyBefore(t: number): number {
    const { samples, syncIdx } = this.index;
    let lo = 0,
      hi = syncIdx.length - 1,
      best = 0;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (samples[syncIdx[mid]].cts <= t) {
        best = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    return syncIdx[best];
  }

  /** Timeline position the next scrub frame will actually show — used to keep
   *  the on-screen timestamp honest while dragging. */
  keyTimeFor(t: number): number {
    return this.index.samples[this.keyBefore(t)].cts;
  }

  /**
   * Ask for the frame at `t`.
   * - `exact: false` (dragging) → the nearest keyframe at or before `t`. One
   *   read, one decode, ~20 ms.
   * - `exact: true` (release) → decodes forward from that keyframe to the real
   *   frame, bounded by one GOP (~30 frames here). Paints the SAME frame the
   *   `<video>` element will land on, so the hand-off is invisible.
   *
   * Latest request wins: an older unstarted request is dropped, never queued.
   * The callback borrows the frame — it is closed as soon as the callback
   * returns, so draw synchronously and don't retain it.
   */
  request(t: number, exact: boolean, cb: (f: VideoFrame) => void) {
    if (this.closed) return;
    this.wanted = { t, exact, cb };
    if (!this.running) void this.pump();
  }

  private async pump() {
    this.running = true;
    try {
      while (this.wanted && !this.closed) {
        const job = this.wanted;
        this.wanted = null;
        try {
          const frame = await this.decodeAt(job.t, job.exact);
          if (frame) {
            // ALWAYS paint. An earlier version skipped the paint when a newer
            // request had already arrived, reasoning that the position was
            // stale — but during a real drag a newer request has ALWAYS
            // arrived (they come ~3x faster than a frame decodes), so the
            // picture painted once at the end and sat frozen in between. That
            // is the exact symptom we set out to remove. A frame one step
            // behind the cursor is the correct output; the next decode catches
            // up. (Caught by the simulated drag in scrub-probe.ts: 1 of 24
            // requests painted.)
            try {
              job.cb(frame);
            } finally {
              frame.close();
            }
          }
        } catch {
          /* one bad frame must not stop the drag */
        }
      }
    } finally {
      this.running = false;
    }
  }

  /** Timing breakdown of the most recent decode. Kept because every wrong
   *  answer in this migration came from reasoning about behaviour instead of
   *  measuring it; the probe reports these. */
  stats: {
    samples: number;
    bytes: number;
    readMs: number;
    flushMs: number;
    timedOut: boolean;
  } | null = null;

  private async decodeAt(t: number, exact: boolean): Promise<VideoFrame | null> {
    const dec = this.ensureDecoder();
    if (!dec) return null;
    const { samples } = this.index;
    const keyIdx = this.keyBefore(t);

    // Which samples to feed. Coarse: the keyframe alone. Exact: everything up
    // to the last sample presenting at or before `t` — with B-frames a needed
    // sample can sit later in decode order, so we take the max index rather
    // than stopping at the first cts past `t`.
    let lastIdx = keyIdx;
    if (exact) {
      for (let i = keyIdx; i < samples.length; i++) {
        if (samples[i].cts <= t + 1e-6) lastIdx = i;
        // Stop once we are well past the target in decode order; GOPs are short
        // and this bounds the work on pathological files.
        if (samples[i].cts > t + 2) break;
      }
    }

    // One contiguous read covers the whole span — samples of a GOP are laid out
    // together, so this is a single sequential read instead of N seeks.
    const first = samples[keyIdx];
    const last = samples[lastIdx];
    const start = first.offset;
    const end = last.offset + last.size;
    const readAt = performance.now();
    const buf = await readRange(this.path, start, end - start);
    const readMs = performance.now() - readAt;
    if (this.closed) return null;

    // Anything still held is from a decode that timed out; it MUST be released
    // or the decoder's output pool runs dry.
    this.dropBest();
    this.wantTs = (exact ? t : samples[keyIdx].cts) * 1e6;
    for (let i = keyIdx; i <= lastIdx; i++) {
      const s = samples[i];
      const o = s.offset - start;
      if (o < 0 || o + s.size > buf.length) continue; // non-contiguous: skip
      dec.decode(
        new EncodedVideoChunk({
          type: i === keyIdx ? "key" : "delta",
          timestamp: Math.round(s.cts * 1e6),
          data: buf.subarray(o, o + s.size),
        }),
      );
    }
    // flush() forces the frames out now rather than pipelining them — the
    // latency profile of "one frame wanted NOW", and what the probe measured.
    const flushAt = performance.now();
    let timedOut = false;
    await Promise.race([
      dec.flush().catch(() => {}),
      new Promise((r) =>
        setTimeout(() => {
          timedOut = true;
          r(null);
        }, DECODE_TIMEOUT_MS),
      ),
    ]);
    this.stats = {
      samples: lastIdx - keyIdx + 1,
      bytes: end - start,
      readMs: Math.round(readMs),
      flushMs: Math.round(performance.now() - flushAt),
      timedOut,
    };
    if (this.closed || !this.best) {
      this.dropBest();
      return null;
    }
    // Chosen as outputs arrived (see onOutput) — presentation order isn't
    // guaranteed with B-frames, so the pick is by timestamp, not by position.
    const frame = this.best;
    this.best = null;
    return frame;
  }

  close() {
    this.closed = true;
    this.wanted = null;
    try {
      this.dropBest();
    } catch {}
    try {
      this.decoder?.close();
    } catch {}
    this.decoder = null;
  }
}

/**
 * Draw a decoded frame into a canvas, honouring container rotation and
 * fitting the canvas to the frame's *displayed* aspect. Sized in device
 * pixels so a 4K frame lands sharp on a HiDPI stage without allocating a 4K
 * backing store for a 900px box.
 */
export function paintFrame(
  canvas: HTMLCanvasElement,
  frame: VideoFrame,
  boxW: number,
  boxH: number,
  rotation: number,
) {
  const swap = rotation === 90 || rotation === 270;
  const fw = swap ? frame.displayHeight : frame.displayWidth;
  const fh = swap ? frame.displayWidth : frame.displayHeight;
  if (!fw || !fh || boxW <= 0 || boxH <= 0) return;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const scale = Math.min(boxW / fw, boxH / fh);
  const w = Math.max(1, Math.round(fw * scale * dpr));
  const h = Math.max(1, Math.round(fh * scale * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  // Backing store is device pixels; the CSS box must be the layout size or a
  // 4K frame would render at 4K CSS pixels on a HiDPI screen.
  canvas.style.width = `${Math.round(w / dpr)}px`;
  canvas.style.height = `${Math.round(h / dpr)}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.save();
  ctx.translate(w / 2, h / 2);
  if (rotation) ctx.rotate((rotation * Math.PI) / 180);
  // After rotation the drawing axes are the frame's own, so draw at the
  // pre-swap size.
  const dw = swap ? h : w;
  const dh = swap ? w : h;
  ctx.drawImage(frame, -dw / 2, -dh / 2, dw, dh);
  ctx.restore();
}
