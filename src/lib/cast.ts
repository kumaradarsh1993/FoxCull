// Thin, typed wrappers over the Rust Chromecast ("cast to TV") commands.
// No UI here — a separate component owns the Cast button and calls into this.
//
// The Rust side (src-tauri/src/cast.rs): discovers Google Cast devices via
// mDNS, runs a tiny local HTTP server that streams the current file to the TV
// (Range-enabled so video seeks), and drives the CASTV2 protocol to LOAD it on
// the TV's Default Media Receiver at native quality (no transcoding).

import { invoke } from "@tauri-apps/api/core";

/** A Cast device found on the LAN. `addr` is an IPv4 string; `port` is ~8009. */
export interface CastDevice {
  id: string;
  name: string;
  addr: string;
  port: number;
}

/** Current cast state, for driving the Cast button's appearance. */
export interface CastStatus {
  connected: boolean;
  deviceName: string | null;
  playingPath: string | null;
}

export const cast = {
  /** Browse mDNS for Google Cast devices for up to `timeoutMs` (default 3s). */
  discover: (timeoutMs = 3000) =>
    invoke<CastDevice[]>("cast_discover", { timeoutMs }),

  /** Cast the file at `path` to `device`; returns the resulting status. */
  start: (path: string, device: CastDevice) =>
    invoke<CastStatus>("cast_start", {
      path,
      deviceAddr: device.addr,
      devicePort: device.port,
      deviceName: device.name,
    }),

  /** Stop casting and close the connection to the TV. */
  stop: () => invoke<CastStatus>("cast_stop"),

  // Transport mirroring: what the laptop's player does, the TV does. All three
  // are best-effort — with no live session they are no-ops on the Rust side, so
  // callers (a keypress, a controller trigger) never have to check first.
  play: () => invoke<void>("cast_play").catch(() => {}),
  pause: () => invoke<void>("cast_pause").catch(() => {}),
  /** Seek the TV to `position` seconds. Throttle this — see the page. */
  seek: (position: number) => invoke<void>("cast_seek", { position }).catch(() => {}),

  /** Poll the current cast status (connected? which device? what's playing?). */
  status: () => invoke<CastStatus>("cast_status"),
};
