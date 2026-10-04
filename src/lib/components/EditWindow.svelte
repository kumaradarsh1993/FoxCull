<script lang="ts">
  // The Edit window (its own OS window since 2026-10-04): the timeline studio,
  // fed by the library. Owns what the studio can't: the window's keyboard, a
  // drop anywhere in the window, ⌘V, the library's inbox, the notices that
  // say what a drop did, and keeping the timeline across close/reopen.
  import { onDestroy, onMount } from "svelte";
  import { api } from "$lib/api";
  import type { ClipRef, EditInbox } from "$lib/types";
  import EditStudio, { type AddResult, type TimelineState } from "./EditStudio.svelte";

  const SAVE_KEY = "foxcull-edit-timeline-v1";

  let studio = $state<ReturnType<typeof EditStudio> | null>(null);
  let restored = false;
  let dropHot = $state(false);

  type Notice = { id: number; text: string; warn: boolean };
  let notices = $state<Notice[]>([]);
  let seq = 0;
  function notify(text: string, warn = false) {
    const id = ++seq;
    notices = [...notices, { id, text, warn }].slice(-5);
    // Information fades; anything the owner should act on stays until closed.
    if (!warn) setTimeout(() => (notices = notices.filter((n) => n.id !== id)), 6000);
  }
  const dismiss = (id: number) => (notices = notices.filter((n) => n.id !== id));

  // ── keeping the timeline ──────────────────────────────────────────────────
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  function persist(st: TimelineState) {
    if (!restored) return; // never overwrite the saved timeline before it's read
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      try {
        if (st.clips.length || st.audio.length) localStorage.setItem(SAVE_KEY, JSON.stringify(st));
        else localStorage.removeItem(SAVE_KEY);
      } catch {
        /* storage full or blocked: the timeline just won't survive a close */
      }
    }, 400);
  }

  async function restoreSaved() {
    let st: TimelineState | null = null;
    try {
      const raw = localStorage.getItem(SAVE_KEY);
      st = raw ? (JSON.parse(raw) as TimelineState) : null;
    } catch {
      st = null;
    }
    if (st?.v === 1 && st.clips?.length && studio) {
      const r = await studio.restore(st);
      notify(`Your timeline from last time is back (${st.clips.length} segment${st.clips.length === 1 ? "" : "s"}). Clear starts a new one.`);
      if (r.missing.length) notify(`Not available right now: ${r.missing.join(", ")}. Is that drive plugged in? They stay on the timeline but won't play or export until it is.`, true);
    }
    restored = true;
  }

  // ── bringing clips in ─────────────────────────────────────────────────────
  async function report(res: AddResult) {
    if (res.segments) {
      notify(`Added ${res.clips} clip${res.clips === 1 ? "" : "s"}${res.segments > res.clips ? ` as ${res.segments} segments (the in/out ranges marked in the library)` : ""}.`);
    }
    if (res.photos) notify(`${res.photos} photo${res.photos === 1 ? "" : "s"} left out: the timeline takes videos.`, true);
    if (res.missing) notify(`${res.missing} missing file${res.missing === 1 ? "" : "s"} left out (shown as “?” in the library).`, true);
    if (res.other) notify(`${res.other} file${res.other === 1 ? "" : "s"} left out: not a video FoxCull can edit.`, true);
    if (!res.segments && !res.photos && !res.missing && !res.other) notify("Nothing to add.", true);
    if (res.paths.length && studio) {
      for (const n of await studio.compatNotes(res.paths)) notify(n, true);
    }
  }

  async function add(clips: ClipRef[]) {
    if (!studio || !clips.length) return;
    await report(await studio.addClips(clips));
  }

  async function drainInbox() {
    const items = await api.takeToolInbox<EditInbox>("edit").catch(() => [] as EditInbox[]);
    for (const it of items) {
      if (it?.type !== "add") continue;
      // The toolbar's Edit button only fills an EMPTY timeline; with work on
      // it, bringing the window forward is all it should do.
      if (it.mode === "seed" && studio && !studio.isEmpty()) continue;
      await add(it.clips);
    }
  }

  async function paste() {
    const clips = await api.stashGet<ClipRef[]>("clips");
    if (!clips?.length) {
      notify("Nothing copied yet: select clips in the library and press ⌘C (Ctrl+C), then paste here.", true);
      return;
    }
    await add(clips);
  }

  // A drop anywhere that isn't a track (the preview, the empty state, the top
  // bar) adds to V1 after the last clip. Track drops stop propagation.
  function onDragOver(e: DragEvent) {
    if (!studio?.isClipDrag(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    dropHot = true;
  }
  function onDragLeave(e: DragEvent) {
    if (e.relatedTarget === null || !(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node)) dropHot = false;
  }
  async function onDrop(e: DragEvent) {
    dropHot = false;
    if (!studio?.isClipDrag(e)) return;
    e.preventDefault();
    await add(await studio.clipsFromDrop(e));
  }

  // ── keyboard (was the library page's Edit branch) ─────────────────────────
  async function onkeydown(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) return;
    if (!studio) return;
    const k = e.key.toLowerCase();
    if ((e.metaKey || e.ctrlKey) && k === "v") { e.preventDefault(); await paste(); return; }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === "Delete" || e.key === "Backspace") { studio.deleteSelected(); e.preventDefault(); return; }
    if (k === "c") { studio.cutAtPlayhead(); e.preventDefault(); return; }
    if (e.key === "," || e.key === "<") { studio.seekBy(-5); e.preventDefault(); return; }
    if (e.key === "." || e.key === ">") { studio.seekBy(5); e.preventDefault(); return; }
    if (e.key === " " || e.code === "Space") { studio.togglePlay(); e.preventDefault(); return; }
    if (e.key === "[") { studio.setIn(); e.preventDefault(); return; }
    if (e.key === "]") { studio.setOut(); e.preventDefault(); return; }
    if (e.shiftKey && e.key === "ArrowRight") { studio.seekBy(5); e.preventDefault(); return; }
    if (e.shiftKey && e.key === "ArrowLeft") { studio.seekBy(-5); e.preventDefault(); return; }
    if (e.key === "ArrowLeft") { studio.stepFrame(-1); e.preventDefault(); return; }
    if (e.key === "ArrowRight") { studio.stepFrame(1); e.preventDefault(); return; }
    if (e.key === "Home") { studio.goToEdge(false); e.preventDefault(); return; }
    if (e.key === "End") { studio.goToEdge(true); e.preventDefault(); return; }
    if (k === "f") {
      e.preventDefault();
      const { getCurrentWindow } = await import("@tauri-apps/api/window").catch(() => ({ getCurrentWindow: null }));
      const w = getCurrentWindow?.();
      const full = (await w?.isFullscreen().catch(() => false)) ?? false;
      await studio.setOutputPreview(!full);
      await w?.setFullscreen(!full).catch(() => {});
      return;
    }
    if (e.key === "Escape") {
      const { getCurrentWindow } = await import("@tauri-apps/api/window").catch(() => ({ getCurrentWindow: null }));
      const w = getCurrentWindow?.();
      if (await w?.isFullscreen().catch(() => false)) await w?.setFullscreen(false).catch(() => {});
      await studio.setOutputPreview(false);
    }
  }

  async function sideBySide() {
    try {
      await api.tileWindows("edit");
    } catch (e) {
      notify(`Couldn't arrange the windows: ${e}`, true);
    }
  }

  let unlisten: (() => void) | null = null;
  onMount(async () => {
    document.title = "FoxCull Edit";
    await restoreSaved();
    await drainInbox();
    unlisten = await api.onToolInbox(() => void drainInbox()).catch(() => null);
  });
  onDestroy(() => {
    unlisten?.();
    if (saveTimer) clearTimeout(saveTimer);
  });
