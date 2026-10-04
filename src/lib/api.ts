import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type {
  TreeDir,
  ScanExcludes,
  SuggestedFolder,
  MergeClip,
  MergeConvert,
  MergeOutcome,
  MediaItem,
  TrashOutcome,
  TrashItem,
  LibraryInfo,
  FilmstripInfo,
  ExportOutcome,
  MoveOutcome,
  EditExportRequest,
  EditExportOutcome,
  EditSourceItem,
  MediaProbe,
  EditSnapshotRequest,
  VideoSegment,
  SegmentExportOutcome,
  EventInfo,
  ScanReport,
  RelinkOutcome,
  MergeStatus,
} from "./types";

export const api = {
  // ── separate windows (Edit, Merge) ────────────────────────────────────────
  /** Open or focus the Edit/Merge window, queueing `payload` (JSON) for it. */
  openToolWindow: (kind: "edit" | "merge", payload: unknown = null) =>
    invoke<void>("open_tool_window", { kind, payload: payload == null ? null : JSON.stringify(payload) }),
  /** Everything queued for this window since it last looked. */
  takeToolInbox: <T>(kind: "edit" | "merge") =>
    invoke<string[]>("take_tool_inbox", { kind }).then((xs) => xs.map((x) => JSON.parse(x) as T)),
  /** "Something new in your inbox" (sent to the window after a queue). */
  onToolInbox: (cb: () => void): Promise<UnlistenFn> => listen("tool-inbox", () => cb()),
  /** Library left half, `kind` right half of the library's screen. */
  tileWindows: (kind: "edit" | "merge") => invoke<{ tiled: boolean }>("tile_windows", { kind }),
  /** Shared scratchpad between windows (⌘C clips, the drag in progress). */
  stashSet: (key: string, value: unknown) =>
    invoke<void>("stash_set", { key, value: value == null ? null : JSON.stringify(value) }).catch(() => {}),
  stashGet: <T>(key: string) =>
    invoke<string | null>("stash_get", { key }).then((v) => (v ? (JSON.parse(v) as T) : null)).catch(() => null),
  /** A merge/export/snapshot wrote a file (any window refreshes its folder). */
  onMediaOutput: (cb: (path: string) => void): Promise<UnlistenFn> =>
    listen<string>("media-output", (e) => cb(e.payload)),
  /** The backend asks the library to confirm quitting with work running. */
  onConfirmQuit: (cb: () => void): Promise<UnlistenFn> => listen("confirm-quit", () => cb()),
  /** Quit now: stop running work, remove half-written files. */
  quitApp: () => invoke<void>("quit_app"),
  /** The running (or just finished) merge. */
  mergeStatus: () => invoke<MergeStatus>("merge_status"),
  mergePause: (paused: boolean) => invoke<MergeStatus>("merge_pause", { paused }),
  mergeDismiss: () => invoke<void>("merge_dismiss").catch(() => {}),

  /** Raw byte-range read, returned as an ArrayBuffer (binary IPC — no JSON
   *  overhead). I/O primitive of the WebCodecs scrub engine: the frontend
   *  parses MP4 sample tables and fetches exactly the bytes it decodes.
   *  A short/empty result means EOF. */
  readFileRange: (path: string, offset: number, len: number) =>
    invoke<ArrayBuffer>("read_file_range", { path, offset, len }),
  /** Activate the per-drive library for `root` and return where it lives. */
  setLibraryRoot: (root: string) =>
    invoke<LibraryInfo>("set_library_root", { root }),
  listDrives: () => invoke<TreeDir[]>("list_drives"),
  listTree: (dir: string) => invoke<TreeDir[]>("list_tree", { dir }),
  /** Push the exclude rules the backend applies to every folder walk. */
  setScanExcludes: (excludes: ScanExcludes) =>
    invoke<void>("set_scan_excludes", { excludes }).catch(() => {}),
  /** Is `dir` the root of the drive the OS runs from (`C:\`, `/`)? */
  isSystemRoot: (dir: string) => invoke<boolean>("is_system_root", { dir }).catch(() => false),
  /** Pictures/Movies/Desktop/Downloads plus camera cards, for the welcome screen. */
  suggestedFolders: () => invoke<SuggestedFolder[]>("suggested_folders").catch(() => [] as SuggestedFolder[]),
  /** Recursive media counts for the given folders (cached; left-pane badges). */
  folderCounts: (paths: string[], recompute = false) =>
    invoke<{ path: string; count: number }[]>("folder_counts", { paths, recompute }),
  /** Drop every cached folder count so the badges recompute. */
  clearFolderCounts: () => invoke<void>("clear_folder_counts").catch(() => {}),
  /** Lengths (seconds) of the videos among `paths`; cached per drive. */
  videoDurations: (dir: string, paths: string[]) =>
    invoke<{ path: string; duration: number }[]>("video_durations", { dir, paths }),
  /** Stream signature, length and recording time per clip, in shooting order. */
  mergeProbe: (paths: string[]) => invoke<MergeClip[]>("merge_probe", { paths }),
  /** Join clips end to end (or convert, then join). Runs as job "merge":
   *  progress through `activity` events and `mergeStatus`, stop with
   *  `cancelJob("merge")`, pause with `mergePause`. */
  mergeVideos: (req: { paths: string[]; destDir: string; name: string; convert?: MergeConvert | null }) =>
    invoke<MergeOutcome>("merge_videos", { req }),
  /** Free bytes on the volume holding `path`. */
  diskFree: (path: string) => invoke<number>("disk_free", { path }),
  listFolderMedia: (dir: string, recursive: boolean) =>
    invoke<MediaItem[]>("list_folder_media", { dir, recursive }),
  listEditSources: (dir: string, recursive: boolean) =>
    invoke<EditSourceItem[]>("list_edit_sources", { dir, recursive }),
  probeMediaInfo: (path: string) =>
    invoke<MediaProbe>("probe_media_info", { path }),
  thumbnail: (path: string, max: number) =>
    invoke<string>("thumbnail", { path, max }),
  /** Fire-and-forget: pre-warm the whole folder's thumbnails in parallel.
   *  `heavy` opts in to RAW previews + video posters (the explicit Prepare
   *  button only — automatic folder-open warming stays images-only). */
  warmThumbnails: (paths: string[], max: number, heavy = false) =>
    invoke<void>("warm_thumbnails", { paths, max, heavy }).catch(() => {}),
  /** Abandon in-flight background warming (free the SSD for previews/playback). */
  cancelWarm: () => invoke<void>("cancel_warm").catch(() => {}),
  /** Write one line into the app log. For decisions the Rust side can't see —
   *  chiefly whether the live scrub decoder took a clip — so a machine we can't
   *  sit in front of can still be diagnosed from its log. Never throws. */
  logNote: (msg: string) => invoke<void>("log_note", { msg }).catch(() => {}),
  loupeSrc: (path: string) => invoke<string>("loupe_src", { path }),
  /** Cached poster frame (filesystem path) for a video, via bundled ffmpeg. */
  videoPoster: (path: string) => invoke<string>("video_poster", { path }),
  /** Sharp ~1280px poster for Focus view (grid uses videoPoster's 480px). */
  videoPosterHires: (path: string) => invoke<string>("video_poster_hires", { path }),
  /** Tiled sprite of frames for decode-free scrubbing (built lazily, cached). */
  videoFilmstrip: (path: string) =>
    invoke<FilmstripInfo>("video_filmstrip", { path }),
  videoScrubstrip: (path: string) =>
    invoke<FilmstripInfo>("video_scrubstrip", { path }),
  /** The hover strip IF already cached — never triggers a build. */
  videoScrubstripCached: (path: string) =>
    invoke<FilmstripInfo | null>("video_scrubstrip_cached", { path }).catch(() => null),
  /** Dense Focus filmstrip IF already cached — never triggers a build. */
  videoFilmstripCached: (path: string) =>
    invoke<FilmstripInfo | null>("video_filmstrip_cached", { path }).catch(() => null),
  /** Stop an in-flight sprite build ("film" = Focus filmstrip, "scrub" = hover strip). */
  cancelSprite: (path: string, kind: "film" | "scrub") =>
    invoke<void>("cancel_sprite", { kind, path }).catch(() => {}),
  /** Cancel every pending sprite build (folder switch). */
  cancelAllSprites: () => invoke<void>("cancel_all_sprites").catch(() => {}),
  /** Real capture timestamps (EXIF/creation_time), cached; for sort + grouping. */
  captureDates: (dir: string, paths: string[]) =>
    invoke<{ path: string; captured: number }[]>("capture_dates", { dir, paths }),
  /** Convert a clip the webview can't decode into a cached H.264 preview. */
  videoProxy: (path: string) => invoke<string>("video_proxy", { path }),
  /** Path of an already-built H.264 proxy for the clip, or null. */
  videoProxyCached: (path: string) =>
    invoke<string | null>("video_proxy_cached", { path }).catch(() => null),
  /** Export files into `dest`: RAW → camera-rendered JPEG, images copied. */
  exportJpegs: (paths: string[], dest: string) =>
    invoke<ExportOutcome>("export_jpegs", { paths, dest }),
  /** Dimensions/size of a RAW's embedded camera JPEG, without writing anything. */
  rawEmbeddedProbe: (path: string) =>
    invoke<{ width: number; height: number; bytes: number; needs_rotate: boolean }>(
      "raw_embedded_probe",
      { path },
    ),
  /** Bulk RAW → JPEG: extract each RAW's embedded camera JPEG (the out-of-camera
   *  look) and write it as `<stem>.JPG` next to the source, never overwriting —
   *  so the export stacks with its RAW in the grid. Progress via
   *  `onRawExportProgress`. */
  exportRawJpegs: (paths: string[]) =>
    invoke<{
      written: string[];
      skipped: [string, string][];
      failed: [string, string][];
    }>("export_raw_jpegs", { paths }),
  /** Per-file progress stream for the in-flight bulk RAW → JPEG export. */
  onRawExportProgress: (
    cb: (p: { done: number; total: number; current: string }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ done: number; total: number; current: string }>(
      "raw-export-progress",
      (e) => cb(e.payload),
    ),
  /** Move (or with `copy`, copy) files into `dest`, on this drive or another.
   *  Progress arrives as `activity` events under `job`; stop with `cancelJob`. */
  moveMediaFiles: (paths: string[], dest: string, opts: { copy?: boolean; job?: string } = {}) =>
    invoke<MoveOutcome>("move_media_files", { paths, dest, copy: opts.copy ?? false, job: opts.job ?? null }),
  /** Stop a running job (a move, a merge). False when it already finished. */
  cancelJob: (id: string) => invoke<boolean>("cancel_job", { id }).catch(() => false),
  /** Create a subfolder inside `parent` and return its absolute path. */
  createFolder: (parent: string, name: string) =>
    invoke<string>("create_folder", { parent, name }),

  // ── catalog integrity: moved/renamed files keep their metadata ────────────
  /** Verify every metadata-carrying entry still resolves to a file, and (with
   *  `relink`) auto-reconnect what moved. Cheap when nothing is missing — it
   *  only walks the library once an absence is actually found. */
  catalogScan: (relink = true) => invoke<ScanReport>("catalog_scan", { relink }),
  listMissing: () => invoke<string[]>("list_missing"),
  relinkMissing: (rel: string, path: string) =>
    invoke<string>("relink_missing", { rel, path }),
  relinkFolder: (relDir: string, newDir: string) =>
    invoke<RelinkOutcome>("relink_folder", { relDir, newDir }),
  /** Drop the marks of entries the user confirms are gone — the only path that
   *  deletes metadata for a missing file. */
  forgetMissing: (rels: string[]) => invoke<number>("forget_missing", { rels }),

  // ── events (virtual collections that span folders) ────────────────────────
  listEvents: () => invoke<EventInfo[]>("list_events"),
  createEvent: (name: string) => invoke<number>("create_event", { name }),
  renameEvent: (id: number, name: string) =>
    invoke<void>("rename_event", { id, name }),
  deleteEvent: (id: number) => invoke<void>("delete_event", { id }),
  addToEvent: (id: number, paths: string[]) =>
    invoke<void>("add_to_event", { id, paths }),
  removeFromEvent: (id: number, paths: string[]) =>
    invoke<void>("remove_from_event", { id, paths }),
  setEventCover: (id: number, path: string | null) =>
    invoke<void>("set_event_cover", { id, path }),
  getTrim: (path: string) => invoke<[number, number] | null>("get_trim", { path }),
  // NB the argument names. Tauri 2 deserializes command args as camelCase by
  // default (tauri-macros: ArgumentCase::Camel, and no `rename_all` override
  // exists in src-tauri), so the Rust `in_s`/`out_s` params are looked up as
  // `inS`/`outS`. These two calls passed snake_case and therefore FAILED
  // deserialization on every invocation — silently, because of the `.catch`
  // that used to sit on setTrim. Net effect: in/out points were never once
  // written to the catalog and looked like they "reset" on every navigation,
  // and Focus's Cut button could never have worked. Fixed 2026-07-22; the
  // failure is now surfaced instead of swallowed.
  setTrim: (path: string, inS: number, outS: number) =>
    invoke<void>("set_trim", { path, inS, outS }),
  clearTrim: (path: string) => invoke<void>("clear_trim", { path }).catch(() => {}),
  trimVideo: (path: string, inS: number, outS: number) =>
    invoke<string>("trim_video", { path, inS, outS }),
  getVideoSegments: (path: string) =>
    invoke<VideoSegment[]>("get_video_segments", { path }),
  setVideoSegments: (path: string, segments: VideoSegment[]) =>
    invoke<void>("set_video_segments", { path, segments }).catch(() => {}),
  exportVideoSegments: (path: string, segments: VideoSegment[]) =>
    invoke<SegmentExportOutcome>("export_video_segments", { path, segments }),
  editExport: (req: EditExportRequest) =>
    invoke<EditExportOutcome>("edit_export", { req }),
  /** Kill the in-flight edit export; its partial output file is deleted. */
  cancelEditExport: () => invoke<void>("cancel_edit_export").catch(() => {}),
  /** Percentage (0–100) stream for the in-flight edit export. */
  onExportProgress: (cb: (pct: number) => void): Promise<UnlistenFn> =>
    listen<number>("export-progress", (e) => cb(e.payload)),
  /** Existence check for the export dialog's filename-taken hint. */
  pathExists: (path: string) =>
    invoke<boolean>("path_exists", { path }).catch(() => false),
  editSnapshot: (req: EditSnapshotRequest) =>
    invoke<string>("edit_snapshot", { req }),
  setRating: (path: string, rating: number) =>
    invoke<void>("set_rating", { path, rating }),
  setLabel: (path: string, label: string | null) =>
    invoke<void>("set_label", { path, label }),
  setFlag: (path: string, flag: string | null) =>
    invoke<void>("set_flag", { path, flag }),
  setRatingMany: (paths: string[], rating: number) =>
    invoke<void>("set_rating_many", { paths, rating }),
  setLabelMany: (paths: string[], label: string | null) =>
    invoke<void>("set_label_many", { paths, label }),
  setFlagMany: (paths: string[], flag: string | null) =>
    invoke<void>("set_flag_many", { paths, flag }),
  addTag: (paths: string[], tag: string) =>
    invoke<void>("add_tag", { paths, tag }),
  removeTag: (paths: string[], tag: string) =>
    invoke<void>("remove_tag", { paths, tag }),
  listTags: () => invoke<[string, number][]>("list_tags"),
  listRejected: () => invoke<string[]>("list_rejected"),
  disposeRejected: (paths: string[], mode: string) =>
    invoke<TrashOutcome>("dispose_rejected", { paths, mode }),
  /** In-app, per-drive Trash (folder-mode deletes). */
  listTrash: () => invoke<TrashItem[]>("list_trash"),
  restoreTrash: (stored: string[]) =>
    invoke<{ restored: number; failed: string[] }>("restore_trash", { stored }),
  purgeTrash: (stored: string[]) => invoke<number>("purge_trash", { stored }),
  libraryInfo: () => invoke<LibraryInfo>("library_info"),
  reveal: (path: string) => invoke<void>("reveal", { path }).catch(() => {}),
  openExternal: (path: string) =>
    invoke<void>("open_external", { path }).catch(() => {}),
  folderWritable: (dir: string) => invoke<boolean>("folder_writable", { dir }),
  logEvent: (msg: string) => invoke<void>("log_event", { msg }).catch(() => {}),

  /** Convert an absolute filesystem path into a webview-loadable asset URL. */
  fileSrc: (path: string) => convertFileSrc(path),

  pickFolder: async (): Promise<string | null> => {
    const r = await openDialog({ directory: true, multiple: false });
    return typeof r === "string" ? r : null;
  },
  /** Point-at-the-file picker for relinking a missing catalog entry. */
  pickMediaFile: async (): Promise<string | null> => {
    const r = await openDialog({
      directory: false,
      multiple: false,
      filters: [
        {
          name: "Photos & video",
          extensions: [
            "jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "heic", "heif",
            "nef", "cr2", "cr3", "arw", "dng", "raf", "orf", "rw2",
            "mp4", "mov", "m4v", "mkv", "avi", "webm", "mts", "m2ts",
          ],
        },
      ],
    });
    return typeof r === "string" ? r : null;
  },
  pickAudio: async (): Promise<string | null> => {
    const r = await openDialog({
      directory: false,
      multiple: false,
      filters: [{ name: "Audio", extensions: ["mp3", "m4a", "aac", "wav", "flac", "ogg"] }],
    });
    return typeof r === "string" ? r : null;
  },
  pickVideos: async (): Promise<string[]> => {
    const r = await openDialog({
      directory: false,
      multiple: true,
      filters: [
        {
          name: "Videos",
          extensions: ["mp4", "mov", "m4v", "mkv", "avi", "webm", "mts", "m2ts"],
        },
      ],
    });
    if (Array.isArray(r)) return r.filter((path): path is string => typeof path === "string");
    return typeof r === "string" ? [r] : [];
  },
};
