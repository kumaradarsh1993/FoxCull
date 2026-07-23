// Game-controller culling — PS5 DualSense / PS4 DualShock (or any pad the
// browser exposes with the "standard" mapping) drives the whole review flow:
// plug the laptop into the TV, pair the pad over Bluetooth, and walk the room
// while flipping/marking photos. Built on the WebView's Gamepad API, which has
// no events for button presses — so we poll on requestAnimationFrame, do our
// own edge detection, and add hold-to-repeat for navigation and analog
// hold-to-shuttle for the triggers.
//
// Bindings are REMAPPABLE: `settings.s.padBindings` stores per-action button
// overrides (Controller panel binds by "press a button now"), merged over the
// DualSense-shaped defaults below. The page registers ONE dispatch handler
// (+page.svelte) that turns action ids into the same functions the keyboard
// uses — this module never touches app state directly.
//
// ## Sticks are bindable too (2026-07-22)
// A pad has 16-18 buttons and the review flow needs ~28 actions, so the two
// analog sticks are exposed as eight synthetic "buttons" (indices 100-107, see
// AXIS_BUTTONS). That keeps the binding store a flat action -> number map — no
// schema change, no second code path in the poll loop — and gives the ratings
// and colour labels a home each: flick the left stick for stars, the right for
// labels. Everything downstream treats them as ordinary button indices.

import { settings, PAD_BINDINGS_VERSION } from "./settings.svelte";

export type PadActionId =
  | "prev"
  | "next"
  | "up"
  | "down"
  | "pick"
  | "reject"
  | "clearMarks"
  | "rate1"
  | "rate2"
  | "rate3"
  | "rate4"
  | "rate5"
  | "label1"
  | "label2"
  | "label3"
  | "label4"
  | "label5"
  | "playPause"
  | "seekBack"
  | "seekFwd"
  | "markIn"
  | "markOut"
  | "toggleView"
  | "gridView"
  | "focusView"
  | "fullscreen"
  | "toggleFilmstrip"
  | "info"
  | "help";

export interface PadActionDef {
  id: PadActionId;
  label: string;
  group: "Navigate" | "Mark" | "Label" | "View" | "Video";
  /** Holding the button re-fires the action (nav + shuttle). */
  repeat?: boolean;
}

export const PAD_ACTIONS: PadActionDef[] = [
  { id: "prev", label: "Previous item", group: "Navigate", repeat: true },
  { id: "next", label: "Next item", group: "Navigate", repeat: true },
  { id: "up", label: "Up a row", group: "Navigate", repeat: true },
  { id: "down", label: "Down a row", group: "Navigate", repeat: true },
  { id: "pick", label: "Pick ⚑", group: "Mark" },
  { id: "reject", label: "Reject ✕", group: "Mark" },
  { id: "clearMarks", label: "Clear all marks", group: "Mark" },
  { id: "rate1", label: "Rate ★", group: "Mark" },
  { id: "rate2", label: "Rate ★★", group: "Mark" },
  { id: "rate3", label: "Rate ★★★", group: "Mark" },
  { id: "rate4", label: "Rate ★★★★", group: "Mark" },
  { id: "rate5", label: "Rate ★★★★★", group: "Mark" },
  // Label order matches the keyboard digits 6/7/8/9/0, so the stick and the
  // number row stay one mental model.
  { id: "label1", label: "Label Blue", group: "Label" },
  { id: "label2", label: "Label Purple", group: "Label" },
  { id: "label3", label: "Label Red", group: "Label" },
  { id: "label4", label: "Label Green", group: "Label" },
  { id: "label5", label: "Label Yellow", group: "Label" },
  { id: "toggleView", label: "Open / close Focus", group: "View" },
  { id: "gridView", label: "Grid view", group: "View" },
  { id: "focusView", label: "Focus view", group: "View" },
  { id: "fullscreen", label: "Play mode (fullscreen)", group: "View" },
  { id: "toggleFilmstrip", label: "Show / hide filmstrip", group: "View" },
  { id: "info", label: "Info overlay", group: "View" },
  { id: "help", label: "Button guide", group: "View" },
  { id: "playPause", label: "Play / pause video", group: "Video" },
  { id: "seekBack", label: "Shuttle back (hold)", group: "Video", repeat: true },
  { id: "seekFwd", label: "Shuttle forward (hold)", group: "Video", repeat: true },
  { id: "markIn", label: "Mark In point", group: "Video" },
  { id: "markOut", label: "Mark Out point", group: "Video" },
];

