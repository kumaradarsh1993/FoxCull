// The job centre's model: everything FoxCull is doing in the background, and
// what it just finished. Backend jobs (moves, merges, exports, thumbnail
// warming, capture dates, proxies) emit `activity` events; frontend-driven jobs
// report through `start`/`update`/`finish`, or the older `local`/`end`/`error`.
//
// Three lifetimes, because not everything deserves the same attention:
//   * quiet housekeeping (thumbnails, capture dates, folder scans) shows while
//     it runs and disappears 2 s after it ends;
//   * real work the owner started (a move, a merge, an export) lands in a
//     Recent list when it ends, with its result and follow-up actions
//     ("Show in folder"), and stays until cleared or 15 minutes old;
//   * failures stay in Recent until dismissed, so a failed move can't scroll
//     past unseen.

import { listen } from "@tauri-apps/api/event";

export type JobState = "running" | "done" | "error" | "cancelled";
export type JobKind =
  | "move"
  | "copy"
  | "merge"
  | "export"
  | "scan"
  | "thumbs"
  | "dates"
  | "prepare"
  | "trash"
  | "relink"
  | "cast"
  | "folder"
  | "info";
/** What done/total count. "pct" = 0-100. */
export type JobUnit = "items" | "bytes" | "pct";

export interface JobAction {
  label: string;
  run: () => void;
}

export interface Job {
  id: string;
  /** What is happening ("Moving 24 items to Seattle"), or what happened. */
  label: string;
  /** Second line: the file in flight, a size, a reason. */
  detail?: string;
  done: number;
  /** 0 = indeterminate (no percentage). */
  total: number;
  unit: JobUnit;
  state: JobState;
  kind: JobKind;
  /** Housekeeping the owner didn't ask for: shown, never kept. */
  quiet: boolean;
  /** Shows a Stop button. */
  cancellable: boolean;
  /** Waiting its turn (a move queued behind another): no progress bar yet. */
  queued?: boolean;
  actions: JobAction[];
  /** When it started, and when it last changed state (for ordering/ages). */
  started: number;
  ended?: number;
}

/** The shape backend `activity` events carry (see `Activity` in commands.rs). */
interface ActivityEvent {
  id: string;
  label: string;
  done: number;
  total: number;
  state: JobState;
  detail?: string;
  unit?: "bytes";
  cancellable?: boolean;
}

const QUIET_LINGER_MS = 2000;
const QUIET_ERROR_MS = 8000;
const RECENT_MAX_AGE_MS = 15 * 60_000;
const RECENT_MAX = 20;
/** How long a just-finished job headlines the collapsed card. */
export const FRESH_MS = 6000;

// ETA estimation: a sliding TIME window of progress samples per job (the last
// 8 s, so a copy that slows on a fragmented file adapts), and how much history
// a job needs before we show a time at all (a number from half a second of
// data whipsaws; no ETA is better than a wrong one). A count-based window
// (16 samples) never qualified once the backend reported every 150 ms.
const ETA_WINDOW_MS = 8000;
const ETA_MIN_SPAN_MS = 2500;
const ETA_MIN_SAMPLES = 3;

/** "~2m 40s" / "~45s" / "~1h 12m" — the tilde says estimate, always. */
export function fmtEta(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "";
  const s = Math.round(seconds);
  if (s < 60) return `~${Math.max(2, s)}s`;
  if (s < 3600) {
    const m = Math.floor(s / 60);
    const r = s % 60;
    return r >= 5 && m < 10 ? `~${m}m ${r}s` : `~${m}m`;
  }
  const h = Math.floor(s / 3600);
  const m = Math.round((s % 3600) / 60);
  return `~${h}h ${m}m`;
}

/** "830 MB", "12.4 GB": decimal, like Finder and Explorer. */
export function fmtBytes(b: number): string {
  if (!Number.isFinite(b) || b <= 0) return "0 MB";
  if (b >= 1e12) return `${(b / 1e12).toFixed(2)} TB`;
  if (b >= 1e9) return `${(b / 1e9).toFixed(1)} GB`;
  if (b >= 1e6) return `${Math.round(b / 1e6)} MB`;
  return `${Math.max(1, Math.round(b / 1e3))} KB`;
}

