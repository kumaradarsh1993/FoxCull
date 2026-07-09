<script lang="ts">
  import { api } from "$lib/api";
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
  }: {
    node: TreeDir;
    currentDir: string | null;
    onselect: (path: string) => void;
    onmove?: (path: string) => void;
    onfoldercontext?: (event: MouseEvent, path: string) => void;
    depth?: number;
    /** Recursive media count for THIS folder (given by the parent), or null. */
    count?: number | null;
    /** Bumped by the tree's ↻ button to force open nodes to recount. */
    countsGen?: number;
  } = $props();

  let open = $state(false);
  let kids = $state<TreeDir[] | null>(null);
  let kidCounts = $state<Record<string, number>>({});
  let loading = $state(false);
  let dropHot = $state(false);

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

  // Keep the selected folder's row visible in the (scrollable) tree pane.
  let rowEl = $state<HTMLDivElement | null>(null);
  $effect(() => {
    if (currentDir === node.path) rowEl?.scrollIntoView({ block: "nearest" });
  });

  function acceptsMediaDrag(e: DragEvent): boolean {
    return !!onmove && Array.from(e.dataTransfer?.types ?? []).includes("application/x-foxcull-paths");
  }

  function onDragOver(e: DragEvent) {
    if (!acceptsMediaDrag(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dropHot = true;
  }

  function onDragLeave() {
    dropHot = false;
  }

  function onDrop(e: DragEvent) {
    if (!acceptsMediaDrag(e) || !onmove) return;
    e.preventDefault();
    dropHot = false;
    onmove(node.path);
  }
</script>

<div
  class="trow"
  class:active={currentDir === node.path}
  class:drophot={dropHot}
  style="padding-left:{4 + depth * 14}px"
  bind:this={rowEl}
  role="presentation"
  ondragover={onDragOver}
  ondragleave={onDragLeave}
  ondrop={onDrop}
  oncontextmenu={(e) => onfoldercontext?.(e, node.path)}
>
  {#if showChevron}
    <button
      class="chev"
      class:open
      onclick={toggle}
      aria-label={open ? "Collapse" : "Expand"}
      title={open ? "Collapse" : "Expand"}
    >
      {open ? "▾" : "▸"}
    </button>
  {:else}
    <span class="chev-spacer"></span>
  {/if}
  <button class="tname" title={node.path} onclick={() => onselect(node.path)}>
    <span class="label">{node.name}</span>
    {#if count != null}<span class="cnt">{count.toLocaleString()}</span>{/if}
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
    />
  {/each}
{/if}

<style>
  /* Lightroom-style tree: quiet monochrome rows, a subtle neutral highlight on
     the selected folder (no loud accent fill), small disclosure triangles, and
     right-aligned muted counts. */
  .trow {
    display: flex;
    align-items: center;
    gap: 1px;
    width: 100%;
    border-radius: 4px;
  }
  .trow.active {
    background: color-mix(in srgb, var(--text-faint) 20%, transparent);
  }
  .trow.drophot {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .trow.active .label {
    color: var(--text);
    font-weight: 600;
  }

  /* Disclosure triangle, clearly separate from the row's select action. Quiet
     Lightroom look but with a comfortable click target (small glyph, big hit
     area) so it is easy to hit on a dense tree. */
  .chev {
    flex: 0 0 auto;
    width: 24px;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    color: var(--text-faint);
    border-radius: 4px;
  }
  .chev:hover {
    color: var(--text);
    background: color-mix(in srgb, var(--text-faint) 16%, transparent);
  }
  .chev-spacer {
    flex: 0 0 auto;
    width: 24px;
  }

  .tname {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 6px;
    text-align: left;
    border-radius: 4px;
    color: var(--text-dim);
    font-size: 12.5px;
    line-height: 1.2;
  }
  .tname:hover {
    color: var(--text);
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Lightroom-style file count, right-aligned and muted. */
  .cnt {
    flex: 0 0 auto;
    margin-left: auto;
    padding-left: 6px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: var(--text-faint);
  }
</style>
