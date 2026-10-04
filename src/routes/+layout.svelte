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

  // "Match system" follows the OS's light/dark switch, live.
  const darkMq = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null;
  let systemDark = $state(darkMq?.matches ?? true);
  darkMq?.addEventListener("change", (e) => (systemDark = e.matches));
  const LIGHT_THEMES = new Set(["daylight", "paper"]);

  // Apply appearance choices at the document root so component-scoped styles
  // and native-looking overlays share one visual system. data-tone tells the
  // accent which shade to use; data-platform lets the chrome follow the OS.
  $effect(() => {
    const root = document.documentElement;
    const t = settings.s.theme === "system" ? (systemDark ? "graphite" : "daylight") : settings.s.theme;
    root.setAttribute("data-theme", t);
    root.setAttribute("data-tone", LIGHT_THEMES.has(t) ? "light" : "dark");
    if (settings.s.accent && settings.s.accent !== "theme") root.setAttribute("data-accent", settings.s.accent);
    else root.removeAttribute("data-accent");
    root.setAttribute("data-surround", settings.s.surround ?? "dark");
    root.setAttribute("data-platform", /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent) ? "mac" : "other");
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
