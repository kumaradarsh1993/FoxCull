<script lang="ts">
  import { api } from "$lib/api";
  import { mediaDrag, wantsCopy } from "$lib/drag.svelte";
  import type { TreeDir } from "$lib/types";
  import Self from "./TreeNode.svelte";

  let {
    node,
    currentDir,
    onselect,
    onmove,
    onfoldercontext,
    depth = 0,
    count = null,
    countsGen = 0,
    treeGen = 0,
    revealPath = null,
  }: {
    node: TreeDir;
    currentDir: string | null;
    onselect: (path: string) => void;
    /** Files dropped on this folder; `copy` when the copy modifier was held. */
    onmove?: (path: string, copy: boolean) => void;
    onfoldercontext?: (event: MouseEvent, path: string) => void;
    depth?: number;
    /** Recursive media count for THIS folder (given by the parent), or null. */
    count?: number | null;
    /** Bumped by the tree's ↻ button to force open nodes to recount. */
    countsGen?: number;
    /** Bumped whenever folders may have appeared or gone (a folder created,
     *  ↻, exclusion rules changed): every expanded node re-lists its children.
     *  A node used to list them once and keep that forever, so a folder made
     *  from the sidebar never showed up until the app restarted. */
    treeGen?: number;
    /** A folder to make visible: its ancestors expand themselves. */
    revealPath?: string | null;
  } = $props();

  let open = $state(false);
  let kids = $state<TreeDir[] | null>(null);
  let kidCounts = $state<Record<string, number>>({});
  let loading = $state(false);
  let dropHot = $state(false);
  let dropCopy = $state(false);

  // Optimistic chevron: every folder claims children (list_tree no longer probes,
  // to stay fast); once an expand turns up no subfolders we hide it.
  let showChevron = $derived(node.has_children && !(kids !== null && kids.length === 0));

  async function loadKids() {
    loading = true;
    try {
      kids = await api.listTree(node.path);
    } catch {
      kids = [];
    }
    loading = false;
    fetchCounts(); // fill child badges (cached → instant; else background)
  }

  async function fetchCounts(recompute = false) {
    if (!kids || !kids.length) return;
    try {
      const cs = await api.folderCounts(
        kids.map((k) => k.path),
        recompute,
      );
      const m: Record<string, number> = { ...kidCounts };
      for (const c of cs) m[c.path] = c.count;
      kidCounts = m;
    } catch {
      /* counts are best-effort — leave badges blank on failure */
    }
  }

  async function toggle() {
    open = !open;
    if (open && kids === null) await loadKids();
  }

  // Re-list children when the tree generation moves on. Open state below is
  // kept: children are keyed by path, so existing nodes survive the reload.
  let seenTreeGen = -1;
  $effect(() => {
    const g = treeGen;
    if (seenTreeGen === -1) {
      seenTreeGen = g;
      return;
    }
    if (g !== seenTreeGen) {
      seenTreeGen = g;
      if (kids !== null) void loadKids();
    }
  });

  // Recount when the user hits ↻ (countsGen changes) and we're expanded. The
  // sentinel start avoids a spurious recount on mount (and capturing the prop).
  let lastGen = -1;
  $effect(() => {
    if (countsGen !== lastGen) {
      lastGen = countsGen;
      if (open && kids && kids.length) fetchCounts(true);
    }
  });

  /** Is `dir` strictly inside `ancestor` (path-boundary-aware, case-insensitive
   *  for Windows drive letters/folders)? */
  function isUnder(dir: string, ancestor: string): boolean {
    const a = ancestor.toLowerCase().replace(/[\\/]+$/, "");
    const d = dir.toLowerCase();
    return (
      d.length > a.length &&
      d.startsWith(a) &&
      (d[a.length] === "\\" || d[a.length] === "/")
    );
  }

  // Cascade-open to the folder that's actually open: when the current folder
  // lives under this node, auto-expand it (each child then does the same, so the
  // chain unfolds down to the selected folder — e.g. restoring the last session).
  // Done at most once per currentDir value, so manually collapsing an ancestor
  // afterwards sticks instead of fighting the effect.
  let autoExpandedFor: string | null = null;
  $effect(() => {
    const cd = currentDir;
    if (!cd || cd === autoExpandedFor) return;
    if (isUnder(cd, node.path)) {
      autoExpandedFor = cd;
      if (!open) {
        open = true;
        if (kids === null) loadKids();
      }
    }
  });

  // Reveal a folder (a new one, say): expand every ancestor on the way to it.
  $effect(() => {
    const rp = revealPath;
    if (rp && isUnder(rp, node.path) && !open) {
      open = true;
      if (kids === null) void loadKids();
    }
  });

  // Keep the selected folder's row visible in the (scrollable) tree pane.
  let rowEl = $state<HTMLDivElement | null>(null);
  $effect(() => {
    if (currentDir === node.path || revealPath === node.path) rowEl?.scrollIntoView({ block: "nearest" });
  });

  function acceptsMediaDrag(e: DragEvent): boolean {
    return !!onmove && Array.from(e.dataTransfer?.types ?? []).includes("application/x-foxcull-paths");
  }

  // Spring-loaded folders, as in Finder: hover a closed folder mid-drag and
  // it opens, so a drop can reach a subfolder without letting go first.
  let springTimer: ReturnType<typeof setTimeout> | null = null;
  function clearSpring() {
    if (springTimer) clearTimeout(springTimer);
    springTimer = null;
  }

  function onDragOver(e: DragEvent) {
    if (!acceptsMediaDrag(e)) return;
    e.preventDefault();
    dropCopy = wantsCopy(e);
    if (e.dataTransfer) e.dataTransfer.dropEffect = dropCopy ? "copy" : "move";
    if (!open && showChevron && !springTimer) {
      springTimer = setTimeout(() => {
        springTimer = null;
        // Still hovering (dragover keeps re-setting dropHot) → open it.
        if (dropHot && !open) void toggle();
      }, 700);
    }
    dropHot = true;
  }

  function onDragLeave(e: DragEvent) {
    // Moving between the row's own children fires leave/enter pairs.
    if (e.relatedTarget instanceof Node && (e.currentTarget as HTMLElement).contains(e.relatedTarget)) return;
    dropHot = false;
  }

  function onDrop(e: DragEvent) {
    if (!acceptsMediaDrag(e) || !onmove) return;
    e.preventDefault();
    dropHot = false;
    clearSpring();
    onmove(node.path, wantsCopy(e));
  }
