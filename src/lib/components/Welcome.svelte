<script lang="ts">
  // First-open placeholder for the viewport. Shown whenever no folder is open,
  // which on a fresh install is the first thing anyone sees.
  //
  // Deliberately quiet: one sentence of guidance, one button, and a short list
  // of real places to start. It never opens anything on its own. The failure it
  // exists to prevent is landing in a whole system drive (241k files, minutes of
  // scanning, a cache on the boot disk); steering toward a specific folder is
  // the whole job.
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { SuggestedFolder } from "$lib/types";

  let {
    treeVisible,
    resumeDir = null,
    onopen,
    onpick,
    onshowtree,
    onexcludes,
  }: {
    /** Whether the folder sidebar is showing (the copy points at it). */
    treeVisible: boolean;
    /** A system drive root the app declined to reopen at launch, if any. */
    resumeDir?: string | null;
    onopen: (path: string) => void;
    onpick: () => void;
    onshowtree: () => void;
    onexcludes: () => void;
  } = $props();

  let suggestions = $state<SuggestedFolder[]>([]);

  onMount(async () => {
    // Camera cards first: a card in the reader is almost certainly why the app
    // was just opened.
    const all = await api.suggestedFolders();
    suggestions = [...all.filter((s) => s.kind === "card"), ...all.filter((s) => s.kind !== "card")].slice(0, 5);
  });

  /** Reads as the start of a sentence: "Macintosh HD", "Your startup disk", "The C: drive". */
  function driveLabel(p: string): string {
    if (p === "/") return "Your startup disk";
    const letter = p.match(/^([A-Za-z]:)[\\/]?$/);
    if (letter) return `The ${letter[1].toUpperCase()} drive`;
    return p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || p;
  }

  /** `/Users/me/Pictures` → `~/Pictures`; `/Volumes/SD_Card/DCIM` → `SD_Card/DCIM`. */
  function shortPath(p: string): string {
    const mac = p.match(/^\/Users\/[^/]+(\/.*)?$/);
    if (mac) return "~" + (mac[1] ?? "");
    const vol = p.match(/^\/Volumes\/(.*)$/);
    if (vol) return vol[1];
    const win = p.match(/^[A-Za-z]:\\Users\\[^\\]+(\\.*)?$/);
    if (win) return "~" + (win[1] ?? "").replace(/\\/g, "/");
    return p;
  }

  const ICON: Record<SuggestedFolder["kind"], string> = {
    card: "M7 3h8l4 4v14H7z M10 3v4 M13 3v4",
    pictures: "M4 5h16v14H4z M4 16l5-5 4 4 2.5-2.5L20 17 M15.5 9.5h.01",
    videos: "M4 6h12v12H4z M16 10l4-2.5v9L16 14",
    desktop: "M3 5h18v11H3z M9 20h6 M12 16v4",
    downloads: "M12 4v11 M7.5 10.5 12 15l4.5-4.5 M5 19.5h14",
  };
</script>

<div class="welcomeScreen">
  <div class="glow" aria-hidden="true"></div>

  <div class="inner">
    <img class="mark" src="/favicon.png" alt="" width="52" height="52" />
    <h1>Welcome to FoxCull</h1>
    <p class="lede">
      <span>{treeVisible ? "Pick a folder on the left to start culling." : "Open a folder to start culling."}</span>
      <span>Nothing is imported or moved.</span>
    </p>

    <div class="actions">
      <button class="btn accent open" onclick={onpick}>Open folder…</button>
      {#if !treeVisible}
        <button class="btn ghost" onclick={onshowtree}>Show sidebar</button>
      {/if}
    </div>

    {#if resumeDir}
      <p class="resume">
        {driveLabel(resumeDir)} was open last time. Whole drives are slow to scan, so it wasn't reopened.
        <button class="link" onclick={() => onopen(resumeDir!)}>Open anyway</button>
      </p>
    {/if}

    {#if suggestions.length}
      <div class="places">
        <span class="label">Start from</span>
        <ul>
          {#each suggestions as s (s.path)}
            <li>
              <button onclick={() => onopen(s.path)} title={s.path}>
                <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={ICON[s.kind]} /></svg>
                <span class="name">{s.kind === "card" ? `${s.label} · camera card` : s.label}</span>
                <span class="path">{shortPath(s.path)}</span>
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  </div>

  <footer>
    <span>Cull photos, scrub video, edit &amp; export</span>
    <span class="dot">·</span>
    <span>Press <kbd>?</kbd> for shortcuts</span>
    <span class="dot">·</span>
    <button class="link" onclick={onexcludes}>Excluded folders</button>
  </footer>
</div>

<style>
  .welcomeScreen {
    position: relative;
    height: 100%;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    color: var(--text-dim);
    isolation: isolate;
  }
  /* A single soft light behind the mark — the only decoration on the page. */
  .glow {
    position: absolute;
    inset: 0;
    z-index: -1;
    pointer-events: none;
    background: radial-gradient(520px 320px at 50% 30%, color-mix(in srgb, var(--accent) 7%, transparent), transparent 70%);
  }
  /* margin:auto (not justify-content:center) so a short window scrolls from
     the top instead of clipping the heading. */
  .inner {
    margin: auto;
    width: 100%;
    max-width: 440px;
    padding: 56px 24px 32px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    animation: rise 420ms cubic-bezier(0.2, 0.7, 0.2, 1) both;
  }
  @keyframes rise {
    from { opacity: 0; transform: translateY(6px); }
    to { opacity: 1; transform: none; }
  }

  .mark {
    border-radius: var(--radius-md);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
  }
  h1 {
    margin: 22px 0 0;
    color: var(--text);
    font-family: var(--font-display);
    font-size: 24px;
    font-weight: var(--fw-semibold);
    letter-spacing: -0.02em;
  }
  .lede {
    display: flex;
    flex-direction: column;
    margin: 8px 0 0;
    font-size: var(--fs-md);
    line-height: 1.55;
  }

  .actions { display: flex; gap: 8px; margin-top: 24px; }
  .open { min-height: 34px; padding: 6px 18px; border-radius: var(--radius-sm); font-weight: var(--fw-semibold); }
  .ghost { background: transparent; border-color: transparent; box-shadow: none; color: var(--text-dim); }

  .resume {
    margin: 18px 0 0;
    font-size: var(--fs-sm);
    line-height: 1.55;
    color: var(--text-faint);
  }

  .places {
    width: 100%;
    margin-top: 40px;
    text-align: left;
  }
  .label {
    display: block;
    margin: 0 0 6px 10px;
    color: var(--text-faint);
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 4px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--bg-panel) 50%, transparent);
  }
  li button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: var(--fs-md);
    text-align: left;
    transition: background 100ms ease;
  }
  li button:hover { background: var(--bg-hover); }
  li button svg { flex: none; color: var(--text-faint); }
  li button:hover svg { color: var(--accent); }
  .name { flex: none; }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: right;
    color: var(--text-faint);
    font-size: var(--fs-sm);
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    align-items: center;
    gap: 4px 8px;
    padding: 16px 24px 20px;
    color: var(--text-faint);
    font-size: var(--fs-sm);
  }
  .dot { opacity: 0.6; }
  kbd {
    display: inline-block;
    min-width: 18px;
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: var(--radius-xs);
    font-family: inherit;
    font-size: var(--fs-xs);
    line-height: 1.5;
    text-align: center;
  }
  .link {
    padding: 0;
    color: var(--text-dim);
    font-size: inherit;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, currentColor 35%, transparent);
    text-underline-offset: 3px;
  }
  .link:hover { color: var(--accent); }
</style>
