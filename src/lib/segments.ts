// In/out segments of one clip, as marked in Focus with [ and ] (owner's spec,
// docs/design/segments-and-reel-mode.md §A). Pure functions over a plain
// state, so the rules are the same wherever they're used and can be tested.
//
// The rules, as the owner described them:
//   [  no segment open    → starts one (its in), at the playhead
//   [  again (still open) → moves that in to the playhead
//   ]  with one open      → closes it (its out)
//   ]  again              → moves that out (later: extends; earlier, inside
//                           the segment: shortens)
//   then [ starts the second segment, ] closes it, and so on.
// Segments never overlap: an out that would run into the next segment stops
// at its start, and the note says so. A [ inside an existing segment moves
// THAT segment's in (refining it) rather than starting an overlapping one.

export interface Seg {
  in_s: number;
  out_s: number;
}

export interface MarkState {
  /** Closed segments: sorted, never overlapping. */
  segments: Seg[];
  /** An in point waiting for its out. */
  open: number | null;
  /** The segment the keys last touched ("] again moves that out"). */
  last: number | null;
}

export type MarkResult = { state: MarkState; note: string | null; changed: boolean };

/** Shortest segment: a tenth of a second (a few frames at any rate). */
export const MIN_LEN = 0.1;

export const emptyMarks = (): MarkState => ({ segments: [], open: null, last: null });

export function fmtT(s: number): string {
  if (!Number.isFinite(s) || s < 0) s = 0;
  const m = Math.floor(s / 60);
  const sec = s - m * 60;
  return `${m}:${sec.toFixed(1).padStart(4, "0")}`;
}

/** Sorted, valid, overlaps merged (for anything loaded from storage). */
export function normalize(segs: Seg[], dur = Infinity): Seg[] {
  const clean = segs
    .filter((s) => Number.isFinite(s.in_s) && Number.isFinite(s.out_s))
    .map((s) => ({ in_s: Math.max(0, s.in_s), out_s: Math.min(dur, s.out_s) }))
    .filter((s) => s.out_s - s.in_s >= MIN_LEN / 2)
    .sort((a, b) => a.in_s - b.in_s);
  const out: Seg[] = [];
  for (const s of clean) {
    const prev = out[out.length - 1];
    if (prev && s.in_s < prev.out_s) prev.out_s = Math.max(prev.out_s, s.out_s);
    else out.push({ ...s });
  }
  return out;
}

/** The segment strictly containing `t` (not on its edges), or -1. */
export function segAt(segs: Seg[], t: number): number {
  return segs.findIndex((s) => t > s.in_s && t < s.out_s);
}

function withSegs(st: MarkState, segments: Seg[], last: number | null, open = st.open): MarkState {
  return { segments, open, last };
}

const same = (st: MarkState, note: string | null): MarkResult => ({ state: st, note, changed: false });

/** `[` at time `t`. */
export function markIn(st: MarkState, t: number, dur: number): MarkResult {
  t = Math.max(0, Math.min(t, dur > 0 ? dur - MIN_LEN : t));
  const inside = segAt(st.segments, t);
  if (st.open != null) {
    if (inside >= 0) return same(st, `That's inside segment ${inside + 1}. Close this one first with ], or drag segment ${inside + 1}'s markers.`);
    return { state: { ...st, open: t }, note: `In moved to ${fmtT(t)}`, changed: true };
  }
  if (inside >= 0) {
    const segs = st.segments.map((s) => ({ ...s }));
    if (segs[inside].out_s - t < MIN_LEN) return same(st, null);
    segs[inside].in_s = t;
    return { state: withSegs(st, segs, inside), note: `Segment ${inside + 1} now starts at ${fmtT(t)}`, changed: true };
  }
  return { state: { ...st, open: t, last: null }, note: `In at ${fmtT(t)}: press ] where this part ends`, changed: true };
}

