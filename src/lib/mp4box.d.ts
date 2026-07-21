// Minimal ambient types for mp4box.js — it ships no TypeScript definitions.
// We keep the surface loose on purpose: the scrub engine touches a handful of
// well-known fields (onReady/appendBuffer/getTrackById/samples) and validates
// shapes at runtime; a hand-maintained full typing would just rot.
declare module "mp4box" {
  export function createFile(): any;
  export const DataStream: any;
}