</script>

<div
  class="trow"
  class:root={depth === 0}
  class:active={currentDir === node.path}
  class:revealed={revealPath === node.path}
  class:drophot={dropHot}
  style="--depth:{depth}"
  bind:this={rowEl}
  role="presentation"
  ondragover={onDragOver}
  ondragleave={onDragLeave}
  ondrop={onDrop}
  oncontextmenu={(e) => onfoldercontext?.(e, node.path)}
>
  <!-- One faint guide per ancestor level: the hierarchy reads at a glance
       without spending width on deep indentation. -->
  {#each { length: depth } as _, g (g)}<span class="guide" style="--g:{g}" aria-hidden="true"></span>{/each}
  {#if showChevron}
    <button class="chev" class:open onclick={toggle} aria-label={open ? "Collapse" : "Expand"} title={open ? "Collapse" : "Expand"}>
      <svg viewBox="0 0 16 16" width="10" height="10" aria-hidden="true"><path d="M6 4l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </button>
  {:else}
    <span class="chev-spacer"></span>
  {/if}
  <button class="tname" title={node.path} onclick={() => onselect(node.path)} ondblclick={toggle}>
    <svg class="ticon" viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      {#if depth === 0 && node.name === "Home"}
        <path d="M4 10.5 12 4l8 6.5V19a1 1 0 0 1-1 1h-4.5v-5.5h-5V20H5a1 1 0 0 1-1-1z" />
      {:else if depth === 0}
        <rect x="3" y="6.5" width="18" height="11" rx="2.6" /><path d="M7 14h5" /><circle cx="17" cy="12" r="0.9" fill="currentColor" stroke="none" />
      {:else}
        <path d="M3.5 7.2c0-.94.76-1.7 1.7-1.7h3.9l1.9 1.9h7.8c.94 0 1.7.76 1.7 1.7v7.7c0 .94-.76 1.7-1.7 1.7H5.2c-.94 0-1.7-.76-1.7-1.7z" />
      {/if}
    </svg>
    <span class="label">{node.name}</span>
    {#if dropHot && mediaDrag.count}
      <span class="dropPill" class:copy={dropCopy}>{dropCopy ? "Copy" : "Move"} {mediaDrag.count.toLocaleString()}</span>
    {:else if count != null && count > 0}<span class="cnt">{count.toLocaleString()}</span>{/if}
  </button>
</div>

{#if open && kids}
  {#each kids as k (k.path)}
    <Self
      node={k}
      {currentDir}
      {onselect}
      {onmove}
      {onfoldercontext}
      depth={depth + 1}
      count={kidCounts[k.path] ?? null}
      {countsGen}
      {treeGen}
      {revealPath}
    />
  {/each}
{/if}

<style>
  /* Compact, Finder/VS Code-style tree: 24px rows, a folder or drive icon per
     row, one faint guide line per level instead of deep indentation, and
     quiet counts. Dense without getting cramped: the chevron and the name
     keep full-height hit areas. */
  .trow {
    --indent: 12px;
    position: relative;
    display: flex;
    align-items: center;
    width: 100%;
    height: 24px;
    padding-left: calc(2px + var(--depth) * var(--indent));
    border-radius: var(--radius-xs);
  }
  .trow:hover { background: color-mix(in srgb, var(--bg-hover) 65%, transparent); }
  .trow.active {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .trow.revealed:not(.active) { animation: flash 1.4s ease-out 1; }
  @keyframes flash {
    0%, 40% { background: color-mix(in srgb, var(--accent) 22%, transparent); }
    100% { background: transparent; }
  }
  .trow.drophot {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  /* "Move 24" on the folder under the drag: says what the drop will do. */
  .dropPill {
    flex: 0 0 auto;
    margin-left: auto;
    padding: 1px 7px;
    border-radius: 999px;
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
    color: var(--accent-on);
    background: var(--accent);
    font-variant-numeric: tabular-nums;
  }
  .dropPill.copy { background: var(--pick); }
  .guide {
    position: absolute;
    top: 0;
    bottom: 0;
    left: calc(9px + var(--g) * var(--indent));
    width: 1px;
    background: color-mix(in srgb, var(--text-faint) 18%, transparent);
    pointer-events: none;
  }

  .chev {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 16px;
    height: 24px;
    color: var(--text-faint);
    border-radius: var(--radius-xs);
  }
  .chev svg { transition: transform 120ms ease; }
  .chev.open svg { transform: rotate(90deg); }
  .chev:hover { color: var(--text); }
  .chev-spacer { flex: 0 0 auto; width: 16px; }

  .tname {
    flex: 1;
    min-width: 0;
    height: 24px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 6px 0 2px;
    text-align: left;
    color: var(--text-dim);
    font-size: var(--fs-md);
  }
  .tname:hover { color: var(--text); }
  .ticon { flex: none; color: var(--text-faint); }
  .root .ticon { color: var(--text-dim); }
  .root .tname { color: var(--text); font-weight: var(--fw-medium); }
  .trow.active .tname { color: var(--text); font-weight: var(--fw-semibold); }
  .trow.active .ticon { color: var(--accent); }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cnt {
    flex: 0 0 auto;
    margin-left: auto;
    padding-left: 6px;
    color: var(--text-faint);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }
</style>
