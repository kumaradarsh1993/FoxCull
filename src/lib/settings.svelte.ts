// Persisted app settings (theme, layout, sorting, delete behavior).
// Mirrors the wispr-fox settings-store pattern: a runes-powered class that
// loads once and writes through to tauri-plugin-store on every change.
import { Store } from "@tauri-apps/plugin-store";
import type { ScanExcludes } from "$lib/types";

/** Six themes (2026-10-04) plus "system", which follows the OS: Graphite at
 *  night, Daylight by day. The old ids migrate in `init` (neutral → graphite,
 *  dark → midnight, warm → amber, light → daylight). */
export type Theme = "graphite" | "studio" | "midnight" | "amber" | "daylight" | "paper" | "system";
/** "theme" = each theme's own accent. Green and red are reserved for pick and
 *  reject, so they're never accents. */
export type Accent = "theme" | "blue" | "indigo" | "teal" | "fox" | "rose" | "mono";
/** What sits behind the pictures: the theme's own dark, black, 18% grey, light. */
export type Surround = "dark" | "black" | "grey" | "light";
export type UiScale = "compact" | "comfortable" | "distance";
export type ViewMode = "grid" | "details" | "loupe";
/** Where the filmstrip docks. "left" sits between the folder tree and the
 *  viewport — the same column your eye is already in when picking folders. */
export type FilmstripPos = "bottom" | "left" | "right" | "hidden";
export type SortBy = "name" | "date" | "capture" | "type" | "size";
export type SortDir = "asc" | "desc";
/** NOTE: `"event"` was briefly a grouping and is deliberately gone. Events are a
 *  banner drawn inside the timeline, not a competing axis — keeping both modes
 *  meant a stored `groupBy: "event"` silently pinned the owner to the old
 *  album view while the new banner sat suppressed. See `migrate` below. */
export type GroupBy = "none" | "folder" | "type" | "year" | "month" | "week" | "day";
export type TypeFilter = "all" | "image" | "video" | "raw";
export type DeleteMode = "recycle" | "folder";
export type RelatedMode = "expanded" | "collapsed";

