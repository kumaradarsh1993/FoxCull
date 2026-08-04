<script lang="ts">
  // The cover frame behind an event's band ("album art"). Deliberately its own
  // tiny component rather than logic inside SectionedGrid's header loop: each
  // band needs its own async fetch, and headers are keyed/recreated as sections
  // change. Goes through the same bounded loader as grid tiles, so a wall of
  // event bands can never outrank the photos the user is actually looking at.
  import { loadThumb, loadVideoPoster, cancelThumb } from "$lib/thumbnail-loader";

  let { path, size = 320 }: { path: string; size?: number } = $props();

  let src = $state<string | null>(null);
  let loaded = $state(false);

  $effect(() => {
    const p = path;
    const tier = size;
    let alive = true;
    src = null;
    loaded = false;
    (async () => {
      // A video cover has no still to decode — fall back to its poster frame.
      let url = await loadThumb(p, tier);
      if (!url && alive) url = await loadVideoPoster(p);
      if (alive) src = url;
    })();
    return () => {
      alive = false;
      cancelThumb(p, tier);
    };
  });
</script>

{#if src}
  <img class="cover" class:in={loaded} {src} alt="" draggable="false" decoding="async" onload={() => (loaded = true)} />
{/if}

<style>
  .cover {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 220ms ease;
  }
  .cover.in {
    opacity: 1;
  }
</style>