// ── analog sticks as synthetic buttons ──────────────────────────────────────
// Standard-mapping axes: 0/1 = left stick X/Y, 2/3 = right stick X/Y, Y
// negative being UP. Indices start at 100 so they can never collide with a real
// button index (a pad reports at most ~20).
export const AXIS_BUTTON_BASE = 100;
/** [pseudo-button index, axis, sign] — sign is the direction that fires it. */
const AXIS_BUTTONS: [number, number, 1 | -1][] = [
  [100, 1, -1], // Left stick ↑
  [101, 0, 1], //  Left stick →
  [102, 1, 1], //  Left stick ↓
  [103, 0, -1], // Left stick ←
  [104, 3, -1], // Right stick ↑
  [105, 2, 1], //  Right stick →
  [106, 3, 1], //  Right stick ↓
  [107, 2, -1], // Right stick ←
];

/** Push past this to fire; fall back under AXIS_RELEASE to re-arm. The gap is
 *  hysteresis — a stick resting near the threshold would otherwise machine-gun
 *  the action, and a worn DualSense stick rarely returns to a clean 0. */
const AXIS_PRESS = 0.6;
const AXIS_RELEASE = 0.4;

/** Human names for the standard-mapping button indices (PlayStation glyphs). */
export const BUTTON_NAMES: Record<number, string> = {
  0: "✕ Cross",
  1: "○ Circle",
  2: "□ Square",
  3: "△ Triangle",
  4: "L1",
  5: "R1",
  6: "L2",
  7: "R2",
  8: "Create / Share",
  9: "Options",
  10: "L3 (stick click)",
  11: "R3 (stick click)",
  12: "D-Pad Up",
  13: "D-Pad Down",
  14: "D-Pad Left",
  15: "D-Pad Right",
  16: "PS",
  17: "Touchpad",
  100: "Left stick ↑",
  101: "Left stick →",
  102: "Left stick ↓",
  103: "Left stick ←",
  104: "Right stick ↑",
  105: "Right stick →",
  106: "Right stick ↓",
  107: "Right stick ←",
};

export function buttonName(idx: number): string {
  if (idx < 0) return "—";
  return BUTTON_NAMES[idx] ?? `Button ${idx}`;
}

/** DualSense/DualShock "standard" mapping defaults. -1 = unbound (bind it in
 *  the Controller panel).
 *
 *  Laid out 2026-07-22 around how the owner actually culls at the TV: the four
 *  action buttons are the verbs he presses most (reject / pick / clear /
 *  play), the touchpad click is "enter" (it is the only button that reads as
 *  a click rather than a glyph), the shoulders mark video in/out, the triggers
 *  shuttle, and the sticks carry the 5 ratings and 5 colour labels — the
 *  actions there are simply too many for the buttons that remain. Bumping
 *  PAD_BINDINGS_VERSION resets stored overrides so this reaches existing
 *  installs; without that, `bind()` having written a full map once would pin
 *  the old defaults forever. */