/** `]` at time `t`. */
export function markOut(st: MarkState, t: number, dur: number): MarkResult {
  t = Math.max(0, dur > 0 ? Math.min(t, dur) : t);
  const segs = st.segments.map((s) => ({ ...s }));
  if (st.open != null) {
    const a = st.open;
    if (t < a + MIN_LEN) return same(st, `The out point has to come after the in (${fmtT(a)})`);
    const next = segs.find((s) => s.in_s >= a);
    let out = t;
    let note = `Segment from ${fmtT(a)} to ${fmtT(out)}`;
    if (next && next.in_s < out) {
      out = next.in_s;
      note = `Stopped at ${fmtT(out)}, where the next segment starts (segments can't overlap)`;
    }
    if (out - a < MIN_LEN) return same(st, "Too short: that's right up against the next segment");
    segs.push({ in_s: a, out_s: out });
    segs.sort((x, y) => x.in_s - y.in_s);
    const idx = segs.findIndex((s) => s.in_s === a);
    return { state: { segments: segs, open: null, last: idx }, note, changed: true };
  }
  const inside = segAt(segs, t);
  if (inside >= 0) {
    if (t - segs[inside].in_s < MIN_LEN) return same(st, null);
    segs[inside].out_s = t;
    return { state: withSegs(st, segs, inside), note: `Segment ${inside + 1} now ends at ${fmtT(t)}`, changed: true };
  }
  // After the playhead's segment: "] again" extends the last segment that
  // ends before here (nothing can start in between, or t would be inside it).
  let prev = -1;
  for (let i = 0; i < segs.length; i++) if (segs[i].out_s <= t) prev = i;
  if (prev >= 0) {
    if (segs[prev].out_s === t) return same(st, null);
    segs[prev].out_s = t;
    return { state: withSegs(st, segs, prev), note: `Segment ${prev + 1} now ends at ${fmtT(t)}`, changed: true };
  }
  // Before every segment, nothing open: a segment from the start.
  const end = segs.length ? Math.min(t, segs[0].in_s) : t;
  if (end < MIN_LEN) return same(st, "Press [ first to mark where the part starts");
  segs.unshift({ in_s: 0, out_s: end });
  return {
    state: { segments: segs, open: null, last: 0 },
    note: `Segment from the start to ${fmtT(end)} (press [ to choose a later start)`,
    changed: true,
  };
}

/** Drag a marker. Clamped between its neighbours and to a minimum length. */
export function moveEdge(st: MarkState, i: number, edge: "in" | "out", t: number, dur: number): MarkState {
  const segs = st.segments.map((s) => ({ ...s }));
  const s = segs[i];
  if (!s) return st;
  if (edge === "in") {
    const lo = i > 0 ? segs[i - 1].out_s : 0;
    s.in_s = Math.max(lo, Math.min(t, s.out_s - MIN_LEN));
  } else {
    const hi = i < segs.length - 1 ? segs[i + 1].in_s : dur > 0 ? dur : Infinity;
    s.out_s = Math.min(hi, Math.max(t, s.in_s + MIN_LEN));
  }
  return withSegs(st, segs, i);
}

/** Drag the open in marker: anywhere not inside a segment. */
export function moveOpen(st: MarkState, t: number, dur: number): MarkState {
  if (st.open == null) return st;
  t = Math.max(0, Math.min(t, dur > 0 ? dur - MIN_LEN : t));
  if (segAt(st.segments, t) >= 0) return st;
  return { ...st, open: t };
}

/** "Remove in point": the segment starts where the previous one ends (or at
 *  the beginning). */
export function removeIn(st: MarkState, i: number): MarkState {
  const segs = st.segments.map((s) => ({ ...s }));
  if (!segs[i]) return st;
  segs[i].in_s = i > 0 ? segs[i - 1].out_s : 0;
  return withSegs(st, segs, i);
}

/** "Remove out point": the segment opens again (its in waits for a new ]),
 *  or, if another in is already open, it runs to the next segment / the end. */
export function removeOut(st: MarkState, i: number, dur: number): MarkState {
  const segs = st.segments.map((s) => ({ ...s }));
  const s = segs[i];
  if (!s) return st;
  if (st.open == null) {
    segs.splice(i, 1);
    return { segments: segs, open: s.in_s, last: null };
  }
  s.out_s = i < segs.length - 1 ? segs[i + 1].in_s : dur;
  return withSegs(st, segs, i);
}

export function removeSeg(st: MarkState, i: number): MarkState {
  const segs = st.segments.filter((_, k) => k !== i);
  return { segments: segs, open: st.open, last: null };
}

export function totalLen(segs: Seg[]): number {
  return segs.reduce((a, s) => a + Math.max(0, s.out_s - s.in_s), 0);
}

/** Snap a time to the clip's frame grid (frame-accurate markers). */
export function snapFrame(t: number, fps: number): number {
  if (!(fps > 0)) return t;
  return Math.round(t * fps) / fps;
}
