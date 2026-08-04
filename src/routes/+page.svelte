<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api } from "$lib/api";
  import { cast, type CastDevice, type CastStatus } from "$lib/cast";
  import { settings, GLIMPSE_MIN, GLIMPSE_MAX } from "$lib/settings.svelte";
  import { activity, fmtEta } from "$lib/activity.svelte";
  import { resetThumbs, prefetchLoupe, loaderStats, loadVideoFilmstrip } from "$lib/thumbnail-loader";
  import {
    LABELS,
    LABEL_BY_DIGIT,
    LABEL_VAR,
    type MediaItem,
    type TreeDir,
    type LibraryInfo,
    type TrashItem,
    type EventInfo,
  } from "$lib/types";
  import TreeNode from "$lib/components/TreeNode.svelte";
  import Thumb from "$lib/components/Thumb.svelte";
  import Loupe from "$lib/components/Loupe.svelte";
  import VirtualGrid from "$lib/components/VirtualGrid.svelte";
  import SectionedGrid from "$lib/components/SectionedGrid.svelte";
  import VirtualStrip from "$lib/components/VirtualStrip.svelte";
  import DetailsView from "$lib/components/DetailsView.svelte";
  import ContextMenu, { type MenuEntry } from "$lib/components/ContextMenu.svelte";
  import TrashPanel from "$lib/components/TrashPanel.svelte";
  import ActivityBar from "$lib/components/ActivityBar.svelte";
  import EditStudio from "$lib/components/EditStudio.svelte";
  import ControllerPanel from "$lib/components/ControllerPanel.svelte";
  import { pad, PAD_ACTIONS, buttonName, type PadActionId } from "$lib/gamepad.svelte";

  type FlagFilter = "all" | "pick" | "reject" | "unflagged";
  type ViewMode = "grid" | "details" | "loupe";

  // Decode thumbnails at (roughly) the size they're DISPLAYED at, not a fixed
  // 320px. At the smallest grid a 320px thumb is ~6× the pixels actually shown —
  // wasted decode + memory. Snapping the request to a few tiers (so dragging the
  // zoom slider doesn't spawn dozens of cache variants) keeps the cached files,
  // the decoded bitmaps and the transfer all proportional to what's on screen.
  // Capped at 2 so a HiDPI panel doesn't quadruple memory.
  const DPR = typeof window !== "undefined" ? Math.min(window.devicePixelRatio || 1, 2) : 1;
  function tierFor(cssPx: number): number {
    const t = cssPx * DPR;
    if (t <= 200) return 192;
    if (t <= 340) return 320;
    return 480;
  }
  // Long edge of the full Focus preview (matches the backend LOUPE_MAX). Used by
  // "Prepare folder" to pre-generate every shot's big preview, not just the thumb.
  const LOUPE_MAX = 1920;
  // How many shots ahead/behind the active one to keep warm in Focus view.
  const PREFETCH_AHEAD = 3;
  const PREFETCH_BEHIND = 2;

  let drives = $state<TreeDir[]>([]);
  let currentDir = $state<string | null>(null);
  let items = $state<MediaItem[]>([]);
  let loading = $state(false);
  let writable = $state(true);

  let activeIndex = $state(0);
  let selected = $state<Set<string>>(new Set());
  let selectionAnchor = $state<string | null>(null);
  let draggingPaths = $state<string[]>([]);
  let cutPaths = $state<string[]>([]);
  let movingFiles = $state(false);

  type RatingOp = ">=" | "<=" | "=";
  let minRating = $state(0);
  // Rating comparison mode: ≥ (default), ≤, or = so a set of ratings can be
  // targeted (e.g. "= 3" isolates exactly-three-star shots). Modifier only —
  // it never counts toward activeFilterCount on its own.
  let ratingOp = $state<RatingOp>(">=");
  // Label filter is Lightroom-style multi-select with OR semantics: any number
  // of colours toggle on independently, plus a distinct "None" that matches
  // unlabeled items. Empty set + labelNone=false ⇒ label filter inactive.
  let labelFilters = $state<Set<string>>(new Set());
  let labelNone = $state(false);
  let flagFilter = $state<FlagFilter>("all");
  let tagFilter = $state<string | null>(null);
  let allTags = $state<[string, number][]>([]);
  let tagInput = $state("");
  /** Isolate one event across every folder in view — the "virtual collection"
   *  read of an event, as opposed to grouping the whole grid by it. */
  let eventFilter = $state<string | null>(null);
  let allEvents = $state<EventInfo[]>([]);
  /** Catalog entries whose files are gone — surfaced as "?" placeholders. */
  let missingRels = $state<string[]>([]);
  let scanning = $state(false);

  let labelFilterActive = $derived(labelFilters.size > 0 || labelNone);

  // How many popover filters are active — shown as a badge. The rating operator
  // is a modifier, not a filter, so only a chosen star count contributes.
  let activeFilterCount = $derived(
    (settings.s.typeFilter !== "all" ? 1 : 0) +
      (flagFilter !== "all" ? 1 : 0) +
      (minRating > 0 ? 1 : 0) +
      (labelFilterActive ? 1 : 0) +
      (tagFilter ? 1 : 0) +
      (eventFilter ? 1 : 0),
  );

  function toggleLabelFilter(key: string) {
    const next = new Set(labelFilters);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    labelFilters = next;
  }
  function clearLabelFilter() {
    labelFilters = new Set();
    labelNone = false;
  }

  /** One-click reset of every popover filter (the "no results" escape hatch). */
  function clearAllFilters() {
    settings.set({ typeFilter: "all" });
    flagFilter = "all";
    minRating = 0;
    ratingOp = ">=";
    tagFilter = null;
    eventFilter = null;
    clearLabelFilter();
  }

  let dimLevel = $state(0); // 0 normal · 1 dim panels · 2 lights out
  let showInfoOverlay = $state(false);
  let settingsOpen = $state(false);
  let filtersOpen = $state(false);
  let arrangeOpen = $state(false);
  let clearOpen = $state(false);
  // ── Cast to TV ────────────────────────────────────────────────────────────
  let castOpen = $state(false);
  let castDevices = $state<CastDevice[]>([]);
  let castDiscovering = $state(false);
  let castStatus = $state<CastStatus>({
    connected: false,
    deviceName: null,
    playingPath: null,
    playerState: null,
    currentTime: null,
    duration: null,
  });
  let castStateLabel = $derived(
    !castStatus.connected
      ? "Connecting"
      : castStatus.playerState === "PAUSED"
        ? "Paused"
        : castStatus.playerState === "BUFFERING"
          ? "Loading"
          : "Live",
  );
  function castLog(message: string) {
    void api.logNote(`cast-ui: ${message}`);
  }
  async function discoverCast() {
    castDiscovering = true;
    try {
      castDevices = await cast.discover();
      castLog(`discovery found ${castDevices.length} device(s)`);
    } catch (e) {
      castDevices = [];
      castLog(`discovery failed: ${e}`);
      activity.error("cast", `Cast discovery failed (${e})`);
    } finally {
      castDiscovering = false;
    }
  }
  function toggleCastMenu() {
    castOpen = !castOpen;
    // Re-browse on every open: cast devices come and go with TV power state,
    // and mDNS discovery is cheap (~3s, non-blocking behind the spinner).
    if (castOpen) void discoverCast();
  }
  /// The device of the live cast session — kept so navigation can re-cast the
  /// newly active item to the same TV without the user reopening the menu.
  let castDevice = $state<CastDevice | null>(null);
  /// What the TV can actually render. The Default Media Receiver is a Chrome
  /// page: videos stream as-is (the TV's hardware decodes HEVC natively), but
  /// still images render in an <img> — which cannot decode HEIC/RAW/TIFF. For
  /// those, cast the cached Focus preview JPEG (1920px — at or beyond what the
  /// receiver canvas renders anyway) instead of the original bytes.
  const CAST_DIRECT_STILL = new Set(["jpg", "jpeg", "jpe", "png", "webp", "gif", "bmp"]);
  async function castablePath(item: MediaItem): Promise<string> {
    if (item.kind === "video") return item.path;
    if (item.kind === "image" && CAST_DIRECT_STILL.has(item.ext.toLowerCase())) return item.path;
    // RAW / HEIC / TIFF / anything else: the loupe cache is a ready-made JPEG.
    return await api.loupeSrc(item.path);
  }
  /// The item we last ASKED the TV to show. Deliberately a plain `let`, not
  /// `$state`: it guards the follow effect below, and if it were reactive the
  /// status poll would re-enter that effect and re-cast on its own.
  let castWantedPath: string | null = null;
  /// Bumped per cast request so a slow one can't land after a newer one.
  let castSeq = 0;
  async function castTo(item: MediaItem, d: CastDevice) {
    const my = ++castSeq;
    castLog(`LOAD requested for ${item.name} -> ${d.name} (sequence ${my})`);
    // This can be SLOW — for RAW/HEIC it generates a JPEG preview first. Two
    // quick presses of → could therefore finish out of order and leave the TV
    // on the earlier shot; the sequence check drops anything superseded.
    const path = await castablePath(item);
    if (my !== castSeq) {
      castLog(`LOAD abandoned before send (sequence ${my} was superseded)`);
      return;
    }
    const st = await cast.start(path, d);
    if (my !== castSeq) {
      castLog(`LOAD response ignored (sequence ${my} was superseded)`);
      return;
    }
    // Track by the LIBRARY path, not the (possibly cache-file) casted path, so
    // the UI names what the user is looking at.
    castStatus = {
      ...st,
      playingPath: item.path,
      playerState: item.kind === "video" ? "BUFFERING" : null,
      currentTime: item.kind === "video" ? 0 : null,
      duration: null,
    };
    castLog(`LOAD queued; backend connected=${st.connected}, playing=${st.playingPath ?? "pending"}`);
  }
  async function startCast(d: CastDevice) {
    if (!active) return;
    castDevice = d;
    try {
      castWantedPath = active.path;
      await castTo(active, d);
      castOpen = false;
    } catch (e) {
      castLog(`start failed for ${d.name}: ${e}`);
      if (castDevice?.id === d.id) castDevice = null;
      castWantedPath = null;
      activity.error("cast", `Cast failed (${e})`);
    }
  }
  async function stopCast() {
    castLog(`stop requested${castDevice ? ` for ${castDevice.name}` : ""}`);
    castDevice = null;
    castWantedPath = null;
    castSeq++; // abandon anything in flight
    try {
      castStatus = await cast.stop();
    } catch {
      castStatus = {
        connected: false,
        deviceName: null,
        playingPath: null,
        playerState: null,
        currentTime: null,
        duration: null,
      };
    }
    castOpen = false;
  }
  // ── cast follows the active item ──────────────────────────────────────────
  // While a session is live, browsing the library IS the remote control: the
  // TV shows whatever photo/video is active, photos and videos alike. Debounced
  // so holding an arrow key across 20 shots sends one LOAD, not 20.
  let castFollowTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const it = active;
    // `castDevice` is the user's live-session intent. Do not gate this on a
    // cached status snapshot: doing that once deadlocked the whole feature when
    // cast_start returned before its actor thread updated `connected`.
    if (!castDevice || !it) return;
    // Compare against what we last ASKED for, not against what the backend
    // reports as playing: the backend answer arrives one round-trip later, and
    // gating on it meant a re-render mid-flight could fire a second LOAD for
    // the same item.
    if (it.path === castWantedPath) return;
    clearTimeout(castFollowTimer);
    const d = castDevice;
    castFollowTimer = setTimeout(() => {
      // Re-check at fire time — the session may have ended mid-debounce.
      if (!castDevice) {
        castLog(`follow skipped for ${it.name}: session ended during debounce`);
        return;
      }
      castWantedPath = it.path;
      castLog(`follow sending ${it.name} -> ${d.name}`);
      castTo(it, d).catch((e) => {
        castLog(`follow failed for ${it.name}: ${e}`);
        castWantedPath = null; // let a later navigation retry
        activity.error("cast", `Cast failed (${e})`);
      });
    }, 350);
    return () => clearTimeout(castFollowTimer);
  });
  // ── the TV follows the laptop's transport ─────────────────────────────────
  // Loupe reports every settled playback change (play / pause / landed seek)
  // from the <video> element's own events, so this one hook covers the
  // transport bar, the keyboard, the controller and a scrub drag alike.
  //
  // Seeks are throttled hard on purpose. A held shuttle trigger lands a seek
  // every ~120 ms locally; the receiver would queue them and lag seconds
  // behind, so we send at most one every CAST_SEEK_EVERY_MS and always send a
  // trailing one so the TV settles on the frame you stopped at. Play/pause is
  // rare and goes through immediately.
  const CAST_SEEK_EVERY_MS = 450;
  let castSeekAt = 0;
  let castSeekTimer: ReturnType<typeof setTimeout> | undefined;
  async function ensureCastVideoTarget(): Promise<CastDevice | null> {
    const d = castDevice;
    const item = active;
    if (!d || !item || item.kind !== "video") return null;
    if (castWantedPath !== item.path) {
      clearTimeout(castFollowTimer);
      castWantedPath = item.path;
      castLog(`control loading ${item.name} before transport command`);
      try {
        await castTo(item, d);
      } catch (e) {
        castWantedPath = null;
        castLog(`control LOAD failed for ${item.name}: ${e}`);
        activity.error("cast", `Cast failed (${e})`);
        return null;
      }
    }
    return castDevice?.id === d.id ? d : null;
  }
  async function toggleCastPlayback() {
    const d = await ensureCastVideoTarget();
    if (!d) return;
    castLog(`TOGGLE -> ${d.name}`);
    await cast.toggle();
  }
  async function seekCastBy(delta: number) {
    const d = await ensureCastVideoTarget();
    if (!d) return;
    castLog(`SEEK ${delta > 0 ? "+" : ""}${delta}s -> ${d.name}`);
    await cast.seekBy(delta);
  }
  function onLoupeTransport(st: { kind: "play" | "pause" | "seek"; paused: boolean; time: number }) {
    // Local play/pause events are deliberately ignored during casting: the TV
    // is the playback authority. A timeline/scrub seek remains useful and is
    // sent as an absolute receiver position.
    if (st.kind !== "seek") return;
    // Only mirror when the TV is actually showing the clip we're scrubbing —
    // during the 350 ms follow debounce it is still on the previous item, and
    // seeking that one would be visible nonsense.
    if (!castDevice) return;
    if (!active || active.kind !== "video") {
      castLog("transport skipped: active item is not a video");
      return;
    }
    if (castWantedPath !== active.path) {
      castLog(`transport skipped: TV target has not followed ${active.name} yet`);
      return;
    }
    const now = performance.now();
    clearTimeout(castSeekTimer);
    if (now - castSeekAt >= CAST_SEEK_EVERY_MS) {
      castSeekAt = now;
      castLog(`SEEK -> ${castDevice.name} at ${st.time.toFixed(2)}s`);
      void cast.seek(st.time);
    } else {
      castSeekTimer = setTimeout(() => {
        castSeekAt = performance.now();
        if (castDevice) castLog(`SEEK (trailing) -> ${castDevice.name} at ${st.time.toFixed(2)}s`);
        void cast.seek(st.time);
      }, CAST_SEEK_EVERY_MS - (now - castSeekAt));
    }
  }

  // ── notice when the session actually dies ─────────────────────────────────
  // Nothing polled the backend, so a connection that dropped (TV off, network
  // blip, someone else casting to it) left the button saying "casting" forever
  // and every subsequent navigation quietly went nowhere. Poll while a session
  // is live and fold the session up honestly when it's gone.
  $effect(() => {
    // Poll whenever the user has an intended session, including while the last
    // status snapshot is false. That lets status self-heal instead of making
    // the poll depend on the very value it exists to refresh.
    if (!castDevice) return;
    const poll = setInterval(async () => {
      try {
        const st = await cast.status();
        castStatus = st;
        if (st.connected) return;
        castLog("backend reports the cast session ended");
        castDevice = null;
        castWantedPath = null;
        castSeq++;
        castStatus = {
          connected: false,
          deviceName: null,
          playingPath: null,
          playerState: null,
          currentTime: null,
          duration: null,
        };
        activity.error("cast", "Cast session ended");
      } catch {
        // A failed status call is not evidence the session is dead — leave it.
      }
    }, 1000);
    return () => clearInterval(poll);
  });
  let libInfo = $state<LibraryInfo | null>(null);
  let trashOpen = $state(false);
  let trashItems = $state<TrashItem[]>([]);
  let controllerOpen = $state(false);
  let padHelpOpen = $state(false);
  let shortcutsOpen = $state(false);

  /** True while any toolbar popover/menu is open (they share light-dismiss). */
  function anyPopoverOpen(): boolean {
    return settingsOpen || filtersOpen || arrangeOpen || clearOpen || castOpen || prepMenuOpen;
  }
  function closeAllPopovers() {
    settingsOpen = false;
    filtersOpen = false;
    arrangeOpen = false;
    clearOpen = false;
    castOpen = false;
    prepMenuOpen = false;
  }
  // Light dismiss, the way every native menu works: pressing anywhere outside
  // an open popover (or its toggle) closes it. Toggles keep working because
  // their wrapping group is excluded from the dismissal test.
  function onGlobalPointerDown(e: PointerEvent) {
    if (!anyPopoverOpen()) return;
    const t = e.target as HTMLElement | null;
    if (t?.closest(".pop, .filtermenu, .arrangeMenu, .clearMenu, .castMenu, .arrange, .filterwrap, .clearWrap, .castWrap, .prepWrap, .gear")) return;
    closeAllPopovers();
  }
  let editOpen = $state(false);
  let treeCollapsed = $state(false);
  // Bumped by the tree's ↻ button to make expanded folders recount their badges.
  let countsGen = $state(0);
  let folderRefreshKey = $state(0);
  let gridComp = $state<{ scrollToIndex: (i: number, center?: boolean) => void; columnCount?: () => number } | null>(null);
  let loupeComp = $state<{ togglePlay: () => void; seekBy: (d: number) => void; setInPoint?: () => void; setOutPoint?: () => void; toggleGlimpse?: () => void } | null>(null);
  let editComp = $state<{
    setOutputPreview?: (on: boolean) => void | Promise<void>;
    setIn?: () => void;
    setOut?: () => void;
    togglePlay?: () => void;
    seekBy?: (d: number) => void;
    deleteSelected?: () => void;
    cutAtPlayhead?: () => void;
  } | null>(null);

  const HOLD_MS = 850;
  let holdMs = $state(0);
  let holdRAF = 0;

  const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  let viewMode = $derived(settings.s.viewMode as ViewMode);

  // Folder-grouped, human-numeric path order (IMG_2 < IMG_10, and each
  // subfolder's shots stay together instead of interleaving by bare filename —
  // that interleaving was the "random order" on recursive folder loads).
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

  // Real capture timestamps (path → Unix secs), filled lazily after a folder
  // loads so folder-open stays instant. Falls back to file mtime until/unless a
  // file's EXIF/creation_time is known.
  let captureMap = $state<Record<string, number>>({});
  const captureOf = (it: MediaItem) => captureMap[it.path] ?? it.mtime;

  // Grouping that needs real capture dates (the date-based sections); folder/type
  // group on the path/kind we already have, so they cost nothing extra.
  const DATE_GROUPS = new Set(["year", "month", "week"]);
  const TYPE_ORDER: Record<string, number> = { image: 0, raw: 1, video: 2, other: 3 };
  const TYPE_LABEL: Record<string, string> = {
    image: "Photos",
    raw: "RAW",
    video: "Video",
    other: "Other",
  };
  const parentOf = (p: string) => p.replace(/[\\/][^\\/]*$/, "");
  const parentName = (p: string) => parentOf(p).split(/[\\/]/).filter(Boolean).pop() ?? "/";
  const samePath = (a: string, b: string) =>
    a.replace(/[\\/]+$/, "").toLowerCase() === b.replace(/[\\/]+$/, "").toLowerCase();
  const isUnder = (path: string, folder: string) => {
    const f = folder.replace(/[\\/]+$/, "").toLowerCase();
    const p = path.toLowerCase();
    return p.length > f.length && p.startsWith(f) && (p[f.length] === "\\" || p[f.length] === "/");
  };

  // ── events ────────────────────────────────────────────────────────────────
  // An event ("Monar trip", "Rashi's birthday") is a virtual collection stored
  // as catalog metadata, deliberately NOT a folder: the same event spans any
  // number of directories, so the block it forms in the grid stays whole no
  // matter how the shots are filed on disk. An item may belong to several; the
  // FIRST one it joined is its primary and decides which block it sits in.
  const NO_EVENT_LABEL = "No event";
  /** Sort rank parked past any real event so unassigned shots trail the blocks.
   *  Numeric because the section collator compares numerically. */
  const NO_EVENT_KEY = "999999";

  const eventOf = (it: MediaItem) => it.events[0] ?? null;

  type EventStat = {
    count: number;
    first: number;
    last: number;
    /** The member explicitly chosen as cover, if it is in this view. */
    cover: MediaItem | null;
    /** Fallback cover: the first real (non-missing) member encountered. */
    firstShot: MediaItem | null;
  };

  /** Per-event facts for the block headers, over the whole loaded folder (not
   *  the filtered view) so an event's cover and date range don't jump around as
   *  filters are toggled. */
  let eventStats = $derived.by(() => {
    const coverRel = new Map(allEvents.map((e) => [e.name, e.cover_rel]));
    const out = new Map<string, EventStat>();
    for (const it of items) {
      for (const name of it.events) {
        let s = out.get(name);
        if (!s) {
          s = { count: 0, first: Infinity, last: -Infinity, cover: null, firstShot: null };
          out.set(name, s);
        }
        s.count++;
        if (!it.missing) {
          const t = captureOf(it);
          if (t < s.first) s.first = t;
          if (t > s.last) s.last = t;
          if (coverRel.get(name) === it.rel) s.cover = it;
          if (!s.firstShot) s.firstShot = it;
        }
      }
    }
    return out;
  });

  function eventCoverPath(name: string): string | null {
    const s = eventStats.get(name);
    return s?.cover?.path ?? s?.firstShot?.path ?? null;
  }

  const dayFmt = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
  function eventDateRange(name: string): string {
    const s = eventStats.get(name);
    if (!s || !Number.isFinite(s.first)) return "";
    const a = dayFmt.format(new Date(s.first * 1000));
    const b = dayFmt.format(new Date(s.last * 1000));
    return a === b ? a : `${a} – ${b}`;
  }

  // Blocks are ordered by name or by each event's earliest capture, and unlike
  // the other groupings that order has to honour ascending/descending — the
  // grouped sort compares section keys directly (never multiplied by sortDir),
  // so the direction is baked into a rank here instead.
  let eventRank = $derived.by(() => {
    const names = [...eventStats.keys()];
    const byDate = settings.s.eventOrder === "date";
    names.sort((a, b) => {
      if (byDate) {
        const fa = eventStats.get(a)?.first ?? 0;
        const fb = eventStats.get(b)?.first ?? 0;
        const c = (Number.isFinite(fa) ? fa : 0) - (Number.isFinite(fb) ? fb : 0);
        if (c !== 0) return c;
      }
      return collator.compare(a, b);
    });
    if (settings.s.sortDir === "desc") names.reverse();
    const m = new Map<string, string>();
    names.forEach((n, i) => m.set(n, String(i).padStart(6, "0")));
    return m;
  });

  let eventByName = $derived(new Map(allEvents.map((e) => [e.name, e])));
  let groupingByEvent = $derived(settings.s.groupBy === "event" || settings.s.subgroupBy === "event");

  type RelatedBadge = "RAW+JPEG" | "Subclip" | "Crop/Edit" | "Burst" | "Motion";
  type RelatedRole = "mother" | "derivative" | "sidecar" | "burst";
  type StemRelation = "original" | "subclip" | "edit" | "burst";

  type RelatedStem = {
    parent: string;
    stem: string;
    root: string;
    relation: StemRelation;
    badges: RelatedBadge[];
    explicit: boolean;
  };

  type RelatedEntry = {
    item: MediaItem;
    order: number;
    stem: RelatedStem;
  };

  type RelatedGroup = {
    id: string;
    items: MediaItem[];
    representative: MediaItem;
    mother: MediaItem;
    badges: RelatedBadge[];
    entries: RelatedEntry[];
    /** True when every member is an edit/export and the original source is gone
     *  — so there's no true mother to tag; we show a "no original" marker. */
    orphaned: boolean;
  };

  type RelatedMeta = {
    group: RelatedGroup;
    index: number;
    count: number;
    role: RelatedRole;
    relation: StemRelation;
  };

  type RelatedIndex = {
    groups: RelatedGroup[];
    metaByPath: Map<string, RelatedMeta>;
    groupByPath: Map<string, RelatedGroup>;
  };

  let expandedRelatedGroups = $state<Set<string>>(new Set());

  function stemOf(it: MediaItem): string {
    return basename(it.name || it.path).replace(/\.[^.]+$/, "");
  }

  function addBadge(values: RelatedBadge[], badge: RelatedBadge) {
    if (!values.includes(badge)) values.push(badge);
  }

  function relatedStem(it: MediaItem): RelatedStem {
    const parent = parentOf(it.path).toLowerCase();
    const stem = stemOf(it);
    let root = stem;
    let relation: StemRelation = "original";
    let explicit = false;
    const badges: RelatedBadge[] = [];

    for (let guard = 0; guard < 3; guard++) {
      const before = root;
      if (/(?:[_\-. ]sub(?:clip)?[_\-. ]?0*\d+)$/i.test(root)) {
        root = root.replace(/(?:[_\-. ]sub(?:clip)?[_\-. ]?0*\d+)$/i, "");
        relation = "subclip";
        explicit = true;
        addBadge(badges, "Subclip");
      }
      // Edit/derivative suffixes — includes FoxCull's own export tags (ig, reel,
      // sq, wide, crop, trim, mobile) so an Instagram/lossless export nests under
      // its original. NOTE: "mix" is deliberately absent — a composite of two
      // sources is its own third state, not part of either parent's stack.
      if (/(?:[_\-. ](?:cut|reel|crop|cropped|square|sq|wide|landscape|mobile|edit|edited|final|export|ig|trim)(?:[_\-. ]?0*\d+)?)$/i.test(root)) {
        root = root.replace(/(?:[_\-. ](?:cut|reel|crop|cropped|square|sq|wide|landscape|mobile|edit|edited|final|export|ig|trim)(?:[_\-. ]?0*\d+)?)$/i, "");
        relation = relation === "subclip" ? relation : "edit";
        explicit = true;
        addBadge(badges, "Crop/Edit");
      }
      const burst = root.match(/^(.+?)(?:[_\-. ](?:burst|bursts|burstshot)[_\-. ]?0*\d+)$/i);
      if (burst?.[1]) {
        root = burst[1];
        relation = relation === "original" ? "burst" : relation;
        explicit = true;
        addBadge(badges, "Burst");
      }
      if (root === before) break;
    }

    return { parent, stem, root: root.toLowerCase(), relation, badges, explicit };
  }

  function relatedKey(stem: RelatedStem): string {
    return `${stem.parent}\0${stem.root}`;
  }

  function hasRawJpeg(entries: RelatedEntry[]): boolean {
    return entries.some((e) => e.item.kind === "raw") && entries.some((e) => e.item.kind === "image");
  }

  function hasMotionPair(entries: RelatedEntry[]): boolean {
    return entries.some((e) => e.item.kind === "image") && entries.some((e) => e.item.kind === "video");
  }

  function groupBadges(entries: RelatedEntry[], extra: RelatedBadge[] = []): RelatedBadge[] {
    const badges: RelatedBadge[] = [];
    if (hasRawJpeg(entries)) addBadge(badges, "RAW+JPEG");
    if (hasMotionPair(entries)) addBadge(badges, "Motion");
    for (const e of entries) for (const b of e.stem.badges) addBadge(badges, b);
    for (const b of extra) addBadge(badges, b);
    return badges;
  }

  function shouldKeepRelatedRoot(entries: RelatedEntry[]): boolean {
    if (entries.length < 2) return false;
    if (entries.some((e) => e.stem.explicit)) return true;
    if (hasRawJpeg(entries) || hasMotionPair(entries)) return true;
    const stems = new Set(entries.map((e) => e.stem.stem.toLowerCase()));
    const exts = new Set(entries.map((e) => e.item.ext.toLowerCase()));
    return stems.size === 1 && exts.size > 1;
  }

  function makeRelatedGroup(id: string, entries: RelatedEntry[], extraBadges: RelatedBadge[] = []): RelatedGroup {
    const inputOrder = [...entries].sort((a, b) => a.order - b.order);
    const badges = groupBadges(inputOrder, extraBadges);
    const rank = (e: RelatedEntry) => {
      if (e.stem.relation === "original") {
        if (badges.includes("RAW+JPEG") && e.item.kind === "raw") return 0;
        if (badges.includes("Motion") && e.item.kind === "image") return 0;
        return 1;
      }
      if (e.stem.relation === "burst") return 2;
      if (e.item.kind === "image" || e.item.kind === "raw") return 3;
      if (e.stem.relation === "subclip" || e.stem.relation === "edit") return 5;
      return 4;
    };
    const ordered = [...inputOrder].sort((a, b) => rank(a) - rank(b) || a.order - b.order);
    const mother = ordered[0].item;
    const orphaned =
      inputOrder.some((e) => e.stem.explicit) && !inputOrder.some((e) => e.stem.relation === "original");
    return {
      id,
      entries: ordered,
      items: ordered.map((e) => e.item),
      representative: mother,
      mother,
      badges,
      orphaned,
    };
  }

  function burstCandidate(e: RelatedEntry): { key: string; n: number } | null {
    if (e.stem.explicit || e.item.kind === "video" || e.item.kind === "other") return null;
    const s = e.stem.stem.toLowerCase();
    let m = s.match(/^((?:img|pxl|mvimg|dsc|dscf|vid)[_\-. ]?\d{8}[_\-. ]\d{6}[_\-. ])(\d{2,4})$/i);
    if (m) return { key: `${e.stem.parent}\0${m[1].toLowerCase()}`, n: Number(m[2]) };
    m = s.match(/^((?:img|pxl|mvimg)[_\-. ])(\d{3,6})$/i);
    if (m) return { key: `${e.stem.parent}\0${m[1].toLowerCase()}`, n: Number(m[2]) };
    return null;
  }

  function looksLikeBurstRun(entries: RelatedEntry[]): boolean {
    if (entries.length < 3 || entries.length > 24) return false;
    const times = entries.map((e) => captureOf(e.item)).sort((a, b) => a - b);
    const span = times[times.length - 1] - times[0];
    let maxGap = 0;
    for (let i = 1; i < times.length; i++) maxGap = Math.max(maxGap, times[i] - times[i - 1]);
    return span <= 8 && maxGap <= 3;
  }

  function relatedRole(e: RelatedEntry, group: RelatedGroup): RelatedRole {
    if (e.stem.relation === "subclip" || e.stem.relation === "edit") return "derivative";
    if (group.badges.includes("Burst")) return "burst";
    if (hasRawJpeg(group.entries)) return e.item.kind === "raw" ? "mother" : "sidecar";
    if (group.badges.includes("Motion")) return e.item.kind === "video" ? "sidecar" : "mother";
    return e.stem.relation === "original" ? "mother" : "derivative";
  }

  function buildRelatedIndex(source: MediaItem[]): RelatedIndex {
    const entries: RelatedEntry[] = source.map((item, order) => ({ item, order, stem: relatedStem(item) }));

    // ── Task 1: scalable stack re-rooting ──────────────────────────────────
    // The whitelist in relatedStem() only strips KNOWN export suffixes, so a
    // hand-named derivative like `DJI_0679_IG_reel_nv.mp4` (the `_nv` isn't in
    // the list) stays a singleton and never nests under `DJI_0679.MP4`. This
    // folder-wide pass applies the user's own convention: a file whose stem
    // starts with ANOTHER file's stem + a separator ([_-. ]) belongs to that
    // file's stack. For every entry whose root is still a singleton (didn't
    // join a same-root group above), re-root it to the LONGEST sibling root R
    // in the same parent that it prefix-matches on a separator boundary. The
    // R.length >= 4 guard avoids degenerate short-prefix collisions.
    {
      const rootsByParent = new Map<string, string[]>();
      const rootCounts = new Map<string, number>(); // parent\0root → files sharing it
      for (const e of entries) {
        const list = rootsByParent.get(e.stem.parent) ?? [];
        if (!list.includes(e.stem.root)) list.push(e.stem.root);
        rootsByParent.set(e.stem.parent, list);
        const k = relatedKey(e.stem);
        rootCounts.set(k, (rootCounts.get(k) ?? 0) + 1);
      }
      const isSep = (c: string) => c === "_" || c === "-" || c === "." || c === " ";
      for (const e of entries) {
        // Only files that didn't already share their root with a sibling — a
        // shared root is already a stack anchor and must not be re-parented.
        if ((rootCounts.get(relatedKey(e.stem)) ?? 0) > 1) continue;
        const roots = rootsByParent.get(e.stem.parent);
        if (!roots) continue;
        let best: string | null = null;
        for (const R of roots) {
          if (R.length < 4 || R.length >= e.stem.root.length) continue;
          if (!e.stem.root.startsWith(R) || !isSep(e.stem.root[R.length])) continue;
          if (!best || R.length > best.length) best = R;
        }
        if (best) {
          e.stem.root = best;
          if (e.stem.relation !== "subclip") e.stem.relation = "edit";
          e.stem.explicit = true;
          addBadge(e.stem.badges, "Crop/Edit");
        }
      }
    }

    // Mutate-in-place bucket fill: the index rebuilds whenever the folder listing
    // changes (not on mark edits — the stack ranking does not read ratings), so
    // per-entry array respreads would make big stacks quadratic.
    const rootBuckets = new Map<string, RelatedEntry[]>();
    for (const e of entries) {
      const key = relatedKey(e.stem);
      const bucket = rootBuckets.get(key);
      if (bucket) bucket.push(e);
      else rootBuckets.set(key, [e]);
    }

    const groups: RelatedGroup[] = [];
    const used = new Set<string>();
    for (const [key, bucket] of rootBuckets) {
      if (!shouldKeepRelatedRoot(bucket)) continue;
      const group = makeRelatedGroup(`root:${key}`, bucket);
      groups.push(group);
      for (const e of bucket) used.add(e.item.path);
    }

    const burstBuckets = new Map<string, { e: RelatedEntry; n: number }[]>();
    for (const e of entries) {
      if (used.has(e.item.path)) continue;
      const c = burstCandidate(e);
      if (!c) continue;
      const bucket = burstBuckets.get(c.key);
      if (bucket) bucket.push({ e, n: c.n });
      else burstBuckets.set(c.key, [{ e, n: c.n }]);
    }
    for (const [key, bucket] of burstBuckets) {
      const ordered = [...bucket].sort((a, b) => a.n - b.n);
      let run: { e: RelatedEntry; n: number }[] = [];
      const flush = () => {
        if (looksLikeBurstRun(run.map((r) => r.e))) {
          const group = makeRelatedGroup(`burst:${key}:${run[0].n}-${run[run.length - 1].n}`, run.map((r) => r.e), ["Burst"]);
          groups.push(group);
          for (const r of run) used.add(r.e.item.path);
        }
      };
      for (const b of ordered) {
        if (run.length && b.n !== run[run.length - 1].n + 1) {
          flush();
          run = [];
        }
        run.push(b);
      }
      flush();
    }

    const metaByPath = new Map<string, RelatedMeta>();
    const groupByPath = new Map<string, RelatedGroup>();
    for (const group of groups) {
      group.entries.forEach((entry, index) => {
        const meta = {
          group,
          index,
          count: group.items.length,
          role: relatedRole(entry, group),
          relation: entry.stem.relation,
        };
        metaByPath.set(entry.item.path, meta);
        groupByPath.set(entry.item.path, group);
      });
    }
    return { groups, metaByPath, groupByPath };
  }

  function groupExpanded(group: RelatedGroup): boolean {
    return settings.s.relatedMode === "expanded" || expandedRelatedGroups.has(group.id);
  }

  function hasBurstLikeNames(source: MediaItem[]): boolean {
    const buckets = new Map<string, number>();
    for (let order = 0; order < source.length; order++) {
      const item = source[order];
      if (item.kind !== "image" && item.kind !== "raw") continue;
      const candidate = burstCandidate({ item, order, stem: relatedStem(item) });
      if (!candidate) continue;
      const count = (buckets.get(candidate.key) ?? 0) + 1;
      if (count >= 3) return true;
      buckets.set(candidate.key, count);
    }
    return false;
  }

  // Section helpers for the grouped grid (folder · type · year · month · week).
  // Dates are UTC to match how capture timestamps are stored. Week = calendar
  // week-of-month (days 1–7 = Week 1, 8–14 = Week 2, …).
  function sectionPartKey(it: MediaItem, g: typeof settings.s.groupBy): string {
    if (g === "folder") return parentOf(it.path);
    if (g === "type") return it.kind;
    if (g === "event") {
      const name = eventOf(it);
      return name ? (eventRank.get(name) ?? NO_EVENT_KEY) : NO_EVENT_KEY;
    }
    if (g === "none") return "";
    const d = new Date(captureOf(it) * 1000);
    if (g === "year") return `${d.getUTCFullYear()}`;
    const base = `${d.getUTCFullYear()}-${d.getUTCMonth()}`;
    if (g === "week") return `${base}-${Math.floor((d.getUTCDate() - 1) / 7)}`;
    return base; // month
  }
  function sectionPartLabel(it: MediaItem, g: typeof settings.s.groupBy): string {
    if (g === "folder") return parentName(it.path);
    if (g === "type") return TYPE_LABEL[it.kind] ?? it.kind;
    if (g === "event") return eventOf(it) ?? NO_EVENT_LABEL;
    if (g === "none") return "";
    const d = new Date(captureOf(it) * 1000);
    if (g === "year") return `${d.getUTCFullYear()}`;
    const mon = d.toLocaleString(undefined, { month: "long", year: "numeric", timeZone: "UTC" });
    if (g === "week") return `${mon} · Week ${Math.floor((d.getUTCDate() - 1) / 7) + 1}`;
    return mon;
  }

  function sectionKey(it: MediaItem): string {
    const primary = sectionPartKey(it, settings.s.groupBy);
    const sub = settings.s.subgroupBy !== settings.s.groupBy ? sectionPartKey(it, settings.s.subgroupBy) : "";
    return [primary, sub].filter(Boolean).join("\0") || "all";
  }

  function sectionLabel(it: MediaItem): string {
    const primary = sectionPartLabel(it, settings.s.groupBy);
    const sub = settings.s.subgroupBy !== settings.s.groupBy ? sectionPartLabel(it, settings.s.subgroupBy) : "";
    return [primary, sub].filter(Boolean).join(" / ") || "All media";
  }

  // type → rating/label/flag/tag filters → sort, in one pass. Grouping by month
  // implies sorting by capture date (that's the order the sections need).
  let baseView = $derived.by(() => {
    let arr = items;
    const tf = settings.s.typeFilter;
    if (tf !== "all") arr = arr.filter((i) => i.kind === tf);
    if (minRating > 0) {
      if (ratingOp === "<=") arr = arr.filter((i) => i.rating <= minRating);
      else if (ratingOp === "=") arr = arr.filter((i) => i.rating === minRating);
      else arr = arr.filter((i) => i.rating >= minRating);
    }
    // Multi-select label OR: a colour match OR (labelNone && unlabeled).
    if (labelFilterActive)
      arr = arr.filter((i) => (i.label ? labelFilters.has(i.label) : labelNone));
    if (tagFilter) arr = arr.filter((i) => i.tags.includes(tagFilter!));
    if (eventFilter) arr = arr.filter((i) => i.events.includes(eventFilter!));
    if (flagFilter === "reject") arr = arr.filter((i) => i.flag === "reject");
    else if (flagFilter === "pick") arr = arr.filter((i) => i.flag === "pick");
    else if (flagFilter === "unflagged") arr = arr.filter((i) => !i.flag);

    const g = settings.s.groupBy;
    const hasGrouping = settings.s.groupBy !== "none" || settings.s.subgroupBy !== "none";
    const dir = settings.s.sortDir === "asc" ? 1 : -1;
    // Date groupings imply a capture-date order (that's the order their sections
    // need); folder/type keep their groups contiguous via a direction-independent
    // primary key, then order within each group by the chosen sort.
    const by = DATE_GROUPS.has(g) || DATE_GROUPS.has(settings.s.subgroupBy) ? "capture" : settings.s.sortBy;
    return [...arr].sort((a, b) => {
      if (hasGrouping) {
        const p = collator.compare(sectionKey(a), sectionKey(b));
        if (p !== 0) return p;
      }
      let c = 0;
      if (by === "capture") c = captureOf(a) - captureOf(b);
      else if (by === "date") c = a.mtime - b.mtime;
      else if (by === "size") c = a.size - b.size;
      else if (by === "type") c = collator.compare(a.kind, b.kind);
      // "name" (and every tie) resolves to folder-grouped numeric path order.
      if (c === 0) c = collator.compare(a.path, b.path);
      return c * dir;
    });
  });

  let relatedIndex = $derived(buildRelatedIndex(baseView));
  let relatedGroupCount = $derived(relatedIndex.groups.length);

  let view = $derived.by(() => {
    const out: MediaItem[] = [];
    const emitted = new Set<string>();
    for (const it of baseView) {
      const group = relatedIndex.groupByPath.get(it.path);
      if (!group) {
        out.push(it);
        continue;
      }
      if (emitted.has(group.id)) continue;
      emitted.add(group.id);
      if (groupExpanded(group)) out.push(...group.items);
      else out.push(group.representative);
    }
    return out;
  });
  let relatedHiddenCount = $derived(Math.max(0, baseView.length - view.length));

  type GridSection = {
    label: string;
    count: number;
    level?: 1 | 2;
    cellCount?: number;
    /** Event blocks get an album-art band: a cover frame and a date range. */
    kind?: "event";
    cover?: string | null;
    sub?: string;
    h?: number;
  };

  /** Height of an event's album-art band vs an ordinary text header. */
  const EVENT_BAND_H = 128;

  // Grouped grid sections over the sorted view. Group + subgroup render as true
  // nested headers: the parent carries a total count, the child owns the cells.
  let sections = $derived.by(() => {
    const out: GridSection[] = [];
    const primaryBy = settings.s.groupBy !== "none" ? settings.s.groupBy : settings.s.subgroupBy;
    const subBy =
      settings.s.groupBy !== "none" && settings.s.subgroupBy !== "none" && settings.s.subgroupBy !== settings.s.groupBy
        ? settings.s.subgroupBy
        : "none";
    let primaryKey = "";
    let subKey = "";
    let primary: GridSection | null = null;
    let leaf: GridSection | null = null;
    for (const it of view) {
      const anchor = relatedIndex.groupByPath.get(it.path)?.representative ?? it;
      const pk = sectionPartKey(anchor, primaryBy);
      if (pk !== primaryKey) {
        const label = sectionPartLabel(anchor, primaryBy) || "All media";
        primary = { label, count: 0, level: 1, cellCount: 0 };
        // An event section is drawn as a cover band, the way an album fronts a
        // trip — that's what makes a long recursive feed readable.
        if (primaryBy === "event" && label !== NO_EVENT_LABEL) {
          primary.kind = "event";
          primary.sub = eventDateRange(label);
          if (settings.s.eventCovers) {
            primary.cover = eventCoverPath(label);
            primary.h = EVENT_BAND_H;
          }
        }
        out.push(primary);
        primaryKey = pk;
        subKey = "";
        leaf = null;
      }
      if (subBy !== "none") {
        const sk = sectionPartKey(anchor, subBy);
        if (sk !== subKey) {
          leaf = { label: sectionPartLabel(anchor, subBy) || "Other", count: 0, level: 2, cellCount: 0 };
          out.push(leaf);
          subKey = sk;
        }
        if (primary) primary.count++;
        if (leaf) {
          leaf.count++;
          leaf.cellCount = (leaf.cellCount ?? 0) + 1;
        }
      } else if (primary) {
        primary.count++;
        primary.cellCount = (primary.cellCount ?? 0) + 1;
      }
    }
    return out;
  });
  let grouped = $derived((settings.s.groupBy !== "none" || settings.s.subgroupBy !== "none") && viewMode === "grid");

  let active = $derived(view.length ? view[Math.min(activeIndex, view.length - 1)] : null);
  let selectedItems = $derived(items.filter((i) => selected.has(i.path)));
  let actionTargets = $derived.by(() => {
    if (selected.size > 1) return items.filter((i) => selected.has(i.path));
    return active ? [active] : [];
  });
  let allTargetsRejected = $derived(actionTargets.length > 0 && actionTargets.every((i) => i.flag === "reject"));
  // "?" entries are excluded from the counts that drive the delete sweep — a
  // rejected mark on a file that is already gone is not something to dispose of.
  let rejectedCount = $derived(items.filter((i) => i.flag === "reject" && !i.missing).length);
  let pickCount = $derived(items.filter((i) => i.flag === "pick" && !i.missing).length);
  /** Catalog entries in this folder view whose file was not found. */
  let missingInView = $derived(items.filter((i) => i.missing).length);
  let stripCell = $derived(Math.max(64, settings.s.filmstripSize - 24));
  // Thumbnail decode sizes, matched to how big the cells are actually drawn.
  let gridThumbTier = $derived(tierFor(settings.s.gridSize));
  let stripThumbTier = $derived(tierFor(stripCell));

  $effect(() => {
    if (activeIndex > view.length - 1) activeIndex = Math.max(0, view.length - 1);
  });

  onMount(async () => {
    // Dev-only WebCodecs feasibility probe (inert without VITE_SCRUB_PROBE).
    if (import.meta.env.DEV) {
      import("$lib/scrub-probe").then((m) => m.maybeRunScrubProbe());
    }
    await settings.init();
    try {
      drives = await api.listDrives();
    } catch {
      drives = [];
    }
    try {
      libInfo = await api.libraryInfo();
    } catch {
      /* */
    }
    // Live progress for the bulk RAW→JPEG export (drives the ActivityBar chip).
    try {
      await api.onRawExportProgress((p) =>
        activity.local("raw-export", "Export JPEG from RAW", p.done, p.total),
      );
    } catch {
      // not inside Tauri (tests) — the awaited result still finalises the job
    }
    // Reopen the last folder AND land on the last photo we were looking at.
    if (settings.s.lastDir) {
      await openFolder(settings.s.lastDir, { selectPath: settings.s.lastActivePath });
      // Then verify the catalog still matches the disk. Deliberately AFTER the
      // folder is on screen — the user should never wait on it to start work,
      // and the pass is cheap unless something actually moved. Silent when
      // everything resolves; it only speaks up if files are unaccounted for.
      if (settings.s.scanOnLaunch) void runCatalogScan();
    }
  });

  // Heartbeat: log heap + loader caches every 20s so the logfile shows whether
  // memory climbs while scrolling a folder (not just across switches). In an
  // $effect (not the async onMount) so the interval is cleaned up correctly.
  $effect(() => {
    const beat = setInterval(() => {
      if (currentDir) logMem(`tick ${basename(currentDir)}`);
    }, 20000);
    return () => clearInterval(beat);
  });

  // ── Compositor (paint) liveness ────────────────────────────────────────────
  // The heartbeat above is a setInterval — it proves only that the JS TIMER
  // queue still runs, NOT that WebView2 is still presenting frames. Those two
  // diverge in exactly the failure we are chasing: a fast scroll can wedge the
  // GPU/compositor process, leaving the screen frozen black and input dead while
  // setInterval keeps firing. This rAF loop advances `rafFrames`, which the MEM
  // tick prints as `raf=N`. If two consecutive ticks show the SAME raf count,
  // the compositor was dead for that whole 20 s window while the renderer lived
  // — the decisive signal separating a compositor stall from a wedged renderer
  // (both ticks stop) or a pure present-path failure (both keep advancing). A
  // resume after a long gap is logged too, for the case where it recovers.
  let rafFrames = 0;
  $effect(() => {
    let handle = 0;
    let last = performance.now();
    const tick = () => {
      rafFrames++;
      const now = performance.now();
      const gap = now - last;
      if (gap > 1500) void api.logNote(`PAINT-RESUME gap=${Math.round(gap)}ms frames=${rafFrames}`);
      last = now;
      handle = requestAnimationFrame(tick);
    };
    handle = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(handle);
  });

  function rootForDir(dir: string): string {
    const d = drives.find((dr) => dir.toLowerCase().startsWith(dr.path.toLowerCase()));
    if (d) return d.path;
    const m = dir.match(/^[A-Za-z]:[\\/]/);
    return m ? m[0] : dir;
  }

  async function refreshTags() {
    try {
      allTags = await api.listTags();
    } catch {
      allTags = [];
    }
  }

  // Diagnostic memory probe → the on-disk logfile (UI MEM …). Lets us confirm the
  // JS heap + loader caches stay FLAT across folder switches instead of climbing
  // (the signature of the old "progressively worse" leak). `performance.memory`
  // is the renderer JS heap; watch msedgewebview2.exe in Task Manager for the
  // decoded-image memory, which Chromium manages off-heap.
  function logMem(tag: string) {
    try {
      const mem = (performance as unknown as { memory?: { usedJSHeapSize: number; jsHeapSizeLimit: number } })
        .memory;
      const s = loaderStats();
      const heap = mem
        ? `heapMB=${Math.round(mem.usedJSHeapSize / 1048576)}/${Math.round(mem.jsHeapSizeLimit / 1048576)}`
        : "heap=n/a";
      api.logEvent(
        `MEM ${tag} ${heap} memo=${s.memo} loupe=${s.loupe} pending=${s.pending} queue=${s.queue}/${s.queueStorage} inflight=${s.inflight}/${s.heavyInflight} raf=${rafFrames}`,
      );
    } catch {
      /* diagnostics only — never throw */
    }
  }

  // Recompute the left-pane folder counts (they're cached and never auto-stale,
  // so this is the manual "I added/removed files" refresh).
  let recounting = $state(false);
  async function refreshCounts() {
    if (recounting) return;
    recounting = true;
    const dir = currentDir;
    const keepPath = active?.path ?? null;
    const keepIndex = activeIndex;
    try {
      // Re-enumerate physical drives first, so a just-plugged-in USB/SSD shows up
      // in the sidebar on manual refresh (there's no OS mount-event listener, so
      // this button is the discovery path). Never let a listing hiccup wipe the
      // tree — keep the previous list if the call fails or returns nothing.
      try {
        const found = await api.listDrives();
        if (found.length) drives = found;
      } catch {
        /* keep existing drives */
      }
      await api.clearFolderCounts();
      countsGen++;
      if (dir) {
        await openFolder(dir, { selectPath: keepPath, selectIndex: keepIndex });
      }
    } finally {
      // Brief spin so the click feels acknowledged even when it's instant.
      setTimeout(() => (recounting = false), 400);
    }
  }

  async function openFolder(
    dir: string,
    opts: { selectPath?: string | null; selectIndex?: number } = {},
  ) {
    currentDir = dir;
    loading = true;
    resetThumbs();
    selected = new Set();
    selectionAnchor = null;
    captureMap = {};
    capturesDir = null;
    try {
      libInfo = await api.setLibraryRoot(rootForDir(dir));
      items = await api.listFolderMedia(dir, settings.s.includeSub);
      folderRefreshKey++;
      writable = await api.folderWritable(dir);
    } catch (e) {
      items = [];
      folderRefreshKey++;
      console.error(e);
    }
    // Land on the requested photo (restore on launch) or index (stay put after a
    // delete), else the top.
    let idx = 0;
    if (opts.selectPath) {
      const found = displayIndexForPath(opts.selectPath);
      if (found >= 0) idx = found;
      else if (opts.selectIndex != null) idx = Math.max(0, Math.min(opts.selectIndex, view.length - 1));
    } else if (opts.selectIndex != null) {
      idx = Math.max(0, Math.min(opts.selectIndex, view.length - 1));
    }
    activeIndex = idx;
    if (view.length) {
      selected = new Set([view[idx].path]);
      selectionAnchor = view[idx].path;
    }
    loading = false;
    settings.set({ lastDir: dir });
    // Let the grid mount, then bring the restored/next photo into view.
    setTimeout(scrollActive, 80);
    // Warm thumbnails in the order they're shown (top-down), but only after the
    // visible cells have had a head start — the on-screen lazy loads grab the
    // disk first, then the warmer trickles the rest in. Guard against a folder
    // switch landing during the delay.
    // Folder-open warming is images-only and capped at 600 on the backend, so
    // send only the first 600 image paths — not thousands of paths (mostly
    // videos it discards) serialized over IPC on every folder open.
    const order = baseView.filter((i) => i.kind === "image" && !i.missing).slice(0, 600).map((i) => i.path);
    const tier = gridThumbTier;
    setTimeout(() => {
      if (currentDir === dir) api.warmThumbnails(order, tier);
    }, 500);
    logMem(`open ${basename(dir)} n=${items.length}`);
    refreshTags();
    refreshEvents();
    // Index real capture dates in the background — only when a date-driven view
    // needs them (sort-by-capture or month grouping). Cached after the first pass.
    maybeFetchCaptures();
  }

  /** Whether the current view depends on real capture dates. */
  // Event blocks ordered by date need real capture times too — an event's rank
  // is its earliest shot, and mtime would rank a re-copied folder wrongly.
  let needCaptures = $derived(DATE_GROUPS.has(settings.s.groupBy) || DATE_GROUPS.has(settings.s.subgroupBy) || settings.s.sortBy === "capture" || (groupingByEvent && settings.s.eventOrder === "date") || hasBurstLikeNames(items));

  let capturesDir: string | null = null;
  async function fetchCaptures(dir: string, paths: string[]) {
    if (!paths.length) return;
    capturesDir = dir;
    try {
      const res = await api.captureDates(dir, paths);
      if (currentDir !== dir) return;
      const m: Record<string, number> = {};
      for (const r of res) m[r.path] = r.captured;
      captureMap = m;
    } catch {
      capturesDir = null; // allow a retry
    }
  }
  function maybeFetchCaptures() {
    if (!currentDir || !needCaptures) return;
    if (capturesDir === currentDir) return; // already fetched for this folder
    fetchCaptures(
      currentDir,
      items.map((i) => i.path),
    );
  }

  async function openFolderPicker() {
    const picked = await api.pickFolder();
    if (picked) {
      if (!drives.length) {
        try {
          drives = await api.listDrives();
        } catch {
          /* */
        }
      }
      openFolder(picked);
    }
  }

  async function toggleSub() {
    await settings.set({ includeSub: !settings.s.includeSub });
    if (currentDir) await openFolder(currentDir);
  }

  function setView(v: ViewMode) {
    editOpen = false;
    settings.set({ viewMode: v });
  }

  function openEditMode() {
    editOpen = true;
    api.cancelWarm();
  }

  function targets(): MediaItem[] {
    return actionTargets;
  }

  function targetPaths(): string[] {
    return targets().map((i) => i.path);
  }

  function pathsForDrag(item: MediaItem): string[] {
    // Never hand a "?" entry to a move — there is no file to move, and the
    // catalog row it stands for is resolved by relinking, not by dragging.
    if (selected.size > 1 && selected.has(item.path))
      return targets().filter((i) => !i.missing).map((i) => i.path);
    return item.missing ? [] : [item.path];
  }

  function beginMediaDrag(e: DragEvent, item: MediaItem, i: number) {
    if (!(selected.size > 1 && selected.has(item.path))) setActiveTo(i);
    const paths = pathsForDrag(item);
    draggingPaths = paths;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("application/x-foxcull-paths", JSON.stringify(paths));
      e.dataTransfer.setData("text/plain", paths.join("\n"));
    }
  }

  function endMediaDrag() {
    draggingPaths = [];
  }

  async function movePathsTo(paths: string[], dest: string) {
    if (!paths.length || movingFiles) return;
    movingFiles = true;
    try {
      const r = await api.moveMediaFiles(paths, dest);
      if (r.moved) {
        activity.local("move-files", `Moved ${r.moved} file${r.moved === 1 ? "" : "s"}`, 1, 1);
      }
      if (r.failed.length) {
        activity.error("move-files-error", `Move failed for ${r.failed.length} file${r.failed.length === 1 ? "" : "s"}${r.errors[0] ? `: ${r.errors[0]}` : ""}`);
      }
      cutPaths = [];
      draggingPaths = [];
      countsGen++;
      if (currentDir) {
        const firstMoved = r.files[0]?.to ?? null;
        const canSeeMoved =
          !!firstMoved && (samePath(dest, currentDir) || (settings.s.includeSub && isUnder(dest, currentDir)));
        await openFolder(currentDir, {
          selectPath: canSeeMoved ? firstMoved : null,
          selectIndex: activeIndex,
        });
      }
    } catch (e) {
      activity.error("move-files-error", `Move failed (${e})`);
    } finally {
      movingFiles = false;
    }
  }

  function cutSelection() {
    const paths = targetPaths();
    if (!paths.length) return;
    cutPaths = paths;
    activity.local("cut-files", `Ready to move ${paths.length} file${paths.length === 1 ? "" : "s"}`, 1, 1);
  }

  async function pasteCutSelection() {
    if (!currentDir || !cutPaths.length) return;
    await movePathsTo(cutPaths, currentDir);
  }

  function scrollActive() {
    gridComp?.scrollToIndex(activeIndex);
  }

  function displayIndexForPath(path: string | null | undefined): number {
    if (!path) return -1;
    const exact = view.findIndex((i) => i.path === path);
    if (exact >= 0) return exact;
    const group = relatedIndex.groupByPath.get(path);
    if (!group) return -1;
    return view.findIndex((i) => i.path === group.representative.path);
  }

  async function refreshAfterMediaOutput(selectPath?: string | null) {
    if (!currentDir) return;
    countsGen++;
    await openFolder(currentDir, { selectPath: selectPath ?? active?.path ?? null, selectIndex: activeIndex });
  }

  function settleActivePath(path: string | null | undefined) {
    requestAnimationFrame(() => {
      const idx = displayIndexForPath(path);
      if (idx >= 0) setActiveTo(idx);
      else if (activeIndex >= view.length) setActiveTo(Math.max(0, view.length - 1));
    });
  }

  function setRelatedMode(mode: typeof settings.s.relatedMode) {
    const keep = active?.path ?? null;
    if (mode === "expanded") expandedRelatedGroups = new Set();
    settings.set({ relatedMode: mode });
    settleActivePath(keep);
  }

  function expandRelatedGroup(group: RelatedGroup, path = active?.path ?? group.representative.path) {
    expandedRelatedGroups = new Set([...expandedRelatedGroups, group.id]);
    settleActivePath(path);
  }

  function collapseRelatedGroup(group: RelatedGroup, path = active?.path ?? group.representative.path) {
    const next = new Set(expandedRelatedGroups);
    next.delete(group.id);
    expandedRelatedGroups = next;
    if (settings.s.relatedMode === "expanded") settings.set({ relatedMode: "collapsed" });
    settleActivePath(path);
  }

  function collapseAllRelated() {
    const keep = active?.path ?? null;
    expandedRelatedGroups = new Set();
    settings.set({ relatedMode: "collapsed" });
    settleActivePath(keep);
  }

  function relatedFor(it: MediaItem): RelatedMeta | undefined {
    return relatedIndex.metaByPath.get(it.path);
  }

  function relatedCollapsed(meta: RelatedMeta | undefined): boolean {
    return !!meta && !groupExpanded(meta.group);
  }

  function isCollapsedRepresentative(it: MediaItem, meta = relatedFor(it)): boolean {
    return relatedCollapsed(meta) && meta?.group.representative.path === it.path;
  }

  // Clicking the golden stack line on a tile toggles its group between the
  // single-line (expanded) and double-line (collapsed) states. Stops the click
  // from also selecting/activating the underlying tile.
  function toggleStack(e: MouseEvent, meta: RelatedMeta | undefined, path: string) {
    if (!meta) return;
    e.stopPropagation();
    e.preventDefault();
    if (relatedCollapsed(meta)) expandRelatedGroup(meta.group, path);
    else collapseRelatedGroup(meta.group, path);
  }

  function relatedRoleLabel(meta: RelatedMeta | undefined): string {
    if (!meta) return "";
    if (meta.role === "mother") return "Original";
    if (meta.relation === "subclip") return "Subclip";
    if (meta.relation === "edit") return "Edit";
    if (meta.role === "burst") return "Burst";
    if (meta.role === "sidecar") return meta.group.badges.includes("RAW+JPEG") ? "Sidecar" : "Motion";
    return "Related";
  }

  function relatedTitle(it: MediaItem): string {
    const meta = relatedFor(it);
    if (!meta) return it.name;
    const badges = meta.group.badges.join(", ");
    const state = isCollapsedRepresentative(it, meta) ? `; showing 1 of ${meta.count}` : "";
    return `${it.name} - ${relatedRoleLabel(meta)} in ${meta.count}-item group${badges ? ` (${badges})` : ""}${state}`;
  }

  function shortRelatedBadge(meta: RelatedMeta | undefined): string {
    const b = meta?.group.badges[0];
    if (!b) return "";
    if (b === "RAW+JPEG") return "R+J";
    if (b === "Crop/Edit") return "Edit";
    return b;
  }

  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  function rememberActive() {
    const a = view[activeIndex];
    if (!a) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => settings.set({ lastActivePath: a.path }), 400);
  }

  function clampViewIndex(i: number) {
    return Math.max(0, Math.min(i, view.length - 1));
  }

  function anchorIndexForSelection() {
    const idx = displayIndexForPath(selectionAnchor);
    return idx >= 0 ? idx : activeIndex;
  }

  function setActiveTo(i: number, opts: { extend?: boolean } = {}) {
    if (!view.length) {
      activeIndex = 0;
      selected = new Set();
      selectionAnchor = null;
      return;
    }
    const nextIndex = clampViewIndex(i);
    const anchorIndex = opts.extend ? anchorIndexForSelection() : nextIndex;
    activeIndex = nextIndex;
    const a = view[activeIndex];
    if (opts.extend) {
      const lo = Math.min(anchorIndex, nextIndex);
      const hi = Math.max(anchorIndex, nextIndex);
      selected = new Set(view.slice(lo, hi + 1).map((item) => item.path));
      selectionAnchor = view[anchorIndex]?.path ?? a?.path ?? null;
    } else {
      selected = a ? new Set([a.path]) : new Set();
      selectionAnchor = a?.path ?? null;
    }
    scrollActive();
    rememberActive();
  }

  function move(delta: number, opts: { extend?: boolean } = {}) {
    setActiveTo(activeIndex + delta, opts);
  }

  function navDelta(key: string) {
    if (viewMode === "grid") {
      const cols = Math.max(1, gridComp?.columnCount?.() ?? 1);
      if (key === "ArrowDown") return cols;
      if (key === "ArrowUp") return -cols;
    }
    if (key === "ArrowRight" || key === "ArrowDown") return 1;
    if (key === "ArrowLeft" || key === "ArrowUp") return -1;
    return 0;
  }

  // ── Focus-view preview prefetch ────────────────────────────────────────────
  // Keep the shots just ahead/behind the active one decoded and warm, biased in
  // the direction of travel, so ←/→ in Focus is instant and short backtracks
  // don't re-blur. Videos are skipped (their poster is already warmed elsewhere).
  let lastPrefetchIndex = 0;
  function prefetchAroundActive() {
    if (viewMode !== "loupe" || !view.length) return;
    const dir = activeIndex >= lastPrefetchIndex ? 1 : -1;
    lastPrefetchIndex = activeIndex;
    const tryAt = (i: number) => {
      const it = view[i];
      if (it && (it.kind === "image" || it.kind === "raw")) prefetchLoupe(it.path);
    };
    for (let k = 1; k <= PREFETCH_AHEAD; k++) tryAt(activeIndex + dir * k);
    for (let k = 1; k <= PREFETCH_BEHIND; k++) tryAt(activeIndex - dir * k);
  }
  // Fire whenever the active shot or the view changes while in Focus.
  $effect(() => {
    activeIndex;
    viewMode;
    view;
    prefetchAroundActive();
  });

  // ── neighbouring-clip scrub prefetch (opt-in) ──────────────────────────────
  // The photo equivalent above is cheap; a video scrub strip is ~40 keyframe
  // decodes, so this one is a SETTING and off by default. When it's on, the
  // clips either side of the one you're watching get their hover strip built in
  // the background, so stepping to the next clip finds skimming already live.
  // Deliberately conservative: only while a video is open in Focus, only with
  // Live Scrub on, a settle delay so arrowing through a folder doesn't queue a
  // build per clip you passed, and the backend still serializes the builds.
  // (Neighbour scrub prefetch was removed on 2026-07-21. It pre-built sprite
  //  sheets for the clips either side of the open one so stepping to the next
  //  could be skimmed immediately — worth it when skimming needed a sprite. It
  //  no longer does: Focus and armed grid tiles both decode frames live, so the
  //  prefetch was spending disk and CPU on artifacts nothing reads.)

  // Restore grid position when returning from Focus: bring the shot you were
  // looking at back into the middle of the grid, instead of snapping to the top
  // (which happened because the grid component remounts at scroll 0).
  let prevViewMode: ViewMode = "grid";
  $effect(() => {
    const vm = viewMode;
    if (vm === "loupe" && prevViewMode !== "loupe") {
      // Entering Focus: abandon background warming so the big preview generation
      // and (especially) video playback get the USB SSD's read bandwidth instead
      // of stuttering behind the warmer.
      api.cancelWarm();
    }
    if (vm !== "loupe" && prevViewMode === "loupe") {
      const i = activeIndex;
      requestAnimationFrame(() => gridComp?.scrollToIndex(i, true));
    }
    prevViewMode = vm;
  });

  // ── Prepare folder: pre-cache full previews for the whole folder up front ──
  // The grid warmer only makes small thumbnails; this generates every shot's big
  // Focus preview (and video posters) so a culling pass through the folder has
  // zero blur. Runs on the backend's bounded pool; safe to keep working meanwhile.
  let preparing = $state(false);
  let prepared = $state(false);
  let prepDone = $state(0);
  let prepTotal = $state(0);
  let prepEta = $state("");
  let prepPct = $derived(prepTotal ? Math.round((prepDone / prepTotal) * 100) : 0);
  /** What Prepare covers. Default stays the whole folder — the scopes exist so
   *  a 20-clip 4K folder can be narrowed to "just what I selected" instead of
   *  committing to the full pass. */
  type PrepScope = "all" | "selection" | "videos" | "photos";
  let prepMenuOpen = $state(false);
  const PREP_SCOPES: { key: PrepScope; label: string }[] = [
    { key: "all", label: "Everything in this folder" },
    { key: "selection", label: "Selection only" },
    { key: "videos", label: "Videos in this folder" },
    { key: "photos", label: "Photos & RAW in this folder" },
  ];
  function prepScopeItems(scope: PrepScope): MediaItem[] {
    // "?" entries have no file to pre-cache; they'd only add guaranteed misses.
    const pool = baseView.filter((i) => !i.missing);
    if (scope === "selection") return actionTargets.filter((i) => !i.missing);
    if (scope === "videos") return pool.filter((i) => i.kind === "video");
    if (scope === "photos") return pool.filter((i) => i.kind === "image" || i.kind === "raw");
    return pool;
  }
  async function prepareFolder(scope: PrepScope = "all") {
    if (!currentDir || preparing) return;
    const src = prepScopeItems(scope);
    if (!src.length) return;
    preparing = true;
    prepared = false;
    const dir = currentDir;
    // Focus previews are the big (1920px) renders; the small grid thumbs are
    // already warmed on folder-open. Photos/RAW run FIRST (fast, and the most
    // common reason to Prepare), then videos (posters + hover scrub strips —
    // seconds each, not milliseconds). Keeping the phases separate is what
    // makes the ETA honest: one blended per-item rate over a folder that's
    // 90% photos and 10% long videos claims "5 minutes" for a 20-minute job.
    const photoPaths = src.filter((i) => i.kind === "image" || i.kind === "raw").map((i) => i.path);
    const videoPaths = src.filter((i) => i.kind === "video").map((i) => i.path);
    prepTotal = photoPaths.length + videoPaths.length;
    prepDone = 0;
    prepEta = "";
    // Per-kind ms/item: measured once that phase has data; until then a prior
    // from the target machines (photo previews ~0.3s; video poster + scrub
    // strip ~4s with keyframe-seek extraction).
    const PHOTO_PRIOR_MS = 300;
    const VIDEO_PRIOR_MS = 4000;
    let photoMs: number | null = null;
    let videoMs: number | null = null;
    const updateEta = () => {
      const photosLeft = Math.max(0, photoPaths.length - Math.min(prepDone, photoPaths.length));
      const videosLeft = Math.max(0, prepTotal - Math.max(prepDone, photoPaths.length));
      const remainMs =
        photosLeft * (photoMs ?? PHOTO_PRIOR_MS) + videosLeft * (videoMs ?? VIDEO_PRIOR_MS);
      prepEta = remainMs > 1500 ? fmtEta(remainMs / 1000) : "almost done";
    };
    const CHUNK = 16;
    const runPhase = async (phase: string[], setRate: (msPerItem: number) => void) => {
      let phaseDone = 0;
      const t0 = performance.now();
      for (let i = 0; i < phase.length; i += CHUNK) {
        if (currentDir !== dir) return false; // folder switched — abandon
        // heavy=true: Prepare explicitly includes RAW previews and video
        // posters/scrub strips (the automatic folder-open warmer skips them
        // by design).
        await api.warmThumbnails(phase.slice(i, i + CHUNK), LOUPE_MAX, true);
        phaseDone = Math.min(phase.length, i + CHUNK);
        prepDone += Math.min(CHUNK, phase.length - i);
        setRate((performance.now() - t0) / phaseDone);
        updateEta();
        // Mirror into the global activity chip (visible from any view).
        activity.local("prepare", "Preparing previews & scrub strips", prepDone, prepTotal);
      }
      return true;
    };
    updateEta();
    try {
      if (await runPhase(photoPaths, (ms) => (photoMs = ms))) {
        await runPhase(videoPaths, (ms) => (videoMs = ms));
      }
    } finally {
      preparing = false;
      activity.end("prepare");
      // Only flash "ready" if we're still on the same folder we prepared.
      if (currentDir === dir) {
        prepared = true;
        setTimeout(() => (prepared = false), 2500);
      }
    }
  }

  // ── undo / redo for culling decisions ─────────────────────────────────────
  // Snapshot-based (never closures): every mark mutation records the affected
  // items' FULL mark state before and after, so undo/redo is just "re-apply a
  // snapshot" — immune to intervening changes and double-application. Scope is
  // deliberately catalog decisions only (rating / label / flag / tags); file
  // operations (delete, move, export) stay outside the stack — they have their
  // own safety nets (in-app Trash, uniquified outputs) and silently undoing
  // filesystem changes is scarier than helpful.
  type MarkSnap = { path: string; rating: number; label: string | null; flag: MediaItem["flag"]; tags: string[] };
  type MarkEntry = { kind: "marks"; label: string; before: MarkSnap[]; after: MarkSnap[] };
  /** A dispose that went to the in-app Trash. Undoing it restores the files —
   *  a filesystem move, so unlike a mark it asks first, and it is deliberately
   *  NOT redoable: "redo" on a delete would silently re-trash files while the
   *  user is stepping back through history. */
  type DeleteEntry = { kind: "delete"; label: string; stored: string[] };
  type UndoEntry = MarkEntry | DeleteEntry;
  const UNDO_CAP = 100;
  let undoStack = $state<UndoEntry[]>([]);
  let redoStack = $state<UndoEntry[]>([]);
  let undoToast = $state<string | null>(null);
  let undoToastTimer: ReturnType<typeof setTimeout> | undefined;
  /** One in-app modal, used both to confirm a filesystem action and to show a
   *  failure the activity chip is too small to explain (`onconfirm` omitted =
   *  a notice with a single Close button). */
  type Ask = {
    title: string;
    body: string;
    confirmLabel?: string;
    /** Present, and the modal becomes a prompt: a single text field whose
     *  trimmed value is handed to `onconfirm`, and Confirm stays disabled while
     *  it is blank. Used for folder and event names. */
    input?: { placeholder?: string; value?: string };
    onconfirm?: (value: string) => void | Promise<void>;
  };
  let ask = $state<Ask | null>(null);
  let askValue = $state("");
  let askInputEl = $state<HTMLInputElement | null>(null);
  /** Always open the modal through here so the input field starts clean. */
  function openAsk(a: Ask) {
    askValue = a.input?.value ?? "";
    ask = a;
  }
  // A prompt is useless if you have to click into it first.
  $effect(() => {
    if (ask?.input && askInputEl) {
      askInputEl.focus();
      askInputEl.select();
    }
  });
  async function runAsk() {
    const a = ask;
    const value = askValue.trim();
    if (a?.input && !value) return;
    ask = null;
    askValue = "";
    await a?.onconfirm?.(value);
  }
  function showUndoToast(msg: string) {
    undoToast = msg;
    clearTimeout(undoToastTimer);
    undoToastTimer = setTimeout(() => (undoToast = null), 2600);
  }
  function snapMarks(ts: MediaItem[]): MarkSnap[] {
    return ts.map((i) => ({ path: i.path, rating: i.rating, label: i.label, flag: i.flag, tags: [...i.tags] }));
  }
  /** Call AFTER a mutation, with the pre-mutation snapshot: pushes an undo entry
   *  (skipping no-ops) and doubles as the action log the user asked for. */
  function commitUndo(label: string, before: MarkSnap[]) {
    const byPath = new Map(items.map((i) => [i.path, i]));
    const after = snapMarks(before.map((s) => byPath.get(s.path)).filter((i): i is MediaItem => !!i));
    if (JSON.stringify(before) === JSON.stringify(after)) return;
    undoStack = [...undoStack.slice(-(UNDO_CAP - 1)), { kind: "marks", label, before, after }];
    redoStack = [];
    api.logEvent(`MARK ${label} (${before.length} item${before.length === 1 ? "" : "s"})`);
  }
  /** Record a dispose so Undo can pull it back out of the in-app Trash. */
  function commitDeleteUndo(stored: string[]) {
    if (!stored.length) return;
    const label = `Delete ${stored.length} file${stored.length === 1 ? "" : "s"}`;
    undoStack = [...undoStack.slice(-(UNDO_CAP - 1)), { kind: "delete", label, stored }];
    redoStack = [];
  }
  /** Re-apply a snapshot: reconcile each item's marks to it and persist only the
   *  fields that actually differ. Items gone from the open folder are skipped. */
  async function applySnaps(snaps: MarkSnap[]) {
    const byPath = new Map(items.map((i) => [i.path, i]));
    let tagsTouched = false;
    for (const s of snaps) {
      const it = byPath.get(s.path);
      if (!it) continue;
      if (it.rating !== s.rating) {
        it.rating = s.rating;
        api.setRating(s.path, s.rating).catch(() => {});
      }
      if (it.label !== s.label) {
        it.label = s.label;
        api.setLabel(s.path, s.label).catch(() => {});
      }
      if (it.flag !== s.flag) {
        it.flag = s.flag;
        api.setFlag(s.path, s.flag).catch(() => {});
      }
      const cur = new Set(it.tags);
      const want = new Set(s.tags);
      for (const t of want) if (!cur.has(t)) api.addTag([s.path], t).catch(() => {});
      for (const t of cur) if (!want.has(t)) api.removeTag([s.path], t).catch(() => {});
      if (cur.size !== want.size || [...cur].some((t) => !want.has(t))) {
        it.tags = [...s.tags];
        tagsTouched = true;
      }
    }
    if (tagsTouched) refreshTags();
  }
  async function undoLast() {
    const e = undoStack.at(-1);
    if (!e) {
      showUndoToast("Nothing to undo");
      return;
    }
    if (e.kind === "delete") {
      // Files come back out of the Trash — a real filesystem move, and one that
      // can arrive mid-way through a run of rapid Ctrl+Z presses. Confirm before
      // undoing it, and never consume the entry unless the user says yes.
      const n = e.stored.length;
      openAsk({
        title: "Restore deleted files?",
        body: `${n} file${n === 1 ? "" : "s"} will be moved back out of the Trash to ${
          n === 1 ? "its" : "their"
        } original location.`,
        confirmLabel: `Restore ${n}`,
        onconfirm: async () => {
          undoStack = undoStack.slice(0, -1);
          const out = await api.restoreTrash(e.stored);
          showUndoToast(
            out.failed.length
              ? `Restored ${out.restored} · ${out.failed.length} couldn't be restored`
              : `Restored ${out.restored} file${out.restored === 1 ? "" : "s"}`,
          );
          api.logEvent(`UNDO ${e.label} → restored ${out.restored}`);
          if (trashOpen) trashItems = await api.listTrash();
          if (currentDir) await openFolder(currentDir, { selectIndex: activeIndex });
        },
      });
      return;
    }
    undoStack = undoStack.slice(0, -1);
    redoStack = [...redoStack, e];
    await applySnaps(e.before);
    showUndoToast(`Undid: ${e.label}`);
    api.logEvent(`UNDO ${e.label}`);
  }
  async function redoLast() {
    const e = redoStack.at(-1);
    if (!e) {
      showUndoToast("Nothing to redo");
      return;
    }
    redoStack = redoStack.slice(0, -1);
    if (e.kind === "delete") return; // never re-delete from history (see DeleteEntry)
    undoStack = [...undoStack, e];
    await applySnaps(e.after);
    showUndoToast(`Redid: ${e.label}`);
    api.logEvent(`REDO ${e.label}`);
  }

  function rate(r: number) {
    const ts = targets();
    if (!ts.length) return;
    const before = snapMarks(ts);
    if (ts.length === 1) {
      ts[0].rating = ts[0].rating === r ? 0 : r;
      api.setRating(ts[0].path, ts[0].rating).catch(() => {});
    } else {
      for (const it of ts) it.rating = r;
      api.setRatingMany(ts.map((i) => i.path), r).catch(() => {});
    }
    commitUndo(r === 0 ? "Clear stars" : `Rate ${"★".repeat(r)}`, before);
  }
  function label(key: string) {
    const ts = targets();
    if (!ts.length) return;
    const before = snapMarks(ts);
    if (ts.length === 1) {
      ts[0].label = ts[0].label === key ? null : key;
      api.setLabel(ts[0].path, ts[0].label).catch(() => {});
    } else {
      for (const it of ts) it.label = key;
      api.setLabelMany(ts.map((i) => i.path), key).catch(() => {});
    }
    commitUndo(`Label ${key}`, before);
  }
  function flag(f: "pick" | "reject") {
    const ts = targets();
    if (!ts.length) return;
    // Toggle semantics that match the Reject/Pick toolbar buttons (rejectSelected):
    // if EVERY target already carries this flag, pressing the key clears it on all
    // of them; otherwise it sets the flag on all. This makes X/P un-flag a whole
    // selection on the second press, not just single items.
    const before = snapMarks(ts);
    const next = ts.every((i) => i.flag === f) ? null : f;
    for (const it of ts) it.flag = next;
    if (ts.length === 1) api.setFlag(ts[0].path, next).catch(() => {});
    else api.setFlagMany(ts.map((i) => i.path), next).catch(() => {});
    commitUndo(next === null ? `Un${f}` : f === "reject" ? "Reject" : "Pick", before);
  }
  function unset() {
    const ts = targets();
    if (!ts.length) return;
    const before = snapMarks(ts);
    for (const it of ts) {
      it.rating = 0;
      it.label = null;
      it.flag = null;
    }
    const paths = ts.map((i) => i.path);
    if (ts.length === 1) {
      api.setRating(paths[0], 0).catch(() => {});
      api.setLabel(paths[0], null).catch(() => {});
      api.setFlag(paths[0], null).catch(() => {});
    } else {
      api.setRatingMany(paths, 0).catch(() => {});
      api.setLabelMany(paths, null).catch(() => {});
      api.setFlagMany(paths, null).catch(() => {});
    }
    commitUndo("Clear marks", before);
  }

  function clearRatings() {
    const ts = targets();
    if (!ts.length) return;
    const before = snapMarks(ts);
    for (const it of ts) it.rating = 0;
    const paths = ts.map((i) => i.path);
    (ts.length === 1 ? api.setRating(paths[0], 0) : api.setRatingMany(paths, 0)).catch(() => {});
    commitUndo("Clear stars", before);
  }

  function clearLabels() {
    const ts = targets();
    if (!ts.length) return;
    const before = snapMarks(ts);
    for (const it of ts) it.label = null;
    const paths = ts.map((i) => i.path);
    (ts.length === 1 ? api.setLabel(paths[0], null) : api.setLabelMany(paths, null)).catch(() => {});
    commitUndo("Clear labels", before);
  }

  function clearFlags() {
    const ts = targets();
    if (!ts.length) return;
    const before = snapMarks(ts);
    for (const it of ts) it.flag = null;
    const paths = ts.map((i) => i.path);
    (ts.length === 1 ? api.setFlag(paths[0], null) : api.setFlagMany(paths, null)).catch(() => {});
    commitUndo("Clear flags", before);
  }

  async function clearTagsOnTargets() {
    const ts = targets();
    if (!ts.length) return;
    const before = snapMarks(ts);
    const paths = ts.map((i) => i.path);
    const tags = [...new Set(ts.flatMap((i) => i.tags))];
    for (const it of ts) it.tags = [];
    for (const tag of tags) await api.removeTag(paths, tag).catch(() => {});
    refreshTags();
    commitUndo("Clear tags", before);
  }

  async function clearAllMarks() {
    unset();
    await clearTagsOnTargets();
  }

  // ── tags ──────────────────────────────────────────────────────────────────
  async function addTagToTargets() {
    const tag = tagInput.trim();
    const ts = targets();
    if (!tag || !ts.length) return;
    const before = snapMarks(ts);
    for (const it of ts) if (!it.tags.includes(tag)) it.tags = [...it.tags, tag];
    tagInput = "";
    await api.addTag(ts.map((i) => i.path), tag).catch(() => {});
    refreshTags();
    commitUndo(`Tag "${tag}"`, before);
  }
  async function removeTagFromActive(tag: string) {
    if (!active) return;
    const before = snapMarks([active]);
    active.tags = active.tags.filter((t) => t !== tag);
    await api.removeTag([active.path], tag).catch(() => {});
    refreshTags();
    commitUndo(`Untag "${tag}"`, before);
  }

  // ── events (virtual collections) ──────────────────────────────────────────
  // Membership is metadata, so every mutation is applied optimistically to the
  // in-memory items AND persisted; nothing here reloads the folder, which would
  // cost a full re-walk just to add a tag-like mark. Events are deliberately
  // OUTSIDE the undo stack for now — like tags they are additive and reversible
  // from the same menu, and the stack's snapshot shape covers marks only.
  async function refreshEvents() {
    try {
      allEvents = await api.listEvents();
    } catch {
      allEvents = [];
    }
  }

  async function addTargetsToEvent(id: number, name: string) {
    const ts = targets();
    if (!ts.length) return;
    for (const it of ts) if (!it.events.includes(name)) it.events = [...it.events, name];
    await api.addToEvent(id, ts.map((i) => i.path)).catch(() => {});
    await refreshEvents();
    activity.local("event-add", `Added ${ts.length} to “${name}”`, 1, 1);
  }

  async function removeTargetsFromEvent(id: number, name: string) {
    const ts = targets();
    if (!ts.length) return;
    for (const it of ts) it.events = it.events.filter((e) => e !== name);
    await api.removeFromEvent(id, ts.map((i) => i.path)).catch(() => {});
    if (eventFilter === name && !items.some((i) => i.events.includes(name))) eventFilter = null;
    await refreshEvents();
  }

  function newEventForTargets() {
    const ts = targets();
    if (!ts.length) return;
    openAsk({
      title: "New event",
      body: `Name this event — the ${ts.length} selected item${ts.length === 1 ? "" : "s"} will start it. Events span folders, so anything you add later joins the same block wherever it lives.`,
      input: { placeholder: "Monar trip" },
      confirmLabel: "Create",
      onconfirm: async (name) => {
        try {
          const id = await api.createEvent(name);
          await addTargetsToEvent(id, name);
          // Show the result immediately — the point of an event is the block.
          if (settings.s.groupBy !== "event") settings.set({ groupBy: "event" });
        } catch (e) {
          openAsk({ title: "Could not create the event", body: String(e) });
        }
      },
    });
  }

  /** Make the active item the face of one of its events. */
  async function setEventCoverFromActive(ev: EventInfo) {
    if (!active) return;
    await api.setEventCover(ev.id, active.path).catch(() => {});
    await refreshEvents();
    activity.local("event-cover", `“${ev.name}” cover set`, 1, 1);
  }

  function renameEventPrompt(ev: EventInfo) {
    openAsk({
      title: "Rename event",
      body: `“${ev.name}” — ${ev.count} item${ev.count === 1 ? "" : "s"}.`,
      input: { value: ev.name },
      confirmLabel: "Rename",
      onconfirm: async (name) => {
        try {
          await api.renameEvent(ev.id, name);
        } catch (e) {
          openAsk({ title: "Could not rename the event", body: String(e) });
          return;
        }
        for (const it of items) {
          if (it.events.includes(ev.name)) it.events = it.events.map((e) => (e === ev.name ? name : e));
        }
        if (eventFilter === ev.name) eventFilter = name;
        await refreshEvents();
      },
    });
  }

  function deleteEventPrompt(ev: EventInfo) {
    openAsk({
      title: `Delete the event “${ev.name}”?`,
      body: `The ${ev.count} photo${ev.count === 1 ? "" : "s"} in it are not touched — only the grouping goes away. Nothing is removed from disk.`,
      confirmLabel: "Delete event",
      onconfirm: async () => {
        await api.deleteEvent(ev.id).catch(() => {});
        for (const it of items) {
          if (it.events.includes(ev.name)) it.events = it.events.filter((e) => e !== ev.name);
        }
        if (eventFilter === ev.name) eventFilter = null;
        await refreshEvents();
      },
    });
  }

  // ── folder management ─────────────────────────────────────────────────────
  /** Create a subfolder under `parent` — the other half of drag-to-move: you
   *  can build the destination without leaving the app. */
  function newSubfolderPrompt(parent: string) {
    openAsk({
      title: "New subfolder",
      body: `Inside ${basename(parent)}.`,
      input: { placeholder: "Folder name" },
      confirmLabel: "Create",
      onconfirm: async (name) => {
        try {
          const created = await api.createFolder(parent, name);
          await api.clearFolderCounts();
          countsGen++;
          activity.local("new-folder", `Created ${basename(created)}`, 1, 1);
          // If files are staged for a move, land them straight in the new
          // folder — "make a folder for these" is the whole reason to be here.
          if (cutPaths.length) await movePathsTo(cutPaths, created);
        } catch (e) {
          openAsk({ title: "Could not create the folder", body: String(e) });
        }
      },
    });
  }

  // ── catalog integrity (missing files) ─────────────────────────────────────
  /** Verify the catalog against the disk and auto-reconnect what moved. Runs on
   *  launch (see settings.scanOnLaunch) and on demand from the folder menu. */
  async function runCatalogScan(opts: { announce?: boolean } = {}) {
    if (scanning) return;
    scanning = true;
    const job = "catalog-scan";
    activity.local(job, "Checking catalog…", 0, 1);
    try {
      const r = await api.catalogScan(true);
      missingRels = r.still_missing ? await api.listMissing().catch(() => []) : [];
      activity.local(job, "Catalog checked", 1, 1);
      api.logEvent(
        `CATALOG-SCAN tracked=${r.tracked} missing=${r.missing} relinked=${r.relinked} unresolved=${r.still_missing} ${r.elapsed_ms}ms`,
      );
      if (r.relinked) {
        activity.local(
          "catalog-relink",
          `Reconnected ${r.relinked} moved file${r.relinked === 1 ? "" : "s"}`,
          1,
          1,
        );
      }
      if (opts.announce || (r.still_missing && r.missing !== r.relinked)) {
        if (r.still_missing) {
          openAsk({
            title: `${r.still_missing} file${r.still_missing === 1 ? "" : "s"} could not be found`,
            body:
              `${r.relinked} moved file${r.relinked === 1 ? " was" : "s were"} reconnected automatically. ` +
              `The rest keep every rating, label and event and show as “?” in the grid — right-click one and choose ` +
              `“Locate…” to point FoxCull at the file, or “Forget” once you're sure it's gone.`,
          });
        } else if (opts.announce) {
          openAsk({
            title: "Catalog is intact",
            body: `${r.tracked} marked file${r.tracked === 1 ? "" : "s"} checked${r.relinked ? `, ${r.relinked} reconnected` : ""}. Nothing is missing.`,
          });
        }
      }
      if (currentDir) await openFolder(currentDir, { selectPath: active?.path ?? null, selectIndex: activeIndex });
      return r;
    } catch (e) {
      activity.error(job, `Catalog check failed (${e})`);
      return null;
    } finally {
      scanning = false;
    }
  }

  /** Point one "?" entry at the real file. */
  async function locateMissingFile(item: MediaItem) {
    const picked = await api.pickMediaFile();
    if (!picked) return;
    try {
      await api.relinkMissing(item.rel, picked);
      activity.local("relink", `Reconnected ${item.name}`, 1, 1);
      missingRels = await api.listMissing().catch(() => []);
      if (currentDir) await openFolder(currentDir, { selectPath: picked });
    } catch (e) {
      openAsk({ title: "Could not reconnect that file", body: String(e) });
    }
  }

  /** "The whole folder moved over there" — relink every "?" under this item's
   *  old folder against a folder the user picks. */
  async function locateMissingFolder(item: MediaItem) {
    const dir = await api.pickFolder();
    if (!dir) return;
    const relDir = item.rel.includes("/") ? item.rel.slice(0, item.rel.lastIndexOf("/")) : "";
    try {
      const r = await api.relinkFolder(relDir, dir);
      missingRels = await api.listMissing().catch(() => []);
      if (currentDir) await openFolder(currentDir, { selectIndex: activeIndex });
      openAsk({
        title: r.relinked ? `Reconnected ${r.relinked} file${r.relinked === 1 ? "" : "s"}` : "Nothing matched",
        body: r.unresolved.length
          ? `${r.unresolved.length} entr${r.unresolved.length === 1 ? "y" : "ies"} had no match in that folder and still show as “?”.`
          : "Every missing entry from that folder found its file.",
      });
    } catch (e) {
      openAsk({ title: "Could not reconnect that folder", body: String(e) });
    }
  }

  /** Drop the metadata of "?" entries the user confirms are gone for good. */
  function forgetMissingTargets(ts: MediaItem[]) {
    const gone = ts.filter((i) => i.missing);
    if (!gone.length) return;
    openAsk({
      title: `Forget ${gone.length} missing file${gone.length === 1 ? "" : "s"}?`,
      body: "Their ratings, labels, tags and event membership are deleted from the catalog. Nothing on disk changes — this only applies to entries whose file is already gone.",
      confirmLabel: "Forget",
      onconfirm: async () => {
        await api.forgetMissing(gone.map((i) => i.rel)).catch(() => {});
        missingRels = await api.listMissing().catch(() => []);
        if (currentDir) await openFolder(currentDir, { selectIndex: activeIndex });
      },
    });
  }

  function selectAllFiltered() {
    selected = new Set(view.map((i) => i.path));
    selectionAnchor = view[activeIndex]?.path ?? view[0]?.path ?? null;
  }
  function rejectSelected() {
    const sel = targets();
    if (!sel.length) return;
    const before = snapMarks(sel);
    const next = sel.every((i) => i.flag === "reject") ? null : "reject";
    for (const it of sel) it.flag = next;
    api.setFlagMany(sel.map((i) => i.path), next).catch(() => {});
    commitUndo(next === null ? "Unreject" : "Reject", before);
  }

  function gridCellClick(e: MouseEvent, i: number) {
    const it = view[i];
    if (!it) return;
    if (e.shiftKey) {
      setActiveTo(i, { extend: true });
    } else if (e.ctrlKey || e.metaKey) {
      const next = new Set(selected);
      if (next.has(it.path)) next.delete(it.path);
      else next.add(it.path);
      selected = next;
      activeIndex = i;
      selectionAnchor = it.path;
      scrollActive();
      rememberActive();
    } else {
      setActiveTo(i);
    }
  }

  // ── right-click context menu (replaces the webview's native menu) ─────────
  const isMac =
    typeof navigator !== "undefined" && navigator.userAgent.includes("Macintosh");
  const revealLabel = isMac ? "Reveal in Finder" : "Show in Explorer";
  let menu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);

  async function copyPath(p: string) {
    try {
      await navigator.clipboard.writeText(p);
    } catch {
      /* clipboard unavailable — ignore */
    }
  }

  function mediaMenuEntries(ctx: MediaItem, ts: MediaItem[]): MenuEntry[] {
    const sfx = ts.length > 1 ? ` (${ts.length})` : "";
    const allPick = ts.length > 0 && ts.every((i) => i.flag === "pick");
    const allReject = ts.length > 0 && ts.every((i) => i.flag === "reject");
    const rel = relatedFor(ctx);
    const relEntries: MenuEntry[] = rel
      ? [
          {
            label: relatedCollapsed(rel) ? `Expand related group (${rel.count})` : "Collapse related group",
            icon: relatedCollapsed(rel) ? "⊞" : "⊟",
            action: () =>
              relatedCollapsed(rel)
                ? expandRelatedGroup(rel.group, ctx.path)
                : collapseRelatedGroup(rel.group, ctx.path),
          },
          {
            label: settings.s.relatedMode === "collapsed" ? "Show all related groups expanded" : "Show related groups collapsed",
            icon: "▦",
            action: () => setRelatedMode(settings.s.relatedMode === "collapsed" ? "expanded" : "collapsed"),
          },
          { separator: true },
        ]
      : [];

    // A "?" entry has no file behind it, so the menu becomes a relink menu:
    // everything that would touch the disk is replaced by the three ways out —
    // point at the file, point at the folder it moved to, or forget it.
    if (ts.some((i) => i.missing)) {
      const gone = ts.filter((i) => i.missing);
      return [
        {
          label: "Locate this file…",
          icon: "🔎",
          disabled: gone.length !== 1,
          action: () => locateMissingFile(gone[0] ?? ctx),
        },
        {
          label: "Locate the folder it moved to…",
          icon: "📁",
          action: () => locateMissingFolder(gone[0] ?? ctx),
        },
        { label: "Check the whole catalog again", icon: "↻", action: () => runCatalogScan({ announce: true }) },
        { separator: true },
        {
          label: `Forget${gone.length > 1 ? ` ${gone.length} entries` : " this entry"} (deletes its marks)`,
          icon: "⌫",
          danger: true,
          action: () => forgetMissingTargets(gone),
        },
        { separator: true },
        { label: "Copy the path it used to have", icon: "⧉", action: () => copyPath(ctx.path) },
      ];
    }

    // Events. Membership is a per-selection question: an event the whole
    // selection is already in offers removal, anything else offers adding.
    const evEntries: MenuEntry[] = [];
    const inAll = (name: string) => ts.length > 0 && ts.every((i) => i.events.includes(name));
    for (const ev of allEvents.slice(0, 8)) {
      evEntries.push(
        inAll(ev.name)
          ? {
              label: `Remove from “${ev.name}”${sfx}`,
              icon: "−",
              on: true,
              action: () => removeTargetsFromEvent(ev.id, ev.name),
            }
          : { label: `Add to “${ev.name}”${sfx}`, icon: "＋", action: () => addTargetsToEvent(ev.id, ev.name) },
      );
    }
    evEntries.push({ label: "New event…" + sfx, icon: "✦", action: newEventForTargets });
    for (const name of ctx.events) {
      const ev = eventByName.get(name);
      if (ev && ev.cover_rel !== ctx.rel) {
        evEntries.push({
          label: `Use as cover of “${name}”`,
          icon: "★",
          action: () => setEventCoverFromActive(ev),
        });
      }
    }
    evEntries.push({ separator: true });

    return [
      { label: "Previous", icon: "←", disabled: activeIndex <= 0, action: () => move(-1) },
      { label: "Next", icon: "→", disabled: activeIndex >= view.length - 1, action: () => move(1) },
      { separator: true },
      ...relEntries,
      {
        label: viewMode === "loupe" ? "Back to grid" : "Open in Focus",
        icon: "▣",
        action: () => setView(viewMode === "loupe" ? "grid" : "loupe"),
      },
      ...(ctx.kind === "video"
        ? [
            {
              label: "Open in Edit",
              icon: "✎",
              action: openEditMode,
            },
          ]
        : []),
      {
        label: ctx.kind === "video" ? "Open in system player" : "Open in default app",
        icon: "▶",
        action: () => api.openExternal(ctx.path),
      },
      { label: revealLabel, icon: "⤴", action: () => api.reveal(ctx.path) },
      { separator: true },
      { label: (allPick ? "Clear pick" : "Pick") + sfx, icon: "✓", on: allPick, action: () => flag("pick") },
      {
        label: (allReject ? "Clear reject" : "Reject") + sfx,
        icon: "✕",
        danger: !allReject,
        on: allReject,
        action: () => flag("reject"),
      },
      { label: "Clear rating & marks" + sfx, icon: "⟲", action: () => unset() },
      { separator: true },
      ...evEntries,
      {
        label: "Export as JPEG…" + sfx,
        icon: "⇩",
        disabled: !ts.some((i) => i.kind === "image" || i.kind === "raw"),
        action: () => exportTargets(),
      },
      ...(ts.some((i) => i.kind === "raw") || ctx.kind === "raw"
        ? [
            {
              label:
                "Export JPEG from RAW" +
                (ts.filter((i) => i.kind === "raw").length > 1
                  ? ` (${ts.filter((i) => i.kind === "raw").length})`
                  : ""),
              icon: "⤓",
              action: () => exportRawToJpeg(ctx),
            } as MenuEntry,
          ]
        : []),
      { label: "Copy file path", icon: "⧉", action: () => copyPath(ctx.path) },
    ];
  }

  function openContextMenu(e: MouseEvent, ctx: MediaItem, i: number) {
    e.preventDefault();
    // Focus the right-clicked item unless it's already in a multi-selection.
    if (!(selected.size > 1 && selected.has(ctx.path))) setActiveTo(i);
    else activeIndex = i;
    menu = { x: e.clientX, y: e.clientY, entries: mediaMenuEntries(ctx, targets()) };
  }

  async function refreshFolderPath(path: string) {
    await api.clearFolderCounts();
    countsGen++;
    if (currentDir && samePath(path, currentDir)) {
      await openFolder(path, { selectPath: active?.path ?? null, selectIndex: activeIndex });
    }
  }

  function openFolderContextMenu(e: MouseEvent, path: string) {
    e.preventDefault();
    e.stopPropagation();
    const isOpen = currentDir ? samePath(path, currentDir) : false;
    menu = {
      x: e.clientX,
      y: e.clientY,
      entries: [
        { label: "Open folder", icon: "▣", on: isOpen, action: () => openFolder(path) },
        { label: "Refresh folder", icon: "↻", action: () => refreshFolderPath(path) },
        { separator: true },
        {
          label: cutPaths.length ? `New subfolder… (and move ${cutPaths.length} here)` : "New subfolder…",
          icon: "＋",
          action: () => newSubfolderPrompt(path),
        },
        {
          label: `Move ${cutPaths.length || ""} file${cutPaths.length === 1 ? "" : "s"} here`.replace("  ", " "),
          icon: "⇥",
          disabled: cutPaths.length === 0,
          action: () => movePathsTo(cutPaths, path),
        },
        {
          label: "Check catalog for moved files",
          icon: "🔎",
          disabled: scanning,
          action: () => runCatalogScan({ announce: true }),
        },
        { separator: true },
        { label: revealLabel, icon: "↗", action: () => api.reveal(path) },
        { label: "Copy folder path", icon: "⧉", action: () => copyPath(path) },
        { separator: true },
        {
          label: settings.s.includeSub ? "Stop including subfolders" : "Include subfolders",
          icon: "⊞",
          action: () => toggleSub(),
        },
      ],
    };
  }

  /** Suppress the webview's native menu everywhere except real text inputs. */
  function onGlobalContextMenu(e: MouseEvent) {
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
    e.preventDefault();
  }

  // ── long-press delete (no modal, no toast) ──────────────────────────────
  function startHold() {
    if (rejectedCount === 0 || !writable) return;
    const t0 = performance.now();
    const tick = () => {
      holdMs = performance.now() - t0;
      if (holdMs >= HOLD_MS) {
        holdMs = 0;
        executeDelete();
      } else {
        holdRAF = requestAnimationFrame(tick);
      }
    };
    holdRAF = requestAnimationFrame(tick);
  }
  function endHold() {
    cancelAnimationFrame(holdRAF);
    holdMs = 0;
  }
  async function executeDelete() {
    const paths = await api.listRejected();
    if (!paths.length) return;
    // A sprite build / warm pass may hold one of these files open — on Windows
    // that open handle makes the dispose rename fail "in use". Cancel background
    // work and give the current ffmpeg frame extraction a beat to drain (builds
    // cancel between frames, ~100-300 ms) before touching the files.
    api.cancelAllSprites();
    api.cancelWarm();
    await new Promise((r) => setTimeout(r, 350));
    // "folder" -> the active drive's _FoxCull/recycle (recoverable in-app Trash);
    // "recycle" → the OS Recycle Bin / Trash.
    const out = await api.disposeRejected(paths, settings.s.deleteMode);
    // Deletes into the in-app Trash are undoable — record the batch BEFORE the
    // folder reload, so Ctrl+Z right after a delete pulls exactly these back.
    commitDeleteUndo(out.trashed);
    if (out.failed.length) {
      // Don't fail silently, and don't guess: the backend now says WHY each file
      // survived (locked by another program vs. Windows permissions), and those
      // need opposite responses. The activity chip is one line, so the real
      // reasons go in a modal the user can actually read.
      const reasons = [...new Set(out.errors)];
      activity.error(
        "delete",
        `${out.failed.length} file${out.failed.length === 1 ? "" : "s"} couldn't be deleted`,
      );
      openAsk({
        title: `${out.failed.length} file${out.failed.length === 1 ? "" : "s"} couldn't be deleted`,
        body: reasons.join("\n\n"),
      });
    }
    // Stay where we were — after the rejected shots vanish, the same index lands
    // on the next surviving photo, not back at the top of the folder.
    if (currentDir) await openFolder(currentDir, { selectIndex: activeIndex });
  }

  // ── panel resizing ──────────────────────────────────────────────────────
  function startTreeResize(e: PointerEvent) {
    e.preventDefault();
    const startX = e.clientX;
    const startW = settings.s.treeWidth;
    const move = (ev: PointerEvent) => {
      settings.s.treeWidth = Math.max(170, Math.min(560, startW + (ev.clientX - startX)));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      settings.set({ treeWidth: settings.s.treeWidth });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
  // ── filmstrip show/hide ───────────────────────────────────────────────────
  // `filmstripPos: "hidden"` was already a valid dock, but the only way to reach
  // it was the Arrange popover — the folder tree has a one-click collapse and
  // the strip deserved the same. Hiding remembers the dock you were in, so
  // unhiding puts it back rather than defaulting to the bottom.
  let lastDock = $state<"bottom" | "left" | "right">("bottom");
  let stripHidden = $derived(settings.s.filmstripPos === "hidden");

  /** Show/hide the strip without touching the per-view memory. */
  function applyStrip(show: boolean) {
    const pos = settings.s.filmstripPos;
    if (show && pos === "hidden") {
      settings.set({ filmstripPos: lastDock });
    } else if (!show && pos !== "hidden") {
      lastDock = pos;
      settings.set({ filmstripPos: "hidden" });
    }
  }

  function stripWantedIn(v: ViewMode): boolean {
    return settings.s.stripShow?.[v] ?? v === "loupe";
  }

  function toggleFilmstrip() {
    const show = stripHidden; // we're about to show it
    applyStrip(show);
    // Teach this view the new answer, so it sticks until you change it again
    // here — but doesn't leak into the other view.
    settings.set({ stripShow: { ...settings.s.stripShow, [viewMode]: show } });
  }

  // ── the strip follows the view ────────────────────────────────────────────
  // In Grid the filmstrip is a second copy of what's already on screen, so it
  // just eats height; in Focus it's the only way to see where you are in the
  // folder. So it hides itself on the way into Grid and comes back on the way
  // into Focus. `stripViewApplied` makes this fire on VIEW CHANGES only — the
  // effect writes `filmstripPos`, which it also reads, and without the guard a
  // manual toggle would be undone on the very next run.
  let stripViewApplied: ViewMode | null = null;
  $effect(() => {
    const v = viewMode;
    if (stripViewApplied === v) return;
    stripViewApplied = v;
    applyStrip(stripWantedIn(v));
  });

  function startStripResize(e: PointerEvent) {
    e.preventDefault();
    const pos = settings.s.filmstripPos;
    const vertical = pos === "right" || pos === "left";
    const start = vertical ? e.clientX : e.clientY;
    const startS = settings.s.filmstripSize;
    const move = (ev: PointerEvent) => {
      // Drag AWAY from the viewport grows the strip: that's -x for a right dock
      // and a bottom dock (both handles sit before the panel), but +x on the
      // left, where the handle is on the panel's far edge.
      const d =
        pos === "left"
          ? ev.clientX - start
          : start - (vertical ? ev.clientX : ev.clientY);
      settings.s.filmstripSize = Math.max(84, Math.min(520, startS + d));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      settings.set({ filmstripSize: settings.s.filmstripSize });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  // ── full-screen mode (F): just the photo, everything else gone ───────────
  // Fullscreen is a 3-state cycle on F: 0 = off, 1 = play mode WITH the bottom
  // filmstrip (dimmed ~20%, so focus stays on the photo while you can still see
  // the strip), 2 = bare photo/video only. Escape always exits to 0. Edit mode
  // and the controller use the simple 0↔1 toggle (there is no filmstrip there).
  let fsMode = $state<0 | 1 | 2>(0);
  let fullscreen = $derived(fsMode > 0);
  let fsPrevView: ViewMode = "grid";
  async function applyFs(next: 0 | 1 | 2) {
    const wasFs = fsMode > 0;
    fsMode = next;
    const nowFs = next > 0;
    if (wasFs !== nowFs) {
      try {
        await getCurrentWindow().setFullscreen(nowFs);
      } catch {
        // wayland/odd WMs can refuse — the chrome still hides, which is most of it
      }
      if (nowFs) {
        fsPrevView = viewMode;
        if (!editOpen && active) setView("loupe");
      } else if (!editOpen) {
        setView(fsPrevView);
      }
    }
  }
  /** F in Focus/grid: off → play+strip → bare photo → off. */
  function cycleFullscreen() {
    void applyFs(((fsMode + 1) % 3) as 0 | 1 | 2);
  }
  /** Simple on/off toggle (edit mode, controller, Escape). */
  async function toggleFullscreen() {
    await applyFs(fsMode > 0 ? 0 : 1);
  }

  // ── export (RAW → camera-rendered JPEG; images copied through) ───────────
  async function exportTargets() {
    const ts = targets().filter((i) => i.kind === "image" || i.kind === "raw");
    if (!ts.length) {
      activity.error("export-result", "Nothing to export (photos and RAW only)");
      return;
    }
    const dest = await api.pickFolder();
    if (!dest) return;
    const rawCount = ts.filter((i) => i.kind === "raw").length;
    const imageCount = ts.length - rawCount;
    const msg = `Export ${ts.length} file${ts.length === 1 ? "" : "s"} to ${dest}?\n\nRAW files will be saved as camera-rendered JPEGs. JPEG/HEIC/photo files will be copied without changing the originals.`;
    if (!confirm(msg + `\n\nRAW: ${rawCount}  Photos: ${imageCount}`)) return;
    try {
      const r = await api.exportJpegs(ts.map((i) => i.path), dest);
      if (r.failed.length) {
        activity.error(
          "export-result",
          `Export: ${r.failed.length} of ${ts.length} failed — ${r.errors[0] ?? ""}`,
        );
      }
      if (currentDir && (samePath(r.dest, currentDir) || (settings.s.includeSub && isUnder(r.dest, currentDir)))) {
        await refreshAfterMediaOutput(active?.path ?? null);
      }
      // Show the result where the files are: open the destination folder.
      api.openExternal(r.dest);
    } catch (e) {
      activity.error("export-result", `Export failed (${e})`);
    }
  }

  // ── bulk RAW → JPEG (in-place, next to each source) ──────────────────────
  async function exportRawToJpeg(ctx?: MediaItem) {
    // Prefer the current selection/active targets; fall back to the right-clicked
    // item so the context-menu entry works even on an unselected tile.
    const pool = targets().length ? targets() : ctx ? [ctx] : [];
    const raws = pool.filter((i) => i.kind === "raw");
    if (!raws.length) {
      activity.error("raw-export", "No RAW files to convert");
      return;
    }
    activity.local("raw-export", "Export JPEG from RAW", 0, raws.length);
    try {
      const r = await api.exportRawJpegs(raws.map((i) => i.path));
      activity.end("raw-export");
      if (r.failed.length)
        activity.error("raw-export", `RAW→JPEG: ${r.failed.length} of ${raws.length} failed`);
      // The new JPEGs land beside their NEFs and auto-stack via same-stem grouping.
      await refreshAfterMediaOutput(active?.path ?? null);
    } catch (e) {
      activity.error("raw-export", `RAW→JPEG failed (${e})`);
    }
  }

  // ── in-app Trash (per-drive recycle folder) ──────────────────────────────
  async function openTrash() {
    try {
      trashItems = await api.listTrash();
    } catch {
      trashItems = [];
    }
    trashOpen = true;
  }
  async function restoreFromTrash(stored: string[]) {
    await api.restoreTrash(stored);
    trashItems = await api.listTrash();
    // A restored file may belong to the open folder — refresh it.
    if (currentDir) await openFolder(currentDir, { selectIndex: activeIndex });
  }
  async function purgeFromTrash(stored: string[]) {
    await api.purgeTrash(stored);
    trashItems = await api.listTrash();
  }

  async function onkeydown(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) return;
    const k = e.key.toLowerCase();
    // A confirm/notice modal is exclusive: it swallows every key but Escape and
    // Enter, so a stray shortcut can't act on the grid behind an open question.
    if (ask) {
      if (e.key === "Escape") ask = null;
      else if (e.key === "Enter") void runAsk();
      e.preventDefault();
      return;
    }
    // Overlays and popovers first, in every mode: ? toggles the shortcut guide,
    // Escape closes the topmost open thing before doing anything else.
    if (e.key === "?") {
      shortcutsOpen = !shortcutsOpen;
      e.preventDefault();
      return;
    }
    if (e.key === "Escape" && shortcutsOpen) {
      shortcutsOpen = false;
      return;
    }
    if (e.key === "Escape" && padHelpOpen) {
      padHelpOpen = false;
      return;
    }
    if (e.key === "Escape" && anyPopoverOpen()) {
      closeAllPopovers();
      return;
    }
    if (editOpen) {
      // Delete/cut the selection, or the clip under the playhead. editComp's
      // deleteSelected/cutAtPlayhead exports land alongside this change (see
      // EditStudio.svelte) — until then these calls are no-ops via `?.`.
      if (e.key === "Delete" || e.key === "Backspace") { editComp?.deleteSelected?.(); e.preventDefault(); return; }
      if (k === "c" && !e.ctrlKey && !e.metaKey && !e.altKey) { editComp?.cutAtPlayhead?.(); e.preventDefault(); return; }
      // Same step-scrub keys as Focus mode (`,`/`.`), for consistency.
      if (e.key === "," || e.key === "<") { editComp?.seekBy?.(-5); e.preventDefault(); return; }
      if (e.key === "." || e.key === ">") { editComp?.seekBy?.(5); e.preventDefault(); return; }
      if (e.key === " " || e.code === "Space") { editComp?.togglePlay?.(); e.preventDefault(); return; }
      if (e.key === "[") { editComp?.setIn?.(); e.preventDefault(); return; }
      if (e.key === "]") { editComp?.setOut?.(); e.preventDefault(); return; }
      if (e.shiftKey && e.key === "ArrowRight") { editComp?.seekBy?.(5); e.preventDefault(); return; }
      if (e.shiftKey && e.key === "ArrowLeft") { editComp?.seekBy?.(-5); e.preventDefault(); return; }
      if (k === "f") {
        const entering = !fullscreen;
        if (entering) await editComp?.setOutputPreview?.(true);
        await toggleFullscreen();
        if (!entering) await editComp?.setOutputPreview?.(false);
        e.preventDefault();
        return;
      }
      if (k === "l") { dimLevel = (dimLevel + 1) % 3; e.preventDefault(); return; }
      if (e.key === "Escape") {
        if (fullscreen) toggleFullscreen();
        else if (dimLevel > 0) dimLevel = 0;
        else editOpen = false;
      }
      return;
    }
    if ((e.ctrlKey || e.metaKey) && k === "z") {
      void (e.shiftKey ? redoLast() : undoLast());
      e.preventDefault();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && k === "y") {
      void redoLast();
      e.preventDefault();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && k === "x") {
      cutSelection();
      e.preventDefault();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && k === "v") {
      pasteCutSelection();
      e.preventDefault();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && k === "a") {
      selectAllFiltered();
      e.preventDefault();
      return;
    }
    if (k === "i") {
      showInfoOverlay = !showInfoOverlay;
      e.preventDefault();
      return;
    }
    // While casting a video, these are TV controls in every library view.
    // Handle them before Grid navigation or the local Focus player sees them.
    if (castDevice && active?.kind === "video") {
      if ((e.key === " " || e.code === "Space") && !e.ctrlKey && !e.metaKey && !e.altKey) {
        void toggleCastPlayback();
        e.preventDefault();
        return;
      }
      if (e.shiftKey && e.key === "ArrowRight") {
        void seekCastBy(5);
        e.preventDefault();
        return;
      }
      if (e.shiftKey && e.key === "ArrowLeft") {
        void seekCastBy(-5);
        e.preventDefault();
        return;
      }
    }
    // Video playback keys (Focus mode, active clip): Space toggles play/pause,
    // , / . and Shift+←/→ step the clip ±5s. For a video, Shift+←/→ seeks rather
    // than extending the selection (that stays the default for photos/grid).
    if (viewMode === "loupe" && active?.kind === "video" && loupeComp) {
      // Ctrl+Space before plain Space, or the play toggle would swallow it.
      if ((e.key === " " || e.code === "Space") && (e.ctrlKey || e.metaKey)) {
        loupeComp.toggleGlimpse?.();
        e.preventDefault();
        return;
      }
      if (e.key === " " || e.code === "Space") { loupeComp.togglePlay(); e.preventDefault(); return; }
      if (e.key === "[") { loupeComp.setInPoint?.(); e.preventDefault(); return; }
      if (e.key === "]") { loupeComp.setOutPoint?.(); e.preventDefault(); return; }
      if (e.shiftKey && e.key === "ArrowRight") { loupeComp.seekBy(5); e.preventDefault(); return; }
      if (e.shiftKey && e.key === "ArrowLeft") { loupeComp.seekBy(-5); e.preventDefault(); return; }
      if (e.key === "," || e.key === "<") { loupeComp.seekBy(-5); e.preventDefault(); return; }
      if (e.key === "." || e.key === ">") { loupeComp.seekBy(5); e.preventDefault(); return; }
    }
    const delta = navDelta(e.key);
    if (delta) {
      // Grid/detail/filmstrip buttons keep DOM focus after a mouse click. The
      // first arrow key then turns that OLD button's global :focus-visible ring
      // on while the real active selector moves, which looks like two selected
      // photos. This app owns arrow navigation at the window level, so release
      // only a media-cell focus before moving; ordinary controls retain their
      // keyboard focus contract.
      if (t?.matches(".cell, .scell, .row")) t.blur();
      // Shift+←/→ extends the selection in grid/details and for a photo in Focus.
      // For a video in Focus it seeks instead (handled in the block above).
      move(delta, { extend: e.shiftKey });
      e.preventDefault();
      return;
    }
    if (e.key === "Enter") { setView(viewMode === "loupe" ? "grid" : "loupe"); e.preventDefault(); return; }
    if (e.key === "Escape") {
      if (fullscreen) toggleFullscreen();
      else if (dimLevel > 0) dimLevel = 0;
      else if (viewMode === "loupe") setView("grid");
      else {
        selected = active ? new Set([active.path]) : new Set();
        selectionAnchor = active?.path ?? null;
      }
      return;
    }
    if (k === "f") { cycleFullscreen(); return; }
    if (k === "l") { dimLevel = (dimLevel + 1) % 3; return; }
    if (k === "g") { setView("grid"); return; }
    if (k === "d") { setView("details"); return; }
    if (k === "b") { toggleFilmstrip(); return; }
    if (e.key >= "1" && e.key <= "5") { rate(Number(e.key)); return; }
    if (e.key === "`") { rate(0); return; }
    if (e.key in LABEL_BY_DIGIT) { label(LABEL_BY_DIGIT[e.key]); return; }
    if (k === "x") { flag("reject"); return; }
    if (k === "p") { flag("pick"); return; }
    if (k === "u") { unset(); return; }
  }

  // ── game-controller culling (PS5/PS4 pad; see gamepad.svelte.ts) ─────────
  // One dispatcher turns remappable action ids into the SAME functions the
  // keyboard uses. The mouse's extra buttons route through it too, so both
  // input surfaces share the mapper in the Controller panel.
  function handlePadAction(a: PadActionId | string, _strength = 1) {
    if (editOpen) return; // the pad drives the culling views only
    switch (a) {
      case "prev": move(-1); break;
      case "next": move(1); break;
      case "up": move(navDelta("ArrowUp")); break;
      case "down": move(navDelta("ArrowDown")); break;
      case "pick": flag("pick"); break;
      case "reject": flag("reject"); break;
      case "clearMarks": unset(); break;
      case "rate1": case "rate2": case "rate3": case "rate4": case "rate5":
        rate(Number(a.slice(4)));
        break;
      // label1..label5 follow the LABELS order (blue/purple/red/green/yellow),
      // which is the same order as the keyboard's 6/7/8/9/0.
      case "label1": case "label2": case "label3": case "label4": case "label5": {
        const def = LABELS[Number(a.slice(5)) - 1];
        if (def) label(def.key);
        break;
      }
      case "playPause":
        if (castDevice && active?.kind === "video") void toggleCastPlayback();
        else if (viewMode === "loupe" && active?.kind === "video") loupeComp?.togglePlay();
        break;
      // In/out marking is a Focus-on-a-video act. Say so rather than no-op:
      // from across the room a dead button is indistinguishable from a bug.
      case "markIn":
      case "markOut": {
        if (viewMode !== "loupe" || active?.kind !== "video") {
          showUndoToast("In/out points need a video open in Focus");
          break;
        }
        if (a === "markIn") loupeComp?.setInPoint?.();
        else loupeComp?.setOutPoint?.();
        showUndoToast(a === "markIn" ? "In point set" : "Out point set");
        break;
      }
      case "seekBack":
      case "seekFwd": {
        const dir = a === "seekFwd" ? 1 : -1;
        if (castDevice && active?.kind === "video") {
          void seekCastBy(dir * 5);
        } else if (viewMode === "loupe" && active?.kind === "video") {
          // Predictable skip: the old strength curve fired at the trigger's
          // first 35% threshold (~2.4s), then jumped to 5s while the trigger was
          // still travelling and repeated every 120ms. One pull is now always
          // ±5s; holding deliberately repeats after the grace period defined in
          // gamepad.svelte.ts.
          loupeComp?.seekBy(dir * 5);
        } else {
          move(dir * 10); // photos: triggers leaf through the folder fast
        }
        break;
      }
      case "gridView": setView("grid"); break;
      case "focusView": if (active) setView("loupe"); break;
      // The touchpad click is the pad's "Enter" — same in/out toggle the Enter
      // key performs, because double-click isn't available at TV distance.
      case "toggleView":
        if (viewMode === "loupe") setView("grid");
        else if (active) setView("loupe");
        break;
      case "viewBack": if (viewMode === "loupe") setView("grid"); break;
      case "viewForward": if (viewMode !== "loupe" && active) setView("loupe"); break;
      case "fullscreen": void toggleFullscreen(); break;
      case "toggleFilmstrip": toggleFilmstrip(); break;
      case "info": showInfoOverlay = !showInfoOverlay; break;
      case "help": padHelpOpen = !padHelpOpen; break;
    }
  }

  $effect(() => {
    pad.start((a, strength) => handlePadAction(a, strength));
    return () => pad.stop();
  });

  // Toast when a controller joins/leaves, so pairing feedback is visible from
  // across the room (the whole point of pad culling is TV distance).
  let padWasConnected = false;
  $effect(() => {
    const c = pad.connected;
    if (c && !padWasConnected) {
      showUndoToast(`🎮 ${pad.name.replace(/\s*\(.*\)$/, "") || "Controller"} connected — Create/Share shows the button guide`);
    } else if (!c && padWasConnected) {
      showUndoToast("🎮 Controller disconnected");
      padHelpOpen = false;
    }
    padWasConnected = c;
  });

  /** Rows for the button-guide overlay: only actions that have a binding. */
  let padGuideRows = $derived(
    PAD_ACTIONS.map((a) => ({ ...a, btn: pad.buttonFor(a.id) })).filter((a) => a.btn >= 0),
  );

  // Mouse back/forward buttons, remappable in the Controller panel (defaults
  // keep the original behavior: Back leaves Focus, Forward enters it).
  // preventDefault stops the webview trying to navigate its history and
  // blanking the single-page app.
  function onmouseup(e: MouseEvent) {
    if (editOpen) return;
    if (e.button === 3) {
      handlePadAction(settings.s.mouseBack);
      e.preventDefault();
    } else if (e.button === 4) {
      handlePadAction(settings.s.mouseForward);
      e.preventDefault();
    }
  }

  // A derivative's history reads off its filename suffix (see EditStudio's
  // exportName): show a tiny corner badge in the grid/strip so exports are
  // recognisable at a glance. Instagram/composite/crop/trim in priority order.
  function derivativeBadge(name: string): string | null {
    const stem = name.replace(/\.[^.]+$/, "").toLowerCase();
    if (/(^|_)mix($|_)/.test(stem)) return "MIX";
    if (/(^|_)ig($|_)|(^|_)reel($|_)|(^|_)sq($|_)|(^|_)wide($|_)/.test(stem)) return "IG";
    if (/(^|_)mobile($|_)/.test(stem)) return "MOB";
    if (/(^|_)crop($|_)/.test(stem)) return "CROP";
    if (/(^|_)trim($|_)/.test(stem)) return "TRIM";
    return null;
  }

  // Per-item RAW/JPG corner tag. A raw file is ALWAYS tagged "RAW" (stacked or
  // standalone) so raws are recognisable at a glance; a plain image is tagged
  // "JPG" only when it's the sibling of a raw in a RAW+JPEG stack (otherwise
  // every ordinary photo in the folder would sprout a redundant tag).
  function rawKindTag(item: MediaItem, meta = relatedFor(item)): "RAW" | "JPG" | null {
    if (item.kind === "raw") return "RAW";
    if (item.kind === "image" && meta?.group.badges.includes("RAW+JPEG")) return "JPG";
    return null;
  }
</script>

<svelte:window {onkeydown} {onmouseup} oncontextmenu={onGlobalContextMenu} onpointerdown={onGlobalPointerDown} />

{#snippet gridCell(item: MediaItem, i: number)}
  {@const rel = relatedFor(item)}
  <button
    class="cell"
    class:active={i === activeIndex}
    class:selected={selected.has(item.path)}
    class:reject={item.flag === "reject"}
    class:gone={item.missing}
    class:related={!!rel}
    class:rel-start={!!rel && rel.index === 0}
    class:rel-mid={!!rel && rel.index > 0 && rel.index < rel.count - 1}
    class:rel-end={!!rel && rel.index === rel.count - 1}
    class:rel-collapsed={isCollapsedRepresentative(item, rel)}
    class:rel-mother={rel?.role === "mother"}
    class:rel-derivative={rel?.role === "derivative"}
    onclick={(e) => gridCellClick(e, i)}
    ondblclick={() => { setActiveTo(i); setView("loupe"); }}
    oncontextmenu={(e) => openContextMenu(e, item, i)}
    draggable={!item.missing}
    ondragstart={(e) => beginMediaDrag(e, item, i)}
    ondragend={endMediaDrag}
    title={item.missing
      ? `File not found — ${item.path}\nIts marks are kept. Right-click to relink or forget it.`
      : relatedFor(item)
        ? relatedTitle(item)
        : undefined}
  >
    {#if rel}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <span
        class="stackline"
        class:dbl={relatedCollapsed(rel)}
        role="button"
        tabindex="-1"
        title={relatedCollapsed(rel) ? `Expand stack (${rel.count})` : "Collapse stack"}
        onclick={(e) => toggleStack(e, rel, item.path)}
      ></span>
    {/if}
    <!-- The stack line lives OUTSIDE this clipping wrapper (as a sibling above)
         so its bar can bleed past the tile edge into the grid gap — see
         .stackline CSS. Everything that still needs rounded-corner clipping
         (the thumbnail image, reject dim, badges) moves in here instead. -->
    <div class="cellclip">
      <Thumb {item} size={gridThumbTier} armed={i === activeIndex} />
      <span class="ov">
        {#if rel}
          <span class="rel-badges">
            {#each rel.group.badges.slice(0, 2) as b}
              <span>{b}</span>
            {/each}
          </span>
          <span class="rel-role">{relatedRoleLabel(rel)}</span>
          {#if rel.group.orphaned && rel.index === 0}
            <span class="rel-orphan" title="Original source is no longer present — these are its edits/exports">no orig</span>
          {/if}
          {#if isCollapsedRepresentative(item, rel)}
            <span class="rel-count">{rel.count}</span>
          {/if}
        {/if}
        {#if item.label}<span class="lbl-dot" style="background:var({LABEL_VAR[item.label]})"></span>{/if}
        {#if item.flag === "reject"}<span class="fl x">✕</span>{/if}
        {#if item.flag === "pick"}<span class="fl pick">✓</span>{/if}
        {#if item.rating > 0}<span class="stars">{"★".repeat(item.rating)}</span>{/if}
        {#if item.tags.length}<span class="tagdot" title={item.tags.join(", ")}>🏷</span>{/if}
        {#if item.events.length}<span class="evtdot" title={`Event: ${item.events.join(", ")}`}>✦</span>{/if}
        {#if item.missing}<span class="gonemark" title="File not found — right-click to relink">?</span>{/if}
        {#if derivativeBadge(item.name)}<span class="deriv-badge" title="Exported by FoxCull ({derivativeBadge(item.name)})">{derivativeBadge(item.name)}</span>{/if}
        {#if rawKindTag(item, rel)}<span class="kind-tag" class:raw={item.kind === "raw"} title={item.kind === "raw" ? "RAW file" : "JPEG sibling of a RAW"}>{rawKindTag(item, rel)}</span>{/if}
      </span>
    </div>
  </button>
{/snippet}

{#snippet stripCellSnip(item: MediaItem, i: number)}
  {@const rel = relatedFor(item)}
  <button
    class="scell"
    class:active={i === activeIndex}
    class:selected={selected.has(item.path)}
    class:reject={item.flag === "reject"}
    class:gone={item.missing}
    class:related={!!rel}
    class:rel-start={!!rel && rel.index === 0}
    class:rel-mid={!!rel && rel.index > 0 && rel.index < rel.count - 1}
    class:rel-end={!!rel && rel.index === rel.count - 1}
    class:rel-collapsed={isCollapsedRepresentative(item, rel)}
    onclick={(e) => gridCellClick(e, i)}
    ondblclick={() => { setActiveTo(i); setView("loupe"); }}
    oncontextmenu={(e) => openContextMenu(e, item, i)}
    title={rel ? relatedTitle(item) : undefined}
  >
    {#if rel}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <span
        class="stackline"
        class:dbl={relatedCollapsed(rel)}
        role="button"
        tabindex="-1"
        title={relatedCollapsed(rel) ? `Expand stack (${rel.count})` : "Collapse stack"}
        onclick={(e) => toggleStack(e, rel, item.path)}
      ></span>
    {/if}
    <!-- Same reasoning as gridCell: keep the stack line out of the clipped
         wrapper so it can bleed into the strip gap between same-stack tiles. -->
    <div class="cellclip">
      <Thumb {item} size={stripThumbTier} armed={i === activeIndex} />
      {#if rel}
        <span class="s-rel">{shortRelatedBadge(rel)}</span>
        <span class="s-role">{relatedRoleLabel(rel).slice(0, 1)}</span>
        {#if isCollapsedRepresentative(item, rel)}<span class="s-count">{rel.count}</span>{/if}
      {/if}
      {#if item.label}<span class="s-lbl" style="background:var({LABEL_VAR[item.label]})"></span>{/if}
      {#if item.rating > 0}<span class="s-stars">{"★".repeat(item.rating)}</span>{/if}
      {#if item.flag === "reject"}<span class="s-x">✕</span>{/if}
      {#if item.flag === "pick"}<span class="s-pick">✓</span>{/if}
      {#if item.missing}<span class="gonemark sm" title="File not found — right-click to relink">?</span>{/if}
      {#if derivativeBadge(item.name)}<span class="s-deriv">{derivativeBadge(item.name)}</span>{/if}
      {#if rawKindTag(item, rel)}<span class="s-kind" class:raw={item.kind === "raw"}>{rawKindTag(item, rel)}</span>{/if}
    </div>
  </button>
{/snippet}

<div class="app" data-dim={dimLevel} class:fs={fullscreen} class:treeCollapsed>
  <!-- ░ left: drives + folder tree ░ -->
  {#if !treeCollapsed}
    <aside class="tree" style="width:{settings.s.treeWidth}px">
      <div class="tree-head">
        <button class="ico sm" onclick={() => (treeCollapsed = true)} title="Hide folders" aria-label="Hide folders">
          <!-- Standard "sidebar panel" glyph: rounded frame + left-panel divider. -->
          <svg class="panelGlyph" viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"/><line x1="9.4" y1="4.6" x2="9.4" y2="19.4"/></svg>
        </button>
        <span class="sidebarIdentity">
          <img src="/favicon.png" alt="" width="26" height="26" />
          <span class="brandLockup"><strong>FoxCull</strong><small>{currentDir ? basename(currentDir) : "Library"}</small></span>
        </span>
        <div class="tree-actions">
          <button
            class="ico sm"
            disabled={!currentDir}
            onclick={() => currentDir && newSubfolderPrompt(currentDir)}
            title={currentDir ? `New subfolder inside ${basename(currentDir)}` : "Open a folder first"}
            aria-label="New subfolder"
          >
            <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7.5h6l2 2h10v9.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M12 13v5M9.5 15.5h5"/></svg>
          </button>
          <button
            class="ico sm"
            class:spin={recounting}
            onclick={refreshCounts}
            title="Refresh folders and current view"
            aria-label="Refresh folders and current view"
          >
            <svg class="refreshIcon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M20 7v5h-5" />
              <path d="M19 12a7 7 0 1 0-1.8 4.7" />
            </svg>
          </button>
          <button class="btn sm openFolder" onclick={openFolderPicker} title="Jump to a folder">
            <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7.5h6l2 2h10v9.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M3 8V5a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v2.5"/></svg>
            Open
          </button>
        </div>
      </div>
      <div class="tree-body">
        {#if drives.length}
          {#each drives as d (d.path)}
            <TreeNode node={d} {currentDir} onselect={openFolder} onmove={(dest) => movePathsTo(draggingPaths, dest)} onfoldercontext={openFolderContextMenu} {countsGen} />
          {/each}
        {:else}
          <p class="hint">No drives detected.</p>
        {/if}
      </div>
      <!-- Background activity sits at the BOTTOM of the sidebar, where a status
           bar belongs — it spent long enough tucked under the header where it
           read as part of the folder chrome. It renders nothing when idle. -->
      <ActivityBar />
    </aside>
    <div class="vsplit treeSplit" role="separator" tabindex="-1" onpointerdown={startTreeResize}></div>
  {:else}
    <button class="treeRestore ico sm" onclick={() => (treeCollapsed = false)} title="Show folders" aria-label="Show folders">
      <svg class="panelGlyph" viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"/><line x1="9.4" y1="4.6" x2="9.4" y2="19.4"/><path d="M13.5 9.5 16 12l-2.5 2.5"/></svg>
    </button>
    <!-- Sidebar hidden: the same chip floats bottom-left instead of disappearing
         with it. "Why is the disk busy" must be answerable in every layout. -->
    <div class="actFloat"><ActivityBar /></div>
  {/if}

  <!-- ░ center ░ -->
  <main class="center">
    {#if !writable && currentDir}
      <div class="banner">Read-only location — rating works; the delete sweep is disabled here.</div>
    {/if}

    <!-- top bar -->
    <div class="bar">
      {#if !editOpen}
      <!-- view mode -->
      <div class="tool-group viewGroup">
        <span class="ctl-label">View</span>
        <div class="seg modes" title="View">
          <button class="chip viewChip" class:on={viewMode === "grid" && !editOpen} onclick={() => setView("grid")} title="Grid (G)">
            <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg><span>Grid</span>
          </button>
          <button class="chip viewChip" class:on={viewMode === "details" && !editOpen} onclick={() => setView("details")} title="Details list (D)">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13"/><circle cx="3.5" cy="6" r="1"/><circle cx="3.5" cy="12" r="1"/><circle cx="3.5" cy="18" r="1"/></svg><span>Details</span>
          </button>
          <button class="chip viewChip" class:on={viewMode === "loupe" && !editOpen} onclick={() => setView("loupe")} title="Focus — one item large (Enter)">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 3H4a1 1 0 0 0-1 1v4M16 3h4a1 1 0 0 1 1 1v4M8 21H4a1 1 0 0 1-1-1v-4M16 21h4a1 1 0 0 0 1-1v-4"/><circle cx="12" cy="12" r="4"/></svg><span>Focus</span>
          </button>
        </div>
      </div>

      <span class="div"></span>

      <!-- sort + grouping + stack display -->
      <div class="grp arrange">
        <button
          class="chip arrangeBtn"
          class:on={arrangeOpen || settings.s.groupBy !== "none" || settings.s.subgroupBy !== "none"}
          onclick={() => (arrangeOpen = !arrangeOpen)}
          title="Sort, group and subgroup"
        >
          <svg class="toolbarIcon" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h10M4 17h16M4 12h16"/><circle cx="17" cy="7" r="2"/><circle cx="8" cy="17" r="2"/><circle cx="10" cy="12" r="2"/></svg><span class="actionText">Arrange</span>
        </button>
        {#if arrangeOpen}
          <div class="arrangeMenu">
            <div class="fm-row">
              <span class="fm-lbl"><span class="fm-ico">⇅</span>Sort</span>
              <select class="sel wide" title="Sort order" bind:value={settings.s.sortBy} onchange={() => { settings.set({ sortBy: settings.s.sortBy }); maybeFetchCaptures(); }}>
                <option value="name">Name</option>
                <option value="date">Modified</option>
                <option value="capture">Capture date</option>
                <option value="type">Type</option>
                <option value="size">Size</option>
              </select>
              <button
                class="ico dirbtn"
                title={settings.s.sortDir === "asc" ? "Ascending — click for descending" : "Descending — click for ascending"}
                onclick={() => settings.set({ sortDir: settings.s.sortDir === "asc" ? "desc" : "asc" })}
              >
                {settings.s.sortDir === "asc" ? "↑" : "↓"}
              </button>
            </div>
            <div class="fm-row">
              <span class="fm-lbl"><span class="fm-ico">▦</span>Group</span>
              <select class="sel wide" title="Primary grouped section" bind:value={settings.s.groupBy} onchange={() => { settings.set({ groupBy: settings.s.groupBy }); maybeFetchCaptures(); }}>
                <option value="none">No groups</option>
                <option value="event">Event</option>
                <option value="folder">Folder</option>
                <option value="type">Type</option>
                <option value="year">Year</option>
                <option value="month">Month</option>
                <option value="week">Week</option>
              </select>
            </div>
            <div class="fm-row">
              <span class="fm-lbl"><span class="fm-ico sub">▤</span>Subgroup</span>
              <select class="sel wide" title="Nested second grouping level" bind:value={settings.s.subgroupBy} onchange={() => { settings.set({ subgroupBy: settings.s.subgroupBy }); maybeFetchCaptures(); }}>
                <option value="none">None</option>
                <option value="event">Event</option>
                <option value="folder">Folder</option>
                <option value="type">Type</option>
                <option value="year">Year</option>
                <option value="month">Month</option>
                <option value="week">Week</option>
              </select>
            </div>
            <!-- Event-block controls. Only meaningful while an event grouping is
                 active, so they are disabled (not hidden) everywhere else — the
                 option stays discoverable without pretending to do something. -->
            <div class="fm-row evtRow" class:off={!groupingByEvent}>
              <span class="fm-lbl"><span class="fm-ico">✦</span>Events</span>
              <label class="chk" title={groupingByEvent ? "Order event blocks by their earliest photo instead of alphabetically. The ↑↓ button above flips ascending/descending either way." : "Set Group (or Subgroup) to Event to use this"}>
                <input
                  type="checkbox"
                  disabled={!groupingByEvent}
                  checked={settings.s.eventOrder === "date"}
                  onchange={(e) => settings.set({ eventOrder: (e.currentTarget as HTMLInputElement).checked ? "date" : "name" })}
                />
                <span>Order by date</span>
              </label>
              <label class="chk" title="Front each event block with a cover photo instead of a plain header">
                <input
                  type="checkbox"
                  disabled={!groupingByEvent}
                  checked={settings.s.eventCovers}
                  onchange={(e) => settings.set({ eventCovers: (e.currentTarget as HTMLInputElement).checked })}
                />
                <span>Cover art</span>
              </label>
            </div>
            {#if groupingByEvent && allEvents.length}
              <div class="evtList">
                {#each allEvents as ev (ev.id)}
                  <div class="evtManageRow">
                    <button class="evtName" class:on={eventFilter === ev.name} title="Show only this event" onclick={() => (eventFilter = eventFilter === ev.name ? null : ev.name)}>
                      <span>{ev.name}</span><span class="cnt">{ev.count}</span>
                    </button>
                    <button class="ico xs" title="Rename event" aria-label="Rename event" onclick={() => renameEventPrompt(ev)}>✎</button>
                    <button class="ico xs" title="Delete event (photos are untouched)" aria-label="Delete event" onclick={() => deleteEventPrompt(ev)}>×</button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <span class="div"></span>

      <!-- media, culling and metadata filters -->
      <div class="grp filterwrap">
        <button class="chip" class:on={filtersOpen || activeFilterCount > 0} onclick={() => (filtersOpen = !filtersOpen)} title="Media, culling and metadata filters">
          <svg class="toolbarIcon" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5h16l-6.4 7.2V19l-3.2 1.5v-8.3z"/></svg><span class="actionText">Filters</span>{#if activeFilterCount}<span class="filterCount">{activeFilterCount}</span>{/if}
        </button>
        <!-- N of M passing filters, pre-stack-folding — always visible while any
             filter is active (baseView = filtered; items = whole folder view). -->
        {#if activeFilterCount > 0}
          <span class="shown-count" title="Items passing the active filters, out of the whole folder">{baseView.length} of {items.length}</span>
        {/if}
        {#if filtersOpen}
          <div class="filtermenu">
            <div class="fm-row">
              <span class="fm-lbl">Type</span>
              <div class="seg">
                {#each [["all", "All"], ["image", "Photos"], ["video", "Video"], ["raw", "RAW"]] as [val, lbl]}
                  <button class="chip" class:on={settings.s.typeFilter === val} onclick={() => settings.set({ typeFilter: val as typeof settings.s.typeFilter })}>{lbl}</button>
                {/each}
              </div>
            </div>
            <div class="fm-row">
              <span class="fm-lbl">Status</span>
              <div class="seg flags">
                <button class="chip" class:on={flagFilter === "all"} onclick={() => (flagFilter = "all")}>All</button>
                <button class="chip pick" class:on={flagFilter === "pick"} onclick={() => (flagFilter = "pick")}>Picks</button>
                <button class="chip rej" class:on={flagFilter === "reject"} onclick={() => (flagFilter = "reject")}>Rejected</button>
                <button class="chip" class:on={flagFilter === "unflagged"} onclick={() => (flagFilter = "unflagged")}>Unflagged</button>
              </div>
            </div>
            <div class="fm-row">
              <span class="fm-lbl">Rating</span>
              <div class="seg">
                <!-- Operator: ≥ (default) / ≤ / = so a set of ratings is targetable. -->
                <div class="opseg" title="Rating comparison: at least / at most / exactly">
                  {#each [[">=", "≥"], ["<=", "≤"], ["=", "="]] as [op, glyph]}
                    <button class="opbtn" class:on={ratingOp === op} onclick={() => (ratingOp = op as RatingOp)}>{glyph}</button>
                  {/each}
                </div>
                {#each [1, 2, 3, 4, 5] as n}
                  <button class="starf" class:on={minRating >= n} onclick={() => (minRating = minRating === n ? 0 : n)} title="{ratingOp} {n} stars">★</button>
                {/each}
                {#if minRating > 0}<button class="fm-clr" onclick={() => (minRating = 0)}>clear</button>{/if}
              </div>
            </div>
            <div class="fm-row">
              <span class="fm-lbl">Label</span>
              <div class="seg">
                <!-- Any = clear all label criteria. -->
                <button class="lblchip" class:on={!labelFilterActive} onclick={clearLabelFilter} title="Any label (clear)">Any</button>
                {#each LABELS as l}
                  <button class="dot" class:on={labelFilters.has(l.key)} style="background:var({l.varName})" title={l.name} aria-label={l.name} onclick={() => toggleLabelFilter(l.key)}></button>
                {/each}
                <!-- None = match unlabeled items; a clean outlined dot with a slash. -->
                <button class="dot none" class:on={labelNone} onclick={() => (labelNone = !labelNone)} title="No label" aria-label="No label">
                  <svg viewBox="0 0 14 14" aria-hidden="true"><circle cx="7" cy="7" r="5.5" fill="none" /><line x1="3.2" y1="10.8" x2="10.8" y2="3.2" /></svg>
                </button>
              </div>
            </div>
            <div class="fm-row col">
              <span class="fm-lbl">Tag</span>
              <div class="fm-tags">
                <button class="tagrow" class:on={tagFilter === null} onclick={() => (tagFilter = null)}>Any tag</button>
                {#if allTags.length}
                  {#each allTags as [t, n]}
                    <button class="tagrow" class:on={tagFilter === t} onclick={() => (tagFilter = t)}>
                      <span>{t}</span><span class="cnt">{n}</span>
                    </button>
                  {/each}
                {:else}
                  <p class="tagempty">No tags yet.</p>
                {/if}
              </div>
            </div>
            <!-- Events are a peer of tags here: one click isolates a trip across
                 every subfolder in view. Grouping (Arrange ▸ Event) is the other
                 read of the same metadata — blocks instead of a filter. -->
            <div class="fm-row col">
              <span class="fm-lbl">Event</span>
              <div class="fm-tags">
                <button class="tagrow" class:on={eventFilter === null} onclick={() => (eventFilter = null)}>Any event</button>
                {#if allEvents.length}
                  {#each allEvents as ev (ev.id)}
                    <button class="tagrow" class:on={eventFilter === ev.name} onclick={() => (eventFilter = eventFilter === ev.name ? null : ev.name)}>
                      <span>{ev.name}</span><span class="cnt">{ev.count}</span>
                    </button>
                  {/each}
                {:else}
                  <p class="tagempty">No events yet — select photos and use “New event…” from the right-click menu.</p>
                {/if}
              </div>
            </div>
            <div class="fm-row">
              <span class="fm-lbl">Scope</span>
              <button class="chip" class:on={settings.s.includeSub} onclick={toggleSub} title="Include photos from subfolders">⊞ Include subfolders</button>
            </div>
            {#if missingRels.length || missingInView}
              {@const n = Math.max(missingRels.length, missingInView)}
              <div class="fm-row">
                <span class="fm-lbl">Catalog</span>
                <span class="missingNote" title="Catalog entries whose file was not found on disk. Their marks are intact — right-click a “?” tile to relink it.">
                  ⚠ {n} missing file{n === 1 ? "" : "s"}
                </span>
                <button class="chip" disabled={scanning} onclick={() => runCatalogScan({ announce: true })}>Re-check</button>
              </div>
            {/if}
          </div>
        {/if}
      </div>

      {#if viewMode === "grid" && !editOpen}
        <span class="div"></span>
        <div class="grp zoom" title="Thumbnail size">
          <span class="mini">▦</span>
          <input type="range" min="110" max="360" bind:value={settings.s.gridSize} onchange={() => settings.set({ gridSize: settings.s.gridSize })} />
        </div>
      {/if}
      {:else}
        <div class="tool-group editModeTitle">
          <span class="ctl-label">Mode</span>
          <strong>Edit</strong>
          <span>{items.filter((item) => item.kind === "video").length} videos in folder</span>
        </div>
      {/if}

      <div class="spacer"></div>

      <div class="rightTools">
        {#if !editOpen}
        <!-- actions (top-right) -->
        <div class="grp prepWrap">
          <button
            class="btn sm prep"
            class:on={preparing || prepared}
            onclick={() => prepareFolder("all")}
            disabled={!baseView.length || preparing}
            title={"Prepare · make this whole folder instant.\n\nPhotos & RAW: caches every shot's full-size Focus preview (no loading blur).\nVideos: caches the poster frame AND the hover scrub strip, so skimming works immediately.\n\nPhotos run first, then videos; progress and a time estimate show here and in the activity chip. Use the ▾ to prepare only the selection, only videos, or only photos. Safe to keep working meanwhile."}
          >
            {#if preparing}<span class="prep-fill" style="width:{prepPct}%"></span>{/if}
            <span class="prep-lbl">
              <span class="prep-ico" aria-hidden="true">
                {#if preparing}◌{:else if prepared}✓{:else}<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true"><path d="M13 2 4.5 13.2c-.4.5 0 1.3.7 1.3H11l-1.4 8.2c-.1.7.8 1.1 1.2.5L19.5 12c.4-.5 0-1.3-.7-1.3H12.9L14.2 2.6c.1-.7-.8-1.1-1.2-.6Z"/></svg>{/if}
              </span>
              <span class="actionText">{#if preparing}{prepPct}%{prepEta ? ` ${prepEta}` : ""}{:else if prepared}Ready{:else}Prepare{/if}</span>
            </span>
          </button>
          <button
            class="btn sm prepCaret"
            class:on={prepMenuOpen}
            onclick={() => (prepMenuOpen = !prepMenuOpen)}
            disabled={!baseView.length || preparing}
            aria-label="Choose what to prepare"
            title="Choose what to prepare"
          >▾</button>
          {#if prepMenuOpen}
            <div class="clearMenu prepMenu">
              {#each PREP_SCOPES as s}
                {@const n = prepScopeItems(s.key).length}
                <button disabled={n === 0} onclick={() => { prepMenuOpen = false; void prepareFolder(s.key); }}>
                  {s.label}<span class="prepCount">{n}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <button class="btn sm danger" onclick={rejectSelected} disabled={actionTargets.length === 0} title="Toggle rejected on the active item or selection (X)">
          <svg class="btn-ico" viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><line x1="5" y1="5" x2="19" y2="19"/><line x1="19" y1="5" x2="5" y2="19"/></svg>
          <span class="actionText">{allTargetsRejected ? "Unreject" : "Reject"}{selected.size > 1 ? ` ${selected.size}` : ""}</span>
        </button>
        <div class="grp clearWrap">
          <button class="btn sm" class:on={clearOpen} onclick={() => (clearOpen = !clearOpen)} disabled={actionTargets.length === 0} title="Clear ratings, labels, flags or tags from the active item or selection">
            <svg class="btn-ico" viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 8 14 2 3 13l6 6h4l7-7z"/><line x1="9" y1="19" x2="21" y2="19"/></svg>
            <span class="actionText">Clear</span>
          </button>
          {#if clearOpen}
            <div class="clearMenu">
              <button onclick={() => { unset(); clearOpen = false; }}>Marks only</button>
              <button onclick={() => { clearRatings(); clearOpen = false; }}>Stars</button>
              <button onclick={() => { clearLabels(); clearOpen = false; }}>Color</button>
              <button onclick={() => { clearFlags(); clearOpen = false; }}>Pick/Reject</button>
              <button onclick={() => { void clearTagsOnTargets(); clearOpen = false; }}>Tags</button>
              <button class="dangerText" onclick={() => { void clearAllMarks(); clearOpen = false; }}>All marks and tags</button>
            </div>
          {/if}
        </div>
        <button
          class="btn sm danger hold"
          disabled={!writable || rejectedCount === 0}
          onpointerdown={startHold}
          onpointerup={endHold}
          onpointerleave={endHold}
          onpointercancel={endHold}
          title="Hold to delete all {rejectedCount} rejected"
        >
          <span class="hold-fill" style="width:{(holdMs / HOLD_MS) * 100}%"></span>
          <span class="hold-lbl">
            <svg class="btn-ico" viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 7h16"/><path d="M9 7V4h6v3"/><path d="M6 7l1 13h10l1-13"/><line x1="10" y1="11" x2="10" y2="16"/><line x1="14" y1="11" x2="14" y2="16"/></svg>
            <span class="actionText">Delete{rejectedCount ? ` ${rejectedCount}` : ""}</span>
          </span>
        </button>
        {/if}
        <!-- Cast to TV: discovery popover; the chip doubles as the connected
             indicator (name shown while casting). -->
        <div class="grp castWrap">
          {#if castDevice}
            <button
              class="castBadge"
              class:playing={castStatus.playerState === "PLAYING" || castStatus.playerState === "BUFFERING"}
              onclick={toggleCastMenu}
              title={`Casting to ${castDevice.name} — click to manage`}
            >
              <span class="castPulse" aria-hidden="true"></span>
              <strong>CASTING</strong>
              <span>{castStateLabel}</span>
            </button>
          {/if}
          <button
            class="ico castBtn"
            class:on={castOpen || !!castDevice}
            onclick={toggleCastMenu}
            title={castDevice ? `${castStatus.connected ? "Casting" : "Connecting"} to ${castDevice.name} — follows as you browse; click to manage` : "Cast to a TV (Chromecast) — the TV then follows whatever photo/video you're on"}
            aria-label="Cast to TV"
          >
            <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 20h.01"/><path d="M2 16.5a3.5 3.5 0 0 1 3.5 3.5"/><path d="M2 13a7 7 0 0 1 7 7"/><path d="M2 9.5V6a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-8.5"/></svg>
          </button>
          {#if castOpen}
            <div class="castMenu">
              {#if castDevice}
                <div class="castNow">
                  {castStatus.connected ? "Casting" : "Connecting"} to
                  <strong>{castDevice.name}</strong> — the TV follows as you browse
                </div>
                <button class="castRow stop" onclick={() => void stopCast()}>Stop casting</button>
                <div class="menuSep"></div>
              {/if}
              {#if castDiscovering}
                <div class="castHint">Searching for Cast devices…</div>
              {:else if castDevices.length}
                {#each castDevices as d (d.id)}
                  <button class="castRow" disabled={!active} onclick={() => void startCast(d)} title={active ? `Cast ${active.name} to ${d.name}` : "Select a photo/video first"}>
                    <strong>{d.name}</strong><span>{d.addr}</span>
                  </button>
                {/each}
              {:else}
                <div class="castHint">No Cast devices found. TV on? Same Wi-Fi? <button class="linklike" onclick={() => void discoverCast()}>Search again</button></div>
              {/if}
              {#if castDevices.length && !castDiscovering}
                <button class="castRow sub" onclick={() => void discoverCast()}>Search again</button>
              {/if}
            </div>
          {/if}
        </div>
        <div class="modeToggle" title="Workspace mode">
          <button class:on={!editOpen} onclick={() => (editOpen = false)}>Library</button>
          <button class:on={editOpen} onclick={openEditMode} disabled={!currentDir}>Edit</button>
        </div>
        <button class="ico gear" class:on={settingsOpen} onclick={() => (settingsOpen = !settingsOpen)} title="Settings" aria-label="Settings">
          <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="3.2"/><path d="M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1.03 1.56V21a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1.03-1.56 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.56-1.03H3a2 2 0 1 1 0-4h.09a1.7 1.7 0 0 0 1.56-1.03 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34h.01a1.7 1.7 0 0 0 1.02-1.56V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1.03 1.56 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87v.01a1.7 1.7 0 0 0 1.56 1.02H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.56 1.03z"/></svg>
        </button>
      </div>
    </div>

    <!-- settings popover -->
    {#if settingsOpen}
      <div class="pop">
        <!-- Grouped into three plain sections (the user's ask: settings live in
             ONE place, logically bunched, no scattered duplicates). Stacks and
             Live Scrub have no other home — this popover is it. -->
        <div class="grpHead">Appearance</div>
        <div class="row appearanceRow"><span>Theme</span>
          <div class="seg themeSeg">
            <button class="chip" class:on={settings.s.theme === "neutral"} onclick={() => settings.set({ theme: "neutral" })} title="Neutral professional editing chrome"><i class="themeSwatch studio"></i>Studio</button>
            <button class="chip" class:on={settings.s.theme === "dark"} onclick={() => settings.set({ theme: "dark" })}><i class="themeSwatch midnight"></i>Midnight</button>
            <button class="chip" class:on={settings.s.theme === "warm"} onclick={() => settings.set({ theme: "warm" })} title="Low-blue-light late-night chrome"><i class="themeSwatch amber"></i>Amber</button>
            <button class="chip" class:on={settings.s.theme === "light"} onclick={() => settings.set({ theme: "light" })}><i class="themeSwatch daylight"></i>Daylight</button>
          </div>
        </div>
        <div class="row"><span>Interface size</span>
          <div class="seg">
            <button class="chip" class:on={settings.s.uiScale === "compact"} onclick={() => settings.set({ uiScale: "compact" })} title="More media on a small laptop display">Compact</button>
            <button class="chip" class:on={settings.s.uiScale === "comfortable"} onclick={() => settings.set({ uiScale: "comfortable" })}>Standard</button>
            <button class="chip" class:on={settings.s.uiScale === "distance"} onclick={() => settings.set({ uiScale: "distance" })} title="Large controls and type for a TV or distant monitor">TV / large</button>
          </div>
        </div>
        <div class="row"><span>Filmstrip</span>
          <div class="seg">
            {#each [["bottom", "Bottom"], ["left", "Left"], ["right", "Right"], ["hidden", "Off"]] as [v, l]}
              <button
                class="chip"
                class:on={settings.s.filmstripPos === v}
                onclick={() => {
                  if (v !== "hidden") lastDock = v as "bottom" | "left" | "right";
                  // Record the intent for THIS view too, or the next view
                  // change would undo the choice made here.
                  settings.set({
                    filmstripPos: v as typeof settings.s.filmstripPos,
                    stripShow: { ...settings.s.stripShow, [viewMode]: v !== "hidden" },
                  });
                }}
              >{l}</button>
            {/each}
          </div>
        </div>
        <div class="grpHead">Browsing</div>
        <div class="row"><span>Stacks</span>
          <div class="seg">
            <button class="chip" class:on={settings.s.relatedMode === "expanded"} onclick={() => setRelatedMode("expanded")}>Open</button>
            <button class="chip" class:on={settings.s.relatedMode === "collapsed"} onclick={collapseAllRelated}>Fold{relatedHiddenCount ? ` ${relatedHiddenCount}` : ""}</button>
          </div>
        </div>
        <div class="row"><span>Focus scrub</span>
          <div class="seg" title="Focus view decodes the real frame under your cursor as you drag — full resolution, nothing to prepare, works the moment a clip opens. Clips whose codec can't be decoded this way fall back automatically. Turn off only to diagnose.">
            <button class="chip" class:on={settings.s.liveDecodeScrub} onclick={() => settings.set({ liveDecodeScrub: true })}>Live decode</button>
            <button class="chip" class:on={!settings.s.liveDecodeScrub} onclick={() => settings.set({ liveDecodeScrub: false })}>Sprites</button>
          </div>
        </div>
        <div class="row"><span>Glimpse speed</span>
          <div class="slider" title="How fast Glimpse (Ctrl+Space) plays, as a plain multiple of real time — the same idea as a player's 2x or 5x. The rate never changes with clip length: at 5x, 20 seconds takes 4 and 10 minutes takes 2.">
            <input
              type="range"
              min={GLIMPSE_MIN}
              max={GLIMPSE_MAX}
              step="1"
              value={settings.s.glimpseSpeed}
              oninput={(e) => settings.set({ glimpseSpeed: +e.currentTarget.value })}
            />
            <span class="sliderVal">{settings.s.glimpseSpeed}×</span>
          </div>
        </div>
        <div class="row"><span>Sprite fallback (pre-built)</span>
          <div class="seg" title="Skimming works WITHOUT this: select a clip in the grid, hover it, and FoxCull decodes real frames live — nothing is pre-built. Turn this on only to also build sprite sheets for clips whose codec the decoder can't take. It costs minutes of ffmpeg and disk per folder.">
            <button class="chip" class:on={settings.s.liveScrub} onclick={() => settings.set({ liveScrub: true })}>On</button>
            <button class="chip" class:on={!settings.s.liveScrub} onclick={() => settings.set({ liveScrub: false })}>Off</button>
          </div>
        </div>
        <div class="row"><span>Video autoplay</span>
          <div class="seg">
            <button class="chip" class:on={settings.s.videoAutoplay} onclick={() => settings.set({ videoAutoplay: true })}>On</button>
            <button class="chip" class:on={!settings.s.videoAutoplay} onclick={() => settings.set({ videoAutoplay: false })}>Off</button>
          </div>
        </div>
        <div class="row"><span>Minimal video bar</span>
          <div class="seg" title="Collapse the transport to a thin hover-to-expand line so the picture stays edge-to-edge. Off pins a classic always-visible bar.">
            <button class="chip" class:on={settings.s.minimalVideoBar} onclick={() => settings.set({ minimalVideoBar: true })}>On</button>
            <button class="chip" class:on={!settings.s.minimalVideoBar} onclick={() => settings.set({ minimalVideoBar: false })}>Off</button>
          </div>
        </div>
        <div class="row"><span>Controller</span>
          <button class="btn sm" onclick={() => { settingsOpen = false; controllerOpen = true; }} title="Pair a PS5/PS4 controller and map its buttons (mouse extras too)">
            🎮 {pad.connected ? "Connected — set up…" : "Set up…"}
          </button>
        </div>
        <div class="row"><span>Shortcuts</span>
          <button class="btn sm" onclick={() => { settingsOpen = false; shortcutsOpen = true; }} title="Every keyboard shortcut, grouped (?)">⌨ Show all… </button>
        </div>
        <div class="grpHead">Files</div>
        <div class="row"><span>On delete</span>
          <div class="seg">
            <button class="chip" class:on={settings.s.deleteMode === "folder"} onclick={() => settings.set({ deleteMode: "folder" })} title="Move to this drive's _FoxCull recycle folder - recoverable in the in-app Trash">In-app Trash</button>
            <button class="chip" class:on={settings.s.deleteMode === "recycle"} onclick={() => settings.set({ deleteMode: "recycle" })} title="Send to the operating system's Recycle Bin / Trash">System Recycle Bin</button>
          </div>
        </div>
        <div class="row"><span>Trash</span>
          <button class="btn sm" onclick={() => { settingsOpen = false; openTrash(); }}>🗑 Open Trash…</button>
        </div>
        <div class="row"><span>Check catalog on launch</span>
          <div class="seg" title="Verify every rated/tagged file is still where the catalog expects it, and auto-reconnect anything that moved or was renamed outside FoxCull. Runs after the folder is on screen; costs nothing unless something is actually missing.">
            <button class="chip" class:on={settings.s.scanOnLaunch} onclick={() => settings.set({ scanOnLaunch: true })}>On</button>
            <button class="chip" class:on={!settings.s.scanOnLaunch} onclick={() => settings.set({ scanOnLaunch: false })}>Off</button>
          </div>
        </div>
        <div class="row"><span>Catalog</span>
          <button class="btn sm" disabled={scanning} onclick={() => { settingsOpen = false; runCatalogScan({ announce: true }); }} title="Run the integrity check now">
            {scanning ? "Checking…" : "🔎 Check now"}
          </button>
        </div>
        <div class="row"><span>Library</span>
          {#if libInfo}
            <button class="btn sm" onclick={() => libInfo && api.reveal(libInfo.catalog)} title="Show the library folder in your file manager">Reveal</button>
          {/if}
        </div>
        {#if libInfo}
          <div class="row sub">
            <span class="path" title={libInfo.dir}>{libInfo.dir}</span>
            <span class="tag">{libInfo.on_drive ? "on drive" : "app-data (read-only mount)"}</span>
          </div>
        {/if}
        <div class="row hintrow">Each drive keeps its own catalog, preview cache &amp; recycle in a <code>_FoxCull</code> folder. Press <kbd>?</kbd> for all shortcuts · <kbd>F</kbd> play mode · <kbd>L</kbd> dim.</div>
      </div>
    {/if}

    {#if trashOpen}
      <TrashPanel
        items={trashItems}
        onclose={() => (trashOpen = false)}
        onrestore={restoreFromTrash}
        onpurge={purgeFromTrash}
      />
    {/if}

    {#if controllerOpen}
      <ControllerPanel onclose={() => (controllerOpen = false)} />
    {/if}

    <!-- Keyboard shortcut guide (?): the one place every key lives, grouped the
         way the app thinks — nothing to memorize up front. -->
    {#if shortcutsOpen}
      <button class="kbBackdrop" aria-label="Close shortcuts" onclick={() => (shortcutsOpen = false)}></button>
      <div class="kbGuide" role="dialog" aria-label="Keyboard shortcuts">
        <div class="kbHead"><span>⌨ Keyboard shortcuts</span><button class="kbClose" onclick={() => (shortcutsOpen = false)} title="Close (Esc)">✕</button></div>
        <div class="kbCols">
          <div>
            <div class="kbGroup">Navigate</div>
            <div class="kbRow"><span class="keys"><kbd>←</kbd><kbd>→</kbd><kbd>↑</kbd><kbd>↓</kbd></span><span>Move between items</span></div>
            <div class="kbRow"><span class="keys"><kbd>Shift</kbd>+<kbd>←/→</kbd></span><span>Extend selection</span></div>
            <div class="kbRow"><span class="keys"><kbd>Enter</kbd></span><span>Focus view ⇄ grid</span></div>
            <div class="kbRow"><span class="keys"><kbd>Esc</kbd></span><span>Close / back out</span></div>
            <div class="kbGroup">Views</div>
            <div class="kbRow"><span class="keys"><kbd>G</kbd></span><span>Grid</span></div>
            <div class="kbRow"><span class="keys"><kbd>D</kbd></span><span>Details</span></div>
            <div class="kbRow"><span class="keys"><kbd>F</kbd></span><span>Play mode — cycle: + strip → bare → off</span></div>
            <div class="kbRow"><span class="keys"><kbd>L</kbd></span><span>Dim lights (cycle)</span></div>
            <div class="kbRow"><span class="keys"><kbd>I</kbd></span><span>Info overlay</span></div>
            <div class="kbRow"><span class="keys"><kbd>B</kbd></span><span>Hide / show the filmstrip</span></div>
            <div class="kbGroup">Files</div>
            <div class="kbRow"><span class="keys"><kbd>Ctrl</kbd>+<kbd>X</kbd> <kbd>Ctrl</kbd>+<kbd>V</kbd></span><span>Move files (cut → paste in folder)</span></div>
            <div class="kbRow"><span class="keys"><kbd>Ctrl</kbd>+<kbd>A</kbd></span><span>Select all (filtered)</span></div>
          </div>
          <div>
            <div class="kbGroup">Culling</div>
            <div class="kbRow"><span class="keys"><kbd>P</kbd></span><span>Pick</span></div>
            <div class="kbRow"><span class="keys"><kbd>X</kbd></span><span>Reject</span></div>
            <div class="kbRow"><span class="keys"><kbd>U</kbd></span><span>Clear marks</span></div>
            <div class="kbRow"><span class="keys"><kbd>1</kbd>–<kbd>5</kbd></span><span>Star rating</span></div>
            <div class="kbRow"><span class="keys"><kbd>`</kbd></span><span>Clear stars</span></div>
            <div class="kbRow"><span class="keys"><kbd>6</kbd>–<kbd>9</kbd></span><span>Color label</span></div>
            <div class="kbRow"><span class="keys"><kbd>Ctrl</kbd>+<kbd>Z</kbd> / <kbd>Y</kbd></span><span>Undo / redo marks</span></div>
            <div class="kbGroup">Video (Focus)</div>
            <div class="kbRow"><span class="keys"><kbd>Space</kbd></span><span>Play / pause</span></div>
            <div class="kbRow"><span class="keys"><kbd>Ctrl</kbd>+<kbd>Space</kbd></span><span>Glimpse — sweep the clip to see what's in it</span></div>
            <div class="kbRow"><span class="keys"><kbd>,</kbd> <kbd>.</kbd> · <kbd>Shift</kbd>+<kbd>←/→</kbd></span><span>Step 5 s back / forward</span></div>
            <div class="kbRow"><span class="keys"><kbd>[</kbd> <kbd>]</kbd></span><span>Set in / out point</span></div>
            <div class="kbGroup">Beyond the keyboard</div>
            <div class="kbRow"><span class="keys">🖱</span><span>Right-click anything for its menu; mouse Back/Forward are remappable</span></div>
            <div class="kbRow"><span class="keys">🎮</span><span>PS5/PS4 pad — Settings → Controller (Create/Share shows its guide)</span></div>
          </div>
        </div>
        <div class="kbFoot">Press <kbd>?</kbd> anytime to show this.</div>
      </div>
    {/if}

    <!-- Controller button guide: toggled by the bound "help" action (default
         Create/Share) so a new player can learn the layout from the couch. -->
    {#if padHelpOpen}
      <div class="padGuide" role="dialog" aria-label="Controller buttons">
        <div class="pgHead">🎮 {pad.name.replace(/\s*\(.*\)$/, "") || "Controller"}</div>
        {#each ["Navigate", "Mark", "View", "Video"] as g (g)}
          {@const rows = padGuideRows.filter((r) => r.group === g)}
          {#if rows.length}
            <div class="pgGroup">{g}</div>
            {#each rows as r (r.id)}
              <div class="pgRow"><span class="pgBtn">{buttonName(r.btn)}</span><span>{r.label}</span></div>
            {/each}
          {/if}
        {/each}
        <div class="pgFoot">Remap in Settings → Controller</div>
      </div>
    {/if}

    {#if undoToast}
      <div class="undoToast" aria-live="polite">{undoToast}</div>
    {/if}

    <!-- Confirm/notice modal: filesystem actions that undo can trigger, and
         delete failures whose real reason doesn't fit in the activity chip. -->
    {#if ask}
      <button class="kbBackdrop" aria-label="Close" onclick={() => (ask = null)}></button>
      <div class="askBox" role="dialog" aria-label={ask.title}>
        <div class="askTitle">{ask.title}</div>
        <div class="askBody">{ask.body}</div>
        {#if ask.input}
          <!-- The global key handler bails out on INPUT targets, so Enter and
               Escape have to be handled here or the modal would trap the user. -->
          <input
            class="askInput"
            bind:this={askInputEl}
            bind:value={askValue}
            placeholder={ask.input.placeholder ?? ""}
            onkeydown={(e) => {
              if (e.key === "Enter") { e.preventDefault(); runAsk(); }
              else if (e.key === "Escape") { e.preventDefault(); ask = null; }
            }}
          />
        {/if}
        <div class="askRow">
          {#if ask.onconfirm}
            <button class="askBtn" onclick={() => (ask = null)}>Cancel</button>
            <button class="askBtn primary" disabled={!!ask.input && !askValue.trim()} onclick={runAsk}>{ask.confirmLabel ?? "Confirm"}</button>
          {:else}
            <button class="askBtn primary" onclick={() => (ask = null)}>Close</button>
          {/if}
        </div>
      </div>
    {/if}

    <!-- body: viewport, with the filmstrip optionally docked left or right -->
    <div class="body">
      {#if !editOpen && settings.s.filmstripPos === "left" && view.length && fsMode !== 2}
        <aside class="lstrip" class:fsDim={fullscreen} style="width:{settings.s.filmstripSize}px">
          <VirtualStrip items={view} {activeIndex} orientation="v" cellSize={stripCell} cell={stripCellSnip} />
        </aside>
        <div class="vsplit" role="separator" tabindex="-1" onpointerdown={startStripResize}></div>
      {/if}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="viewport"
        class:lit={dimLevel > 0}
        oncontextmenu={(e) => {
          if (viewMode === "loupe" && active) openContextMenu(e, active, activeIndex);
        }}
      >
        {#if loading}
          <div class="welcome"><p>Scanning {currentDir ? basename(currentDir) : ""}…</p></div>
        {:else if !currentDir}
          <div class="welcome">
            <div class="welcomeMark"><img class="wIcon" src="/favicon.png" alt="" width="74" height="74" /></div>
            <div class="welcomeCopy">
              <span class="welcomeEyebrow">Photo &amp; video review studio</span>
              <h1>Make the keepers obvious.</h1>
              <p>Open any folder and start culling in place. Nothing is imported, duplicated or changed until you ask.</p>
            </div>
            <button class="btn accent welcomeOpen" onclick={openFolderPicker}>
              <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7.5h6l2 2h10v9.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M3 8V5a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v2.5"/></svg>
              Open a folder
            </button>
            <div class="welcomeHints">
              <span><kbd>P</kbd> Pick</span><span><kbd>X</kbd> Reject</span><span><kbd>Enter</kbd> Focus</span><span><kbd>?</kbd> All shortcuts</span>
            </div>
          </div>
        {:else if editOpen}
          <EditStudio {active} {selectedItems} sourceItems={items} currentDir={currentDir} recursive={settings.s.includeSub} refreshKey={folderRefreshKey} onexported={() => void refreshAfterMediaOutput()} bind:this={editComp} />
        {:else if view.length === 0}
          <div class="welcome">
            {#if items.length > 0 && activeFilterCount > 0}
              <p>No items match the current filters ({items.length} in the folder).</p>
              <button class="btn" onclick={clearAllFilters}>Clear filters</button>
            {:else}
              <p>No photos or videos in this folder{settings.s.includeSub ? " or its subfolders" : ""}.</p>
            {/if}
          </div>
        {:else if viewMode === "loupe"}
          <Loupe
            item={active}
            showInfo={showInfoOverlay}
            onchanged={refreshAfterMediaOutput}
            ontransport={onLoupeTransport}
            casting={!!castDevice}
            castPlayerState={castStatus.playerState}
            oncasttoggle={() => void toggleCastPlayback()}
            bind:this={loupeComp}
          />
        {:else if viewMode === "details"}
          <DetailsView
            items={view}
            {activeIndex}
            {selected}
            onrowclick={gridCellClick}
            onrowdblclick={(i) => { setActiveTo(i); setView("loupe"); }}
            onrowcontext={(e, item, i) => openContextMenu(e, item, i)}
            onrowdragstart={beginMediaDrag}
            onrowdragend={endMediaDrag}
          />
        {:else if grouped}
          <SectionedGrid
            items={view}
            groups={sections}
            {activeIndex}
            cellMin={settings.s.gridSize}
            bind:this={gridComp}
            cell={gridCell}
          />
        {:else}
          <VirtualGrid items={view} {activeIndex} cellMin={settings.s.gridSize} bind:this={gridComp} cell={gridCell} />
        {/if}
      </div>

      {#if !editOpen && settings.s.filmstripPos === "right" && view.length && fsMode !== 2}
        <div class="vsplit" role="separator" tabindex="-1" onpointerdown={startStripResize}></div>
        <aside class="rstrip" class:fsDim={fullscreen} style="width:{settings.s.filmstripSize}px">
          <VirtualStrip items={view} {activeIndex} orientation="v" cellSize={stripCell} cell={stripCellSnip} />
        </aside>
      {/if}
    </div>

    <!-- active-item info bar -->
    {#if active && !editOpen}
      <div class="info">
        <span class="activeIdentity">
          <span class="name" title={active.path}>{active.name}</span>
          <span class="meta">{active.kind} · {activeIndex + 1} of {view.length}</span>
        </span>
        <span class="infoDivider"></span>
        <div class="rate" title="Star rating (1–5 · ` clears)">
          {#each [1, 2, 3, 4, 5] as n}
            <button class="star" class:on={active.rating >= n} onclick={() => rate(n)}>★</button>
          {/each}
        </div>
        {#each LABELS as l}
          <button class="dot sm" class:on={active.label === l.key} style="background:var({l.varName})" title={l.name} aria-label={l.name} onclick={() => label(l.key)}></button>
        {/each}
        <button class="btn sm" class:on={active.flag === "pick"} onclick={() => flag("pick")} title="Pick (P)">Pick</button>
        <button class="btn sm danger" class:on={active.flag === "reject"} onclick={() => flag("reject")} title="Reject (X)">{active.flag === "reject" ? "Unreject" : "Reject"}</button>

        <!-- events, then tags: the event says which trip this shot belongs to,
             which is the coarser fact, so it reads first. -->
        {#if active.events.length}
          <div class="tags evts">
            {#each active.events as name}
              {@const ev = eventByName.get(name)}
              <span class="tag evt" title="Event — click to see only this event">
                <button class="evtChip" onclick={() => (eventFilter = eventFilter === name ? null : name)}>✦ {name}</button>
                {#if ev}<button class="tagx" title="Remove from this event" aria-label="Remove from event" onclick={() => removeTargetsFromEvent(ev.id, name)}>×</button>{/if}
              </span>
            {/each}
          </div>
        {/if}
        <div class="tags">
          {#each active.tags as t}
            <span class="tag">{t}<button class="tagx" onclick={() => removeTagFromActive(t)} aria-label="Remove tag">×</button></span>
          {/each}
          <input
            class="taginput"
            placeholder="+ tag"
            bind:value={tagInput}
            onkeydown={(e) => { if (e.key === "Enter") addTagToTargets(); }}
          />
        </div>

        <span class="spacer"></span>
        <button class="ico" title="Reveal in file manager" aria-label="Reveal in file manager" onclick={() => active && api.reveal(active.path)}><svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6"/><path d="m20 4-9 9"/><path d="M18 13v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h5"/></svg></button>
        <span class="counts"><span class="pickCount">✓ {pickCount}</span><span class="rejectCount">✕ {rejectedCount}</span></span>
      </div>
    {/if}

    <!-- bottom filmstrip — hidden in the bare fullscreen state (fsMode 2), shown
         and dimmed in play-mode-with-strip (fsMode 1), normal otherwise. -->
    <!-- The 8px handle doubles as the show/hide control and stays put when the
         strip is hidden, so there is always something to click to bring it back
         (and it costs 8px, not a panel). Hiding from a left/right dock parks the
         control here too — one consistent place for "the strip is away". -->
    {#if !editOpen && view.length && fsMode !== 2 && (settings.s.filmstripPos === "bottom" || stripHidden)}
      <div
        class="hsplit"
        class:collapsed={stripHidden}
        role="separator"
        tabindex="-1"
        onpointerdown={stripHidden ? undefined : startStripResize}
        title={stripHidden ? "" : "Drag to resize"}
      >
        <span class="grip"></span>
        <button
          class="stripToggle"
          onpointerdown={(e) => e.stopPropagation()}
          onclick={toggleFilmstrip}
          title={stripHidden ? "Show filmstrip (B)" : "Hide filmstrip (B)"}
          aria-label={stripHidden ? "Show filmstrip" : "Hide filmstrip"}
        >
          <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d={stripHidden ? "M6 15l6-6 6 6" : "M6 9l6 6 6-6"} />
          </svg>
        </button>
      </div>
      {#if !stripHidden}
        <div class="bstrip" class:fsDim={fullscreen} style="height:{settings.s.filmstripSize}px">
          <VirtualStrip items={view} {activeIndex} orientation="h" cellSize={stripCell} cell={stripCellSnip} />
        </div>
      {/if}
    {/if}
  </main>

  <!-- dim / lights-out scrim: darkens all chrome, the photo viewport stays lit -->
  {#if dimLevel > 0}
    <button class="scrim" aria-label="Exit dim mode" onclick={() => (dimLevel = 0)}></button>
  {/if}

  {#if menu}
    <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onclose={() => (menu = null)} />
  {/if}
</div>

<style>
  .app { position: relative; display: flex; height: 100vh; overflow: hidden; }
  /* Full-screen "play mode" (F, state 1): every panel and bar disappears except
     the filmstrip, which stays in WHICHEVER dock is configured — bottom, left or
     right — dimmed ~20% and still resizable. (It used to be bottom-only: left
     and right were hidden outright, and the resize handles went with them, so
     picking a side dock meant losing the strip the moment you pressed F.)
     State 2 is the bare picture and drops the strip entirely; that's what the
     `fsMode !== 2` gates on the markup do. */
  .app.fs .tree,
  .app.fs .treeSplit,
  .app.fs .bar,
  .app.fs .banner,
  .app.fs .info,
  .app.fs .pop,
  .app.fs .treeRestore { display: none; }
  .tree { display: flex; flex-direction: column; background: var(--bg-panel); border-right: 1px solid var(--border); flex: 0 0 auto; min-width: 0; transition: width 0.14s ease; }
  .tree-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 45px; padding: 9px 10px; border-bottom: 1px solid var(--border); }
  .treeRestore {
    position: absolute;
    /* The command bar is an isolated z=100 stacking context so its popovers
       stay above media. This recovery control must sit above that context or
       collapsing the tree removes the only route to bring it back. */
    z-index: 160;
    left: 8px;
    top: 8px;
    background: var(--bg-elev);
    border-color: var(--border-strong);
    box-shadow: var(--shadow);
  }
  /* Floating stand-in for the sidebar's activity chip while the sidebar is
     collapsed. Self-hides when idle (the component renders nothing), so it only
     ever overlaps the filmstrip corner while there is something to report. */
  .actFloat {
    position: absolute;
    z-index: 80;
    left: 8px;
    bottom: 8px;
    width: 260px;
    max-width: 40vw;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    box-shadow: var(--shadow);
  }
  .app.fs .actFloat { display: none; }
  .tree-actions { display: flex; align-items: center; gap: 6px; }
  .ico.sm { width: 26px; height: 26px; font-size: 13px; }
  .ico.spin { animation: spin 0.5s linear; color: var(--accent); border-color: var(--accent); }
  @keyframes spin { to { transform: rotate(360deg); } }
  .tree-body { overflow-y: auto; padding: 6px; flex: 1; }
  .hint { padding: 10px; color: var(--text-faint); font-size: 12.5px; }

  .vsplit { flex: 0 0 5px; cursor: col-resize; background: transparent; }
  .vsplit:hover { background: color-mix(in srgb, var(--accent) 40%, transparent); }
  .hsplit { position: relative; flex: 0 0 8px; cursor: row-resize; display: flex; align-items: center; justify-content: center; background: var(--bg-panel); border-top: 1px solid var(--border); }
  .hsplit .grip { width: 46px; height: 3px; border-radius: 3px; background: var(--text-faint); opacity: 0.4; }
  .hsplit:hover { background: color-mix(in srgb, var(--accent) 22%, var(--bg-panel)); }
  .hsplit:hover .grip { opacity: 0.9; background: var(--accent); }
  /* With the strip hidden there is nothing to resize — only to restore. */
  .hsplit.collapsed { flex-basis: 14px; cursor: default; }
  .hsplit.collapsed:hover { background: var(--bg-panel); }
  .hsplit.collapsed .grip { opacity: 0.22; }
  /* Overhangs its 8px rail so it stays a real click target without the rail
     having to grow. */
  .stripToggle {
    position: absolute;
    right: 10px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 17px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--bg-panel);
    color: var(--text-dim);
    cursor: pointer;
    z-index: 3;
  }
  .stripToggle:hover { color: var(--accent); border-color: var(--accent); }

  .center { display: flex; flex-direction: column; flex: 1; min-width: 0; height: 100vh; }

  .bar { position: relative; z-index: 100; isolation: isolate; display: flex; align-items: center; gap: 8px; min-height: 48px; padding: 6px 10px; border-bottom: 1px solid var(--border); background: var(--bg-panel); flex-wrap: nowrap; }
  /* Reserve the restore button's footprint instead of covering the first
     command-bar control when the folder tree is hidden. */
  .treeCollapsed .bar { padding-left: 48px; }
  .tool-group { display: flex; align-items: center; gap: 5px; min-width: 0; flex: 0 0 auto; }
  .ctl-label { color: var(--text-faint); font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0; white-space: nowrap; }
  .viewGroup { padding-right: 1px; }
  .rightTools { display: flex; align-items: center; gap: 7px; flex: 0 0 auto; }
  .grp { display: flex; align-items: center; gap: 4px; }
  .seg { display: flex; align-items: center; gap: 3px; }
  .seg.flags { gap: 2px; }
  .seg.modes { gap: 2px; padding: 2px; background: var(--bg-elev); border: 1px solid var(--border); border-radius: 8px; }
  .spacer { flex: 1 1 auto; min-width: 10px; }
  .sel { max-width: 128px; background: var(--bg-elev); color: var(--text); border: 1px solid var(--border); border-radius: 7px; padding: 4px 6px; font-size: 12.5px; }
  .sel.wide { flex: 1; max-width: none; min-width: 145px; }
  .ico { width: 28px; height: 28px; border-radius: 7px; border: 1px solid var(--border); background: var(--bg-elev); font-size: 14px; line-height: 1; }
  .ico:hover { background: var(--bg-hover); }
  .ico.on { border-color: var(--accent); color: var(--accent); }
  .panelGlyph { display: block; opacity: 0.9; }
  .refreshIcon { display: block; width: 15px; height: 15px; }
  .chip { padding: 4px 9px; border-radius: 6px; font-size: 12px; color: var(--text-dim); border: 1px solid transparent; white-space: nowrap; }
  .chip:hover { background: var(--bg-hover); }
  .chip.on { background: var(--accent); color: var(--accent-on); }
  .chip.rej.on { background: var(--reject); border-color: var(--reject); }
  .chip.pick.on { background: var(--pick); border-color: var(--pick); }
  .starf { font-size: 14px; color: var(--text-faint); padding: 0 1px; }
  .starf.on { color: var(--star); }
  .dot { width: 14px; height: 14px; border-radius: 3px; border: 1px solid rgba(0,0,0,0.25); opacity: 0.5; }
  .dot.sm { width: 13px; height: 13px; }
  .dot.on { opacity: 1; outline: 2px solid var(--accent); outline-offset: 1px; }
  .zoom { gap: 6px; }
  .zoom .mini { color: var(--text-faint); font-size: 12px; }
  .zoom input { width: 90px; accent-color: var(--accent); }
  .modeToggle { display: inline-flex; gap: 3px; padding: 3px; border: 1px solid var(--border); border-radius: 10px; background: var(--bg-elev); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--bg-hover) 55%, transparent); }
  .modeToggle button { min-width: 72px; padding: 7px 12px; border-radius: 8px; color: var(--text-dim); font-size: 13px; font-weight: 800; }
  .modeToggle button:hover { background: var(--bg-hover); }
  .modeToggle button.on { background: var(--accent); color: var(--accent-on); }
  .modeToggle button:disabled { opacity: 0.45; cursor: not-allowed; }
  .editModeTitle { gap: 9px; }
  .editModeTitle strong { font-size: 13.5px; }
  .editModeTitle span:last-child { color: var(--text-faint); font-size: 12px; white-space: nowrap; }
  .btn.sm { padding: 5px 9px; border-radius: 7px; font-size: 12.5px; }
  .btn.sm.on { border-color: var(--accent); color: var(--accent); }
  /* Sized to its own content. The old 96px floor existed to stop the button
     resizing as the label cycles Prepare → 42% 1m → Ready, but it left the
     idle state — the one you look at all day — visibly padded out. Now the
     floor just fits "Prepare"; the progress label is allowed to grow it. */
  .prep { position: relative; overflow: hidden; min-width: 84px; text-align: center; }
  .prep-fill { position: absolute; left: 0; top: 0; bottom: 0; background: color-mix(in srgb, var(--accent) 30%, transparent); transition: width 0.2s ease; }
  .prep-lbl { position: relative; z-index: 1; display: inline-flex; align-items: center; justify-content: center; gap: 5px; white-space: nowrap; }
  /* The bolt is the button's identity in a crowded toolbar — gold (the same
     token the rating stars use) and large enough to register at a glance. */
  .prep-ico { font-size: 14px; line-height: 1; color: var(--star); display: inline-flex; align-items: center; }
  .prep-ico svg { display: block; }

  .div { flex: 0 0 auto; align-self: stretch; width: 1px; margin: 2px 4px; background: var(--border); }
  .arrange,
  .filterwrap { position: relative; }
  .arrangeMenu,
  /* Sized so the widest row (Status: All/Picks/Rejected/Unflagged) fits, and
     rows WRAP as a backstop — a chip must never clip past the popover edge. */
  .filtermenu { position: absolute; top: 34px; left: 0; z-index: 120; width: 316px; max-width: min(316px, 90vw); background: var(--bg-elev); border: 1px solid var(--border); border-radius: 10px; box-shadow: var(--shadow); padding: 11px; display: flex; flex-direction: column; gap: 11px; }
  .filtermenu .seg { flex-wrap: wrap; min-width: 0; }
  /* "N of M" passing the active filters — lives beside the Filters chip. */
  .shown-count {
    font-size: 11.5px;
    color: var(--text-dim);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    padding: 0 2px;
  }
  /* Rating comparison operator (≥ / ≤ / =). */
  .opseg { display: flex; gap: 1px; padding: 1px; margin-right: 4px; border: 1px solid var(--border); border-radius: 6px; }
  .opbtn { width: 20px; padding: 1px 0; font-size: 12px; line-height: 1.3; color: var(--text-faint); border-radius: 4px; }
  .opbtn.on { background: var(--accent); color: var(--accent-on); }
  .opbtn:hover:not(.on) { color: var(--text); background: var(--bg-hover); }
  /* "Any" label chip (clears the multi-select). */
  .lblchip { font-size: 11px; padding: 1px 7px; border: 1px solid var(--border); border-radius: 999px; color: var(--text-dim); }
  .lblchip.on { border-color: var(--accent); color: var(--accent); }
  /* "None" (unlabeled) — outlined dot with a slash, drawn crisply as SVG
     strokes instead of the old misaligned ∅ glyph. */
  .dot.none { display: inline-flex; align-items: center; justify-content: center; background: var(--bg-elev); opacity: 1; padding: 0; }
  .dot.none svg { width: 12px; height: 12px; display: block; }
  .dot.none svg circle, .dot.none svg line { stroke: var(--text-faint); stroke-width: 1.4; }
  .dot.none.on svg circle, .dot.none.on svg line { stroke: var(--accent); }
  .arrangeMenu { width: 315px; }
  .fm-row { display: flex; align-items: center; gap: 10px; }
  .fm-row.col { flex-direction: column; align-items: stretch; gap: 5px; }
  /* Wide enough for the longest label ("Subgroup") so every row's control
     column starts at the same x — mismatched indents read as misalignment. */
  .fm-lbl { flex: 0 0 58px; font-size: 12px; color: var(--text-dim); }
  /* Arrange rows carry a glyph so Sort / Group / Subgroup are scannable
     without reading. Wider basis than a plain label to fit glyph + word. */
  .arrangeMenu .fm-lbl {
    flex: 0 0 82px;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .arrangeMenu .fm-ico {
    font-size: 13px;
    line-height: 1;
    color: var(--text-faint);
  }
  /* Subgroup is a nested level — its glyph is indented to read that way. */
  .arrangeMenu .fm-ico.sub { margin-left: 5px; font-size: 11px; }
  /* The direction arrow is the one control here you flip constantly, so it
     reads as a real button rather than a muted hint. */
  .arrangeMenu .dirbtn {
    font-size: 17px;
    font-weight: 700;
    color: var(--text);
    flex: 0 0 auto;
  }
  .arrangeMenu .dirbtn:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .fm-tags { display: flex; flex-direction: column; gap: 2px; max-height: 200px; overflow-y: auto; }
  .fm-clr { font-size: 11px; color: var(--text-faint); padding: 0 4px; margin-left: 4px; }
  .fm-clr:hover { color: var(--text); }
  .tagrow { display: flex; justify-content: space-between; gap: 10px; width: 100%; text-align: left; padding: 6px 9px; border-radius: 6px; font-size: 12.5px; color: var(--text); }
  .tagrow:hover { background: var(--bg-hover); }
  .tagrow.on { background: var(--accent); color: var(--accent-on); }
  .tagrow .cnt { color: var(--text-faint); }
  .tagrow.on .cnt { color: var(--accent-on); }
  .tagempty { padding: 8px 9px; color: var(--text-faint); font-size: 12px; margin: 0; }
  .missingNote { font-size: 12px; color: color-mix(in srgb, var(--warn, #d9a441) 85%, var(--text)); }

  /* Arrange ▸ Events: the ordering toggles plus a compact manage list. */
  .evtRow { flex-wrap: wrap; }
  .evtRow.off { opacity: 0.5; }
  .chk { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-dim); cursor: pointer; }
  .chk input { accent-color: var(--accent); }
  .chk input:disabled { cursor: default; }
  .evtList { display: flex; flex-direction: column; gap: 2px; max-height: 190px; overflow-y: auto; margin-top: 2px; border-top: 1px solid var(--border-soft); padding-top: 6px; }
  .evtManageRow { display: flex; align-items: center; gap: 3px; }
  .evtName { flex: 1; min-width: 0; display: flex; justify-content: space-between; gap: 10px; text-align: left; padding: 5px 8px; border-radius: 6px; font-size: 12.5px; color: var(--text); }
  .evtName:hover { background: var(--bg-hover); }
  .evtName.on { background: var(--accent); color: var(--accent-on); }
  .evtName span:first-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .evtName .cnt { color: var(--text-faint); }
  .evtName.on .cnt { color: var(--accent-on); }
  .ico.xs { width: 22px; height: 22px; font-size: 12px; flex: 0 0 auto; }

  .hold { position: relative; overflow: hidden; }
  .hold-fill { position: absolute; left: 0; top: 0; bottom: 0; background: color-mix(in srgb, var(--reject) 35%, transparent); }
  .hold-lbl { position: relative; z-index: 1; }
  .clearWrap { position: relative; }
  .clearMenu { position: absolute; top: 32px; right: 0; z-index: 35; width: 170px; padding: 6px; display: grid; gap: 2px; border: 1px solid var(--border); border-radius: 9px; background: var(--bg-elev); box-shadow: var(--shadow); }
  .clearMenu button { text-align: left; padding: 7px 9px; border-radius: 6px; color: var(--text-dim); font-size: 12px; }
  .clearMenu button:hover { background: var(--bg-hover); color: var(--text); }
  .clearMenu .dangerText { color: var(--reject); }
  /* Prepare split button: primary action + a caret for the scope menu. */
  .prepWrap { position: relative; display: flex; }
  .prepWrap .prep { border-top-right-radius: 0; border-bottom-right-radius: 0; }
  .prepCaret {
    margin-left: -1px;
    padding-left: 6px;
    padding-right: 6px;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }
  .prepMenu { width: 232px; }
  .prepMenu button { display: flex; justify-content: space-between; gap: 10px; }
  .prepMenu button:disabled { opacity: 0.45; }
  .prepCount { color: var(--text-faint); font-variant-numeric: tabular-nums; }
  /* Inline icon inside a toolbar text button — optically aligned with the label. */
  .btn-ico { vertical-align: -1px; margin-right: 4px; }
  .hold-lbl .btn-ico { margin-right: 3px; }
  /* Cast to TV */
  .castWrap { position: relative; display: flex; align-items: center; gap: 5px; }
  .castBtn.on {
    color: var(--accent);
    border-color: var(--accent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .castBadge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 25px;
    padding: 0 8px;
    border: 1px solid color-mix(in srgb, var(--accent) 70%, var(--border));
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 14%, var(--bg-elev));
    color: var(--accent);
    font-size: 9px;
    letter-spacing: 0.06em;
    box-shadow: 0 0 12px color-mix(in srgb, var(--accent) 20%, transparent);
  }
  .castBadge strong { font-size: 9.5px; }
  .castBadge > span:last-child {
    color: var(--text-dim);
    letter-spacing: 0;
    text-transform: none;
  }
  .castPulse {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .castBadge.playing .castPulse { animation: castGlow 1.45s ease-in-out infinite; }
  @keyframes castGlow {
    0%, 100% { box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent), 0 0 4px var(--accent); }
    50% { box-shadow: 0 0 0 6px transparent, 0 0 11px var(--accent); }
  }
  .castMenu { position: absolute; top: 32px; right: 0; z-index: 35; width: 230px; padding: 6px; display: grid; gap: 2px; border: 1px solid var(--border); border-radius: 9px; background: var(--bg-elev); box-shadow: var(--shadow); }
  .castRow { display: flex; flex-direction: column; gap: 1px; text-align: left; padding: 7px 9px; border-radius: 6px; color: var(--text); font-size: 12.5px; }
  .castRow span { font-size: 10.5px; color: var(--text-faint); }
  .castRow:hover:not(:disabled) { background: var(--bg-hover); }
  .castRow:disabled { opacity: 0.5; }
  .castRow.stop { color: var(--reject); font-weight: 600; }
  .castRow.sub { color: var(--text-faint); font-size: 11.5px; }
  .castNow { padding: 6px 9px 4px; font-size: 11.5px; color: var(--text-dim); }
  .castNow strong { color: var(--text); }
  .castHint { padding: 8px 9px; font-size: 11.5px; color: var(--text-dim); line-height: 1.45; }
  .linklike { display: inline; padding: 0; color: var(--accent); text-decoration: underline; font-size: inherit; }
  .menuSep { height: 1px; margin: 3px 4px; background: var(--border); }
  /* Undo/redo feedback (Ctrl+Z / Ctrl+Y): transient, bottom-center, never
     intercepts the pointer. */
  .undoToast {
    position: fixed;
    left: 50%;
    bottom: 74px;
    transform: translateX(-50%);
    z-index: 300;
    padding: 7px 14px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg-elev);
    color: var(--text);
    font-size: 12.5px;
    box-shadow: var(--shadow);
    pointer-events: none;
  }

  /* Confirm / notice modal (shares the shortcut guide's backdrop). */
  .askBox {
    position: fixed;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 296;
    width: min(520px, 90vw);
    padding: 16px 18px 14px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--bg-elev);
    box-shadow: var(--shadow);
  }
  .askTitle { font-size: 14px; font-weight: 650; color: var(--text); }
  .askBody {
    margin-top: 8px;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--text-dim);
    white-space: pre-wrap;
    max-height: 46vh;
    overflow-y: auto;
  }
  /* Prompt field — the modal doubles as a name prompt (new folder, new event). */
  .askInput {
    width: 100%;
    margin-top: 12px;
    padding: 8px 11px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-panel);
    color: var(--text);
    font-size: 13.5px;
  }
  .askInput:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 26%, transparent);
  }
  .askRow { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
  .askBtn {
    padding: 6px 14px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: transparent;
    color: var(--text);
    font-size: 12.5px;
    cursor: pointer;
  }
  .askBtn:hover { background: color-mix(in srgb, var(--text) 8%, transparent); }
  .askBtn:disabled { opacity: 0.45; pointer-events: none; }
  .askBtn.primary { border-color: var(--accent); background: var(--accent); color: #fff; }

  /* Keyboard shortcut guide (?): centered card over a dim backdrop. */
  .kbBackdrop {
    position: fixed;
    inset: 0;
    z-index: 294;
    background: rgba(0, 0, 0, 0.45);
    border: none;
    cursor: default;
  }
  .kbGuide {
    position: fixed;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 295;
    width: min(760px, calc(100vw - 60px));
    max-height: calc(100vh - 80px);
    overflow-y: auto;
    padding: 16px 20px 14px;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--bg-elev);
    box-shadow: var(--shadow);
  }
  .kbHead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-weight: 700;
    font-size: 14.5px;
    margin-bottom: 8px;
  }
  .kbClose {
    width: 28px;
    height: 28px;
    border-radius: 7px;
    color: var(--text-dim);
  }
  .kbClose:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .kbCols {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px 28px;
  }
  @media (max-width: 720px) {
    .kbCols { grid-template-columns: 1fr; }
  }
  .kbGroup {
    margin: 10px 0 4px;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.5px;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .kbRow {
    display: flex;
    align-items: baseline;
    gap: 10px;
    font-size: 12.5px;
    line-height: 1.9;
    color: var(--text-dim);
  }
  .kbRow .keys {
    flex: 0 0 128px;
    white-space: nowrap;
    color: var(--text);
  }
  .kbGuide kbd,
  .kbFoot kbd {
    display: inline-block;
    min-width: 17px;
    padding: 0 5px;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 5px;
    background: var(--bg-panel);
    font-family: inherit;
    font-size: 11px;
    line-height: 1.6;
    text-align: center;
  }
  .kbFoot {
    margin-top: 12px;
    padding-top: 9px;
    border-top: 1px solid var(--border);
    font-size: 11.5px;
    color: var(--text-faint);
  }

  /* Controller button-guide overlay: readable from TV distance, never blocks
     input (the pad keeps working while it's up). */
  .padGuide {
    position: fixed;
    right: 26px;
    top: 62px;
    z-index: 290;
    width: 300px;
    max-height: calc(100vh - 120px);
    overflow-y: auto;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: color-mix(in srgb, var(--bg-elev) 92%, transparent);
    box-shadow: var(--shadow);
    pointer-events: none;
  }
  .padGuide .pgHead {
    font-weight: 700;
    font-size: 14px;
    margin-bottom: 4px;
  }
  .padGuide .pgGroup {
    margin: 9px 0 3px;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.5px;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .padGuide .pgRow {
    display: flex;
    align-items: baseline;
    gap: 9px;
    font-size: 13px;
    line-height: 1.75;
  }
  .padGuide .pgBtn {
    flex: 0 0 118px;
    font-weight: 600;
    color: var(--accent);
  }
  .padGuide .pgFoot {
    margin-top: 10px;
    font-size: 11px;
    color: var(--text-faint);
  }

  .pop { position: absolute; right: 10px; top: 46px; z-index: 30; background: var(--bg-elev); border: 1px solid var(--border); border-radius: 10px; box-shadow: var(--shadow); padding: 12px; width: 340px; display: flex; flex-direction: column; gap: 10px; }
  .pop .grpHead { margin-top: 2px; font-size: 10px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; color: var(--text-faint); border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent); padding-bottom: 3px; }
  .pop .grpHead:first-child { margin-top: 0; }
  .pop .row { display: flex; align-items: center; justify-content: space-between; gap: 10px; font-size: 13px; }
  .pop .row.sub { padding-left: 6px; flex-wrap: nowrap; }
  /* Range control in a settings row (Glimpse speed). Sized so the numeric
     readout can't reflow the row as the value changes width. */
  .slider { display: flex; align-items: center; gap: 8px; }
  .slider input[type="range"] { width: 120px; accent-color: var(--accent); }
  .slider .sliderVal {
    min-width: 34px;
    text-align: right;
    font-size: 12px;
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
  }
  .pop .path { flex: 1; min-width: 0; color: var(--text-dim); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pop .row.sub .tag { flex: 0 0 auto; }
  /* Prose row — MUST be block, not flex: flex + space-between turns the text
     fragments around <code>/<kbd> into separate squeezed flex items and the
     whole sentence collapses into a one-word-per-line column. */
  .pop .row.hintrow { display: block; color: var(--text-faint); font-size: 12px; line-height: 1.7; }
  kbd { background: var(--bg-panel); border: 1px solid var(--border); border-radius: 4px; padding: 0 5px; font-size: 11px; }

  .body { flex: 1; display: flex; min-height: 0; }
  .viewport { flex: 1; min-width: 0; background: var(--viewport-bg); overflow: hidden; display: flex; flex-direction: column; }
  .viewport.lit { position: relative; z-index: 50; }
  .rstrip { flex: 0 0 auto; border-left: 1px solid var(--border); }
  .lstrip { flex: 0 0 auto; border-right: 1px solid var(--border); background: var(--bg-panel); }
  .bstrip { flex: 0 0 auto; }
  /* Play mode (fsMode 1): the strip stays in view but dimmed ~20% so attention
     stays on the photo/video. fsMode 2 doesn't render it at all. */
  .app.fs .lstrip.fsDim,
  .app.fs .rstrip.fsDim,
  .app.fs .bstrip.fsDim {
    filter: brightness(0.8);
    transition: filter 0.15s ease;
  }

  .welcome { height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; color: var(--text-dim); text-align: center; padding: 24px; }
  .welcome h1 { font-size: 28px; margin: 0; }
  .welcome .wIcon { border-radius: 16px; opacity: 0.95; }
  .welcome kbd {
    display: inline-block;
    min-width: 16px;
    padding: 0 4px;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 4px;
    background: var(--bg-panel);
    font-family: inherit;
    font-size: 10.5px;
    line-height: 1.6;
    text-align: center;
  }

  /* Every tile reserves a thin top band so the golden stack line (when present)
     sits above the thumbnail without shrinking it unevenly across a row.
     NOTE: no overflow:hidden here — clipping now lives on .cellclip (the inner
     wrapper around Thumb+overlays) so the stackline bar, a direct child of
     .cell, is free to bleed past the tile edge into the grid gap. The 2px
     border still renders with rounded corners on its own without needing
     overflow:hidden. */
  .cell { position: relative; width: 100%; height: 100%; border: 2px solid transparent; border-radius: 6px; padding: 8px 0 0; background: var(--viewport-bg); }
  .cell.selected { border-color: var(--select); }
  .cell.active { border-color: var(--accent); }
  .cell.reject :global(.media) { opacity: 0.35; }
  /* Missing-file tile: a dashed frame says "this slot is a catalog entry, not a
     photo" at a glance, without stealing the selection colours. */
  .cell.gone,
  .scell.gone { border-style: dashed; border-color: color-mix(in srgb, var(--warn, #d9a441) 60%, transparent); }
  .cell.gone.active,
  .scell.gone.active { border-color: var(--accent); }

  /* Clips the thumbnail + overlay badges to the tile's rounded corners — the
     job .cell's own overflow:hidden used to do before it had to let the
     stackline bleed out. Sits below .stackline (lower in DOM, no z-index
     conflict since stackline is a sibling, not a descendant). */
  .cellclip { position: relative; width: 100%; height: 100%; overflow: hidden; border-radius: 4px; }
  .scell .cellclip { border-radius: 3px; }

  /* Related/stack tiles: a single golden line on top for an expanded stack,
     a double line for a collapsed stack. The band is the click target (toggles
     expand/collapse) and shows a subtle hover wash. */
  .stackline {
    position: absolute;
    top: 0; left: 0; right: 0;
    height: 8px;
    z-index: 5;
    cursor: pointer;
    background: transparent;
    transition: background 0.12s ease;
  }
  /* The bar runs full-bleed by default so adjacent tiles of the SAME stack join
     into one continuous golden line. Only the group's outer ends get a rounded
     cap — which both closes off a lone/collapsed stack and leaves a clear break
     between two neighbouring stacks. Non-edge sides extend past the tile by
     border-width(2px) + half the grid/strip gap(3px) so two neighbouring
     same-stack bars meet (with a safe 1px overlap) in the middle of the gap
     instead of stopping dead at each tile's own edge. .cell/.scell no longer
     clip (see .cellclip above), so this bleed is actually visible. */
  .stackline::before {
    content: "";
    position: absolute;
    left: -5px; right: -5px;
    top: 2.5px;
    height: 2.5px;
    background: var(--stack);
    transition: background 0.12s ease, top 0.12s ease;
  }
  .stackline.dbl::before { top: 1px; }
  .stackline.dbl::after {
    content: "";
    position: absolute;
    left: -5px; right: -5px;
    top: 4.5px;
    height: 2.5px;
    background: var(--stack);
    transition: background 0.12s ease;
  }
  /* Rounded cap at the true start / end of a stack (and both, when a stack is
     collapsed to a single representative tile). */
  .rel-start .stackline::before,
  .rel-start .stackline.dbl::after { left: 3px; border-top-left-radius: 2px; border-bottom-left-radius: 2px; }
  .rel-end .stackline::before,
  .rel-end .stackline.dbl::after { right: 3px; border-top-right-radius: 2px; border-bottom-right-radius: 2px; }
  .rel-collapsed .stackline::before,
  .rel-collapsed .stackline.dbl::after { left: 3px; right: 3px; border-radius: 2px; }
  .stackline:hover { background: color-mix(in srgb, var(--stack) 16%, transparent); }
  .stackline:hover::before,
  .stackline:hover::after { background: var(--stack-strong); }

  /* top:0 (not 8px) — .ov now lives inside .cellclip, which already starts
     8px down thanks to .cell's padding-top, so the old manual offset would
     double up. */
  .ov { position: absolute; top: 0; left: 0; right: 0; bottom: 0; z-index: 3; pointer-events: none; }
  .lbl-dot { position: absolute; top: 5px; right: 5px; width: 12px; height: 12px; border-radius: 3px; border: 1px solid rgba(0,0,0,0.4); }
  .fl { position: absolute; top: 4px; left: 6px; font-weight: 700; text-shadow: 0 1px 3px rgba(0,0,0,0.6); }
  .cell.related .fl { top: 25px; }
  .fl.x { color: var(--reject); }
  .fl.pick { color: var(--pick); }
  .stars { position: absolute; bottom: 4px; left: 6px; color: var(--star); font-size: 13px; text-shadow: 0 1px 3px rgba(0,0,0,0.6); }
  .tagdot { position: absolute; bottom: 4px; right: 6px; font-size: 11px; filter: drop-shadow(0 1px 2px rgba(0,0,0,0.6)); }
  /* Event marker — sits inboard of the tag glyph so a photo can carry both. */
  .evtdot { position: absolute; bottom: 4px; right: 22px; font-size: 11px; color: var(--accent); filter: drop-shadow(0 1px 2px rgba(0,0,0,0.65)); }
  .gonemark {
    position: absolute;
    top: 5px;
    right: 6px;
    min-width: 17px;
    height: 17px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 700;
    color: #fff;
    background: color-mix(in srgb, var(--warn, #d9a441) 88%, #000);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.5);
  }
  .gonemark.sm { min-width: 14px; height: 14px; font-size: 9px; top: 3px; right: 3px; }
  /* Derivative badge (FoxCull export): a small accent pill, top-right under the
     colour-label dot, marking IG / MIX / CROP / TRIM exports. */
  .deriv-badge {
    position: absolute; top: 22px; right: 5px;
    padding: 1px 5px;
    font-size: 9px; font-weight: 800; letter-spacing: 0.03em;
    color: var(--accent-on);
    background: color-mix(in srgb, var(--accent) 88%, #000);
    border-radius: 4px;
    text-shadow: none;
    box-shadow: 0 1px 2px rgba(0,0,0,0.5);
  }
  .cell.related .deriv-badge { top: 22px; }
  /* RAW / JPG kind tag: bottom-left over the thumbnail. RAW gets a distinct
     slate pill (it's an attribute of the file, not a FoxCull export, so it must
     not read like the accent deriv-badge); the JPG sibling tag echoes it dimmer. */
  .kind-tag {
    position: absolute; bottom: 21px; left: 6px;
    padding: 1px 5px;
    font-size: 9px; font-weight: 800; letter-spacing: 0.04em;
    color: #dfe6ea;
    background: rgba(40, 58, 70, 0.9);
    border: 1px solid rgba(255,255,255,0.22);
    border-radius: 4px;
    text-shadow: none;
    box-shadow: 0 1px 2px rgba(0,0,0,0.5);
  }
  .kind-tag.raw { background: rgba(96, 66, 22, 0.92); }
  .cell.related .kind-tag { bottom: 40px; }
  .rel-badges { position: absolute; top: 5px; left: 5px; right: 24px; display: flex; gap: 3px; overflow: hidden; }
  .rel-badges span,
  .rel-role,
  .rel-count {
    border: 1px solid rgba(255,255,255,0.18);
    background: rgba(0,0,0,0.58);
    color: #fff;
    text-shadow: none;
    font-size: 9px;
    font-weight: 800;
    line-height: 1.2;
    border-radius: 4px;
    white-space: nowrap;
  }
  .rel-badges span { min-width: 0; max-width: 74px; overflow: hidden; text-overflow: ellipsis; padding: 2px 5px; }
  .rel-role { position: absolute; left: 6px; bottom: 21px; padding: 2px 5px; color: color-mix(in srgb, var(--accent) 18%, #fff); }
  .rel-orphan {
    position: absolute; left: 6px; bottom: 40px;
    padding: 2px 5px; border-radius: 4px;
    font-size: 9px; font-weight: 800;
    color: #fff; background: rgba(150, 90, 20, 0.85);
    border: 1px solid rgba(255,255,255,0.18);
  }
  .rel-count { position: absolute; right: 6px; bottom: 21px; min-width: 22px; padding: 3px 6px; text-align: center; font-size: 11px; background: color-mix(in srgb, var(--accent) 72%, #000); }

  .scell { position: relative; width: 100%; height: 100%; border: 2px solid transparent; border-radius: 5px; padding: 0; background: var(--viewport-bg); }
  .scell.selected { border-color: var(--select); }
  .scell.active { border-color: var(--accent); }
  .scell.reject { opacity: 0.45; }
  /* Strip tiles: same golden stack line, sized down and drawn over the top edge
     (no reserved band) so strip layout stays compact. */
  .scell .stackline { height: 6px; }
  .scell .stackline::before { top: 1.5px; height: 2px; }
  .scell .stackline.dbl::before { top: 0.5px; }
  .scell .stackline.dbl::after { top: 3px; height: 2px; }
  .scell.related .s-rel { top: 8px; }
  .s-lbl { position: absolute; top: 3px; right: 3px; width: 10px; height: 10px; border-radius: 2px; }
  .s-deriv {
    position: absolute; bottom: 2px; right: 3px;
    padding: 0 3px;
    font-size: 8px; font-weight: 800;
    color: var(--accent-on);
    background: color-mix(in srgb, var(--accent) 88%, #000);
    border-radius: 3px;
  }
  .s-kind {
    /* Bottom-right, stacked just above .s-deriv (top-right belongs to the
       colour-label dot on these small tiles). */
    position: absolute; bottom: 15px; right: 3px;
    padding: 0 3px;
    font-size: 8px; font-weight: 800;
    color: #dfe6ea;
    background: rgba(40, 58, 70, 0.9);
    border-radius: 3px;
  }
  .s-kind.raw { background: rgba(96, 66, 22, 0.92); }
  .s-stars { position: absolute; bottom: 2px; left: 3px; font-size: 10px; color: var(--star); text-shadow: 0 1px 2px rgba(0,0,0,0.6); }
  .s-x { position: absolute; top: 2px; left: 4px; color: var(--reject); font-weight: 700; text-shadow: 0 1px 2px rgba(0,0,0,0.6); }
  .s-pick { position: absolute; top: 2px; left: 4px; color: var(--pick); font-weight: 700; text-shadow: 0 1px 2px rgba(0,0,0,0.6); }
  .scell.related .s-x,
  .scell.related .s-pick { top: 21px; }
  .s-rel,
  .s-role,
  .s-count {
    position: absolute;
    z-index: 3;
    border-radius: 3px;
    background: rgba(0,0,0,0.6);
    color: #fff;
    font-weight: 800;
    text-shadow: none;
    line-height: 1;
  }
  .s-rel { top: 3px; left: 3px; max-width: calc(100% - 20px); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding: 2px 4px; font-size: 8.5px; }
  .s-role { left: 3px; bottom: 16px; width: 14px; height: 14px; display: flex; align-items: center; justify-content: center; font-size: 9px; color: color-mix(in srgb, var(--accent) 18%, #fff); }
  .s-count { right: 3px; bottom: 3px; min-width: 15px; padding: 2px 3px; font-size: 9px; text-align: center; }

  .info { display: flex; align-items: center; gap: 10px; padding: 5px 12px; border-top: 1px solid var(--border); background: var(--bg-panel); }
  .info .name { font-weight: 600; max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .info .meta { color: var(--text-faint); font-size: 12px; }
  .info .counts { color: var(--text-faint); font-size: 12.5px; }
  .rate { display: flex; }
  .star { color: var(--text-faint); font-size: 16px; }
  .star.on { color: var(--star); }

  .tags { display: flex; align-items: center; gap: 5px; flex-wrap: nowrap; overflow: hidden; }
  .tag { display: inline-flex; align-items: center; gap: 3px; font-size: 11px; background: var(--bg-elev); border: 1px solid var(--border); border-radius: 11px; padding: 1px 4px 1px 8px; color: var(--text-dim); white-space: nowrap; }
  .tagx { font-size: 13px; line-height: 1; color: var(--text-faint); padding: 0 2px; }
  .tagx:hover { color: var(--reject); }
  /* Event chips carry the accent so a trip is distinguishable from a tag at a
     glance, and clicking one isolates that event across the whole view. */
  .tag.evt { border-color: color-mix(in srgb, var(--accent) 45%, var(--border)); padding-left: 3px; }
  .evtChip { font-size: 11px; color: var(--accent); padding: 1px 4px; border-radius: 9px; }
  .evtChip:hover { background: color-mix(in srgb, var(--accent) 14%, transparent); }
  .tags.evts { flex: 0 1 auto; }
  .taginput { width: 70px; background: var(--bg-elev); border: 1px solid var(--border); border-radius: 11px; padding: 2px 8px; font-size: 11.5px; color: var(--text); }
  .taginput:focus { outline: none; border-color: var(--accent); width: 110px; }

  /* dim / lights-out scrim */
  .scrim { position: fixed; inset: 0; z-index: 40; border: none; padding: 0; cursor: pointer; background: rgba(0,0,0,0.55); transition: background 0.18s; }
  .app[data-dim="2"] .scrim { background: rgba(0,0,0,0.93); }

  /* ── 2026 studio finish ─────────────────────────────────────────────── */
  .app {
    background: var(--bg);
  }
  .tree {
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--bg-panel) 96%, white 4%), var(--bg-panel));
    border-right-color: var(--border-soft);
    box-shadow: 1px 0 0 color-mix(in srgb, black 18%, transparent);
  }
  .tree-head {
    min-height: 54px;
    padding: 8px 10px;
    border-bottom-color: var(--border-soft);
  }
  .sidebarIdentity { min-width: 0; display: flex; align-items: center; gap: 8px; margin-right: auto; }
  .sidebarIdentity img { flex: 0 0 auto; border-radius: 7px; box-shadow: 0 4px 14px rgba(0,0,0,.28); }
  .brandLockup { min-width: 0; display: flex; flex-direction: column; line-height: 1.05; }
  .brandLockup strong { font-family: var(--font-display); font-size: 13px; font-weight: 720; letter-spacing: -.01em; }
  .brandLockup small { max-width: 95px; margin-top: 4px; overflow: hidden; color: var(--text-faint); font-size: 9.5px; font-weight: 620; letter-spacing: .075em; text-overflow: ellipsis; text-transform: uppercase; white-space: nowrap; }
  .tree-actions { gap: 5px; }
  .tree-body { padding: 8px 7px 10px; }
  .hint { margin: 0; padding: 18px 10px; line-height: 1.5; }
  .vsplit { flex-basis: 4px; }
  .vsplit:hover { background: color-mix(in srgb, var(--accent) 55%, transparent); }

  .bar {
    min-height: 54px;
    gap: 7px;
    padding: 7px 10px;
    border-bottom-color: var(--border-soft);
    background: var(--bg-panel);
    box-shadow: 0 1px 0 color-mix(in srgb, black 18%, transparent), 0 7px 20px rgba(0,0,0,.08);
  }
  .ctl-label { margin-right: 2px; letter-spacing: .08em; }
  .div { margin: 5px 3px; background: var(--border-soft); }
  .seg.modes {
    gap: 1px;
    padding: 3px;
    border-color: var(--border-soft);
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg-elev) 76%, transparent);
    box-shadow: inset 0 1px 3px rgba(0,0,0,.16);
  }
  .chip {
    min-height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    padding: 4px 9px;
    border-radius: 7px;
    font-weight: 540;
    transition: background 100ms ease, color 100ms ease, border-color 100ms ease, transform 90ms ease;
  }
  .chip:hover { color: var(--text); }
  .chip:active { transform: translateY(1px); }
  .chip.on { box-shadow: inset 0 1px color-mix(in srgb, white 16%, transparent), 0 2px 7px color-mix(in srgb, var(--accent) 16%, transparent); }
  .viewChip svg,
  .toolbarIcon { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .filterCount { min-width: 17px; height: 17px; display: inline-flex; align-items: center; justify-content: center; margin-left: 1px; border-radius: 999px; background: var(--accent); color: var(--accent-on); font-size: 10px; font-weight: 750; }
  .ico {
    border-color: var(--border-soft);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bg-elev) 82%, transparent);
    box-shadow: inset 0 1px color-mix(in srgb, white 6%, transparent);
    transition: background 100ms ease, border-color 100ms ease, color 100ms ease, transform 90ms ease;
  }
  .ico:hover { border-color: var(--border-strong); color: var(--text); }
  .ico:active { transform: translateY(1px); }
  .btn.sm { min-height: 29px; border-radius: 8px; font-weight: 560; }
  .rightTools { gap: 6px; }
  .modeToggle { gap: 2px; padding: 3px; border-color: var(--border-soft); border-radius: 10px; box-shadow: inset 0 1px 4px rgba(0,0,0,.2); }
  .modeToggle button { min-width: 68px; padding: 6px 10px; border-radius: 7px; font-size: 12.5px; }
  .modeToggle button.on { box-shadow: inset 0 1px color-mix(in srgb, white 16%, transparent), 0 2px 8px color-mix(in srgb, var(--accent) 20%, transparent); }
  .zoom input { width: 78px; }
  .prep { min-width: 78px; }
  .castBadge { height: 27px; padding-inline: 9px; border-color: color-mix(in srgb, var(--accent) 48%, var(--border)); background: color-mix(in srgb, var(--accent) 10%, var(--bg-elev)); }

  .arrangeMenu,
  .filtermenu,
  .clearMenu,
  .castMenu,
  .pop {
    border-color: color-mix(in srgb, var(--border-strong) 72%, transparent);
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--bg-elev) 94%, transparent);
    box-shadow: var(--shadow);
    backdrop-filter: blur(22px) saturate(1.12);
  }
  .arrangeMenu,
  .filtermenu { top: 37px; padding: 14px; gap: 12px; }
  .clearMenu,
  .castMenu { top: 35px; padding: 7px; }
  .clearMenu button,
  .castRow,
  .tagrow { border-radius: 8px; }
  .pop {
    top: 58px;
    right: 10px;
    width: 396px;
    max-height: calc(100vh / var(--ui-scale) - 72px);
    overflow-y: auto;
    padding: 15px;
    gap: 11px;
  }
  .pop .grpHead { margin-top: 5px; padding-bottom: 6px; letter-spacing: .095em; }
  .pop .row { min-height: 29px; }
  .appearanceRow { align-items: flex-start !important; }
  .themeSeg { width: 250px; display: grid; grid-template-columns: 1fr 1fr; }
  .themeSeg .chip { justify-content: flex-start; }
  .themeSwatch { width: 13px; height: 13px; border: 1px solid rgba(255,255,255,.16); border-radius: 4px; box-shadow: inset 0 0 0 1px rgba(0,0,0,.12); }
  .themeSwatch.studio { background: linear-gradient(135deg, #17191d 50%, #78b9ef 50%); }
  .themeSwatch.midnight { background: linear-gradient(135deg, #0d1015 50%, #63b7f2 50%); }
  .themeSwatch.amber { background: linear-gradient(135deg, #1b1917 50%, #d8ad68 50%); }
  .themeSwatch.daylight { background: linear-gradient(135deg, #f7f9fb 50%, #2d7fc2 50%); }
  .sel { min-height: 29px; border-color: var(--border-soft); }

  .viewport {
    background:
      radial-gradient(circle at 50% 38%, color-mix(in srgb, var(--bg-elev) 9%, transparent), transparent 42%),
      var(--viewport-bg);
  }
  .welcome { gap: 0; padding: 42px; }
  .welcomeMark { display: grid; place-items: center; width: 94px; height: 94px; margin-bottom: 22px; border: 1px solid color-mix(in srgb, var(--accent) 20%, var(--border-soft)); border-radius: 26px; background: color-mix(in srgb, var(--bg-elev) 36%, transparent); box-shadow: 0 22px 60px rgba(0,0,0,.28), inset 0 1px rgba(255,255,255,.06); }
  .welcome .wIcon { border-radius: 18px; filter: drop-shadow(0 8px 16px rgba(0,0,0,.32)); }
  .welcomeCopy { max-width: 610px; }
  .welcomeEyebrow { color: var(--accent); font-size: 10.5px; font-weight: 720; letter-spacing: .14em; text-transform: uppercase; }
  .welcome h1 { margin-top: 8px; color: var(--text); font-family: var(--font-display); font-size: clamp(30px, 3.2vw, 47px); font-weight: 690; letter-spacing: -.035em; line-height: 1.04; }
  .welcome p { max-width: 560px; margin: 13px auto 0; font-size: 14px; line-height: 1.65; }
  .welcomeOpen { min-height: 39px; margin-top: 23px; padding: 8px 18px; border-radius: 10px; font-weight: 680; }
  .welcomeHints { display: flex; flex-wrap: wrap; justify-content: center; gap: 10px 18px; margin-top: 24px; color: var(--text-faint); font-size: 11.5px; }
  .welcomeHints span { display: inline-flex; align-items: center; gap: 6px; }
  .welcome kbd { min-width: 21px; padding: 1px 6px; border-color: var(--border-strong); border-radius: 6px; background: color-mix(in srgb, var(--bg-elev) 75%, transparent); box-shadow: 0 2px 0 rgba(0,0,0,.24); }

  .cell { padding-top: 9px; border-radius: 10px; transition: border-color 100ms ease, background 100ms ease, transform 100ms ease; }
  .cellclip { border-radius: 8px; background: #060708; box-shadow: 0 2px 7px rgba(0,0,0,.28); }
  .cell:hover { background: color-mix(in srgb, var(--bg-elev) 18%, var(--viewport-bg)); }
  .cell.selected { background: color-mix(in srgb, var(--select) 8%, var(--viewport-bg)); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--select) 18%, transparent); }
  .cell.active { box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 28%, transparent), 0 0 0 1px color-mix(in srgb, var(--accent) 22%, transparent); }
  .scell { border-radius: 7px; }
  .scell .cellclip { border-radius: 5px; }

  .info { min-height: 49px; gap: 9px; padding: 6px 11px; border-top-color: var(--border-soft); background: color-mix(in srgb, var(--bg-panel) 96%, transparent); box-shadow: 0 -6px 20px rgba(0,0,0,.08); }
  .activeIdentity { min-width: 0; display: flex; flex-direction: column; line-height: 1.1; }
  .info .name { max-width: 260px; font-size: 12px; font-weight: 650; }
  .info .meta { margin-top: 4px; font-size: 10px; letter-spacing: .03em; text-transform: uppercase; }
  .infoDivider { align-self: stretch; width: 1px; margin: 3px 1px; background: var(--border-soft); }
  .rate { gap: 1px; padding: 2px 5px; border: 1px solid var(--border-soft); border-radius: 999px; background: color-mix(in srgb, var(--bg-elev) 58%, transparent); }
  .star { width: 19px; height: 22px; padding: 0; font-size: 15px; transition: color 90ms ease, transform 90ms ease; }
  .star:hover { color: color-mix(in srgb, var(--star) 65%, var(--text-faint)); transform: translateY(-1px); }
  .tag { border-color: var(--border-soft); background: color-mix(in srgb, var(--bg-elev) 70%, transparent); }
  .taginput { height: 25px; border-color: var(--border-soft); background: color-mix(in srgb, var(--bg-elev) 65%, transparent); transition: width 120ms ease, border-color 120ms ease; }
  .info .counts { display: inline-flex; gap: 9px; padding: 4px 7px; border: 1px solid var(--border-soft); border-radius: 999px; background: color-mix(in srgb, var(--bg-elev) 50%, transparent); }
  .pickCount { color: var(--pick); }
  .rejectCount { color: var(--reject); }

  .askBox,
  .kbGuide,
  .padGuide { border-color: var(--border-strong); border-radius: var(--radius-xl); background: color-mix(in srgb, var(--bg-elev) 96%, transparent); box-shadow: var(--shadow); backdrop-filter: blur(24px) saturate(1.1); }
  .askBox { padding: 21px 22px 18px; }
  .askTitle { font-family: var(--font-display); font-size: 17px; }
  .askBtn { min-height: 33px; border-radius: 9px; }
  .kbBackdrop { background: rgba(0,0,0,.60); backdrop-filter: blur(5px); }

  /* Breakpoints use the whole window, while the toolbar only owns the window
     minus the folder pane. Start compacting early enough for a 1440 px laptop. */
  @media (max-width: 1600px) {
    .ctl-label { display: none; }
    .bar { gap: 5px; }
    .div { margin-inline: 1px; }
    .rightTools { gap: 4px; }
    .zoom input { width: 62px; }
    .modeToggle button { min-width: 58px; padding-inline: 8px; }
  }

  /* XPS split-screen / small window: preserve every control in two calm rows
     instead of clipping the destructive actions off-screen. */
  @media (max-width: 1400px) {
    .tree { max-width: 230px; }
    .bar { flex-wrap: wrap; align-content: center; min-height: 91px; }
    .bar > .spacer { display: none; }
    .rightTools { width: 100%; justify-content: flex-end; }
    .viewGroup { margin-right: auto; }
    .pop { top: 94px; }
  }

  @media (max-width: 760px) {
    .tree { max-width: 190px; }
    .openFolder { width: 29px; padding-inline: 0; font-size: 0; }
    .actionText { display: none; }
    .prep { min-width: 35px; }
    .viewChip span { display: none; }
    .viewChip { width: 31px; padding-inline: 0; }
    .zoom { display: none; }
    .modeToggle button { min-width: 48px; }
    .info .name { max-width: 145px; }
    .tags { display: none; }
  }

  /* Scaling changes the app's effective layout width without changing the
     browser media-query width. Give TV mode the same two-row command bar a
     genuinely narrower window receives, so zoom never hides Settings. */
  :global(html[data-ui-scale="distance"]) .bar {
    flex-wrap: wrap;
    align-content: center;
    min-height: 91px;
  }
  :global(html[data-ui-scale="distance"]) .bar > .spacer { display: none; }
  :global(html[data-ui-scale="distance"]) .rightTools { width: 100%; justify-content: flex-end; }
  :global(html[data-ui-scale="distance"]) .viewGroup { margin-right: auto; }
  :global(html[data-ui-scale="distance"]) .pop { top: 94px; }
</style>
