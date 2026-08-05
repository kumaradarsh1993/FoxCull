<script lang="ts" module>
  /** A contiguous stretch of the view belonging to one event, in global item
   *  indices (inclusive). Computed once in +page.svelte and handed to whichever
   *  virtual surface is rendering. */
  export type EventRun = { name: string; start: number; end: number };

  /** Width of the gutter the rails live in. Reserved for the whole grid while
   *  rails are on, so cells never shift as you scroll past an event boundary. */
  export const RAIL_W = 30;

  /** Deterministic tint per event name. Events are user-named and unbounded, so
   *  the colour has to come from the name itself — the same trip is the same
   *  colour in every folder and across sessions, with no state to store. */
  export function railHue(name: string): number {
    let h = 0;
    for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
    return Math.abs(h) % 360;
  }
</script>

<script lang="ts">
  // The banner itself: a rounded vertical bar spanning the rows an event
  // occupies, with the name running up it. Deliberately not a cover image —
  // the owner's call after seeing the album-art version: an event should read as
  // a segment marker inside the timeline, not as a separate album screen.
  let {
    name,
    top,
    height,
    width = RAIL_W,
  }: { name: string; top: number; height: number; width?: number } = $props();

  let hue = $derived(railHue(name));
  // Below this a rotated label has nowhere to go; the bar alone still marks the
  // stretch, which is the load-bearing part.
  let showLabel = $derived(height >= 92);
</script>

<div
  class="rail"
  style="top:{top}px; height:{height}px; width:{width - 8}px; --rh:{hue}"
  title={name}
>
  {#if showLabel}
    <span class="railName" style="max-width:{height - 24}px">{name}</span>
  {/if}
</div>

<style>
  .rail {
    position: absolute;
    left: 0;
    border-radius: 999px;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    background: linear-gradient(
      180deg,
      hsl(var(--rh) 62% 52% / 0.92),
      hsl(calc(var(--rh) + 26) 58% 44% / 0.92)
    );
    box-shadow: inset 0 0 0 1px hsl(var(--rh) 62% 70% / 0.45);
  }
  .railName {
    /* Bottom-to-top so the name reads naturally when you tilt your head left,
       which is the convention for spine text. */
    writing-mode: vertical-rl;
    transform: rotate(180deg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--font-display);
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    color: #fff;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.4);
  }
</style>
