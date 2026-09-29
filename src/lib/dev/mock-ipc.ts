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
      missing: false,
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

const HANDLERS: Record<string, (a: Args) => unknown> = {
  list_drives: () => DRIVES,
  list_tree: (a) =>
    a.dir === "/"
      ? [{ name: "Users", path: "/Users", has_children: true }]
      : SUBFOLDERS.map((name) => ({ name, path: `${a.dir.replace(/\/$/, "")}/${name}`, has_children: true })),
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
  list_folder_media: (a) => folderItems(a.dir).map(({ seed: _s, aspect: _a, ...rest }) => rest),
  folder_writable: () => true,
  thumbnail: (a) => artFor(a.path, Math.min(a.max ?? 320, 480)),
  loupe_src: (a) => artFor(a.path, 1600),
  video_poster: (a) => artFor(a.path, 480),
  video_poster_hires: (a) => artFor(a.path, 1600),
  capture_dates: (a) => (a.paths as string[]).map((path) => ({ path, captured: byPath.get(path)?.mtime ?? 0 })),
  probe_media_info: (a) => ({
    duration: 83.4,
    width: 3840,
    height: 2160,
    fps: 29.97,
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
  // Mostly the Osmo main set; every 7th clip 29.97 fps 8-bit, every 11th vertical.
  merge_probe: (a) =>
    (a.paths as string[])
      .map((p, i) => {
        const it = byPath.get(p)!;
        if (it.kind !== "video")
          return { path: p, name: it.name, kind: "photo", size: it.size, duration: 0, captured: it.mtime, width: 0, height: 0, fps: 0, rotation: 0, vcodec: "", profile: "", pix_fmt: "", acodec: null, arate: 0, alayout: "", signature: "", error: null };
        const slow = i % 7 === 6;
        const vert = i % 11 === 10;
        const [w, h] = vert ? [1728, 3072] : [3840, 2160];
        const fps = slow ? 29.97 : 59.94;
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
          rotation: 0,
          vcodec: "hevc",
          profile,
          pix_fmt: pix,
          acodec: "aac",
          arate: 48000,
          alayout: "stereo",
          signature: `hevc|${profile}|${pix}|${w}x${h}|${fps}|0|aac|48000|stereo`,
          error: null,
        };
      })
      .sort((x, y) => (x.captured ?? 0) - (y.captured ?? 0) || x.name.localeCompare(y.name)),
  // The card is nearly full, so the dialog's not-enough-space state shows.
  disk_free: (a) => (a.path.startsWith("/Users") ? 76e9 : a.path === SD ? 9e9 : 1.2e12),
  merge_videos: async (a) => {
    await new Promise((r) => setTimeout(r, 1500));
    return { path: `${a.req.destDir}/${a.req.name}.mp4`, bytes: 63.1e9 };
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
  list_trash: () => [],
  list_missing: () => [],
  catalog_scan: () => ({ tracked: 0, missing: 0, relinked: 0, still_missing: 0, scanned_files: 0, elapsed_ms: 3 }),
  get_trim: () => null,
  get_video_segments: () => [],
  video_scrubstrip_cached: () => null,
  video_filmstrip_cached: () => null,
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
    convertFileSrc: (p: string) => p,
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
