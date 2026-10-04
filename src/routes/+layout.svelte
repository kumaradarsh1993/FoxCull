<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { settings } from "$lib/settings.svelte";
  import { activity } from "$lib/activity.svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { windowKind } from "$lib/windows";
  import EditWindow from "$lib/components/EditWindow.svelte";
  import MergeWindow from "$lib/components/MergeWindow.svelte";
  import ReelWindow from "$lib/components/ReelWindow.svelte";

  // Every FoxCull window loads this app; its label decides what it is. The
  // library page (children) only ever mounts in the main window.
  const kind = windowKind();

  /** Page zoom per interface size. TV makes everything 22% larger for a screen
   *  across the room; Compact gives a small laptop 10% more canvas. */
  const ZOOM = { compact: 0.9, comfortable: 1, distance: 1.22 } as const;
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
    void getCurrentWebview()
      .setZoom(ZOOM[settings.s.uiScale] ?? 1)
      .catch(() => {});
  });
</script>

{#if kind === "edit"}
  <EditWindow />
{:else if kind === "merge"}
  <MergeWindow />
{:else if kind === "reel"}
  <ReelWindow />
{:else}
  {@render children()}
{/if}
