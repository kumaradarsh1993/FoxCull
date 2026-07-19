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

import { settings } from "./settings.svelte";

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
  | "playPause"
  | "seekBack"
  | "seekFwd"
  | "gridView"
  | "focusView"
  | "fullscreen"
  | "info"
  | "help";

export interface PadActionDef {
  id: PadActionId;
  label: string;
  group: "Navigate" | "Mark" | "View" | "Video";
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
  { id: "clearMarks", label: "Clear marks", group: "Mark" },
  { id: "rate1", label: "Rate ★", group: "Mark" },
  { id: "rate2", label: "Rate ★★", group: "Mark" },
  { id: "rate3", label: "Rate ★★★", group: "Mark" },
  { id: "rate4", label: "Rate ★★★★", group: "Mark" },
  { id: "rate5", label: "Rate ★★★★★", group: "Mark" },
  { id: "gridView", label: "Grid view", group: "View" },
  { id: "focusView", label: "Focus view", group: "View" },
  { id: "fullscreen", label: "Play mode (fullscreen)", group: "View" },
  { id: "info", label: "Info overlay", group: "View" },
  { id: "help", label: "Button guide", group: "View" },
  { id: "playPause", label: "Play / pause video", group: "Video" },
  { id: "seekBack", label: "Shuttle back (hold)", group: "Video", repeat: true },
  { id: "seekFwd", label: "Shuttle forward (hold)", group: "Video", repeat: true },
];

/** DualSense/DualShock "standard" mapping defaults. -1 = unbound (bind it in
 *  the Controller panel). Cross picks, Circle rejects — the user's two core
 *  culling verbs on the two thumb-nearest buttons. */
export const DEFAULT_BINDINGS: Record<PadActionId, number> = {
  prev: 14, // D-Pad Left
  next: 15, // D-Pad Right
  up: 12, // D-Pad Up
  down: 13, // D-Pad Down
  pick: 0, // ✕ Cross
  reject: 1, // ○ Circle
  playPause: 2, // □ Square
  clearMarks: 3, // △ Triangle
  gridView: 4, // L1
  focusView: 5, // R1
  seekBack: 6, // L2 (analog)
  seekFwd: 7, // R2 (analog)
  help: 8, // Create / Share
  fullscreen: 9, // Options
  info: 10, // L3
  rate1: -1,
  rate2: -1,
  rate3: -1,
  rate4: -1,
  rate5: -1,
};

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
};

export function buttonName(idx: number): string {
  if (idx < 0) return "—";
  return BUTTON_NAMES[idx] ?? `Button ${idx}`;
}

// Hold-to-repeat pacing: navigation steps at a readable clip after a short
// grace period; the analog triggers shuttle faster and start sooner.
const NAV_REPEAT_DELAY_MS = 380;
const NAV_REPEAT_EVERY_MS = 135;
const SHUTTLE_DELAY_MS = 220;
const SHUTTLE_EVERY_MS = 120;

type PadHandler = (action: PadActionId, strength: number) => void;

class Pad {
  connected = $state(false);
  /** The pad's self-reported id, e.g. "DualSense Wireless Controller". */
  name = $state("");
  /** Set while the Controller panel waits for a press to bind. */
  capturing = $state(false);

  private handler: PadHandler | null = null;
  private captureCb: ((button: number) => void) | null = null;
  private raf = 0;
  private prev: boolean[] = [];
  private holdSince: number[] = [];
  private lastFire: number[] = [];

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
    await settings.set({ padBindings: next });
  }

  async unbind(action: PadActionId) {
    await settings.set({ padBindings: { ...this.bindings(), [action]: -1 } });
  }

  async resetBindings() {
    await settings.set({ padBindings: {} });
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
    this.prev = [];
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
      this.prev = [];
      return;
    }
    if (!this.connected) {
      this.connected = true;
      this.name = gp.id;
    }
    if (!settings.s.padEnabled && !this.captureCb) {
      this.prev = [];
      return;
    }

    const now = performance.now();
    const byButton = new Map<number, PadActionDef>();
    const bound = this.bindings();
    for (const def of PAD_ACTIONS) {
      const b = bound[def.id];
      if (b != null && b >= 0) byButton.set(b, def);
    }

    for (let i = 0; i < gp.buttons.length; i++) {
      const b = gp.buttons[i];
      // Triggers report analog values; count a light squeeze as pressed.
      const pressed = b.pressed || b.value > 0.35;
      const was = this.prev[i] ?? false;
      if (pressed && !was) {
        this.prev[i] = true;
        this.holdSince[i] = now;
        this.lastFire[i] = now;
        if (this.captureCb) {
          const cb = this.captureCb;
          this.captureCb = null;
          this.capturing = false;
          cb(i);
          continue;
        }
        const def = byButton.get(i);
        if (def) this.handler?.(def.id, b.value || 1);
      } else if (pressed && was) {
        const def = byButton.get(i);
        if (def?.repeat && !this.captureCb) {
          const shuttle = def.id === "seekBack" || def.id === "seekFwd";
          const delay = shuttle ? SHUTTLE_DELAY_MS : NAV_REPEAT_DELAY_MS;
          const every = shuttle ? SHUTTLE_EVERY_MS : NAV_REPEAT_EVERY_MS;
          if (now - (this.holdSince[i] ?? now) >= delay && now - (this.lastFire[i] ?? 0) >= every) {
            this.lastFire[i] = now;
            this.handler?.(def.id, b.value || 1);
          }
        }
      } else if (!pressed) {
        this.prev[i] = false;
      }
    }
  }
}

export const pad = new Pad();
