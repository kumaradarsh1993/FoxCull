<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { settings } from "$lib/settings.svelte";
  import { activity } from "$lib/activity.svelte";
  let { children } = $props();

  onMount(() => {
    settings.init();
    activity.init(); // start listening for backend `activity` events
  });

  // Apply appearance choices at the document root so component-scoped styles
  // and native-looking overlays share one visual system.
  $effect(() => {
    document.documentElement.setAttribute("data-theme", settings.s.theme);
    document.documentElement.setAttribute("data-ui-scale", settings.s.uiScale);
  });
</script>

{@render children()}
