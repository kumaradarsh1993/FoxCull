// The Reel window's rules (docs/design/segments-and-reel-mode.md §C), as pure
// functions so they can be tested apart from the UI.
//
// A reel is the song's chosen section cut into consecutive windows, one per
// piece (a clip, or one marked segment of a clip), each cut on a beat:
//
//   song:   |S ────── c1 ────── c2 ─────────── c3 ── … ── E|
//   piece:   [ clip 1 ][ clip 2 ][   clip 3    ] …
//
// Each piece shows the part of its clip starting at its offset (the window's
// position on the clip's strip, which the owner drags) for the length of its
// slot. The owner's rules:
//   * Default: every window starts at the beginning of its clip, and the
//     lengths spread the section over the pieces, cut on the strong beats
//     (downbeats), and never longer than the clip.
//   * Moving a window changes which part of the clip plays, never its length.
//   * Resizing a window snaps its end to a beat. The pieces after it keep
//     their offsets and re-flow: their lengths come from the beats that are
//     left. (A length the owner set by hand is kept, re-snapped to the beat
//     nearest to it.)
//   * A window longer than what's left of its clip overflows: shown in red,
//     and the export waits until it's fixed.

export interface Piece {
  /** `${path}#${segment index}`, or `${path}#all` for the whole clip. */
  key: string;
  path: string;
  name: string;
  /** The part of the clip this piece may use. */
  srcIn: number;
  srcOut: number;
  /** Which marked segment (0-based), or null for the whole clip. */
  seg: number | null;
  segCount: number;
}

export interface Slot {
  /** Song time (absolute seconds in the song) where this piece starts. */
  start: number;
  len: number;
}

export interface Beats {
  beats: number[];
  major: number[];
}

export const pieceLen = (p: Piece) => Math.max(0, p.srcOut - p.srcIn);

/** Pieces for a clip: its segments (when used), else the whole clip. */
export function piecesFor(
  path: string,
  name: string,
  duration: number,
  segs: { in_s: number; out_s: number }[],
  useSegs: boolean,
): Piece[] {
  if (useSegs && segs.length) {
    return segs.map((s, i) => ({
      key: `${path}#${i}`,
      path,
      name,
      srcIn: Math.max(0, s.in_s),
      srcOut: duration > 0 ? Math.min(duration, s.out_s) : s.out_s,
      seg: i,
      segCount: segs.length,
    }));
  }
  return [{ key: `${path}#all`, path, name, srcIn: 0, srcOut: duration, seg: null, segCount: segs.length }];
}

const EPS = 1e-6;

/** Beats strictly after `a` and at most `b`. */
function between(list: number[], a: number, b: number) {
  return list.filter((x) => x > a + EPS && x <= b + EPS);
}

/** The usual gap between beats (for "at least one beat"). */
export function beatGap(beats: number[]): number {
  if (beats.length < 2) return 0.5;
  const gaps = beats.slice(1).map((b, i) => b - beats[i]).sort((x, y) => x - y);
  return gaps[Math.floor(gaps.length / 2)] || 0.5;
}

/** Where a piece starting at `c` ends by default. `avail` = the clip's
 *  usable length, `target` = its fair share of what's left of the section. */
export function defaultEnd(c: number, avail: number, target: number, b: Beats, secEnd: number): number {
  const gap = beatGap(b.beats);
  const minEnd = c + Math.min(0.2, gap * 0.4);
  const limit = Math.min(c + avail, secEnd);
  // The section end counts as a strong cut point.
  const majors = [...between(b.major, minEnd, limit)];
  if (secEnd > minEnd && secEnd <= limit + EPS && !majors.some((m) => Math.abs(m - secEnd) < 0.05)) majors.push(secEnd);
  const all = between(b.beats, minEnd, limit);
  const pool = majors.length ? majors : all;
  if (!pool.length) {
    // Shorter than one beat (or no beats found): run to the next beat anyway,
    // which overflows and shows red, rather than cut off the beat.
    const next = b.beats.find((x) => x > minEnd);
    if (next != null) return Math.min(next, secEnd);
    // No beats found at all (a near-silent track): an even share.
    return Math.min(c + Math.max(Math.min(avail, target), 0.2), secEnd);
  }
  const want = c + target;
  let best = pool[0];
  for (const x of pool) {
    // Nearest to the fair share; on a tie, the later one (lean long).
    if (Math.abs(x - want) < Math.abs(best - want) - EPS || (Math.abs(Math.abs(x - want) - Math.abs(best - want)) <= EPS && x > best)) best = x;
  }
  return best;
}

