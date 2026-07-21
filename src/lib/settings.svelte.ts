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
  /** With Live Scrub on, also pre-build the hover strips for the few clips
   *  either side of the one open in Focus, so stepping to the next clip finds
   *  skimming already live. Off by default: each strip is ~40 keyframe decodes,
   *  and on an HDD library that background work is worth opting into, not
   *  inheriting. */
  scrubPrefetch: boolean;
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
  scrubPrefetch: false,
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
