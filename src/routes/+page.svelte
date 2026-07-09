<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api } from "$lib/api";
  import { settings } from "$lib/settings.svelte";
  import { activity } from "$lib/activity.svelte";
  import { resetThumbs, prefetchLoupe, loaderStats } from "$lib/thumbnail-loader";
  import {
    LABELS,
    LABEL_BY_DIGIT,
    LABEL_VAR,
    type MediaItem,
    type TreeDir,
    type LibraryInfo,
    type TrashItem,
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

  let minRating = $state(0);
  let labelFilter = $state<string | null>(null);
  let flagFilter = $state<FlagFilter>("all");
  let tagFilter = $state<string | null>(null);
  let allTags = $state<[string, number][]>([]);
  let tagInput = $state("");

  // How many popover filters are active — shown as a badge.
  let activeFilterCount = $derived(
    (settings.s.typeFilter !== "all" ? 1 : 0) +
      (flagFilter !== "all" ? 1 : 0) +
      (minRating > 0 ? 1 : 0) +
      (labelFilter ? 1 : 0) +
      (tagFilter ? 1 : 0),
  );

  let dimLevel = $state(0); // 0 normal · 1 dim panels · 2 lights out
  let showInfoOverlay = $state(false);
  let settingsOpen = $state(false);
  let filtersOpen = $state(false);
  let arrangeOpen = $state(false);
  let clearOpen = $state(false);
  let libInfo = $state<LibraryInfo | null>(null);
  let trashOpen = $state(false);
  let trashItems = $state<TrashItem[]>([]);
  let editOpen = $state(false);
  let treeCollapsed = $state(false);
  // Bumped by the tree's ↻ button to make expanded folders recount their badges.
  let countsGen = $state(0);
  let folderRefreshKey = $state(0);
  let gridComp = $state<{ scrollToIndex: (i: number, center?: boolean) => void; columnCount?: () => number } | null>(null);
  let loupeComp = $state<{ togglePlay: () => void; seekBy: (d: number) => void; setInPoint?: () => void; setOutPoint?: () => void } | null>(null);
  let editComp = $state<{
    setOutputPreview?: (on: boolean) => void | Promise<void>;
    setIn?: () => void;
    setOut?: () => void;
    togglePlay?: () => void;
    seekBy?: (d: number) => void;
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
      if (/(?:[_\-. ](?:cut|reel|crop|cropped|square|landscape|mobile|edit|edited|final|export)(?:[_\-. ]?0*\d+)?)$/i.test(root)) {
        root = root.replace(/(?:[_\-. ](?:cut|reel|crop|cropped|square|landscape|mobile|edit|edited|final|export)(?:[_\-. ]?0*\d+)?)$/i, "");
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

  function relatedScore(e: RelatedEntry, entries: RelatedEntry[]): number {
    const it = e.item;
    let score = 0;
    if (it.flag === "pick") score += 1000;
    if (it.flag === "reject") score -= 800;
    score += it.rating * 80;
    if (it.label) score += 10;
    if (e.stem.relation === "original") score += 35;
    if (it.kind === "image") score += 24;
    else if (it.kind === "raw") score += hasRawJpeg(entries) ? 18 : 22;
    else if (it.kind === "video") score += 14;
    if (e.stem.relation === "subclip") score -= 12;
    if (e.stem.relation === "edit") score -= 8;
    score -= e.order / 100000;
    return score;
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
    return {
      id,
      entries: ordered,
      items: ordered.map((e) => e.item),
      representative: mother,
      mother,
      badges,
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
    const rootBuckets = new Map<string, RelatedEntry[]>();
    for (const e of entries) {
      const key = relatedKey(e.stem);
      rootBuckets.set(key, [...(rootBuckets.get(key) ?? []), e]);
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
      burstBuckets.set(c.key, [...(burstBuckets.get(c.key) ?? []), { e, n: c.n }]);
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
    if (minRating > 0) arr = arr.filter((i) => i.rating >= minRating);
    if (labelFilter) arr = arr.filter((i) => i.label === labelFilter);
    if (tagFilter) arr = arr.filter((i) => i.tags.includes(tagFilter!));
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

  type GridSection = { label: string; count: number; level?: 1 | 2; cellCount?: number };

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
        primary = { label: sectionPartLabel(anchor, primaryBy) || "All media", count: 0, level: 1, cellCount: 0 };
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
  let rejectedCount = $derived(items.filter((i) => i.flag === "reject").length);
  let pickCount = $derived(items.filter((i) => i.flag === "pick").length);
  let stripCell = $derived(Math.max(64, settings.s.filmstripSize - 24));
  // Thumbnail decode sizes, matched to how big the cells are actually drawn.
  let gridThumbTier = $derived(tierFor(settings.s.gridSize));
  let stripThumbTier = $derived(tierFor(stripCell));

  $effect(() => {
    if (activeIndex > view.length - 1) activeIndex = Math.max(0, view.length - 1);
  });

  onMount(async () => {
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
    // Reopen the last folder AND land on the last photo we were looking at.
    if (settings.s.lastDir)
      openFolder(settings.s.lastDir, { selectPath: settings.s.lastActivePath });
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
        `MEM ${tag} ${heap} memo=${s.memo} loupe=${s.loupe} pending=${s.pending} queue=${s.queue} inflight=${s.inflight}`,
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
    const order = baseView.map((i) => i.path);
    const tier = gridThumbTier;
    setTimeout(() => {
      if (currentDir === dir) api.warmThumbnails(order, tier);
    }, 500);
    logMem(`open ${basename(dir)} n=${items.length}`);
    refreshTags();
    // Index real capture dates in the background — only when a date-driven view
    // needs them (sort-by-capture or month grouping). Cached after the first pass.
    maybeFetchCaptures();
  }

  /** Whether the current view depends on real capture dates. */
  let needCaptures = $derived(DATE_GROUPS.has(settings.s.groupBy) || DATE_GROUPS.has(settings.s.subgroupBy) || settings.s.sortBy === "capture" || hasBurstLikeNames(items));

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
    if (selected.size > 1 && selected.has(item.path)) return targetPaths();
    return [item.path];
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
  async function prepareFolder() {
    if (!currentDir || preparing || !baseView.length) return;
    preparing = true;
    prepared = false;
    const dir = currentDir;
    // Focus previews are the big (1920px) renders; the small grid thumbs are
    // already warmed on folder-open. We chunk the work so the button can show
    // real progress + a time estimate instead of an opaque spinner.
    const paths = baseView.filter((i) => i.kind !== "other").map((i) => i.path);
    prepTotal = paths.length;
    prepDone = 0;
    prepEta = "";
    const t0 = performance.now();
    const CHUNK = 16;
    try {
      for (let i = 0; i < paths.length; i += CHUNK) {
        if (currentDir !== dir) break; // folder switched — abandon
        await api.warmThumbnails(paths.slice(i, i + CHUNK), LOUPE_MAX);
        prepDone = Math.min(paths.length, i + CHUNK);
        const elapsed = performance.now() - t0;
        const remain = (elapsed / prepDone) * (paths.length - prepDone);
        prepEta = remain > 1500 ? `~${Math.ceil(remain / 1000)}s` : "almost done";
        // Mirror into the global activity chip (visible from any view).
        activity.local("prepare", "Preparing full-size previews", prepDone, prepTotal);
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

  function rate(r: number) {
    const ts = targets();
    if (ts.length === 1) {
      ts[0].rating = ts[0].rating === r ? 0 : r;
      api.setRating(ts[0].path, ts[0].rating).catch(() => {});
    } else if (ts.length > 1) {
      for (const it of ts) it.rating = r;
      api.setRatingMany(ts.map((i) => i.path), r).catch(() => {});
    }
  }
  function label(key: string) {
    const ts = targets();
    if (ts.length === 1) {
      ts[0].label = ts[0].label === key ? null : key;
      api.setLabel(ts[0].path, ts[0].label).catch(() => {});
    } else if (ts.length > 1) {
      for (const it of ts) it.label = key;
      api.setLabelMany(ts.map((i) => i.path), key).catch(() => {});
    }
  }
  function flag(f: "pick" | "reject") {
    const ts = targets();
    if (ts.length === 1) {
      ts[0].flag = ts[0].flag === f ? null : f;
      api.setFlag(ts[0].path, ts[0].flag).catch(() => {});
    } else if (ts.length > 1) {
      for (const it of ts) it.flag = f;
      api.setFlagMany(ts.map((i) => i.path), f).catch(() => {});
    }
  }
  function unset() {
    const ts = targets();
    if (!ts.length) return;
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
  }

  function clearRatings() {
    const ts = targets();
    if (!ts.length) return;
    for (const it of ts) it.rating = 0;
    const paths = ts.map((i) => i.path);
    (ts.length === 1 ? api.setRating(paths[0], 0) : api.setRatingMany(paths, 0)).catch(() => {});
  }

  function clearLabels() {
    const ts = targets();
    if (!ts.length) return;
    for (const it of ts) it.label = null;
    const paths = ts.map((i) => i.path);
    (ts.length === 1 ? api.setLabel(paths[0], null) : api.setLabelMany(paths, null)).catch(() => {});
  }

  function clearFlags() {
    const ts = targets();
    if (!ts.length) return;
    for (const it of ts) it.flag = null;
    const paths = ts.map((i) => i.path);
    (ts.length === 1 ? api.setFlag(paths[0], null) : api.setFlagMany(paths, null)).catch(() => {});
  }

  async function clearTagsOnTargets() {
    const ts = targets();
    if (!ts.length) return;
    const paths = ts.map((i) => i.path);
    const tags = [...new Set(ts.flatMap((i) => i.tags))];
    for (const it of ts) it.tags = [];
    for (const tag of tags) await api.removeTag(paths, tag).catch(() => {});
    refreshTags();
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
    for (const it of ts) if (!it.tags.includes(tag)) it.tags = [...it.tags, tag];
    tagInput = "";
    await api.addTag(ts.map((i) => i.path), tag).catch(() => {});
    refreshTags();
  }
  async function removeTagFromActive(tag: string) {
    if (!active) return;
    active.tags = active.tags.filter((t) => t !== tag);
    await api.removeTag([active.path], tag).catch(() => {});
    refreshTags();
  }

  function selectAllFiltered() {
    selected = new Set(view.map((i) => i.path));
    selectionAnchor = view[activeIndex]?.path ?? view[0]?.path ?? null;
  }
  function rejectSelected() {
    const sel = targets();
    if (!sel.length) return;
    const next = sel.every((i) => i.flag === "reject") ? null : "reject";
    for (const it of sel) it.flag = next;
    api.setFlagMany(sel.map((i) => i.path), next).catch(() => {});
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
              icon: "E",
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
      {
        label: "Export as JPEG…" + sfx,
        icon: "⇩",
        disabled: !ts.some((i) => i.kind === "image" || i.kind === "raw"),
        action: () => exportTargets(),
      },
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
        {
          label: "Paste moved files here",
          icon: "⇥",
          disabled: cutPaths.length === 0,
          action: () => movePathsTo(cutPaths, path),
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
    // "folder" -> the active drive's _FoxCull/recycle (recoverable in-app Trash);
    // "recycle" → the OS Recycle Bin / Trash.
    await api.disposeRejected(paths, settings.s.deleteMode);
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
  function startStripResize(e: PointerEvent) {
    e.preventDefault();
    const right = settings.s.filmstripPos === "right";
    const start = right ? e.clientX : e.clientY;
    const startS = settings.s.filmstripSize;
    const move = (ev: PointerEvent) => {
      const d = right ? start - ev.clientX : start - ev.clientY;
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
  let fullscreen = $state(false);
  let fsPrevView: ViewMode = "grid";
  async function toggleFullscreen() {
    fullscreen = !fullscreen;
    try {
      await getCurrentWindow().setFullscreen(fullscreen);
    } catch {
      // wayland/odd WMs can refuse — the chrome still hides, which is most of it
    }
    if (fullscreen) {
      fsPrevView = viewMode;
      if (!editOpen && active) setView("loupe");
    } else {
      if (!editOpen) setView(fsPrevView);
    }
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
    if (editOpen) {
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
    // Video playback keys (Focus mode, active clip): Space toggles play/pause,
    // and , / . scrub the clip (Shift+←/→ is reserved for extending the item
    // selection, just like the grid, so the filmstrip behaves like Lightroom).
    if (viewMode === "loupe" && active?.kind === "video" && loupeComp) {
      if (e.key === " " || e.code === "Space") { loupeComp.togglePlay(); e.preventDefault(); return; }
      if (e.key === "[") { loupeComp.setInPoint?.(); e.preventDefault(); return; }
      if (e.key === "]") { loupeComp.setOutPoint?.(); e.preventDefault(); return; }
      if (e.key === "," || e.key === "<") { loupeComp.seekBy(-5); e.preventDefault(); return; }
      if (e.key === "." || e.key === ">") { loupeComp.seekBy(5); e.preventDefault(); return; }
    }
    const delta = navDelta(e.key);
    if (delta) {
      // Shift+←/→ extends the selection to the neighbouring item everywhere —
      // grid, details AND the Focus filmstrip (previously it scrubbed video).
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
    if (k === "f") { toggleFullscreen(); return; }
    if (k === "l") { dimLevel = (dimLevel + 1) % 3; return; }
    if (k === "g") { setView("grid"); return; }
    if (k === "d") { setView("details"); return; }
    if (e.key >= "1" && e.key <= "5") { rate(Number(e.key)); return; }
    if (e.key === "`") { rate(0); return; }
    if (e.key in LABEL_BY_DIGIT) { label(LABEL_BY_DIGIT[e.key]); return; }
    if (k === "x") { flag("reject"); return; }
    if (k === "p") { flag("pick"); return; }
    if (k === "u") { unset(); return; }
  }

  // Mouse back/forward buttons → a simple Focus⇄grid toggle (no history stack):
  // Forward (button 4) on a selected shot opens Focus; Back (button 3) from Focus
  // returns to the grid (which scroll-restores to that shot). preventDefault stops
  // the webview trying to navigate its history and blanking the single-page app.
  function onmouseup(e: MouseEvent) {
    if (editOpen) return;
    if (e.button === 3) {
      if (viewMode === "loupe") { setView("grid"); e.preventDefault(); }
    } else if (e.button === 4) {
      if (viewMode !== "loupe" && active) { setView("loupe"); e.preventDefault(); }
    }
  }
</script>

<svelte:window {onkeydown} {onmouseup} oncontextmenu={onGlobalContextMenu} />

{#snippet gridCell(item: MediaItem, i: number)}
  {@const rel = relatedFor(item)}
  <button
    class="cell"
    class:active={i === activeIndex}
    class:selected={selected.has(item.path)}
    class:reject={item.flag === "reject"}
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
    draggable={true}
    ondragstart={(e) => beginMediaDrag(e, item, i)}
    ondragend={endMediaDrag}
    title={relatedTitle(item)}
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
    <Thumb {item} size={gridThumbTier} />
    <span class="ov">
      {#if rel}
        <span class="rel-badges">
          {#each rel.group.badges.slice(0, 2) as b}
            <span>{b}</span>
          {/each}
        </span>
        <span class="rel-role">{relatedRoleLabel(rel)}</span>
        {#if isCollapsedRepresentative(item, rel)}
          <span class="rel-count">{rel.count}</span>
        {/if}
      {/if}
      {#if item.label}<span class="lbl-dot" style="background:var({LABEL_VAR[item.label]})"></span>{/if}
      {#if item.flag === "reject"}<span class="fl x">✕</span>{/if}
      {#if item.flag === "pick"}<span class="fl pick">✓</span>{/if}
      {#if item.rating > 0}<span class="stars">{"★".repeat(item.rating)}</span>{/if}
      {#if item.tags.length}<span class="tagdot" title={item.tags.join(", ")}>🏷</span>{/if}
    </span>
  </button>
{/snippet}

{#snippet stripCellSnip(item: MediaItem, i: number)}
  {@const rel = relatedFor(item)}
  <button
    class="scell"
    class:active={i === activeIndex}
    class:selected={selected.has(item.path)}
    class:reject={item.flag === "reject"}
    class:related={!!rel}
    class:rel-start={!!rel && rel.index === 0}
    class:rel-mid={!!rel && rel.index > 0 && rel.index < rel.count - 1}
    class:rel-end={!!rel && rel.index === rel.count - 1}
    class:rel-collapsed={isCollapsedRepresentative(item, rel)}
    onclick={(e) => gridCellClick(e, i)}
    ondblclick={() => { setActiveTo(i); setView("loupe"); }}
    oncontextmenu={(e) => openContextMenu(e, item, i)}
    title={rel ? relatedTitle(item) : item.name}
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
    <Thumb {item} size={stripThumbTier} />
    {#if rel}
      <span class="s-rel">{shortRelatedBadge(rel)}</span>
      <span class="s-role">{relatedRoleLabel(rel).slice(0, 1)}</span>
      {#if isCollapsedRepresentative(item, rel)}<span class="s-count">{rel.count}</span>{/if}
    {/if}
    {#if item.label}<span class="s-lbl" style="background:var({LABEL_VAR[item.label]})"></span>{/if}
    {#if item.rating > 0}<span class="s-stars">{"★".repeat(item.rating)}</span>{/if}
    {#if item.flag === "reject"}<span class="s-x">✕</span>{/if}
    {#if item.flag === "pick"}<span class="s-pick">✓</span>{/if}
  </button>
{/snippet}

<div class="app" data-dim={dimLevel} class:fs={fullscreen}>
  <!-- ░ left: drives + folder tree ░ -->
  {#if !treeCollapsed}
    <aside class="tree" style="width:{settings.s.treeWidth}px">
      <div class="tree-head">
        <button class="ico sm" onclick={() => (treeCollapsed = true)} title="Hide folders" aria-label="Hide folders">
          <span class="sidebarGlyph" aria-hidden="true"><span></span></span>
        </button>
        <span class="brand">Folders</span>
        <div class="tree-actions">
          <button
            class="ico sm"
            class:spin={recounting}
            onclick={refreshCounts}
            title="Refresh folders and current view"
            aria-label="Refresh folders and current view"
          ><span class="refreshGlyph" aria-hidden="true"></span></button>
          <button class="btn sm" onclick={openFolderPicker} title="Jump to a folder">Open</button>
        </div>
      </div>
      <ActivityBar />
      <div class="tree-body">
        {#if drives.length}
          {#each drives as d (d.path)}
            <TreeNode node={d} {currentDir} onselect={openFolder} onmove={(dest) => movePathsTo(draggingPaths, dest)} onfoldercontext={openFolderContextMenu} {countsGen} />
          {/each}
        {:else}
          <p class="hint">No drives detected.</p>
        {/if}
      </div>
    </aside>
    <div class="vsplit" role="separator" tabindex="-1" onpointerdown={startTreeResize}></div>
  {:else}
    <button class="treeRestore ico sm" onclick={() => (treeCollapsed = false)} title="Show folders" aria-label="Show folders">
      <span class="sidebarGlyph closed" aria-hidden="true"><span></span></span>
    </button>
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
          <button class="chip" class:on={viewMode === "grid" && !editOpen} onclick={() => setView("grid")}>Grid</button>
          <button class="chip" class:on={viewMode === "details" && !editOpen} onclick={() => setView("details")}>Details</button>
          <button class="chip" class:on={viewMode === "loupe" && !editOpen} onclick={() => setView("loupe")}>Focus</button>
        </div>
      </div>

      <span class="div"></span>

      <!-- sort + grouping + stack display -->
      <div class="grp arrange">
        <button
          class="chip arrangeBtn"
          class:on={arrangeOpen || settings.s.groupBy !== "none" || settings.s.subgroupBy !== "none" || settings.s.relatedMode === "collapsed"}
          onclick={() => (arrangeOpen = !arrangeOpen)}
          title="Sort, group, subgroup and related-stack display"
        >
          Arrange
        </button>
        {#if arrangeOpen}
          <div class="arrangeMenu">
            <div class="fm-row">
              <span class="fm-lbl">Sort</span>
              <select class="sel wide" title="Sort order" bind:value={settings.s.sortBy} onchange={() => { settings.set({ sortBy: settings.s.sortBy }); maybeFetchCaptures(); }}>
                <option value="name">Name</option>
                <option value="date">Modified</option>
                <option value="capture">Capture date</option>
                <option value="type">Type</option>
                <option value="size">Size</option>
              </select>
              <button class="ico" title="Sort direction" onclick={() => settings.set({ sortDir: settings.s.sortDir === "asc" ? "desc" : "asc" })}>
                {settings.s.sortDir === "asc" ? "↑" : "↓"}
              </button>
            </div>
            <div class="fm-row">
              <span class="fm-lbl">Group</span>
              <select class="sel wide" title="Primary grouped section" bind:value={settings.s.groupBy} onchange={() => { settings.set({ groupBy: settings.s.groupBy }); maybeFetchCaptures(); }}>
                <option value="none">No groups</option>
                <option value="folder">Folder</option>
                <option value="type">Type</option>
                <option value="year">Year</option>
                <option value="month">Month</option>
                <option value="week">Week</option>
              </select>
            </div>
            <div class="fm-row">
              <span class="fm-lbl">Subgroup</span>
              <select class="sel wide" title="Nested second grouping level" bind:value={settings.s.subgroupBy} onchange={() => { settings.set({ subgroupBy: settings.s.subgroupBy }); maybeFetchCaptures(); }}>
                <option value="none">None</option>
                <option value="folder">Folder</option>
                <option value="type">Type</option>
                <option value="year">Year</option>
                <option value="month">Month</option>
                <option value="week">Week</option>
              </select>
            </div>
            <div class="fm-row">
              <span class="fm-lbl">Stacks</span>
              <div class="seg stackSeg" title="Show detected related files as separate items or folded groups">
                <button class="chip" class:on={settings.s.relatedMode === "expanded"} disabled={relatedGroupCount === 0} onclick={() => setRelatedMode("expanded")}>Open</button>
                <button class="chip" class:on={settings.s.relatedMode === "collapsed"} disabled={relatedGroupCount === 0} onclick={collapseAllRelated}>Fold{relatedHiddenCount ? ` ${relatedHiddenCount}` : ""}</button>
              </div>
            </div>
          </div>
        {/if}
      </div>

      <span class="div"></span>

      <!-- media, culling and metadata filters -->
      <div class="grp filterwrap">
        <button class="chip" class:on={filtersOpen || activeFilterCount > 0} onclick={() => (filtersOpen = !filtersOpen)} title="Media, culling and metadata filters">
          Filters{activeFilterCount ? ` ${activeFilterCount}` : ""}
        </button>
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
                {#each [1, 2, 3, 4, 5] as n}
                  <button class="starf" class:on={minRating >= n} onclick={() => (minRating = minRating === n ? 0 : n)} title="{n}+ stars">★</button>
                {/each}
                {#if minRating > 0}<button class="fm-clr" onclick={() => (minRating = 0)}>clear</button>{/if}
              </div>
            </div>
            <div class="fm-row">
              <span class="fm-lbl">Label</span>
              <div class="seg">
                <button class="dot any" class:on={labelFilter === null} onclick={() => (labelFilter = null)} title="Any label">∅</button>
                {#each LABELS as l}
                  <button class="dot" class:on={labelFilter === l.key} style="background:var({l.varName})" title={l.name} aria-label={l.name} onclick={() => (labelFilter = labelFilter === l.key ? null : l.key)}></button>
                {/each}
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
            <div class="fm-row">
              <span class="fm-lbl">Scope</span>
              <button class="chip" class:on={settings.s.includeSub} onclick={toggleSub} title="Include photos from subfolders">⊞ Include subfolders</button>
            </div>
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
        <button
          class="chip scrubToggle"
          class:on={settings.s.liveScrub}
          onclick={() => settings.set({ liveScrub: !settings.s.liveScrub })}
          title="Toggle thumbnail and timeline hover scrubbing. Off keeps video previews static and avoids scrub-strip generation."
        >
          Live Scrub {settings.s.liveScrub ? "On" : "Off"}
        </button>
        {#if !editOpen}
        <!-- actions (top-right) -->
        <button
          class="btn sm prep"
          class:on={preparing || prepared}
          onclick={prepareFolder}
          disabled={!baseView.length || preparing}
          title={"Prepare · full-quality Focus previews for this whole folder.\n\nWhat it does: decodes and caches every shot's large preview up front.\nWhen to use it: before reviewing a folder in Focus view, so flipping shot-to-shot is instant with no loading blur.\nHow: click once — it runs in the background (progress shown here) and only needs doing once per folder. Grid thumbnails are already cached when a folder opens; this is the extra step for the big Focus previews."}
        >
          {#if preparing}<span class="prep-fill" style="width:{prepPct}%"></span>{/if}
          <span class="prep-lbl">
            <span class="prep-ico" aria-hidden="true">
              {#if preparing}◌{:else if prepared}✓{:else}<svg viewBox="0 0 24 24" width="11" height="11" fill="currentColor" aria-hidden="true"><path d="M13 2 4.5 13.2c-.4.5 0 1.3.7 1.3H11l-1.4 8.2c-.1.7.8 1.1 1.2.5L19.5 12c.4-.5 0-1.3-.7-1.3H12.9L14.2 2.6c.1-.7-.8-1.1-1.2-.6Z"/></svg>{/if}
            </span>
            {#if preparing}{prepPct}%{prepEta ? ` ${prepEta}` : ""}{:else if prepared}Ready{:else}Prepare{/if}
          </span>
        </button>
        <button class="btn sm danger" onclick={rejectSelected} disabled={actionTargets.length === 0} title="Toggle rejected on the active item or selection">
          {allTargetsRejected ? "Unreject" : "Reject"}{selected.size > 1 ? ` ${selected.size}` : ""}
        </button>
        <div class="grp clearWrap">
          <button class="btn sm" class:on={clearOpen} onclick={() => (clearOpen = !clearOpen)} disabled={actionTargets.length === 0} title="Clear ratings, labels, flags or tags from the active item or selection">Clear</button>
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
          <span class="hold-lbl">Delete{rejectedCount ? ` ${rejectedCount}` : ""}</span>
        </button>
        {/if}
        <div class="modeToggle" title="Workspace mode">
          <button class:on={!editOpen} onclick={() => (editOpen = false)}>Library</button>
          <button class:on={editOpen} onclick={openEditMode} disabled={!currentDir}>Edit</button>
        </div>
        <button class="ico gear" class:on={settingsOpen} onclick={() => (settingsOpen = !settingsOpen)} title="Settings">...</button>
      </div>
    </div>

    <!-- settings popover -->
    {#if settingsOpen}
      <div class="pop">
        <div class="row"><span>Theme</span>
          <div class="seg">
            <button class="chip" class:on={settings.s.theme === "dark"} onclick={() => settings.set({ theme: "dark" })}>Dark</button>
            <button class="chip" class:on={settings.s.theme === "neutral"} onclick={() => settings.set({ theme: "neutral" })} title="Lightroom-like neutral graphite chrome; the photo stage stays neutral">Neutral</button>
            <button class="chip" class:on={settings.s.theme === "warm"} onclick={() => settings.set({ theme: "warm" })} title="Warm late-night graphite chrome for yellow-lamp work">Warm</button>
            <button class="chip" class:on={settings.s.theme === "light"} onclick={() => settings.set({ theme: "light" })}>Light</button>
          </div>
        </div>
        <div class="row"><span>Video autoplay</span>
          <div class="seg">
            <button class="chip" class:on={settings.s.videoAutoplay} onclick={() => settings.set({ videoAutoplay: true })}>On</button>
            <button class="chip" class:on={!settings.s.videoAutoplay} onclick={() => settings.set({ videoAutoplay: false })}>Off</button>
          </div>
        </div>
        <div class="row"><span>Filmstrip</span>
          <div class="seg">
            {#each [["bottom", "Bottom"], ["right", "Right"], ["hidden", "Off"]] as [v, l]}
              <button class="chip" class:on={settings.s.filmstripPos === v} onclick={() => settings.set({ filmstripPos: v as typeof settings.s.filmstripPos })}>{l}</button>
            {/each}
          </div>
        </div>
        <div class="row"><span>Related groups</span>
          <div class="seg">
            <button class="chip" class:on={settings.s.relatedMode === "expanded"} onclick={() => setRelatedMode("expanded")}>Open</button>
            <button class="chip" class:on={settings.s.relatedMode === "collapsed"} onclick={collapseAllRelated}>Fold</button>
          </div>
        </div>
        <div class="row"><span>Live Scrub</span>
          <div class="seg">
            <button class="chip" class:on={settings.s.liveScrub} onclick={() => settings.set({ liveScrub: true })}>On</button>
            <button class="chip" class:on={!settings.s.liveScrub} onclick={() => settings.set({ liveScrub: false })}>Off</button>
          </div>
        </div>
        <div class="row"><span>On delete</span>
          <div class="seg">
            <button class="chip" class:on={settings.s.deleteMode === "folder"} onclick={() => settings.set({ deleteMode: "folder" })} title="Move to this drive's _FoxCull recycle folder - recoverable in the in-app Trash">In-app Trash</button>
            <button class="chip" class:on={settings.s.deleteMode === "recycle"} onclick={() => settings.set({ deleteMode: "recycle" })} title="Send to the operating system's Recycle Bin / Trash">System Recycle Bin</button>
          </div>
        </div>
        <div class="row"><span>Trash</span>
          <button class="btn sm" onclick={() => { settingsOpen = false; openTrash(); }}>🗑 Open Trash…</button>
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
        <div class="row hintrow">Each drive keeps its own catalog, preview cache &amp; recycle in a <code>_FoxCull</code> folder. Press <kbd>F</kbd> full screen · <kbd>L</kbd> dim · <kbd>G</kbd> grid · <kbd>D</kbd> details.</div>
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

    <!-- body: viewport (+ optional right filmstrip) -->
    <div class="body">
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
            <h1>FoxCull</h1>
            <p>Pick a folder on the left to start culling. Browse-in-place — nothing is imported or changed.</p>
          </div>
        {:else if editOpen}
          <EditStudio {active} {selectedItems} sourceItems={items} currentDir={currentDir} recursive={settings.s.includeSub} refreshKey={folderRefreshKey} bind:this={editComp} />
        {:else if view.length === 0}
          <div class="welcome"><p>Nothing here matches the current filters.</p></div>
        {:else if viewMode === "loupe"}
          <Loupe item={active} showInfo={showInfoOverlay} onchanged={refreshAfterMediaOutput} bind:this={loupeComp} />
        {:else if viewMode === "details"}
          <DetailsView
            items={view}
            {activeIndex}
            {selected}
            onrowclick={gridCellClick}
            onrowdblclick={(i) => { setActiveTo(i); setView("loupe"); }}
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

      {#if !editOpen && settings.s.filmstripPos === "right" && view.length}
        <div class="vsplit" role="separator" tabindex="-1" onpointerdown={startStripResize}></div>
        <aside class="rstrip" style="width:{settings.s.filmstripSize}px">
          <VirtualStrip items={view} {activeIndex} orientation="v" cellSize={stripCell} cell={stripCellSnip} />
        </aside>
      {/if}
    </div>

    <!-- active-item info bar -->
    {#if active && !editOpen}
      <div class="info">
        <span class="name" title={active.path}>{active.name}</span>
        <span class="meta">{active.kind} · {activeIndex + 1}/{view.length}</span>
        <div class="rate">
          {#each [1, 2, 3, 4, 5] as n}
            <button class="star" class:on={active.rating >= n} onclick={() => rate(n)}>★</button>
          {/each}
        </div>
        {#each LABELS as l}
          <button class="dot sm" class:on={active.label === l.key} style="background:var({l.varName})" title={l.name} aria-label={l.name} onclick={() => label(l.key)}></button>
        {/each}
        <button class="btn sm" class:on={active.flag === "pick"} onclick={() => flag("pick")}>Pick</button>
        <button class="btn sm danger" class:on={active.flag === "reject"} onclick={() => flag("reject")}>{active.flag === "reject" ? "Unreject" : "Reject"}</button>

        <!-- tags -->
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
        <button class="ico" title="Reveal in file manager" onclick={() => active && api.reveal(active.path)}>⤴</button>
        <span class="counts">✓ {pickCount} · ✕ {rejectedCount}</span>
      </div>
    {/if}

    <!-- bottom filmstrip -->
    {#if !editOpen && settings.s.filmstripPos === "bottom" && view.length}
      <div class="hsplit" role="separator" tabindex="-1" onpointerdown={startStripResize} title="Drag to resize"><span class="grip"></span></div>
      <div class="bstrip" style="height:{settings.s.filmstripSize}px">
        <VirtualStrip items={view} {activeIndex} orientation="h" cellSize={stripCell} cell={stripCellSnip} />
      </div>
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
  /* Full-screen mode (F): nothing but the photo stage — every panel, bar and
     strip disappears and the viewport fills the (OS-fullscreened) window. */
  .app.fs .tree,
  .app.fs .vsplit,
  .app.fs .hsplit,
  .app.fs .bar,
  .app.fs .banner,
  .app.fs .info,
  .app.fs .bstrip,
  .app.fs .rstrip,
  .app.fs .pop,
  .app.fs .treeRestore { display: none; }
  .tree { display: flex; flex-direction: column; background: var(--bg-panel); border-right: 1px solid var(--border); flex: 0 0 auto; min-width: 0; transition: width 0.14s ease; }
  .tree-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 45px; padding: 9px 10px; border-bottom: 1px solid var(--border); }
  .treeRestore {
    position: absolute;
    z-index: 80;
    left: 8px;
    top: 8px;
    box-shadow: var(--shadow);
  }
  .tree-actions { display: flex; align-items: center; gap: 6px; }
  .ico.sm { width: 26px; height: 26px; font-size: 13px; }
  .ico.spin { animation: spin 0.5s linear; color: var(--accent); border-color: var(--accent); }
  @keyframes spin { to { transform: rotate(360deg); } }
  .brand { font-weight: 700; }
  .tree-body { overflow-y: auto; padding: 6px; flex: 1; }
  .hint { padding: 10px; color: var(--text-faint); font-size: 12.5px; }

  .vsplit { flex: 0 0 5px; cursor: col-resize; background: transparent; }
  .vsplit:hover { background: color-mix(in srgb, var(--accent) 40%, transparent); }
  .hsplit { flex: 0 0 8px; cursor: row-resize; display: flex; align-items: center; justify-content: center; background: var(--bg-panel); border-top: 1px solid var(--border); }
  .hsplit .grip { width: 46px; height: 3px; border-radius: 3px; background: var(--text-faint); opacity: 0.4; }
  .hsplit:hover { background: color-mix(in srgb, var(--accent) 22%, var(--bg-panel)); }
  .hsplit:hover .grip { opacity: 0.9; background: var(--accent); }

  .center { display: flex; flex-direction: column; flex: 1; min-width: 0; height: 100vh; }

  .bar { position: relative; display: flex; align-items: center; gap: 8px; min-height: 48px; padding: 6px 10px; border-bottom: 1px solid var(--border); background: var(--bg-panel); flex-wrap: nowrap; }
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
  .sidebarGlyph {
    position: relative;
    display: block;
    width: 15px;
    height: 14px;
    border-left: 2px solid currentColor;
    opacity: 0.9;
  }
  .sidebarGlyph::before,
  .sidebarGlyph::after,
  .sidebarGlyph span {
    content: "";
    position: absolute;
    left: 5px;
    right: 0;
    height: 2px;
    border-radius: 2px;
    background: currentColor;
  }
  .sidebarGlyph::before { top: 1px; }
  .sidebarGlyph span { top: 6px; }
  .sidebarGlyph::after { bottom: 1px; }
  .sidebarGlyph.closed { transform: scaleX(-1); }
  .refreshGlyph {
    position: relative;
    display: block;
    width: 15px;
    height: 15px;
    border: 2px solid currentColor;
    border-right-color: transparent;
    border-radius: 50%;
  }
  .refreshGlyph::after {
    content: "";
    position: absolute;
    right: -1px;
    top: -3px;
    width: 0;
    height: 0;
    border-left: 5px solid currentColor;
    border-top: 4px solid transparent;
    border-bottom: 4px solid transparent;
    transform: rotate(18deg);
  }
  .chip { padding: 4px 9px; border-radius: 6px; font-size: 12px; color: var(--text-dim); border: 1px solid transparent; white-space: nowrap; }
  .chip:hover { background: var(--bg-hover); }
  .chip.on { background: var(--accent); color: var(--accent-on); }
  .chip.rej.on { background: var(--reject); border-color: var(--reject); }
  .chip.pick.on { background: var(--pick); border-color: var(--pick); }
  .scrubToggle { border-color: var(--border); background: var(--bg-elev); }
  .starf { font-size: 14px; color: var(--text-faint); padding: 0 1px; }
  .starf.on { color: var(--star); }
  .dot { width: 14px; height: 14px; border-radius: 3px; border: 1px solid rgba(0,0,0,0.25); opacity: 0.5; }
  .dot.any { background: var(--bg-elev); color: var(--text-faint); font-size: 10px; line-height: 12px; opacity: 1; }
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
  .prep { position: relative; overflow: hidden; min-width: 96px; text-align: center; }
  .prep-fill { position: absolute; left: 0; top: 0; bottom: 0; background: color-mix(in srgb, var(--accent) 30%, transparent); transition: width 0.2s ease; }
  .prep-lbl { position: relative; z-index: 1; display: inline-flex; align-items: center; justify-content: center; gap: 5px; white-space: nowrap; }
  .prep-ico { font-size: 13px; line-height: 1; color: var(--accent); display: inline-flex; align-items: center; }
  .prep-ico svg { display: block; }

  .div { flex: 0 0 auto; align-self: stretch; width: 1px; margin: 2px 4px; background: var(--border); }
  .arrange,
  .filterwrap { position: relative; }
  .arrangeMenu,
  .filtermenu { position: absolute; top: 34px; left: 0; z-index: 30; width: 290px; background: var(--bg-elev); border: 1px solid var(--border); border-radius: 10px; box-shadow: var(--shadow); padding: 11px; display: flex; flex-direction: column; gap: 11px; }
  .arrangeMenu { width: 315px; }
  .stackSeg { padding: 2px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg-panel); }
  .fm-row { display: flex; align-items: center; gap: 10px; }
  .fm-row.col { flex-direction: column; align-items: stretch; gap: 5px; }
  .fm-lbl { flex: 0 0 46px; font-size: 12px; color: var(--text-dim); }
  .fm-tags { display: flex; flex-direction: column; gap: 2px; max-height: 200px; overflow-y: auto; }
  .fm-clr { font-size: 11px; color: var(--text-faint); padding: 0 4px; margin-left: 4px; }
  .fm-clr:hover { color: var(--text); }
  .tagrow { display: flex; justify-content: space-between; gap: 10px; width: 100%; text-align: left; padding: 6px 9px; border-radius: 6px; font-size: 12.5px; color: var(--text); }
  .tagrow:hover { background: var(--bg-hover); }
  .tagrow.on { background: var(--accent); color: var(--accent-on); }
  .tagrow .cnt { color: var(--text-faint); }
  .tagrow.on .cnt { color: var(--accent-on); }
  .tagempty { padding: 8px 9px; color: var(--text-faint); font-size: 12px; margin: 0; }

  .hold { position: relative; overflow: hidden; }
  .hold-fill { position: absolute; left: 0; top: 0; bottom: 0; background: color-mix(in srgb, var(--reject) 35%, transparent); }
  .hold-lbl { position: relative; z-index: 1; }
  .clearWrap { position: relative; }
  .clearMenu { position: absolute; top: 32px; right: 0; z-index: 35; width: 170px; padding: 6px; display: grid; gap: 2px; border: 1px solid var(--border); border-radius: 9px; background: var(--bg-elev); box-shadow: var(--shadow); }
  .clearMenu button { text-align: left; padding: 7px 9px; border-radius: 6px; color: var(--text-dim); font-size: 12px; }
  .clearMenu button:hover { background: var(--bg-hover); color: var(--text); }
  .clearMenu .dangerText { color: var(--reject); }

  .pop { position: absolute; right: 10px; top: 46px; z-index: 30; background: var(--bg-elev); border: 1px solid var(--border); border-radius: 10px; box-shadow: var(--shadow); padding: 12px; width: 340px; display: flex; flex-direction: column; gap: 10px; }
  .pop .row { display: flex; align-items: center; justify-content: space-between; gap: 10px; font-size: 13px; }
  .pop .row.sub { padding-left: 6px; flex-wrap: nowrap; }
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
  .bstrip { flex: 0 0 auto; }

  .welcome { height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; color: var(--text-dim); text-align: center; padding: 24px; }
  .welcome h1 { font-size: 28px; margin: 0; }

  /* Every tile reserves a thin top band so the golden stack line (when present)
     sits above the thumbnail without shrinking it unevenly across a row. */
  .cell { position: relative; width: 100%; height: 100%; border: 2px solid transparent; border-radius: 6px; overflow: hidden; padding: 8px 0 0; background: var(--viewport-bg); }
  .cell.selected { border-color: var(--select); }
  .cell.active { border-color: var(--accent); }
  .cell.reject :global(.media) { opacity: 0.35; }

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
     between two neighbouring stacks. */
  .stackline::before {
    content: "";
    position: absolute;
    left: 0; right: 0;
    top: 2.5px;
    height: 2.5px;
    background: var(--stack);
    transition: background 0.12s ease, top 0.12s ease;
  }
  .stackline.dbl::before { top: 1px; }
  .stackline.dbl::after {
    content: "";
    position: absolute;
    left: 0; right: 0;
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

  .ov { position: absolute; top: 8px; left: 0; right: 0; bottom: 0; z-index: 3; pointer-events: none; }
  .lbl-dot { position: absolute; top: 5px; right: 5px; width: 12px; height: 12px; border-radius: 3px; border: 1px solid rgba(0,0,0,0.4); }
  .fl { position: absolute; top: 4px; left: 6px; font-weight: 700; text-shadow: 0 1px 3px rgba(0,0,0,0.6); }
  .cell.related .fl { top: 25px; }
  .fl.x { color: var(--reject); }
  .fl.pick { color: var(--pick); }
  .stars { position: absolute; bottom: 4px; left: 6px; color: var(--star); font-size: 13px; text-shadow: 0 1px 3px rgba(0,0,0,0.6); }
  .tagdot { position: absolute; bottom: 4px; right: 6px; font-size: 11px; filter: drop-shadow(0 1px 2px rgba(0,0,0,0.6)); }
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
  .rel-count { position: absolute; right: 6px; bottom: 21px; min-width: 22px; padding: 3px 6px; text-align: center; font-size: 11px; background: color-mix(in srgb, var(--accent) 72%, #000); }

  .scell { position: relative; width: 100%; height: 100%; border: 2px solid transparent; border-radius: 5px; overflow: hidden; padding: 0; background: var(--viewport-bg); }
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
  .taginput { width: 70px; background: var(--bg-elev); border: 1px solid var(--border); border-radius: 11px; padding: 2px 8px; font-size: 11.5px; color: var(--text); }
  .taginput:focus { outline: none; border-color: var(--accent); width: 110px; }

  /* dim / lights-out scrim */
  .scrim { position: fixed; inset: 0; z-index: 40; border: none; padding: 0; cursor: pointer; background: rgba(0,0,0,0.55); transition: background 0.18s; }
  .app[data-dim="2"] .scrim { background: rgba(0,0,0,0.93); }
</style>
