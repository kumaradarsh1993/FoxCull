// DEV-ONLY fake backend, for inspecting the UI in a plain browser.
//
// `npm run dev` + a normal browser at http://localhost:1460 has no Tauri: every
// `invoke` would fail and the app would sit on an empty tree. Installed from
// `src/hooks.client.ts` ONLY when `import.meta.env.DEV` is true AND there is no
// real Tauri bridge — so it can never run inside the app, and production builds
// drop it entirely.
//
// It exists for layout audits (docs/UX-AUDIT-*.md): a realistic library — long
// drive and folder names, a few hundred photos/RAWs/videos in mixed aspect
// ratios, marks, tags, events — so every surface can be checked at real window
// sizes and themes. Settings persist in localStorage so a theme or UI scale
// survives a reload. Nothing here is a spec of backend behaviour.

type Args = Record<string, any>;

const HOME = "/Users/demo";
const SD = "/Volumes/SD_Card";
const SSD = "/Volumes/Samsung T7 Shield — Travel & Client Work 2026";

const DRIVES = [
  { name: "Home", path: HOME, has_children: true },
  { name: "Macintosh HD", path: "/", has_children: true },
  { name: "SD_Card", path: SD, has_children: true },
  { name: "Samsung T7 Shield — Travel & Client Work 2026", path: SSD, has_children: true },
];

const SUBFOLDERS = [
  "DCIM",
  "2026-09-14 Seattle — Discovery Park sunset walk with the whole family",
  "Drone",
  "Exports",
  "Client — Mahindra launch event (final selects)",
  "Phone backup",
];

/** Deterministic PRNG so every reload shows the same library. */
function rng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

const ASPECTS: [number, number][] = [
  [3, 2],
  [2, 3],
  [16, 9],
  [4, 3],
  [1, 1],
  [9, 16],
];