export const DEFAULT_BINDINGS: Record<PadActionId, number> = {
  prev: 14, // D-Pad Left
  next: 15, // D-Pad Right
  up: 12, // D-Pad Up
  down: 13, // D-Pad Down
  reject: 0, // ✕ Cross
  clearMarks: 1, // ○ Circle
  playPause: 2, // □ Square
  pick: 3, // △ Triangle
  markIn: 4, // L1
  markOut: 5, // R1
  seekBack: 6, // L2 (analog)
  seekFwd: 7, // R2 (analog)
  toggleFilmstrip: 8, // Create / Share
  help: 9, // Options
  rate5: 10, // L3 — the fifth star is the stick's own click
  label5: 11, // R3 — likewise the fifth label
  fullscreen: 16, // PS
  toggleView: 17, // Touchpad click = enter/leave Focus
  rate1: 100, // Left stick ↑
  rate2: 101, // Left stick →
  rate3: 102, // Left stick ↓
  rate4: 103, // Left stick ←
  label1: 104, // Right stick ↑ — Blue
  label2: 105, // Right stick → — Purple
  label3: 106, // Right stick ↓ — Red
  label4: 107, // Right stick ← — Green
  // Nothing sensible is left on a DualSense for these; they stay available in
  // the Controller panel for anyone who wants them.
  gridView: -1,
  focusView: -1,
  info: -1,
};

// Hold-to-repeat pacing: navigation steps at a readable clip after a short
// grace period. Triggers are deliberately slower to enter repeat: an ordinary
// squeeze must remain one predictable 5-second skip, not become a surprise
// second/third seek while the analog trigger is still travelling.
const NAV_REPEAT_DELAY_MS = 380;
const NAV_REPEAT_EVERY_MS = 135;
const SHUTTLE_DELAY_MS = 500;
const SHUTTLE_EVERY_MS = 250;

type PadHandler = (action: PadActionId, strength: number) => void;

/** One polled input: a real button or a synthetic stick direction. */
interface RawInput {
  idx: number;
  pressed: boolean;
  value: number;
}

class Pad {
  connected = $state(false);
  /** The pad's self-reported id, e.g. "DualSense Wireless Controller". */
  name = $state("");
  /** Set while the Controller panel waits for a press to bind. */
  capturing = $state(false);
  /** Live indices held down right now — drives the panel's button tester, so
   *  "does this pad even report PS / the touchpad?" is answered by pressing it
   *  rather than by us guessing what Chromium exposes on this OS. */
  pressedNow = $state<number[]>([]);

  private handler: PadHandler | null = null;
  private captureCb: ((button: number) => void) | null = null;
  private raf = 0;
  private prev = new Map<number, boolean>();
  private holdSince = new Map<number, number>();
  private lastFire = new Map<number, number>();

  /** Effective bindings: stored overrides on top of the DualSense defaults. */
  bindings(): Record<PadActionId, number> {
    return { ...DEFAULT_BINDINGS, ...(settings.s.padBindings as Record<PadActionId, number>) };
  }

  /** Button currently bound to `action` (-1 = unbound). */
  buttonFor(action: PadActionId): number {
    return this.bindings()[action] ?? -1;
  }

  /** Persist a binding; any other action holding this button is unbound. */
  async bind(action: PadActionId, button: number) {
    const next: Record<string, number> = { ...this.bindings() };
    for (const [a, b] of Object.entries(next)) {
      if (b === button && a !== action) next[a] = -1;
    }
    next[action] = button;
    await settings.set({ padBindings: next, padBindingsVersion: PAD_BINDINGS_VERSION });
  }

  async unbind(action: PadActionId) {
    await settings.set({ padBindings: { ...this.bindings(), [action]: -1 } });
  }

  async resetBindings() {
    await settings.set({ padBindings: {}, padBindingsVersion: PAD_BINDINGS_VERSION });
  }

  /** Panel remap flow: resolve with the next pressed button. Returns a cancel. */
  captureNextButton(cb: (button: number) => void): () => void {
    this.captureCb = cb;
    this.capturing = true;
    return () => {
      this.captureCb = null;
      this.capturing = false;
    };
  }

