// WebCodecs feasibility probe — DEV ONLY, the go/no-go gate for the
// "Architecture C" scrub engine (docs/design/video-player-migration.md).
//
// Question it answers by MEASUREMENT (this project has been burned by
// inference three times): inside the shipped WebView2, on this machine, can a
// persistent hardware VideoDecoder decode single HEVC Main 10 keyframes from a
// real Osmo Pocket 3 4K60 file fast enough to scrub with (~10-20 ms/frame)?
//
// How to run: set VITE_SCRUB_PROBE to an absolute .mp4/.mov path and start
// `tauri dev`. The report lands in %TEMP%/foxcull-scrub-probe.json (via the
// scrub_probe_report command) and in the app log, so an agent driving the dev
// loop can read the verdict without seeing the webview console.
//
// The probe exercises the exact pipeline the real engine will use:
//   read_file_range → mp4box.js moov-only parse (handles moov-at-end via
//   appendBuffer's next-position return) → sample table → hvcC/avcC
//   description → VideoDecoder → decode N sync samples, timing each.

import * as MP4Box from "mp4box";
import { invoke } from "@tauri-apps/api/core";
import { api } from "./api";

const PARSE_CHUNK = 2 * 1024 * 1024;
/** Keyframes decoded for timing. First one includes decoder warm-up. */
const PROBE_KEYFRAMES = 6;

interface SampleInfo {
  offset: number;
  size: number;
  cts: number;
  timescale: number;
  is_sync: boolean;
}

async function readRange(path: string, offset: number, len: number): Promise<Uint8Array> {
  return new Uint8Array(await api.readFileRange(path, offset, len));
}

/** Progressively feed mp4box until the moov is parsed. For camera files the
 *  moov usually sits AFTER the multi-GB mdat; mp4box's appendBuffer returns
 *  the next byte offset it wants, which jumps clean over mdat — so this reads
 *  a few MB total regardless of file size. */
async function parseMoov(path: string): Promise<{ info: any; file: any; bytesRead: number }> {
  const file = MP4Box.createFile();
  let info: any = null;
  let err: unknown = null;
  file.onReady = (i: any) => (info = i);
  file.onError = (e: unknown) => (err = e);

  let pos = 0;
  let bytesRead = 0;
  for (let i = 0; i < 4096 && !info && !err; i++) {
    const bytes = await readRange(path, pos, PARSE_CHUNK);
    if (bytes.length === 0) break; // EOF
    bytesRead += bytes.length;
    // mp4box wants an ArrayBuffer with a fileStart property. The invoke result
    // is a fresh ArrayBuffer already, so no copy is needed.
    const ab = bytes.buffer as ArrayBuffer & { fileStart: number };
    ab.fileStart = pos;
    const next = file.appendBuffer(ab);
    if (bytes.length < PARSE_CHUNK) break; // short read = EOF
    pos = typeof next === "number" ? next : pos + bytes.length;
  }
  if (err) throw new Error(`mp4box error: ${String(err)}`);
  if (!info) throw new Error("moov not found (unsupported container?)");
  return { info, file, bytesRead };
}

/** Serialize the codec-config box (hvcC/avcC/…) minus its 8-byte box header —
 *  the `description` bytes VideoDecoder expects for length-prefixed samples. */
function codecDescription(file: any, trackId: number): Uint8Array {
  const entries = file.getTrackById(trackId)?.mdia?.minf?.stbl?.stsd?.entries ?? [];
  for (const e of entries) {
    const box = e.hvcC ?? e.avcC ?? e.vpcC ?? e.av1C;
    if (box) {
      const stream = new MP4Box.DataStream(undefined, 0, MP4Box.DataStream.BIG_ENDIAN);
      box.write(stream);
      return new Uint8Array(stream.buffer, 8, stream.getPosition() - 8);
    }
  }
  throw new Error("no hvcC/avcC/vpcC/av1C in stsd");
}

