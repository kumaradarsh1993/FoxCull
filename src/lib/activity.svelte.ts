// Background-activity tracker — the model behind the Lightroom-style progress
// chip in the left pane. Backend jobs (thumbnail warming, capture-date reads,
// exports, proxy transcodes, filmstrip builds) emit `activity` events; purely
// frontend-driven jobs (Prepare folder) report through `local()`. Finished
// jobs linger briefly so quick operations are still visible, then drop out.

import { listen } from "@tauri-apps/api/event";

export interface Job {
  id: string;
  label: string;
  done: number;
  /** 0 = indeterminate (spinner, no percentage). */
  total: number;
  state: "running" | "done" | "error";
  /** Last-update timestamp, for stable ordering. */
  ts: number;
}

const LINGER_DONE_MS = 1800;
const LINGER_ERROR_MS = 6000;

// ETA estimation: how many recent progress samples to keep per job, and how
// much observed history a job needs before we show a time at all (a number
// from half a second of data whipsaws; no ETA is better than a wrong one).
const ETA_SAMPLES = 12;
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

class ActivityStore {
  jobs = $state<Record<string, Job>>({});
  private started = false;
  private reapers = new Map<string, ReturnType<typeof setTimeout>>();
  // Recent (t, done) samples per running job — the basis for ETAs. Kept out of
  // $state (reads happen during renders the jobs-object update already drives).
  private samples = new Map<string, { t: number; done: number }[]>();

  list = $derived(Object.values(this.jobs).sort((a, b) => a.ts - b.ts));
  running = $derived(this.list.filter((j) => j.state === "running"));

  /** Seconds remaining for a determinate running job, from the recent rate —
   *  a sliding window, so multi-phase jobs (fast photos, then slow videos)
   *  adapt instead of averaging the phases into nonsense. NaN = don't show. */
  etaSeconds(id: string): number {
    const j = this.jobs[id];
    if (!j || j.state !== "running" || j.total <= 0 || j.done <= 0) return NaN;
    const s = this.samples.get(id);
    if (!s || s.length < ETA_MIN_SAMPLES) return NaN;
    const first = s[0];
    const last = s[s.length - 1];
    const spanMs = last.t - first.t;
    const doneInSpan = last.done - first.done;
    if (spanMs < ETA_MIN_SPAN_MS || doneInSpan <= 0) return NaN;
    const perItemMs = spanMs / doneInSpan;
    return ((j.total - j.done) * perItemMs) / 1000;
  }

  /** Formatted ETA for a job, or "" when unknown/indeterminate. */
  eta(id: string): string {
    return fmtEta(this.etaSeconds(id));
  }

  async init() {
    if (this.started) return;
    this.started = true;
    try {
      await listen<Omit<Job, "ts">>("activity", (e) => this.apply(e.payload));
    } catch {
      // not running inside Tauri (tests) — local jobs still work
    }
  }

  private apply(j: Omit<Job, "ts">) {
    // Feed the ETA window. A job that restarts (done went backwards) resets it.
    if (j.state === "running" && j.total > 0) {
      let s = this.samples.get(j.id);
      if (!s || (s.length && s[s.length - 1].done > j.done)) {
        s = [];
        this.samples.set(j.id, s);
      }
      if (!s.length || s[s.length - 1].done !== j.done) {
        s.push({ t: Date.now(), done: j.done });
        if (s.length > ETA_SAMPLES) s.shift();
      }
    } else {
      this.samples.delete(j.id);
    }
    this.jobs[j.id] = { ...j, ts: this.jobs[j.id]?.ts ?? Date.now() };
    const old = this.reapers.get(j.id);
    if (old) clearTimeout(old);
    if (j.state !== "running") {
      const wait = j.state === "error" ? LINGER_ERROR_MS : LINGER_DONE_MS;
      this.reapers.set(
        j.id,
        setTimeout(() => {
          if (this.jobs[j.id]?.state !== "running") delete this.jobs[j.id];
          this.reapers.delete(j.id);
        }, wait),
      );
    }
  }

  /** Report a frontend-driven job (e.g. Prepare folder). Call with done===total
   *  (or `end()`) to finish it. */
  local(id: string, label: string, done: number, total: number) {
    this.apply({ id, label, done, total, state: done >= total && total > 0 ? "done" : "running" });
  }

  end(id: string) {
    const j = this.jobs[id];
    if (j) this.apply({ ...j, state: "done" });
  }

  /** Surface a one-off failure message (lingers a few seconds, then clears). */
  error(id: string, label: string) {
    this.apply({ id, label, done: 0, total: 1, state: "error" });
  }
}

export const activity = new ActivityStore();
