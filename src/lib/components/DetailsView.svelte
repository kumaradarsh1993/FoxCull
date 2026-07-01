<script lang="ts">
  import { api } from "$lib/api";
  import Thumb from "./Thumb.svelte";
  import { LABELS, LABEL_VAR, type MediaItem, type MediaProbe } from "$lib/types";

  let {
    items,
    activeIndex = 0,
    selected,
    onrowclick,
    onrowdblclick,
    onrowdragstart,
    onrowdragend,
  }: {
    items: MediaItem[];
    activeIndex?: number;
    selected: Set<string>;
    onrowclick: (e: MouseEvent, i: number) => void;
    onrowdblclick: (i: number) => void;
    onrowdragstart?: (e: DragEvent, item: MediaItem, i: number) => void;
    onrowdragend?: () => void;
  } = $props();

  type ColId =
    | "thumb"
    | "name"
    | "marks"
    | "kind"
    | "resolution"
    | "fps"
    | "duration"
    | "codec"
    | "camera"
    | "size"
    | "date"
    | "folder"
    | "tags";

  type Col = { id: ColId; label: string; width: number; min: number; optional?: boolean; align?: "right" };

  const COLUMNS: Col[] = [
    { id: "thumb", label: "", width: 54, min: 46 },
    { id: "name", label: "Name", width: 260, min: 150 },
    { id: "marks", label: "Marks", width: 160, min: 90, optional: true },
    { id: "kind", label: "Type", width: 82, min: 64 },
    { id: "resolution", label: "Resolution", width: 130, min: 92, optional: true },
    { id: "fps", label: "FPS", width: 70, min: 54, optional: true },
    { id: "duration", label: "Duration", width: 86, min: 66, optional: true },
    { id: "codec", label: "Codec", width: 92, min: 68, optional: true },
    { id: "camera", label: "Camera", width: 150, min: 90, optional: true },
    { id: "size", label: "Size", width: 92, min: 72, align: "right" },
    { id: "date", label: "Date", width: 156, min: 118 },
    { id: "folder", label: "Folder", width: 170, min: 100, optional: true },
    { id: "tags", label: "Tags", width: 170, min: 90, optional: true },
  ];
  const DEFAULT_COLS: ColId[] = ["thumb", "name", "marks", "kind", "resolution", "fps", "duration", "codec", "camera", "size", "date"];

  const ROW = 52;
  const OVERSCAN = 8;
  const KB = 1024;

  let viewport = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let vpHeight = $state(0);
  let widths = $state<Record<ColId, number>>(Object.fromEntries(COLUMNS.map((c) => [c.id, c.width])) as Record<ColId, number>);
  let shown = $state<Set<ColId>>(new Set(DEFAULT_COLS));
  let columnsOpen = $state(false);
  let probes = $state<Record<string, MediaProbe>>({});
  const probing = new Set<string>();

  let visibleColumns = $derived(COLUMNS.filter((c) => shown.has(c.id)));
  let gridTemplate = $derived(visibleColumns.map((c) => `${widths[c.id] ?? c.width}px`).join(" "));
  let tableWidth = $derived(visibleColumns.reduce((sum, c) => sum + (widths[c.id] ?? c.width), 0));
  let total = $derived(items.length * ROW);
  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  let last = $derived(Math.min(items.length - 1, Math.ceil((scrollTop + vpHeight) / ROW) + OVERSCAN));

  let visible = $derived.by(() => {
    const out: { item: MediaItem; index: number; y: number }[] = [];
    for (let i = first; i <= last; i++) {
      if (i < 0 || i >= items.length) continue;
      out.push({ item: items[i], index: i, y: i * ROW });
    }
    return out;
  });

  $effect(() => {
    const el = viewport;
    if (!el) return;
    const measure = () => (vpHeight = el.clientHeight);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });

  $effect(() => {
    for (const v of visible) {
      if (v.item.kind === "video") ensureProbe(v.item.path);
    }
  });

  $effect(() => {
    const el = viewport;
    const i = activeIndex;
    if (!el || !vpHeight) return;
    const y = i * ROW;
    if (y < el.scrollTop) el.scrollTop = y;
    else if (y + ROW > el.scrollTop + vpHeight) el.scrollTop = y + ROW - vpHeight;
  });

  function ensureProbe(path: string) {
    if (probes[path] || probing.has(path)) return;
    probing.add(path);
    api.probeMediaInfo(path)
      .then((p) => (probes = { ...probes, [path]: p }))
      .catch(() => {})
      .finally(() => probing.delete(path));
  }

  function toggleColumn(id: ColId) {
    const next = new Set(shown);
    if (next.has(id)) {
      if (next.size <= 3) return;
      next.delete(id);
    } else {
      next.add(id);
    }
    shown = next;
  }

  function startResize(e: PointerEvent, col: Col) {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = widths[col.id] ?? col.width;
    const move = (ev: PointerEvent) => {
      widths = { ...widths, [col.id]: Math.max(col.min, startW + ev.clientX - startX) };
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function parentName(p: string) {
    return p.replace(/[\\/][^\\/]*$/, "").split(/[\\/]/).filter(Boolean).pop() ?? "";
  }

  function fmtSize(n: number): string {
    if (!n) return "-";
    if (n < KB) return `${n} B`;
    if (n < KB * KB) return `${(n / KB).toFixed(0)} KB`;
    if (n < KB * KB * KB) return `${(n / (KB * KB)).toFixed(1)} MB`;
    return `${(n / (KB * KB * KB)).toFixed(2)} GB`;
  }

  function fmtDate(epochSecs: number): string {
    if (!epochSecs) return "-";
    return new Date(epochSecs * 1000).toLocaleString(undefined, {
      year: "numeric",
      month: "short",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function fmtTime(s: number): string {
    if (!Number.isFinite(s) || s <= 0) return "-";
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = Math.floor(s % 60);
    return h ? `${h}:${m.toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}` : `${m}:${sec.toString().padStart(2, "0")}`;
  }

  function fmtResolution(p: MediaProbe | undefined): string {
    if (!p?.width || !p?.height) return "-";
    const max = Math.max(p.width, p.height);
    const label = max >= 3800 ? "4K" : max >= 2500 ? "2.7K" : max >= 1900 ? "Full HD" : max >= 1200 ? "HD" : "Other";
    return `${label} (${p.width}x${p.height})`;
  }

  const labelColor = (k: string) => LABEL_VAR[k] ?? "--text-faint";

  function value(item: MediaItem, id: ColId) {
    const p = probes[item.path];
    if (id === "name") return item.name;
    if (id === "kind") return item.kind === "image" ? item.ext.toUpperCase() : item.kind.toUpperCase();
    if (id === "resolution") return item.kind === "video" ? fmtResolution(p) : "-";
    if (id === "fps") return p?.fps ? `${Math.round(p.fps)}` : "-";
    if (id === "duration") return item.kind === "video" ? fmtTime(p?.duration ?? 0) : "-";
    if (id === "codec") return p?.codec?.toUpperCase() ?? "-";
    if (id === "camera") return p?.camera ?? "-";
    if (id === "size") return fmtSize(item.size);
    if (id === "date") return fmtDate(item.mtime);
    if (id === "folder") return parentName(item.path);
    if (id === "tags") return item.tags.join(", ") || "-";
    return "";
  }
</script>

<div class="details">
  <div class="toolbar">
    <span>{items.length} items</span>
    <button class="colsBtn" class:on={columnsOpen} onclick={() => (columnsOpen = !columnsOpen)}>Columns</button>
    {#if columnsOpen}
      <div class="colsMenu">
        {#each COLUMNS.filter((c) => c.id !== "thumb") as col}
          <label><input type="checkbox" checked={shown.has(col.id)} onchange={() => toggleColumn(col.id)} /> {col.label}</label>
        {/each}
      </div>
    {/if}
  </div>
  <div class="head" style="grid-template-columns:{gridTemplate}; min-width:{tableWidth}px">
    {#each visibleColumns as col}
      <span class:ar={col.align === "right"}>
        {col.label}
        {#if col.id !== "thumb"}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <i class="resizer" onpointerdown={(e) => startResize(e, col)}></i>
        {/if}
      </span>
    {/each}
  </div>
  <div
    class="vp"
    bind:this={viewport}
    onscroll={() => {
      if (viewport) scrollTop = viewport.scrollTop;
    }}
  >
    <div class="canvas" style="height:{total}px; min-width:{tableWidth}px">
      {#each visible as v (v.index)}
        <button
          class="row"
          class:active={v.index === activeIndex}
          class:selected={selected.has(v.item.path)}
          class:reject={v.item.flag === "reject"}
          style="transform:translateY({v.y}px); grid-template-columns:{gridTemplate}"
          onclick={(e) => onrowclick(e, v.index)}
          ondblclick={() => onrowdblclick(v.index)}
          draggable={!!onrowdragstart}
          ondragstart={(e) => onrowdragstart?.(e, v.item, v.index)}
          ondragend={() => onrowdragend?.()}
          title={v.item.path}
        >
          {#each visibleColumns as col}
            {#if col.id === "thumb"}
              <span class="c-thumb"><Thumb item={v.item} size={320} /></span>
            {:else if col.id === "marks"}
              <span class="c-marks">
                {#if v.item.rating > 0}<span class="stars">{"*".repeat(v.item.rating)}</span>{/if}
                {#if v.item.label}<span class="dot" style="background:var({labelColor(v.item.label)})"></span>{/if}
                {#if v.item.flag === "pick"}<span class="fl pick">Pick</span>{/if}
                {#if v.item.flag === "reject"}<span class="fl rej">Reject</span>{/if}
              </span>
            {:else}
              <span class="txt" class:ar={col.align === "right"}>{value(v.item, col.id)}</span>
            {/if}
          {/each}
        </button>
      {/each}
    </div>
  </div>
</div>

<style>
  .details { width: 100%; height: 100%; display: flex; flex-direction: column; background: var(--viewport-bg); overflow: hidden; }
  .toolbar { position: relative; flex: 0 0 34px; display: flex; align-items: center; justify-content: flex-end; gap: 8px; padding: 5px 10px; border-bottom: 1px solid var(--border); background: var(--bg-panel); color: var(--text-faint); font-size: 12px; }
  .colsBtn { padding: 4px 9px; border: 1px solid var(--border); border-radius: 7px; background: var(--bg-elev); color: var(--text-dim); }
  .colsBtn.on { color: var(--accent); border-color: var(--accent); }
  .colsMenu { position: absolute; z-index: 50; top: 30px; right: 10px; width: 210px; max-height: 320px; overflow: auto; padding: 8px; display: grid; gap: 6px; border: 1px solid var(--border); border-radius: 9px; background: var(--bg-elev); box-shadow: var(--shadow); }
  .colsMenu label { display: flex; align-items: center; gap: 7px; color: var(--text-dim); }
  .head,
  .row { display: grid; align-items: center; gap: 0; padding: 0 10px; }
  .head { flex: 0 0 32px; overflow: hidden; border-bottom: 1px solid var(--border); background: var(--bg-panel); color: var(--text-faint); font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.04em; }
  .head > span { position: relative; height: 100%; display: flex; align-items: center; padding-right: 10px; min-width: 0; }
  .resizer { position: absolute; top: 5px; right: 0; width: 7px; height: 22px; cursor: col-resize; border-right: 1px solid color-mix(in srgb, var(--border) 80%, transparent); }
  .resizer:hover { border-color: var(--accent); }
  .vp { flex: 1; overflow: auto; }
  .canvas { position: relative; width: 100%; }
  .row { position: absolute; top: 0; left: 0; min-width: 100%; height: 52px; text-align: left; border-bottom: 1px solid color-mix(in srgb, var(--border) 55%, transparent); color: var(--text); background: transparent; }
  .row:hover { background: var(--bg-hover); }
  .row.selected { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .row.active { background: color-mix(in srgb, var(--accent) 22%, transparent); box-shadow: inset 2px 0 0 var(--accent); }
  .row.reject { opacity: 0.5; }
  .c-thumb { width: 42px; height: 42px; display: flex; align-items: center; justify-content: center; overflow: hidden; border-radius: 5px; }
  .txt { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding-right: 10px; color: var(--text-dim); font-size: 12.5px; }
  .row .txt:nth-child(2) { color: var(--text); font-weight: 600; }
  .ar { justify-content: flex-end; text-align: right; }
  .c-marks { display: flex; align-items: center; gap: 5px; overflow: hidden; }
  .stars { color: var(--star); font-size: 12px; }
  .dot { width: 11px; height: 11px; border-radius: 3px; }
  .fl { font-weight: 700; font-size: 12px; }
  .fl.pick { color: var(--pick); }
  .fl.rej { color: var(--reject); }
</style>