export async function runScrubProbe(path: string): Promise<void> {
  const report: Record<string, unknown> = {
    path,
    startedAt: new Date().toISOString(),
    userAgent: navigator.userAgent,
  };
  try {
    report.hasVideoDecoder = typeof VideoDecoder !== "undefined";
    if (!report.hasVideoDecoder) throw new Error("VideoDecoder API missing in this WebView2");

    // ── 1. Demux: moov-only parse + sample table ──────────────────────────
    let t = performance.now();
    const { info, file, bytesRead } = await parseMoov(path);
    report.moovParseMs = Math.round(performance.now() - t);
    report.moovBytesRead = bytesRead;

    const track = info.videoTracks?.[0];
    if (!track) throw new Error("no video track");
    report.codec = track.codec;
    report.coded = { w: track.video?.width, h: track.video?.height };
    report.nbSamples = track.nb_samples;
    report.durationS = info.duration && info.timescale ? +(info.duration / info.timescale).toFixed(2) : null;

    const samples: SampleInfo[] = file.getTrackById(track.id)?.samples ?? [];
    report.sampleTableLen = samples.length;
    const syncs = samples.filter((s) => s.is_sync);
    report.keyframes = syncs.length;
    if (syncs.length === 0) throw new Error("sample table empty or no sync samples — mp4box did not build sample lists from moov");
    // Keyframe cadence tells us the worst-case precise-seek decode span.
    if (syncs.length > 1 && samples.length > 0) {
      report.avgGopFrames = Math.round(samples.length / syncs.length);
    }

    // ── 2. isConfigSupported, hardware and any ────────────────────────────
    const description = codecDescription(file, track.id);
    report.descriptionBytes = description.length;
    const base: VideoDecoderConfig = {
      codec: track.codec,
      codedWidth: track.video?.width,
      codedHeight: track.video?.height,
      description,
    };
    const hw = await VideoDecoder.isConfigSupported({ ...base, hardwareAcceleration: "prefer-hardware" });
    const any = await VideoDecoder.isConfigSupported(base);
    report.configSupported = { preferHardware: !!hw.supported, noPreference: !!any.supported };
    if (!any.supported) throw new Error(`isConfigSupported=false for ${track.codec}`);

    // ── 3. Decode real keyframes on ONE persistent decoder, timing each ───
    const frames: { ms: number; readMs: number; w: number; h: number; format: string | null }[] = [];
    let decodeError: unknown = null;
    let resolveFrame: (() => void) | null = null;
    let lastFrame: VideoFrame | null = null;
    const decoder = new VideoDecoder({
      output: (frame) => {
        lastFrame = frame;
        resolveFrame?.();
      },
      error: (e) => {
        decodeError = e;
        resolveFrame?.();
      },
    });
    decoder.configure({ ...base, hardwareAcceleration: hw.supported ? "prefer-hardware" : "no-preference" });

    // Spread the probe across the clip like a real drag would.
    const picks: SampleInfo[] = [];
    for (let i = 0; i < PROBE_KEYFRAMES; i++) {
      picks.push(syncs[Math.floor((i * (syncs.length - 1)) / Math.max(1, PROBE_KEYFRAMES - 1))]);
    }

    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d");
    let painted: number[] | null = null;

    for (const s of picks) {
      const tRead = performance.now();
      const data = await readRange(path, s.offset, s.size);
      const readMs = performance.now() - tRead;
      const chunk = new EncodedVideoChunk({
        type: "key",
        timestamp: Math.round((s.cts / s.timescale) * 1e6),
        data,
      });
      t = performance.now();
      const got = new Promise<void>((res) => (resolveFrame = res));
      decoder.decode(chunk);
      // flush() forces the decoder to emit rather than pipeline the frame —
      // exactly the latency profile scrubbing has (one frame wanted NOW).
      await decoder.flush().catch(() => {});
      await got;
      if (decodeError) throw new Error(`decode failed: ${String(decodeError)}`);
      const frame = lastFrame!;
      frames.push({
        ms: Math.round(performance.now() - t),
        readMs: Math.round(readMs),
        w: frame.displayWidth,
        h: frame.displayHeight,
        format: frame.format,
      });
      // Paint the last one and sample a pixel — proves the canvas conversion
      // path (10-bit → display) works, not just that a frame object exists.
      if (s === picks[picks.length - 1] && ctx) {
        canvas.width = 320;
        canvas.height = Math.round((320 * frame.displayHeight) / frame.displayWidth);
        ctx.drawImage(frame, 0, 0, canvas.width, canvas.height);
        const px = ctx.getImageData(canvas.width >> 1, canvas.height >> 1, 1, 1).data;
        painted = [px[0], px[1], px[2], px[3]];
      }
      frame.close();
      lastFrame = null;
    }
    decoder.close();
    report.decodes = frames;
    report.centerPixelRGBA = painted;
    // The verdict number: hot-decoder latency = decodes after the first.
    const hot = frames.slice(1).map((f) => f.ms);
    report.hotDecodeAvgMs = hot.length ? Math.round(hot.reduce((a, b) => a + b, 0) / hot.length) : null;
    report.verdict = "OK";
  } catch (e) {
    report.verdict = "FAIL";
    report.error = e instanceof Error ? `${e.message}` : String(e);
  }
  report.finishedAt = new Date().toISOString();
  try {
    await invoke("scrub_probe_report", { report: JSON.stringify(report, null, 2) });
  } catch {
    console.error("scrub probe report failed to persist", report);
  }
  console.log("scrub probe", report);
}

/** Called from +page.svelte onMount in dev; inert unless VITE_SCRUB_PROBE set. */
export function maybeRunScrubProbe(): void {
  const target = import.meta.env.VITE_SCRUB_PROBE as string | undefined;
  if (!import.meta.env.DEV || !target) return;
  // Give the app shell a beat to settle so timings aren't polluted by startup.
  setTimeout(() => void runScrubProbe(target), 1500);
}