/** "45 s", "3 min 20 s", "1 h 12 min". */
export function fmtDuration(ms: number): string {
  const s = Math.max(1, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min${s % 60 ? ` ${s % 60} s` : ""}`;
  return `${Math.floor(s / 3600)} h ${Math.round((s % 3600) / 60)} min`;
}

/** What a finished job did: "4.2 GB in 38 s", "227 items in 12 s". */
function summary(j: Job): string | undefined {
  const took = Date.now() - j.started;
  const time = took >= 1500 ? ` in ${fmtDuration(took)}` : "";
  if (j.unit === "bytes" && j.total > 0) return `${fmtBytes(j.total)}${time}`;
  if (j.unit === "items" && j.total > 1) return `${j.total.toLocaleString()} items${time}`;
  return time ? `Took ${fmtDuration(took)}` : undefined;
}

/** Kind and quietness of a job from its id, for backend jobs that don't say. */
function classify(id: string): { kind: JobKind; quiet: boolean } {
  // The launch-time catalog check is housekeeping; what it reconnects isn't.
  if (id === "catalog-scan") return { kind: "scan", quiet: true };
  const pre = id.split(/[:-]/)[0];
  switch (pre) {
    case "warm":
    case "strip":
    case "scrub":
    case "thumbs":
      return { kind: "thumbs", quiet: true };
    case "captures":
      return { kind: "dates", quiet: true };
    case "scan":
      return { kind: "scan", quiet: true };
    case "proxy":
      return { kind: "export", quiet: true };
    case "move":
      return { kind: "move", quiet: false };
    case "copy":
      return { kind: "copy", quiet: false };
    case "merge":
      return { kind: "merge", quiet: false };
    case "edit":
    case "export":
    case "subclips":
    case "raw":
      return { kind: "export", quiet: false };
    case "prepare":
      return { kind: "prepare", quiet: false };
    case "trash":
      return { kind: "trash", quiet: false };
    case "relink":
    case "catalog":
      return { kind: "relink", quiet: false };
    case "cast":
      return { kind: "cast", quiet: false };
    case "new":
    case "exclude":
    case "event":
    case "cut":
      return { kind: "folder", quiet: false };
    default:
      return { kind: "info", quiet: false };
  }
}

export interface StartOpts {
  label: string;
  detail?: string;
  total?: number;
  done?: number;
  unit?: JobUnit;
  kind?: JobKind;
  quiet?: boolean;
  /** Called by the Stop button. */
  cancel?: () => void;
  actions?: JobAction[];
  queued?: boolean;
}

class ActivityStore {
  jobs = $state<Record<string, Job>>({});
  /** Bumped every second while something runs, so ages and ETAs re-render. */
  tick = $state(0);
  private started = false;
  private reapers = new Map<string, ReturnType<typeof setTimeout>>();
  private cancels = new Map<string, () => void>();
  // Recent (t, done) samples per running job — the basis for ETAs and speeds.
  // Kept out of $state (reads happen during renders the jobs update drives).
  private samples = new Map<string, { t: number; done: number }[]>();
  private ticker: ReturnType<typeof setInterval> | null = null;

  list = $derived(Object.values(this.jobs).sort((a, b) => a.started - b.started));
  running = $derived(this.list.filter((j) => j.state === "running"));
  /** Running work the owner started (not housekeeping). */
  foreground = $derived(this.running.filter((j) => !j.quiet));
  /** Finished jobs worth keeping, newest first. */
  recent = $derived(
    this.list
      .filter((j) => j.state !== "running" && !j.quiet)
      .sort((a, b) => (b.ended ?? 0) - (a.ended ?? 0)),
  );
  problems = $derived(this.recent.filter((j) => j.state === "error").length);

  /** Seconds remaining for a determinate running job, from the recent rate —
   *  a sliding window, so multi-phase jobs (fast photos, then slow videos)
   *  adapt instead of averaging the phases into nonsense. NaN = don't show. */
  etaSeconds(id: string): number {
    const j = this.jobs[id];
    if (!j || j.state !== "running" || j.total <= 0 || j.done <= 0) return NaN;
    const r = this.rate(id);
    return r > 0 ? (j.total - j.done) / r : NaN;
  }

  /** Units per second over the sample window (bytes/s for a copy). 0 = unknown. */
  rate(id: string): number {
    const s = this.samples.get(id);
    if (!s || s.length < ETA_MIN_SAMPLES) return 0;
    const first = s[0];
    const last = s[s.length - 1];
    const span = last.t - first.t;
    const did = last.done - first.done;
    if (span < ETA_MIN_SPAN_MS || did <= 0) return 0;
    return (did * 1000) / span;
  }

  /** Formatted ETA for a job, or "" when unknown/indeterminate. */
  eta(id: string): string {
    return fmtEta(this.etaSeconds(id));
  }

  async init() {
    if (this.started) return;
    this.started = true;
    try {
      await listen<ActivityEvent>("activity", (e) => this.ingest(e.payload));
    } catch {
      // not running inside Tauri (tests, the browser harness) — local jobs work
    }
  }

  /** A backend event. Fields it leaves empty keep what the frontend set (a
   *  move's title is written by the page; the backend only reports bytes). */
  ingest(e: ActivityEvent) {
    const prev = this.jobs[e.id];
    const c = classify(e.id);
    this.put({
      id: e.id,
      label: e.label || prev?.label || "Working…",
      detail: e.detail ?? (e.state === prev?.state ? prev?.detail : undefined),
      done: e.done,
      total: e.total,
      unit: e.unit ?? (e.total === 100 ? "pct" : "items"),
      state: e.state,
      kind: prev?.kind ?? c.kind,
      quiet: prev?.quiet ?? c.quiet,
      cancellable: e.state === "running" && (!!e.cancellable || this.cancels.has(e.id)),
      actions: prev?.actions ?? [],
      queued: false,
      started: prev && prev.state === "running" ? prev.started : Date.now(),
    });
  }

  private put(j: Job) {
    const prev = this.jobs[j.id];
    // Feed the rate window. A job that restarts (done went backwards) resets it.
    if (j.state === "running" && j.total > 0) {
      let s = this.samples.get(j.id);
      if (!s || (s.length && s[s.length - 1].done > j.done)) {
        s = [];
        this.samples.set(j.id, s);
      }
      if (!s.length || s[s.length - 1].done !== j.done) {
        const t = Date.now();
        s.push({ t, done: j.done });
        while (s.length > 2 && t - s[1].t > ETA_WINDOW_MS) s.shift();
        if (s.length > 400) s.shift();
      }
    } else if (j.state !== "running") {
      this.samples.delete(j.id);
    }
    // A finished job keeps the time it first finished, through later edits
    // (a merge's actions are attached after the backend reports it done).
    j.ended = j.state === "running" ? undefined : prev && prev.state !== "running" && prev.ended ? prev.ended : Date.now();
    this.jobs[j.id] = j;

    const old = this.reapers.get(j.id);
    if (old) clearTimeout(old);
    this.reapers.delete(j.id);
    if (j.state !== "running") {
      this.cancels.delete(j.id);
      if (j.quiet) {
        const wait = j.state === "error" ? QUIET_ERROR_MS : QUIET_LINGER_MS;
        this.reapers.set(
          j.id,
          setTimeout(() => {
            if (this.jobs[j.id]?.state !== "running") delete this.jobs[j.id];
            this.reapers.delete(j.id);
          }, wait),
        );
      } else {
        this.trimRecent();
      }
    }
    this.syncTicker();
  }

  private trimRecent() {
    const now = Date.now();
    const done = Object.values(this.jobs)
      .filter((j) => j.state !== "running" && !j.quiet)
      .sort((a, b) => (b.ended ?? 0) - (a.ended ?? 0));
    done.forEach((j, i) => {
      // Errors stay until dismissed; successes age out.
      const old = j.state !== "error" && now - (j.ended ?? now) > RECENT_MAX_AGE_MS;
      if (old || i >= RECENT_MAX) delete this.jobs[j.id];
    });
  }

  /** A one-second clock while anything runs or just finished, for ages/ETAs. */
  private syncTicker() {
    const busy = Object.values(this.jobs).some((j) => j.state === "running" || Date.now() - (j.ended ?? 0) < FRESH_MS + 1000);
    if (busy && !this.ticker) {
      this.ticker = setInterval(() => {
        this.tick++;
        this.trimRecent();
        this.syncTicker();
      }, 1000);
    } else if (!busy && this.ticker) {
      clearInterval(this.ticker);
      this.ticker = null;
    }
  }

  // ── frontend jobs ─────────────────────────────────────────────────────────

  start(id: string, o: StartOpts) {
    const c = classify(id);
    if (o.cancel) this.cancels.set(id, o.cancel);
    else this.cancels.delete(id);
    this.samples.delete(id);
    this.put({
      id,
      label: o.label,
      detail: o.detail,
      done: o.done ?? 0,
      total: o.total ?? 0,
      unit: o.unit ?? "items",
      state: "running",
      kind: o.kind ?? c.kind,
      quiet: o.quiet ?? c.quiet,
      cancellable: !!o.cancel,
      actions: o.actions ?? [],
      queued: o.queued,
      started: Date.now(),
    });
  }

  update(id: string, patch: Partial<Pick<Job, "label" | "detail" | "done" | "total" | "unit" | "actions" | "queued">>) {
    const j = this.jobs[id];
    if (!j) return;
    this.put({ ...j, ...patch });
  }

  /** End a job: "done" by default. Its label/detail/actions can be rewritten
   *  in the past tense ("Moved 24 items to Seattle") with follow-ups. */
  finish(id: string, patch: Partial<Pick<Job, "label" | "detail" | "actions" | "state" | "kind">> = {}) {
    const j = this.jobs[id];
    const c = classify(id);
    // The running detail ("2 of 24 · IMG_2041.CR2") is stale once it ends; a
    // job that doesn't say otherwise reports what it did and how long it took.
    const detail = "detail" in patch ? patch.detail : j && (patch.state ?? "done") === "done" ? summary(j) : j?.detail;
    this.put({
      id,
      label: patch.label ?? j?.label ?? "Done",
      detail,
      done: j?.total ?? 1,
      total: j?.total ?? 1,
      unit: j?.unit ?? "items",
      state: patch.state ?? "done",
      kind: patch.kind ?? j?.kind ?? c.kind,
      quiet: j?.quiet ?? c.quiet,
      cancellable: false,
      actions: patch.actions ?? j?.actions ?? [],
      started: j?.started ?? Date.now(),
    });
  }

  /** Attach follow-up actions to a job, running or finished. */
  setActions(id: string, actions: JobAction[]) {
    const j = this.jobs[id];
    if (j) this.jobs[id] = { ...j, actions };
  }

  /** A one-off notice ("Created folder Seattle"): goes straight to Recent. */
  notify(id: string, label: string, o: { detail?: string; kind?: JobKind; actions?: JobAction[] } = {}) {
    this.start(id, { label, kind: o.kind, detail: o.detail });
    this.finish(id, { label, detail: o.detail, actions: o.actions, kind: o.kind });
  }

  /** The Stop button. */
  cancel(id: string) {
    const f = this.cancels.get(id);
    if (f) f();
    else void import("$lib/api").then(({ api }) => api.cancelJob(id));
    const j = this.jobs[id];
    if (j && j.state === "running") this.jobs[id] = { ...j, detail: "Stopping…", cancellable: false };
  }

  dismiss(id: string) {
    const r = this.reapers.get(id);
    if (r) clearTimeout(r);
    this.reapers.delete(id);
    if (this.jobs[id]?.state !== "running") delete this.jobs[id];
  }

  clearFinished() {
    for (const j of Object.values(this.jobs)) if (j.state !== "running") this.dismiss(j.id);
  }

  // ── the older API, kept for its many callers ─────────────────────────────

  /** Report a frontend-driven job. Call with done===total (or `end()`) to finish
   *  it; a 1/1 call is a one-off notice. */
  local(id: string, label: string, done: number, total: number) {
    if (total > 0 && done >= total) {
      if (this.jobs[id]?.state === "running") this.finish(id, { label });
      else this.notify(id, label);
      return;
    }
    const j = this.jobs[id];
    if (j && j.state === "running") this.update(id, { label, done, total });
    else this.start(id, { label, done, total });
  }

  end(id: string) {
    const j = this.jobs[id];
    if (j && j.state === "running") this.finish(id);
  }

  /** Surface a failure. It stays in Recent until dismissed. */
  error(id: string, label: string, detail?: string) {
    const j = this.jobs[id];
    const c = classify(id);
    this.put({
      id,
      label,
      detail,
      done: 0,
      total: 0,
      unit: "items",
      state: "error",
      kind: j?.kind ?? c.kind,
      quiet: j?.quiet ?? c.quiet,
      cancellable: false,
      actions: j?.actions ?? [],
      started: j?.started ?? Date.now(),
    });
  }
}

export const activity = new ActivityStore();