function art(seed: number, w: number, h: number, video = false): string {
  const r = rng(seed * 7919 + 13);
  const hue = Math.floor(r() * 360);
  const hue2 = (hue + 40 + Math.floor(r() * 80)) % 360;
  const sun = { x: 20 + r() * 60, y: 20 + r() * 35, rad: 6 + r() * 10 };
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}">
<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="hsl(${hue},55%,62%)"/><stop offset="1" stop-color="hsl(${hue2},45%,28%)"/></linearGradient></defs>
<rect width="100%" height="100%" fill="url(#g)"/>
<circle cx="${(sun.x / 100) * w}" cy="${(sun.y / 100) * h}" r="${(sun.rad / 100) * Math.min(w, h)}" fill="hsl(${(hue + 180) % 360},80%,85%)" opacity=".85"/>
<path d="M0 ${h * 0.72} Q ${w * 0.3} ${h * 0.55} ${w * 0.55} ${h * 0.7} T ${w} ${h * 0.62} V ${h} H 0 Z" fill="hsl(${hue2},35%,18%)" opacity=".9"/>
${video ? `<circle cx="${w / 2}" cy="${h / 2}" r="${Math.min(w, h) * 0.09}" fill="rgba(0,0,0,.45)"/><path d="M${w / 2 - Math.min(w, h) * 0.03} ${h / 2 - Math.min(w, h) * 0.045} l${Math.min(w, h) * 0.075} ${Math.min(w, h) * 0.045} l${-Math.min(w, h) * 0.075} ${Math.min(w, h) * 0.045}z" fill="#fff"/>` : ""}
</svg>`;
  return "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);
}

interface Item {
  name: string;
  path: string;
  rel: string;
  kind: "image" | "raw" | "video";
  ext: string;
  mtime: number;
  size: number;
  rating: number;
  label: string | null;
  flag: "pick" | "reject" | null;
  tags: string[];
  events: string[];
  missing: boolean;
  ranges?: { in_s: number; out_s: number }[];
  seed: number;
  aspect: [number, number];
}

const byPath = new Map<string, Item>();

function folderItems(dir: string): Item[] {
  const r = rng([...dir].reduce((a, c) => a + c.charCodeAt(0), 0));
  const n = dir === "/" || dir === HOME ? 0 : 60 + Math.floor(r() * 180);
  const labels = [null, null, null, "red", "green", "yellow", "blue", "purple"];
  const tags = ["family", "sunset", "keeper", "instagram", "client-final", "b-roll"];
  const out: Item[] = [];
  const base = Date.UTC(2026, 8, 14, 17, 0, 0) / 1000;
  for (let i = 0; i < n; i++) {
    const roll = r();
    const kind: Item["kind"] = roll < 0.2 ? "video" : roll < 0.4 ? "raw" : "image";
    const stem =
      i % 17 === 5
        ? `DJI_20260914_${String(1000 + i)}_panorama_stitched_from_twelve_frames_final_v2`
        : kind === "video"
          ? `DJI_20260914_${String(170000 + i * 7)}_D`
          : `DSC_${String(4000 + i).padStart(4, "0")}`;
    const ext = kind === "video" ? (i % 3 ? "MP4" : "MOV") : kind === "raw" ? "NEF" : "JPG";
    const name = `${stem}.${ext}`;
    const path = `${dir.replace(/\/$/, "")}/${name}`;
    const it: Item = {
      name,
      path,
      rel: path.replace(/^\/Volumes\/[^/]+\//, ""),
      kind,
      ext: ext.toLowerCase(),
      mtime: base + i * 47,
      size: Math.floor((kind === "video" ? 400e6 : kind === "raw" ? 26e6 : 9e6) * (0.4 + r())),
      rating: r() < 0.35 ? 1 + Math.floor(r() * 5) : 0,
      label: labels[Math.floor(r() * labels.length)],
      flag: r() < 0.15 ? "pick" : r() < 0.1 ? "reject" : null,
      tags: r() < 0.2 ? [tags[Math.floor(r() * tags.length)], tags[Math.floor(r() * tags.length)]] : [],
      events: i > 20 && i < 70 ? ["Seattle — Discovery Park"] : [],
      // A few "?" entries, for the missing-item menus (folder and grid).
      missing: i % 23 === 11 && !forgotten.has(path),
      // Some clips carry in/out ranges marked in Focus (the ✂ badge).
      ranges: kind === "video" && i % 4 === 1 ? [{ in_s: 3, out_s: 9.5 }, { in_s: 21, out_s: 30 }] : kind === "video" && i % 4 === 3 ? [{ in_s: 5, out_s: 12 }] : undefined,
      seed: i + n,
      aspect: ASPECTS[Math.floor(r() * ASPECTS.length)],
    };
    byPath.set(path, it);
    out.push(it);
  }
  return out;
}

function artFor(path: string, max: number): string {
  const it = byPath.get(path);
  const [a, b] = it?.aspect ?? [3, 2];
  const w = a >= b ? max : Math.round((max * a) / b);
  const h = a >= b ? Math.round((max * b) / a) : max;
  return art(it?.seed ?? 1, w, h, it?.kind === "video");
}

const STORE_KEY = "foxcull-mock-store";
function storeRead(): Record<string, unknown> {
  try {
    return JSON.parse(localStorage.getItem(STORE_KEY) || "{}");
  } catch {
    return {};
  }
}
function storeWrite(v: Record<string, unknown>) {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(v));
  } catch {
    /* private window */
  }
}

let callbackId = 1;

/** Missing entries the user removed from the (fake) catalog. */
const forgotten = new Set<string>();
/** Jobs stopped through cancel_job. */
const stopped = new Set<string>();

/** The fake merge's status (merge_status), shaped like the backend's. */
const mockMerge: Record<string, any> = { state: "idle", paused: false, pct: 0 };

/** Window inboxes and the shared clipboard live in localStorage so a second
 *  tab opened as `?window=edit` / `?window=merge` sees what the library sent. */
function lsGet(key: string): any {
  try {
    return JSON.parse(localStorage.getItem(key) || "null");
  } catch {
    return null;
  }
}
function lsSet(key: string, v: unknown) {
  try {
    if (v == null) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* private window */
  }
}

/** Play a backend job into the job centre the way `activity` events would. */
async function fakeJob(id: string, total: number, ms: number, o: { unit?: "bytes"; label?: string; detail?: (f: number) => string } = {}) {
  const { activity } = await import("$lib/activity.svelte");
  const steps = Math.max(1, Math.round(ms / 150));
  for (let k = 0; k <= steps; k++) {
    if (stopped.delete(id)) return false;
    while (id === "merge" && mockMerge.paused) {
      await new Promise((r) => setTimeout(r, 150));
      if (stopped.delete(id)) return false;
    }
    const f = k / steps;
    if (id === "merge") mockMerge.pct = Math.round(f * 100);
    // Slow start, like a real copy warming up, so the ETA visibly settles.
    activity.ingest({ id, label: o.label ?? "", done: Math.round(total * f), total, state: "running", unit: o.unit, cancellable: true, detail: o.detail?.(f) });
    await new Promise((r) => setTimeout(r, 150));
  }
  return true;
}

// A drive's Trash: the first 14 items of a folder, "deleted" over the last few
// days, so the Trash view has real rows to restore and purge.
const TRASH = "FoxCull Trash";
const isTrash = (dir: string) => dir.replace(/\/$/, "").endsWith(`/${TRASH}`);
let trashRows: { stored: string; orig: string; path: string; name: string; kind: string; ext: string; deleted_at: number }[] | null = null;
function trashFor(root: string) {
  if (!trashRows) {
    const now = Math.floor(Date.now() / 1000);
    trashRows = folderItems(`${root}/DCIM`).slice(0, 14).map((it, i) => {
      const path = `${root}/${TRASH}/${it.name}`;
      byPath.set(path, { ...it, path });
      return { stored: it.name, orig: `DCIM/100MSDCF/${it.name}`, path, name: it.name, kind: it.kind, ext: it.ext, deleted_at: now - i * 7200 - (i > 8 ? 86400 * 3 : 0) };
    });
  }
  return trashRows;
}
/** Folders made with "New subfolder", so the tree can be checked to show them. */
const created = new Map<string, string[]>();

const HANDLERS: Record<string, (a: Args) => unknown> = {
  list_drives: () => DRIVES,
  create_folder: (a) => {
    const list = created.get(a.parent) ?? [];
    list.push(a.name);
    created.set(a.parent, list);
    return `${a.parent.replace(/\/$/, "")}/${a.name}`;
  },
  list_tree: (a) =>
    [...(created.get(a.dir) ?? []).map((name) => ({ name, path: `${a.dir.replace(/\/$/, "")}/${name}`, has_children: true }))].concat(
    a.dir === "/"
      ? [{ name: "Users", path: "/Users", has_children: true }]
      : SUBFOLDERS.map((name) => ({ name, path: `${a.dir.replace(/\/$/, "")}/${name}`, has_children: true }))),
  folder_counts: (a) => (a.paths as string[]).map((path, i) => ({ path, count: [132, 4821, 38, 0, 1207, 96][i % 6] })),
  set_library_root: (a) => ({
    root: a.root,
    dir: `${a.root}/_FoxCull`,
    catalog: `${a.root}/_FoxCull/catalog.sqlite`,
    recycle: `${a.root}/FoxCull Trash`,
    on_drive: true,
    writable: true,
  }),
  library_info: () => HANDLERS.set_library_root({ root: SD }),
  list_folder_media: (a) => {
    if (isTrash(a.dir)) {
      const root = a.dir.replace(/\/?FoxCull Trash\/?$/, "");
      return trashFor(root).map((r) => {
        const { seed: _s, aspect: _a, ...rest } = byPath.get(r.path)!;
        return { ...rest, rating: 0, label: null, flag: null, tags: [], events: [] };
      });
    }
    return folderItems(a.dir)
      .filter((i) => !(i.missing && forgotten.has(i.path)))
      .map(({ seed: _s, aspect: _a, ...rest }) => rest);
  },
  folder_writable: () => true,
  thumbnail: (a) => artFor(a.path, Math.min(a.max ?? 320, 480)),
  loupe_src: (a) => (/\.(mp4|mov)$/i.test(a.path) ? a.path : artFor(a.path, 1600)),
  video_poster: (a) => artFor(a.path, 480),
  video_poster_hires: (a) => artFor(a.path, 1600),
  capture_dates: (a) => (a.paths as string[]).map((path) => ({ path, captured: byPath.get(path)?.mtime ?? 0 })),
  probe_media_info: (a) => ({
    // static/dev-sample.mp4 is 40.000 s. A container a few ms longer than what
    // plays is common on phone footage, and it froze Edit playback on the real
    // app (2026-10-04): keep the mock that way so the harness covers it.
    duration: 40.005,
    ...(() => {
      // Another tab (the Edit window) hasn't listed this folder: do it now.
      if (!byPath.has(a.path)) folderItems(a.path.replace(/\/[^/]*$/, ""));
      const it = byPath.get(a.path);
      const vert = it ? it.aspect[0] < it.aspect[1] : false;
      const small = it ? it.seed % 5 === 0 : false;
      return { width: vert ? 2160 : small ? 1920 : 3840, height: vert ? 3840 : small ? 1080 : 2160 };
    })(),
    fps: (byPath.get(a.path)?.seed ?? 0) % 7 === 3 ? 29.97 : 59.94,
    codec: "hevc",
    camera: byPath.get(a.path)?.kind === "video" ? "DJI Mini 4 Pro" : "NIKON D5200",
    captured: byPath.get(a.path)?.mtime ?? null,
    hdr: false,
  }),
  list_edit_sources: (a) =>
    folderItems(a.dir)
      .filter((i) => i.kind === "video")
      .map((i) => ({ name: i.name, path: i.path, kind: "video", ext: i.ext, mtime: i.mtime, size: i.size })),
  video_durations: (a) =>
    (a.paths as string[])
      .filter((p) => byPath.get(p)?.kind === "video")
      .map((p) => ({ path: p, duration: 20 + ((byPath.get(p)!.seed * 37) % 300) })),
  // Mostly the Osmo main set. Every 5th averages 59.71 (variable rate: must
  // NOT be flagged), every 7th is 29.97 fps 8-bit and every 9th a slightly
  // smaller crop (both fixable by converting), every 11th vertical (can't be).
  merge_probe: (a) =>
    (a.paths as string[])
      .map((p, i) => {
        // Another tab (the Merge window) hasn't listed this folder: do it now.
        if (!byPath.has(p)) folderItems(p.replace(/\/[^/]*$/, ""));
        const it = byPath.get(p)!;
        if (it.kind !== "video")
          return { path: p, name: it.name, kind: "photo", size: it.size, duration: 0, captured: it.mtime, width: 0, height: 0, fps: 0, fps_class: 0, rotation: 0, vcodec: "", profile: "", pix_fmt: "", vbitrate: 0, color: "", acodec: null, arate: 0, alayout: "", signature: "", error: null };
        const slow = i % 7 === 6;
        const vert = i % 11 === 10;
        const crop = i % 9 === 8;
        const [w, h] = vert ? [1728, 3072] : crop ? [3712, 2088] : [3840, 2160];
        const fps = slow ? 29.97 : i % 5 === 4 ? 59.71 : 59.94;
        const fpsClass = slow ? 30 : 60;
        const profile = slow ? "Main" : "Main 10";
        const pix = slow ? "yuv420p" : "yuv420p10le";
        return {
          path: p,
          name: it.name,
          kind: "video",
          size: it.size,
          duration: 20 + ((it.seed * 37) % 300),
          captured: it.mtime,
          width: w,
          height: h,
          fps,
          fps_class: fpsClass,
          rotation: 0,
          vcodec: "hevc",
          profile,
          pix_fmt: pix,
          vbitrate: slow ? 70000 : 108000,
          color: "sdr",
          acodec: "aac",
          arate: 48000,
          alayout: "stereo",
          signature: `hevc|${profile}|${pix}|${w}x${h}|${fpsClass}|0|sdr|aac|48000|stereo`,
          error: null,
        };
      })
      .sort((x, y) => (x.captured ?? 0) - (y.captured ?? 0) || x.name.localeCompare(y.name)),
  // The card is nearly full, so the dialog's not-enough-space state shows.
  disk_free: (a) => (a.path.startsWith("/Users") ? 76e9 : a.path === SD ? 9e9 : 1.2e12),
  merge_videos: async (a) => {
    const n = (a.req.paths as string[]).length;
    const label = `Merging ${n} clips → ${a.req.name}.mp4`;
    Object.assign(mockMerge, {
      state: "running", paused: false, pct: 0, label, name: `${a.req.name}.mp4`, out_path: `${a.req.destDir}/${a.req.name}.mp4`,
      dest_dir: a.req.destDir, clips: n, parts: a.req.parts?.length || n, total_s: 60 * n, in_bytes: 63.1e9, convert: !!a.req.convert, detail: null,
      started_ms: Date.now(), finished_ms: 0, out_bytes: 0, error: null,
    });
    const ok = await fakeJob("merge", 100, a.req.convert ? 9000 : 6000, {
      label,
      detail: (f) => (a.req.convert ? `Converting clip ${Math.min(n, 1 + Math.floor(f * n))} of ${n}` : `${(f * 63.1).toFixed(1)} of 63.1 GB · 1.1 GB/s`),
    });
    const { activity } = await import("$lib/activity.svelte");
    if (!ok) {
      Object.assign(mockMerge, { state: "cancelled", finished_ms: Date.now(), paused: false });
      activity.ingest({ id: "merge", label: "Merge stopped", done: 0, total: 100, state: "cancelled", detail: "Nothing was saved" });
      throw "export cancelled";
    }
    Object.assign(mockMerge, { state: "done", pct: 100, finished_ms: Date.now(), out_bytes: 63.1e9 });
    activity.ingest({ id: "merge", label: label.replace("Merging", "Merged"), done: 100, total: 100, state: "done", detail: "63.1 GB in 1 min 2 s", path: mockMerge.out_path });
    return { path: `${a.req.destDir}/${a.req.name}.mp4`, bytes: a.req.convert ? 88.4e9 : 63.1e9 };
  },
  cancel_job: (a) => {
    stopped.add(a.id);
    if (a.id === "merge") mockMerge.paused = false;
    return true;
  },
  merge_status: () => ({ ...mockMerge }),
  merge_pause: async (a) => {
    mockMerge.paused = !!a.paused;
    const { activity } = await import("$lib/activity.svelte");
    activity.ingest({ id: "merge", label: mockMerge.label, done: mockMerge.pct, total: 100, state: "running", detail: a.paused ? "Paused" : undefined, paused: !!a.paused, cancellable: true });
    return { ...mockMerge };
  },
  merge_dismiss: () => {
    if (mockMerge.state !== "running") Object.assign(mockMerge, { state: "idle", paused: false, pct: 0 });
  },
  open_tool_window: (a) => {
    const key = `foxcull-mock-inbox-${a.kind}`;
    if (a.payload) lsSet(key, [...(lsGet(key) ?? []), a.payload]);
    console.info(`[mock-ipc] open_tool_window ${a.kind}: open ${location.origin}/?window=${a.kind} to see it`);
    return null;
  },
  take_tool_inbox: (a) => {
    const key = `foxcull-mock-inbox-${a.kind}`;
    const xs = lsGet(key) ?? [];
    lsSet(key, null);
    return xs;
  },
  tile_windows: () => ({ tiled: true }),
  show_in_library: (a) => {
    console.info(`[mock-ipc] show_in_library ${a.path}`);
    return null;
  },
  stash_set: (a) => lsSet(`foxcull-mock-stash-${a.key}`, a.value),
  stash_get: (a) => lsGet(`foxcull-mock-stash-${a.key}`),
  quit_app: () => null,
  // A cross-drive move: bytes, speed and a Stop button in the job centre.
  move_media_files: async (a) => {
    const paths = a.paths as string[];
    const bytes = paths.reduce((t, p) => t + (byPath.get(p)?.size ?? 9e6), 0);
    const cross = !a.dest.startsWith(SD);
    const job = a.job ?? "move";
    const ok = await fakeJob(job, cross || a.copy ? bytes : 0, cross || a.copy ? 7000 : 300, {
      unit: cross || a.copy ? "bytes" : undefined,
      detail: (f) => `${Math.min(paths.length, 1 + Math.floor(f * paths.length))} of ${paths.length} · ${paths[Math.min(paths.length - 1, Math.floor(f * paths.length))].split("/").pop()}`,
    });
    const moved = ok ? paths.length : Math.floor(paths.length / 2);
    return {
      moved,
      dest: a.dest,
      files: paths.slice(0, moved).map((p) => ({ from: p, to: `${a.dest}/${p.split("/").pop()}` })),
      failed: [],
      errors: [],
      copied: !!a.copy,
      cross_drive: cross,
      cancelled: !ok,
    };
  },
  list_tags: () => [
    ["family", 42],
    ["sunset", 17],
    ["keeper", 88],
    ["instagram", 12],
    ["client-final", 31],
  ],
  list_events: () => [
    { id: 1, name: "Seattle — Discovery Park", created_at: 0, cover_rel: null, count: 49 },
    { id: 2, name: "Mahindra launch", created_at: 0, cover_rel: null, count: 120 },
  ],
  list_rejected: () => [],
  list_trash: () => trashFor(SD),
  restore_trash: (a) => {
    const n = trashFor(SD).length;
    trashRows = trashFor(SD).filter((r) => !(a.stored as string[]).includes(r.stored));
    return { restored: n - trashRows.length, failed: [] };
  },
  purge_trash: (a) => {
    const n = trashFor(SD).length;
    trashRows = trashFor(SD).filter((r) => !(a.stored as string[]).includes(r.stored));
    return n - trashRows.length;
  },
  list_missing: () => [...byPath.values()].filter((i) => i.missing && !forgotten.has(i.path)).map((i) => i.rel),
  forget_missing: (a) => {
    for (const it of byPath.values()) if ((a.rels as string[]).includes(it.rel)) forgotten.add(it.path);
    return (a.rels as string[]).length;
  },
  catalog_scan: () => ({ tracked: 0, missing: 0, relinked: 0, still_missing: 0, scanned_files: 0, elapsed_ms: 3 }),
  get_trim: () => null,
  // Segments persist per tab in localStorage, starting from the item's ranges,
  // so the Merge tab sees what the library tab marked.
  get_video_segments: (a) => lsGet(`foxcull-mock-segs-${a.path}`) ?? byPath.get(a.path)?.ranges ?? [],
  video_ranges: (a) =>
    Object.fromEntries((a.paths as string[]).map((p) => [p, lsGet(`foxcull-mock-segs-${p}`) ?? byPath.get(p)?.ranges ?? []])),
  set_video_segments: (a) => {
    lsSet(`foxcull-mock-segs-${a.path}`, a.segments);
    const it = byPath.get(a.path);
    if (it) it.ranges = a.segments;
    return null;
  },
  video_scrubstrip_cached: () => null,
  video_filmstrip_cached: () => null,
  video_filmstrip: () => ({ src: "/dev-sprite.jpg", cols: 8, rows: 5, count: 40, tile_w: 160, tile_h: 90, duration: 40 }),
  video_range_strip: (a) => ({ src: "/dev-sprite.jpg", cols: 8, rows: 5, count: 40, tile_w: 160, tile_h: 90, duration: Math.max(0.1, a.outS - a.inS) }),
  // A 112 BPM song, 3 minutes: beats from 0.4 s, a downbeat every 4.
  analyze_beats: async (a) => {
    await new Promise((r) => setTimeout(r, 600));
    if (!/\.(mp3|m4a|wav|aac|flac|ogg|opus|aiff?)$/i.test(a.path)) throw "That isn't an audio file FoxCull can use (MP3, M4A, AAC, WAV, FLAC, OGG, Opus, AIFF).";
    const period = 60 / 112;
    const beats = Array.from({ length: Math.floor((180 - 0.4) / period) }, (_, i) => +(0.4 + i * period).toFixed(3));
    const wave = Array.from({ length: 180 * 40 }, (_, i) => {
      const t = i / 40;
      const ph = ((t - 0.4) / period) % 1;
      const kick = ph >= 0 && ph < 0.12 ? 1 - ph * 5 : 0;
      return Math.min(1, 0.25 + 0.15 * Math.sin(t / 7) + 0.55 * kick * (Math.round((t - 0.4) / period) % 4 === 0 ? 1 : 0.6));
    });
    return { duration: 180, bpm: 112, beats, major: beats.filter((_, i) => i % 4 === 0), wave, wave_rate: 40 };
  },
  reel_export: async (a) => {
    const n = a.req.pieces.length;
    const ok = await fakeJob("reel-export", 100, 5000, { label: `Exporting reel → ${a.req.name}.mp4`, detail: () => `${n} clips · 1080×1920` });
    const { activity } = await import("$lib/activity.svelte");
    const path = `${a.req.destDir}/${a.req.name}.mp4`;
    if (!ok) {
      activity.ingest({ id: "reel-export", label: "Export cancelled", done: 100, total: 100, state: "done" });
      throw "export cancelled";
    }
    activity.ingest({ id: "reel-export", label: `Exported ${a.req.name}.mp4`, done: 100, total: 100, state: "done", detail: "48 MB in 5 s", path });
    return { path, mode: "reencoded-x264", reencoded: true };
  },
  video_proxy_cached: () => null,
  path_exists: () => true,
  is_system_root: (a) => a.dir === "/",
  suggested_folders: () => [
    { label: "SD_Card", path: `${SD}/DCIM`, kind: "card" },
    { label: "Pictures", path: `${HOME}/Pictures`, kind: "pictures" },
    { label: "Movies", path: `${HOME}/Movies`, kind: "videos" },
    { label: "Desktop", path: `${HOME}/Desktop`, kind: "desktop" },
    { label: "Downloads", path: `${HOME}/Downloads`, kind: "downloads" },
  ],
  update_status: () => ({
    product: "FoxCull",
    current: "1.5.0",
    current_is_nightly: false,
    stable: {
      tag: "v1.5.0",
      version: "1.5.0",
      html_url: "https://github.com/kumaradarsh1993/FoxCull/releases/tag/v1.5.0",
      published_at: "2026-09-27T05:25:15Z",
      prerelease: false,
      newer: false,
      asset: null,
      summary: "Excluded folders, a calmer welcome screen, and no more whole-drive scans.",
    },
    nightly: null,
    can_self_install: false,
    update_available: false,
    releases_url: "https://github.com/kumaradarsh1993/FoxCull/releases",
  }),
  cast_status: () => ({ connected: false, deviceName: null, playingPath: null, playerState: null, currentTime: null, duration: null }),
  cast_discover: () => [{ id: "tv1", name: "Living Room TV", addr: "192.168.1.20", port: 8009 }],

  // Plugins.
  "plugin:store|load": () => 1,
  "plugin:store|get_store": () => 1,
  "plugin:store|get": (a) => {
    const s = storeRead();
    return [s[a.key], a.key in s];
  },
  "plugin:store|set": (a) => {
    const s = storeRead();
    s[a.key] = a.value;
    storeWrite(s);
  },
  "plugin:store|save": () => null,
  "plugin:event|listen": () => callbackId++,
  "plugin:event|unlisten": () => null,
  "plugin:dialog|open": () => `${SD}/DCIM`,
  "plugin:window|set_fullscreen": () => null,
  // Native page zoom can't be reproduced from page JS; audit a zoomed size by
  // resizing the viewport to (width / zoom, height / zoom) instead.
  "plugin:webview|set_webview_zoom": () => null,
};

export function installMockIpc() {
  const w = window as unknown as Record<string, unknown>;
  w.__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: Args = {}) => {
      const h = HANDLERS[cmd];
      if (h) return h(args);
      console.debug("[mock-ipc] unhandled", cmd, args);
      return null;
    },
    transformCallback: () => callbackId++,
    unregisterCallback: () => {},
    // Videos play a local sample (static/dev-sample.mp4, generated for QA and
    // gitignored) so Focus, markers and the Edit preview can be exercised.
    // Songs play the sample's soundtrack; the sprite is static/dev-sprite.jpg
    // (also generated, 8×5 frames of the sample, gitignored).
    convertFileSrc: (p: string) => (/\.(mp4|mov|mp3|m4a|wav|aac|flac|ogg)$/i.test(p) ? "/dev-sample.mp4" : p),
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
  };
  // `__setSetting({ filmstripPos: "left" })` then reload: flip any persisted
  // setting for an audit permutation without clicking through Settings.
  w.__setSetting = (patch: Record<string, unknown>) => {
    const st = storeRead();
    st.settings = { ...((st.settings as object) ?? {}), ...patch };
    storeWrite(st);
  };
  console.info("[mock-ipc] fake backend installed (dev, no Tauri)");
}
