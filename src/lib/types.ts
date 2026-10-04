export interface TreeDir {
  name: string;
  path: string;
  has_children: boolean;
}

/** Folders every scan skips (Settings → Excluded folders). The five groups are
 *  built-in rule sets; `paths` and `names` are the user's own. Mirrors the Rust
 *  `ScanExcludes` — the backend applies it to the tree, scans and badges. */
export interface ScanExcludes {
  windowsSystem: boolean;
  macosSystem: boolean;
  appData: boolean;
  developer: boolean;
  games: boolean;
  /** Specific folders (absolute paths), including everything inside them. */
  paths: string[];
  /** Folder names skipped wherever they appear; `*` is a wildcard. */
  names: string[];
}

/** A starting point offered on the welcome screen. */
export interface SuggestedFolder {
  label: string;
  path: string;
  kind: "pictures" | "videos" | "desktop" | "downloads" | "card";
}

export interface MediaItem {
  name: string;
  path: string;
  rel: string;
  kind: "image" | "raw" | "video" | "other";
  ext: string;
  mtime: number;
  size: number;
  rating: number;
  label: string | null;
  flag: "pick" | "reject" | null;
  tags: string[];
  /** Events this file belongs to, in join order — `[0]` is the primary event
   *  (what the grid groups it under when several apply). */
  events: string[];
  /** A catalog entry whose file was not on disk at the last scan. Its marks are
   *  intact; the grid draws it as a "?" placeholder so it can be relinked. */
  missing: boolean;
  /** A video's marked in/out ranges (Focus → Mark range, else its trim).
   *  Absent = the whole clip. Each becomes a segment on the Edit timeline. */
  ranges?: VideoSegment[];
}

/** A clip handed from the library to the Edit window (drag, paste, E). */
export interface ClipRef {
  path: string;
  name: string;
  kind: "image" | "raw" | "video" | "other";
  ext: string;
  mtime: number;
  size: number;
  /** In/out ranges marked in the library; empty = the whole clip. */
  ranges: VideoSegment[];
  missing?: boolean;
}

/** What the library sends the Edit window. `seed` only fills an EMPTY
 *  timeline (the toolbar's Edit button); `append` always adds. */
export interface EditInbox {
  type: "add";
  clips: ClipRef[];
  mode: "append" | "seed";
}

/** What the library sends the Merge window. */
export type MergeInbox = { type: "review"; items: MediaItem[]; sourceDir: string } | { type: "show" };

/** The merge that's running or just finished (backend-owned). */
export interface MergeStatus {
  state: "idle" | "running" | "done" | "error" | "cancelled";
  paused: boolean;
  label: string;
  name: string;
  out_path: string;
  dest_dir: string;
  clips: number;
  total_s: number;
  in_bytes: number;
  convert: boolean;
  pct: number;
  detail: string | null;
  started_ms: number;
  finished_ms: number;
  out_bytes: number;
  error: string | null;
}

/** A named virtual collection ("Monar trip") — a peer of tags, not a folder. */
export interface EventInfo {
  id: number;
  name: string;
  created_at: number;
  /** Rel-path of the member that fronts the event's block, or null for "first". */
  cover_rel: string | null;
  count: number;
}

/** Result of a catalog integrity pass (see `api.catalogScan`). */
export interface ScanReport {
  tracked: number;
  missing: number;
  relinked: number;
  still_missing: number;
  scanned_files: number;
  elapsed_ms: number;
}

export interface RelinkOutcome {
  relinked: number;
  unresolved: string[];
}

export interface EditSourceItem {
  name: string;
  path: string;
  kind: "video" | "audio";
  ext: string;
  mtime: number;
  size: number;
}

export interface MediaProbe {
  duration: number;
  width: number;
  height: number;
  fps: number;
  codec: string | null;
  camera: string | null;
  captured: number | null;
  /** HDR transfer present (PQ/HLG) — the Instagram export tone-maps these to SDR. */
  hdr: boolean;
}

export interface VideoSegment {
  in_s: number;
  out_s: number;
}

export interface SegmentExportOutcome {
  exported: string[];
  failed: string[];
  errors: string[];
}

export interface TrashOutcome {
  deleted: number;
  failed: string[];
  errors: string[];
  /** Trash keys of what this dispose put in the in-app Trash — the handle an
   *  Undo needs to restore exactly this batch. Empty in OS-recycle-bin mode. */
  trashed: string[];
}

export interface LibraryInfo {
  /** Drive/volume root the library belongs to (catalog keys are relative to it). */
  root: string;
  /** The active library folder: `<drive>/_FoxCull` or an app-data fallback. */
  dir: string;
  catalog: string;
  recycle: string;
  /** True if on the drive itself; false = app-data fallback (read-only mount). */
  on_drive: boolean;
  /** Whether the drive root is writable (proxy for "can delete here"). */
  writable: boolean;
}

export interface TrashItem {
  stored: string;
  orig: string;
  /** Absolute path of the file inside the recycle folder (for its thumbnail). */
  path: string;
  name: string;
  kind: "image" | "raw" | "video" | "other";
  ext: string;
  deleted_at: number;
}