export interface AppSettings {
  theme: Theme;
  accent: Accent;
  surround: Surround;
  /** Whole-interface scale. Distance is intentionally large enough for an HDMI
   *  TV workflow; compact recovers canvas space on the XPS 13. */
  uiScale: UiScale;
  viewMode: ViewMode;
  filmstripPos: FilmstripPos;
  treeWidth: number;
  filmstripSize: number;
  gridSize: number;
  sortBy: SortBy;
  sortDir: SortDir;
  /** Section the grid by real capture date (year, month, week or day), folder or type. */
  groupBy: GroupBy;
  subgroupBy: GroupBy;
  /** Paint events as a continuous banner down the left of the rows they occupy,
   *  inside the normal date-ordered grid — the lightweight read of an event.
   *  Self-disables under name/size ordering, where a run isn't meaningful. */
  eventRail: boolean;
  /** Verify the catalog against the disk when the app opens a library, and
   *  auto-reconnect anything that moved. Costs a few seconds on a big catalog;
   *  turning it off means moved files silently keep showing as "?". */
  scanOnLaunch: boolean;
  typeFilter: TypeFilter;
  includeSub: boolean;
  liveScrub: boolean;
  /** @deprecated Retired 2026-07-21 — skimming decodes live and needs no
   *  pre-built strips. Kept in the type so a stored value from an older build
   *  loads without a schema error; nothing reads it. */
  scrubPrefetch?: boolean;
  /** Focus-view scrubbing decodes real frames on demand (WebCodecs) instead of
   *  painting a pre-built sprite sheet. Full resolution, no pre-caching, and it
   *  works on a clip the moment it opens. Falls back automatically per clip if
   *  the codec/container can't be decoded this way, so turning it off is only
   *  for diagnosis. See docs/design/video-player-migration.md. */
  liveDecodeScrub: boolean;
  /** Glimpse speed as a plain multiple of realtime, like a player's 2x/5x.
   *  Constant regardless of clip length: 5x turns 20 s into 4 s and 10 min into
   *  2 min. (Until 2026-07-22 this was a 10-100x "sweep" that compressed every
   *  clip to a fixed duration — unlearnable, since the apparent rate changed
   *  with the clip. Old stored values are clamped on load.) */
  glimpseSpeed: number;
  videoAutoplay: boolean;
  /** Collapse the video transport to a thin hover-to-expand line (vs a pinned
   *  always-visible bar). Keeps the picture edge-to-edge in Focus/full-screen. */
  minimalVideoBar: boolean;
  /** Game-controller culling (PS5/PS4 pad over Bluetooth/USB). */
  padEnabled: boolean;
  /** Controller action-id → button-index overrides; unset actions use the
   *  defaults in gamepad.svelte.ts. */
  padBindings: Record<string, number>;
  /** Schema stamp for `padBindings`. Binding ONE action writes the whole merged
   *  map, so a stored map pins whatever the defaults were that day; bumping
   *  PAD_BINDINGS_VERSION clears it so a new default layout actually lands. */
  padBindingsVersion: number;
  /** What the mouse's extra Back/Forward buttons do (action ids). */
  mouseBack: string;
  mouseForward: string;
  /** Whether the filmstrip is wanted in each view, remembered per view. The
   *  strip duplicates the grid but is the whole point of Focus, so it follows
   *  the view by default (hidden in Grid, out in Focus) — and toggling it by
   *  hand teaches it a new answer for THAT view only. */
  stripShow: Record<string, boolean>;
  relatedMode: RelatedMode;
  relatedStrip: boolean;
  deleteMode: DeleteMode;
  /** What grid tiles show besides the picture: the length of each video
   *  (a small badge, bottom-right) and the file name (a caption line). */
  tileInfo: { duration: boolean; name: boolean };
  /** Folders every scan skips. System folders are pre-selected; the user can
   *  untick groups and add their own folders or name patterns. */
  scanExcludes: ScanExcludes;
  rejectFolder: string | null;
  lastDir: string | null;
  lastActivePath: string | null;
}

/** Every built-in group on, no custom rules. A function, not a constant, so
 *  "Reset to defaults" can never hand out a shared array to mutate. */
export function defaultScanExcludes(): ScanExcludes {
  return { windowsSystem: true, macosSystem: true, appData: true, developer: true, games: true, paths: [], names: [] };
}

const DEFAULTS: AppSettings = {
  theme: "graphite",
  accent: "theme",
  surround: "dark",
  uiScale: "comfortable",
  viewMode: "grid",
  filmstripPos: "bottom",
  treeWidth: 270,
  filmstripSize: 132,
  gridSize: 176,
  sortBy: "name",
  sortDir: "asc",
  groupBy: "none",
  subgroupBy: "none",
  eventRail: true,
  scanOnLaunch: true,
  typeFilter: "all",
  includeSub: true,
  liveScrub: false,
  liveDecodeScrub: true,
  glimpseSpeed: 5,
  videoAutoplay: false,
  minimalVideoBar: true,
  padEnabled: true,
  padBindings: {},
  padBindingsVersion: 2,
  mouseBack: "viewBack",
  mouseForward: "viewForward",
  stripShow: { grid: false, details: false, loupe: true },
  relatedMode: "expanded",
  relatedStrip: true,
  deleteMode: "folder",
  scanExcludes: defaultScanExcludes(),
  tileInfo: { duration: true, name: false },
  rejectFolder: null,
  lastDir: null,
  lastActivePath: null,
};

/** Glimpse multiplier bounds. 5x sits mid-slider and is the recommended pace. */
export const GLIMPSE_MIN = 2;
export const GLIMPSE_MAX = 10;

/** Bump when DEFAULT_BINDINGS changes shape (see gamepad.svelte.ts). v2 =
 *  the 2026-07-22 TV-culling layout: touchpad enters Focus, shoulders mark
 *  in/out, the sticks carry ratings + labels. */
export const PAD_BINDINGS_VERSION = 2;