</script>

<svelte:window {onkeydown} ondragend={() => (dropHot = false)} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- dropcapture: a drop on a track stops propagation (it's handled there),
     so the hint is cleared on the way down instead. -->
<div class="editWin" class:dropHot ondragover={onDragOver} ondragleave={onDragLeave} ondrop={onDrop} ondropcapture={() => (dropHot = false)}>
  <EditStudio bind:this={studio} onchange={persist} onsidebyside={sideBySide} ondropped={(r) => void report(r)} />
  {#if dropHot}
    <div class="dropHint" aria-hidden="true">Drop to add after the last clip on V1 · or drop on a track to place it there</div>
  {/if}
  {#if notices.length}
    <div class="notices" role="status" aria-live="polite">
      {#each notices as n (n.id)}
        <div class="notice" class:warn={n.warn}>
          <span>{n.text}</span>
          <button onclick={() => dismiss(n.id)} aria-label="Dismiss">×</button>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .editWin {
    position: fixed;
    inset: 0;
    display: flex;
    background: var(--bg);
  }
  .editWin.dropHot {
    outline: 2px dashed var(--accent);
    outline-offset: -6px;
  }
  .dropHint {
    position: absolute;
    left: 50%;
    top: 14px;
    transform: translateX(-50%);
    z-index: 60;
    padding: 7px 14px;
    border-radius: 999px;
    font-size: var(--fs-sm);
    font-weight: var(--fw-semibold);
    color: var(--accent-on);
    background: var(--accent);
    box-shadow: var(--shadow);
    pointer-events: none;
  }
  /* Over the preview's top-left: clear of the Look panel and the Export menu. */
  .notices {
    position: absolute;
    left: 14px;
    top: 68px;
    z-index: 70;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(420px, calc(100vw - 28px));
  }
  .notice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--bg-elev) 96%, transparent);
    box-shadow: var(--shadow);
    font-size: var(--fs-md);
    line-height: 1.45;
    color: var(--text);
    backdrop-filter: blur(16px);
  }
  .notice.warn {
    border-color: color-mix(in srgb, var(--star) 55%, var(--border));
    box-shadow: inset 3px 0 0 var(--star), var(--shadow);
  }
  .notice span { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .notice button {
    flex: 0 0 auto;
    width: 20px;
    height: 20px;
    border-radius: var(--radius-xs);
    color: var(--text-faint);
  }
  .notice button:hover { background: var(--bg-hover); color: var(--text); }
</style>