/** A tiled sprite of frames for decode-free video scrubbing (Tier 2). */
export interface FilmstripInfo {
  src: string;
  cols: number;
  rows: number;
  count: number;
  tile_w: number;
  tile_h: number;
  duration: number;
}

/** Result of a JPEG export run (RAW → camera-rendered JPEG; images copied). */
export interface ExportOutcome {
  exported: number;
  copied: number;
  skipped: number;
  failed: string[];
  errors: string[];
  dest: string;
}

export interface MoveRecord {
  from: string;
  to: string;
}

export interface MoveOutcome {
  moved: number;
  dest: string;
  files: MoveRecord[];
  failed: string[];
  errors: string[];
  /** The originals were kept. */
  copied: boolean;
  /** Went to another drive (its catalog took the marks). */
  cross_drive: boolean;
  /** Stopped from the job centre; `files` lists what got through. */
  cancelled: boolean;
}

export interface EditClip {
  path: string;
  in_s: number;
  out_s: number;
  crop_x: number;
  crop_y: number;
  zoom: number;
}

export interface EditAdjustments {
  brightness: number;
  contrast: number;
  saturation: number;
  warmth: number;
  sharpen: number;
  /** Orange & teal split-tone strength, 0 = off. Optional so requests built
   *  before this field existed still satisfy the type. */
  splitTone: number;
}

export interface EditExportRequest {
  clips: EditClip[];
  output_w: number;
  output_h: number;
  fit: "crop" | "original";
  encoder: "auto" | "x264" | "nvenc";
  quality: "best" | "high" | "standard" | "small";
  adjustments: EditAdjustments;
  music_path: string | null;
  preserve_source_audio: boolean;
  destination: string | null;
  basename: string | null;
  /** Social normalisation: tone-map HDR→SDR + faststart (Instagram exports). */
  normalize?: boolean;
  /** When normalising, keep HDR (HLG 10-bit HEVC) instead of tone-mapping to SDR. */
  keep_hdr?: boolean;
  /** Target frame rate (source fps capped at 60). null = keep source timing. */
  fps?: number | null;
}

export interface EditExportOutcome {
  path: string;
  mode: string;
  reencoded: boolean;
}

export interface EditSnapshotRequest {
  path: string;
  time_s: number;
  output_w: number;
  output_h: number;
  fit: "crop" | "original";
  crop_x: number;
  crop_y: number;
  zoom: number;
  adjustments: EditAdjustments;
  basename: string | null;
}

export interface Filter {
  minRating: number;
  label: string | null;
  flag: "pick" | "reject" | "unflagged" | null;
}

/** Color labels. Digits chosen to match the user's Lightroom muscle memory
 *  (8/9/0 = red/green/yellow), with 6/7 as bonus blue/purple. */
export interface LabelDef {
  key: string;
  digit: string;
  varName: string;
  name: string;
}

export const LABELS: LabelDef[] = [
  { key: "blue", digit: "6", varName: "--label-blue", name: "Blue" },
  { key: "purple", digit: "7", varName: "--label-purple", name: "Purple" },
  { key: "red", digit: "8", varName: "--label-red", name: "Red" },
  { key: "green", digit: "9", varName: "--label-green", name: "Green" },
  { key: "yellow", digit: "0", varName: "--label-yellow", name: "Yellow" },
];

export const LABEL_BY_DIGIT: Record<string, string> = Object.fromEntries(
  LABELS.map((l) => [l.digit, l.key]),
);

export const LABEL_VAR: Record<string, string> = Object.fromEntries(
  LABELS.map((l) => [l.key, l.varName]),
);

/** One clip as the merge dialog sees it (Rust `MergeClip`). */
export interface MergeClip {
  path: string;
  name: string;
  /** Everything selected is listed; only videos can be merged. */
  kind: "video" | "photo" | "other";
  size: number;
  duration: number;
  /** Recording time (unix secs) — clips arrive sorted by it. */
  captured: number | null;
  width: number;
  height: number;
  /** ffmpeg's AVERAGE rate: variable-rate clips read 29.73 for a 30. */
  fps: number;
  /** The nominal rate it was shot at (24, 25, 30, 60…): compare this. */
  fps_class: number;
  rotation: number;
  vcodec: string;
  profile: string;
  pix_fmt: string;
  /** Video bitrate, kb/s (0 = unknown). */
  vbitrate: number;
  /** "hlg" | "pq" | "sdr" */
  color: string;
  acodec: string | null;
  arate: number;
  alayout: string;
  /** Everything that must match for a lossless join, as one string. */
  signature: string;
  error: string | null;
}

/** "Convert to match": the one format every clip is re-encoded to. */
export interface MergeConvert {
  width: number;
  height: number;
  /** ffmpeg rate, "30" or "30000/1001". */
  fps: string;
  tenBit: boolean;
  color: string;
  bitrateKbps: number;
}

export interface MergeOutcome {
  path: string;
  bytes: number;
}

