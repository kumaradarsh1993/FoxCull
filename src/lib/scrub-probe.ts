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

// The demux/description helpers are imported from the ENGINE rather than
// copied, so re-running this probe always measures the parser the app actually
// ships. A forked copy here would quietly stop testing the real thing.
import { invoke } from "@tauri-apps/api/core";
import { api } from "./api";
import { parseMoov, codecDescription, ScrubEngine, paintFrame } from "./scrub-engine";

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

/**
 * Drive `ScrubEngine` the way a real drag does: a burst of requests far faster
 * than decoding, then a final exact request — and check what actually came out.
 *
 * What this catches that phase 3 cannot: a backlog (requests queueing instead
 * of the newest winning, which is what makes a picture lag the cursor), a
 * coalescer that drops the LAST request too, painting failures, and an exact
 * decode that lands on the keyframe instead of the requested frame.
 */
async function exerciseEngine(path: string): Promise<Record<string, unknown>> {
  const out: Record<string, unknown> = {};
  const t0 = performance.now();
  const engine = await ScrubEngine.open(path);
  out.openMs = Math.round(performance.now() - t0);
  out.rotation = engine.index.rotation;
  out.keyframes = engine.index.syncIdx.length;
  try {
    const canvas = document.createElement("canvas");
    const dur = engine.index.durationS;

    // A drag: 24 positions across the clip, issued every 8 ms — roughly 3x
    // faster than a frame can decode, so coalescing MUST drop most of them.
    const painted: { t: number; ms: number; w: number; h: number }[] = [];
    const started = performance.now();
    for (let i = 0; i < 24; i++) {
      const t = (i / 23) * dur * 0.98;
      const at = performance.now();
      engine.request(t, false, (f) => {
        paintFrame(canvas, f, 900, 600, engine.index.rotation);
        painted.push({
          t: +(f.timestamp / 1e6).toFixed(2),
          ms: Math.round(performance.now() - at),
          w: canvas.width,
          h: canvas.height,
        });
      });
      await new Promise((r) => setTimeout(r, 8));
    }
    out.dragLastStats = engine.stats;
    // Let the tail finish.
    await new Promise((r) => setTimeout(r, 600));
    out.dragRequests = 24;
    out.dragPainted = painted.length;
    out.dragSpanMs = Math.round(performance.now() - started);
    out.dragFrames = painted;
    // Coalescing is working iff far fewer frames painted than were requested,
    // AND the ones that did paint are spread across the clip (not a prefix).
    out.dragLastT = painted.length ? painted[painted.length - 1].t : null;

    // Release: the exact frame. Ask for a time deliberately BETWEEN keyframes
    // so "exact" is distinguishable from "nearest keyframe".
    const target = Math.min(dur * 0.5 + 0.37, dur - 0.05);
    out.exactTarget = +target.toFixed(3);
    out.exactKeyframeWouldBe = +engine.keyTimeFor(target).toFixed(3);
    const exactAt = performance.now();
    const got = await new Promise<number | null>((resolve) => {
      let done = false;
      engine.request(target, true, (f) => {
        done = true;
        paintFrame(canvas, f, 900, 600, engine.index.rotation);
        resolve(f.timestamp / 1e6);
      });
      setTimeout(() => !done && resolve(null), 5000);
    });
    out.exactMs = Math.round(performance.now() - exactAt);
    out.exactLandedAt = got == null ? null : +got.toFixed(3);
    out.exactErrorMs =
      got == null ? null : Math.round(Math.abs(got - target) * 1000);
    // Where the time actually went (read vs decode, and whether flush stalled).
    out.exactStats = engine.stats;
  } finally {
    engine.close();
  }
  return out;
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
    const { info, file } = await parseMoov(path);
    report.moovParseMs = Math.round(performance.now() - t);

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
    if (!description) throw new Error("no hvcC/avcC/vpcC/av1C in stsd");
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

    // ── 4. The real engine, driven through a simulated drag ───────────────
    // Phases 1-3 prove the PLATFORM can do this. This phase proves OUR class
    // does — index, latest-wins coalescing, painting, and the exact-frame
    // decode on release — without needing a hand on the mouse.
    report.engine = await exerciseEngine(path);
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