const FILE = "foxcull-settings.json";
const KEY = "settings";

class Settings {
  s = $state<AppSettings>({ ...DEFAULTS });
  ready = $state(false);
  private store: Store | null = null;

  async init() {
    if (this.ready) return;
    try {
      this.store = await Store.load(FILE);
      let loaded = await this.store.get<AppSettings & { groupByMonth?: boolean }>(KEY);
      if (loaded) {
        const OLD_THEMES: Record<string, Theme> = { neutral: "graphite", dark: "midnight", warm: "amber", light: "daylight" };
        const THEMES: Theme[] = ["graphite", "studio", "midnight", "amber", "daylight", "paper", "system"];
        const rawTheme = (loaded.theme as string | undefined) ?? DEFAULTS.theme;
        const migrated: Partial<AppSettings> = {
          ...loaded,
          theme: OLD_THEMES[rawTheme] ?? (THEMES.includes(rawTheme as Theme) ? (rawTheme as Theme) : DEFAULTS.theme),
          uiScale: loaded.uiScale ?? DEFAULTS.uiScale,
        };
        // Migrate the old boolean month toggle to the new granularity field.
        if (loaded.groupBy === undefined && loaded.groupByMonth) migrated.groupBy = "month";
        // `groupBy: "event"` was the short-lived album-block mode. Anyone holding
        // it was, by definition, trying to look at events — so drop the grouping
        // AND put them on a capture-date order, which is what the replacement
        // banner needs to draw. Without the sort change they would land on a
        // plain grid with the new feature silently switched off, which is
        // exactly the dead end this migration exists to prevent.
        const legacyEvent = (v: unknown) => v === "event";
        if (legacyEvent(migrated.groupBy)) {
          migrated.groupBy = "none";
          migrated.sortBy = "capture";
        }
        if (legacyEvent(migrated.subgroupBy)) migrated.subgroupBy = "none";
        // glimpseSpeed changed meaning on 2026-07-22 (fixed-length sweep ratio
        // -> plain realtime multiple). A stored 10-100 would now mean 10-100x
        // realtime, which is a blur; snap anything out of range back to default.
        const gs = migrated.glimpseSpeed;
        if (typeof gs !== "number" || gs < GLIMPSE_MIN || gs > GLIMPSE_MAX) {
          migrated.glimpseSpeed = DEFAULTS.glimpseSpeed;
        }
        // Controller layout changed shape: drop stored overrides so the new
        // defaults apply. Rebinding one button used to freeze all twenty.
        if ((loaded.padBindingsVersion ?? 0) < PAD_BINDINGS_VERSION) {
          migrated.padBindings = {};
          migrated.padBindingsVersion = PAD_BINDINGS_VERSION;
        }
        // Fill in any exclude group added after this store was written, so a
        // new built-in group arrives switched on rather than undefined.
        migrated.scanExcludes = { ...defaultScanExcludes(), ...(loaded.scanExcludes ?? {}) };
        migrated.tileInfo = { ...DEFAULTS.tileInfo, ...(loaded.tileInfo ?? {}) };
        this.s = { ...DEFAULTS, ...migrated };
      }
    } catch {
      // first run / store unavailable — defaults stand
    }
    this.ready = true;
    // Appearance follows across windows: a theme picked in the library's
    // Settings repaints Edit, Merge and Reel at once.
    try {
      await this.store?.onKeyChange<AppSettings>(KEY, (v) => {
        if (!v) return;
        for (const k of ["theme", "accent", "surround", "uiScale"] as const) {
          if (v[k] !== undefined && v[k] !== this.s[k]) (this.s as unknown as Record<string, unknown>)[k] = v[k];
        }
      });
    } catch {
      /* the store can't notify here (browser harness): each window keeps its own */
    }
  }

  async set(patch: Partial<AppSettings>) {
    Object.assign(this.s, patch);
    try {
      if (this.store) {
        await this.store.set(KEY, { ...this.s });
        await this.store.save();
      }
    } catch {
      // ignore persistence failures (settings still apply in-session)
    }
  }
}

export const settings = new Settings();