  /** Install the page's dispatcher and start polling. Idempotent. */
  start(handler: PadHandler) {
    this.handler = handler;
    if (this.raf) return;
    const tick = () => {
      this.raf = requestAnimationFrame(tick);
      this.poll();
    };
    this.raf = requestAnimationFrame(tick);
  }

  stop() {
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.handler = null;
    this.prev.clear();
    this.pressedNow = [];
  }

  /** Flatten the pad into one list of inputs so buttons and stick directions
   *  share a single edge-detection path below. */
  private read(gp: Gamepad): RawInput[] {
    const out: RawInput[] = [];
    for (let i = 0; i < gp.buttons.length; i++) {
      const b = gp.buttons[i];
      // Triggers report analog values; count a light squeeze as pressed.
      out.push({ idx: i, pressed: b.pressed || b.value > 0.35, value: b.value || 1 });
    }
    for (const [idx, axis, sign] of AXIS_BUTTONS) {
      const v = (gp.axes[axis] ?? 0) * sign;
      // Hysteresis: once held, stay held until the stick falls back past the
      // lower threshold.
      const held = this.prev.get(idx) ?? false;
      out.push({ idx, pressed: v >= (held ? AXIS_RELEASE : AXIS_PRESS), value: Math.min(1, v) });
    }
    return out;
  }

  private poll() {
    const pads = typeof navigator !== "undefined" && navigator.getGamepads ? navigator.getGamepads() : [];
    let gp: Gamepad | null = null;
    for (const p of pads) {
      if (p && p.connected) {
        gp = p;
        break;
      }
    }
    if (!gp) {
      if (this.connected) {
        this.connected = false;
        this.name = "";
      }
      this.prev.clear();
      if (this.pressedNow.length) this.pressedNow = [];
      return;
    }
    if (!this.connected) {
      this.connected = true;
      this.name = gp.id;
    }
    if (!settings.s.padEnabled && !this.captureCb) {
      this.prev.clear();
      if (this.pressedNow.length) this.pressedNow = [];
      return;
    }

    const now = performance.now();
    const byButton = new Map<number, PadActionDef>();
    const bound = this.bindings();
    for (const def of PAD_ACTIONS) {
      const b = bound[def.id];
      if (b != null && b >= 0) byButton.set(b, def);
    }

    const inputs = this.read(gp);
    const held: number[] = [];
    for (const { idx, pressed, value } of inputs) {
      if (pressed) held.push(idx);
      const was = this.prev.get(idx) ?? false;
      if (pressed && !was) {
        this.prev.set(idx, true);
        this.holdSince.set(idx, now);
        this.lastFire.set(idx, now);
        if (this.captureCb) {
          const cb = this.captureCb;
          this.captureCb = null;
          this.capturing = false;
          cb(idx);
          continue;
        }
        const def = byButton.get(idx);
        if (def) this.handler?.(def.id, value);
      } else if (pressed && was) {
        const def = byButton.get(idx);
        if (def?.repeat && !this.captureCb) {
          const shuttle = def.id === "seekBack" || def.id === "seekFwd";
          const delay = shuttle ? SHUTTLE_DELAY_MS : NAV_REPEAT_DELAY_MS;
          const every = shuttle ? SHUTTLE_EVERY_MS : NAV_REPEAT_EVERY_MS;
          if (now - (this.holdSince.get(idx) ?? now) >= delay && now - (this.lastFire.get(idx) ?? 0) >= every) {
            this.lastFire.set(idx, now);
            this.handler?.(def.id, value);
          }
        }
      } else {
        this.prev.set(idx, false);
      }
    }
    // Only publish when it actually changed — this runs 60x/second and every
    // assignment would otherwise re-render the Controller panel.
    if (held.length !== this.pressedNow.length || held.some((v, i) => v !== this.pressedNow[i])) {
      this.pressedNow = held;
    }
  }
}

export const pad = new Pad();
