// Persisted app settings (theme, layout, sorting, delete behavior).
// Mirrors the wispr-fox settings-store pattern: a runes-powered class that
// loads once and writes through to tauri-plugin-store on every change.
import { Store } from "@tauri-apps/plugin-store";

export type Theme = "light" | "dark" | "neutral" | "warm";
export type ViewMode = "grid" | "details" | "loupe";
/** Where the filmstrip docks. "left" sits between the folder tree and the
 *  viewport — the same column your eye is already in when picking folders. */
export type FilmstripPos = "bottom" | "left" | "right" | "hidden";
export type SortBy = "name" | "date" | "capture" | "type" | "size";
export type SortDir = "asc" | "desc";
export type GroupBy = "none" | "folder" | "type" | "year" | "month" | "week";
export type TypeFilter = "all" | "image" | "video" | "raw";
export type DeleteMode = "recycle" | "folder";
export type RelatedMode = "expanded" | "collapsed";

export interface AppSettings {
  theme: Theme;
  viewMode: ViewMode;
  filmstripPos: FilmstripPos;
  treeWidth: number;
  filmstripSize: number;
  gridSize: number;
  sortBy: SortBy;
  sortDir: SortDir;
  /** Section the grid by real capture date — off, by month, or by week. */
  groupBy: GroupBy;
  subgroupBy: GroupBy;
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
  /** Glimpse sweep speed, x realtime. A clip is swept by its keyframes at this
   *  rate, floored so even a short clip takes a few seconds. 40x puts a
   *  9-minute clip at ~14 s — long enough to read, short enough to be worth
   *  doing while culling. */
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
  /** What the mouse's extra Back/Forward buttons do (action ids). */
  mouseBack: string;
  mouseForward: string;
  relatedMode: RelatedMode;
  relatedStrip: boolean;
  deleteMode: DeleteMode;
  rejectFolder: string | null;
  lastDir: string | null;
  lastActivePath: string | null;
}

const DEFAULTS: AppSettings = {
  theme: "neutral",
  viewMode: "grid",
  filmstripPos: "bottom",
  treeWidth: 270,
  filmstripSize: 132,
  gridSize: 176,
  sortBy: "name",
  sortDir: "asc",
  groupBy: "none",
  subgroupBy: "none",
  typeFilter: "all",
  includeSub: true,
  liveScrub: false,
  liveDecodeScrub: true,
  glimpseSpeed: 40,
  videoAutoplay: false,
  minimalVideoBar: true,
  padEnabled: true,
  padBindings: {},
  mouseBack: "viewBack",
  mouseForward: "viewForward",
  relatedMode: "expanded",
  relatedStrip: true,
  deleteMode: "folder",
  rejectFolder: null,
  lastDir: null,
  lastActivePath: null,
};

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
        const migrated: Partial<AppSettings> = {
          ...loaded,
          theme: loaded.theme ?? DEFAULTS.theme,
        };
        // Migrate the old boolean month toggle to the new granularity field.
        if (loaded.groupBy === undefined && loaded.groupByMonth) migrated.groupBy = "month";
        this.s = { ...DEFAULTS, ...migrated };
      }
    } catch {
      // first run / store unavailable — defaults stand
    }
    this.ready = true;
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