/** The beat nearest to `want` after `c` (any beat), for a length set by hand. */
function nearestBeat(c: number, want: number, b: Beats, secEnd: number): number {
  const gap = beatGap(b.beats);
  const cand = b.beats.filter((x) => x > c + gap * 0.4);
  if (!cand.length) return Math.min(want, secEnd);
  let best = cand[0];
  for (const x of cand) if (Math.abs(x - want) < Math.abs(best - want)) best = x;
  return Math.min(best, secEnd);
}

/** Lay the pieces out on the song section. `pins`: lengths set by hand,
 *  by piece key. Null = past the end of the section (doesn't fit). */
export function layout(pieces: Piece[], b: Beats, secStart: number, secEnd: number, pins: Record<string, number>): (Slot | null)[] {
  const out: (Slot | null)[] = [];
  let c = secStart;
  for (let i = 0; i < pieces.length; i++) {
    const p = pieces[i];
    if (c >= secEnd - 0.05) {
      out.push(null);
      continue;
    }
    const pin = pins[p.key];
    let e: number;
    if (pin != null && pin > 0) e = nearestBeat(c, c + pin, b, secEnd);
    else e = defaultEnd(c, pieceLen(p), (secEnd - c) / (pieces.length - i), b, secEnd);
    e = Math.min(Math.max(e, c + 0.1), secEnd);
    out.push({ start: c, len: e - c });
    c = e;
  }
  return out;
}

/** Snap a dragged end (song time) to a beat: a downbeat within `radius`
 *  seconds wins, else the nearest beat. `free` (⌥) doesn't snap. */
export function snapEnd(c: number, raw: number, b: Beats, radius: number, free: boolean): number {
  const gap = beatGap(b.beats);
  const min = c + Math.min(0.2, gap * 0.4);
  if (free || !b.beats.length) return Math.max(min, raw);
  const majors = b.major.filter((x) => x > min && Math.abs(x - raw) <= radius);
  if (majors.length) return majors.reduce((best, x) => (Math.abs(x - raw) < Math.abs(best - raw) ? x : best));
  const cand = b.beats.filter((x) => x > min);
  if (!cand.length) return Math.max(min, raw);
  return cand.reduce((best, x) => (Math.abs(x - raw) < Math.abs(best - raw) ? x : best));
}

/** How far a window runs past the end of its clip (0 = fits). */
export function overflowOf(p: Piece, offset: number, slot: Slot | null): number {
  if (!slot) return 0;
  return Math.max(0, offset + slot.len - pieceLen(p));
}

/** The longest beat-aligned length that fits what's left of the clip after
 *  `offset` (for "Fix": shorten an overflowing window). */
export function longestFit(p: Piece, offset: number, slot: Slot, b: Beats): number | null {
  const room = pieceLen(p) - offset;
  const fits = b.beats.filter((x) => x > slot.start + 0.05 && x - slot.start <= room + EPS);
  if (!fits.length) return null;
  return fits[fits.length - 1] - slot.start;
}

/** Song section default: from the first downbeat, about 2.5 s a piece
 *  (8–60 s), ending on a downbeat. */
export function defaultSection(b: Beats, duration: number, pieces: Piece[]): [number, number] {
  const start = b.major[0] ?? b.beats[0] ?? 0;
  const total = pieces.reduce((s, p) => s + pieceLen(p), 0);
  const want = Math.min(Math.max(8, pieces.length * 2.5), 60, total || 60);
  const end = start + want;
  const majors = b.major.filter((m) => m > start + 1);
  let e = majors.length ? majors.reduce((best, m) => (Math.abs(m - end) < Math.abs(best - end) ? m : best)) : end;
  e = Math.min(e, duration);
  return [start, Math.max(start + 1, e)];
}

/** Snap a section edge to a downbeat within `radius` seconds. */
export function snapToMajor(t: number, b: Beats, radius: number): number {
  let best = t;
  let d = radius;
  for (const m of b.major) {
    if (Math.abs(m - t) <= d) {
      d = Math.abs(m - t);
      best = m;
    }
  }
  return best;
}

export function fmtS(s: number): string {
  if (!Number.isFinite(s)) return "–";
  const m = Math.floor(s / 60);
  const sec = s - m * 60;
  return m ? `${m}:${sec.toFixed(1).padStart(4, "0")}` : `${sec.toFixed(1)} s`;
}
