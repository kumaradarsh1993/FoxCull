// Shared update client — identical in wispr-fox, FoxCull, Fox MD and Fox Mark.
//
// Two jobs, deliberately split from the panel that renders them:
//
//  1. Type the `update_status` / `download_and_install` contract once, so a
//     change to `src-tauri/src/updates.rs` breaks compilation in all four apps
//     the same way rather than in three of them silently.
//  2. Hold ONE background check per app run, so a badge on the settings button
//     can say "there's a newer build" without the user going to look. The check
//     is a single unauthenticated GitHub call and is not repeated on its own.
//
// If you fix something here, fix it in all four — divergence is a bug.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface ReleaseAsset {
  name: string;
  size: number;
}

export interface ReleaseInfo {
  tag: string;
  version: string;
  html_url: string;
  published_at: string | null;
  prerelease: boolean;
  /** Newer than the build that is running right now. */
  newer: boolean;
  /** `null` when the release shipped no artifact installable on this platform. */
  asset: ReleaseAsset | null;
  summary: string | null;
}

export interface UpdateStatus {
  product: string;
  current: string;
  current_is_nightly: boolean;
  stable: ReleaseInfo | null;
  nightly: ReleaseInfo | null;
  /** Windows can finish an install unattended; elsewhere the last step is manual. */
  can_self_install: boolean;
  update_available: boolean;
  releases_url: string;
}

export interface UpdateProgress {
  phase: "starting" | "downloading" | "verifying" | "launching";
  downloaded: number;
  total: number;
  tag: string;
}

/** Same event name in every app. Declared in `src-tauri/src/updates.rs`. */
export const PROGRESS_EVENT = "update://progress";

export const checkForUpdates = () => invoke<UpdateStatus>("update_status");

/** Takes a TAG, never a URL: the download URL is re-resolved in Rust so a
 *  compromised renderer cannot ask the app to run an arbitrary file. */
export const downloadAndInstall = (tag: string) =>
  invoke<string>("download_and_install", { tag });

export const onUpdateProgress = (cb: (p: UpdateProgress) => void) =>
  listen<UpdateProgress>(PROGRESS_EVENT, (e) => cb(e.payload));

/**
 * The one-run background check.
 *
 * `available` is what a badge binds to. It stays false on any failure — being
 * offline is not news, and a settings button that shouts because GitHub was
 * unreachable trains the user to ignore it.
 */
export const updates = $state({
  checked: false,
  available: false,
  status: null as UpdateStatus | null,
});

let inflight: Promise<void> | null = null;

/**
 * Check once per app run, unless `force`.
 *
 * Call it from the app's root layout after mount. Deliberately not on a timer:
 * the answer only matters when there is somewhere to show it, and a background
 * poll would spend the user's GitHub rate limit on nothing.
 */
export function primeUpdateCheck(force = false): Promise<void> {
  if (!force && (updates.checked || inflight)) return inflight ?? Promise.resolve();
  inflight = checkForUpdates()
    .then((s) => {
      updates.status = s;
      updates.available = s.update_available;
    })
    .catch(() => {
      // Swallowed on purpose — see `available` above.
      updates.available = false;
    })
    .finally(() => {
      updates.checked = true;
      inflight = null;
    });
  return inflight;
}
