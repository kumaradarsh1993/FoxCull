<script lang="ts" module>
  export type SettingsPage =
    | "appearance"
    | "playback"
    | "speed"
    | "files"
    | "controls"
    | "about"
    | "excludes"
    | "controller";
</script>

<script lang="ts">
  // Settings, as one sheet (2026-10-04). It replaced a 22-row popover that had
  // grown past the window height. Everything settable lives here and nowhere
  // else: a sidebar of six sections, cards of rows inside each, two deeper
  // pages (Excluded folders, Controller) and the update panel. Every row says
  // what it does underneath its name instead of hiding that in a tooltip.
  // Search filters every row of every section; a result row is the real
  // control, so a setting changes right there in the results.
  //
  // The page owns anything that touches the library (opening the Trash,
  // checking the catalog, building previews, folding stacks), so those arrive
  // as callbacks; plain preferences write straight to the settings store.
  import { onMount, tick } from "svelte";
  import { api } from "$lib/api";
  import { activity, fmtBytes } from "$lib/activity.svelte";
  import { pad } from "$lib/gamepad.svelte";
  import { settings, GLIMPSE_MIN, GLIMPSE_MAX, type FilmstripPos, type Theme, type UiScale } from "$lib/settings.svelte";
  import { updates } from "$lib/updates.svelte";
  import type { LibraryInfo } from "$lib/types";
  import ControllerPanel from "./ControllerPanel.svelte";
  import ExcludePanel from "./ExcludePanel.svelte";
  import UpdatePanel from "./UpdatePanel.svelte";

  let {
    page = $bindable<SettingsPage>("appearance"),
    onclose,
    onfilmstrip,
    onstacks,
    foldableCount = 0,
    currentDir = null,
    folderCount = 0,
    preparing = false,
    onprepare,
    libInfo = null,
    driveLabel = "",
    trashCount = 0,
    onopentrash,
    scanning = false,
    oncheckcatalog,
    onshortcuts,
  }: {
    page?: SettingsPage;
    onclose: () => void;
    /** Docks the filmstrip (the page also remembers it per view). */
    onfilmstrip: (pos: FilmstripPos) => void;
    onstacks: (mode: "expanded" | "collapsed") => void;
    /** How many tiles folding stacks would hide in this folder. */
    foldableCount?: number;
    currentDir?: string | null;
    folderCount?: number;
    preparing?: boolean;
    onprepare: () => void;
    libInfo?: LibraryInfo | null;
    driveLabel?: string;
    trashCount?: number;
    onopentrash: () => void;
    scanning?: boolean;
    oncheckcatalog: () => void;
    onshortcuts: () => void;
  } = $props();

  type Section = Exclude<SettingsPage, "excludes" | "controller">;
  const SECTIONS: { id: Section; title: string; blurb: string }[] = [
    { id: "appearance", title: "Appearance", blurb: "How FoxCull looks, and what the grid shows." },
    { id: "playback", title: "Playback", blurb: "How videos play in the grid, in Focus and in Glimpse." },
    { id: "speed", title: "Speed & storage", blurb: "Building previews ahead of time, and what they take on disk." },
    { id: "files", title: "Files & catalog", blurb: "Deleting, the Trash, and the catalog that remembers your marks." },
    { id: "controls", title: "Controls", blurb: "Keyboard, mouse buttons and a game controller." },
    { id: "about", title: "About & updates", blurb: "The version you're running and what's newer." },
  ];
  const SUBPAGES: Record<"excludes" | "controller", { title: string; parent: Section; blurb: string }> = {
    excludes: { title: "Excluded folders", parent: "files", blurb: "FoxCull never scans, counts or shows these. Saved on this device." },
    controller: { title: "Game controller", parent: "controls", blurb: "Pair a PS5 or PS4 controller and choose what each button does." },
  };

  /** Every row, in display order. `card` groups rows inside a section; `keys`
   *  are extra words search should match (what people call the thing). */
  type Row = { id: string; section: Section; card: string; label: string; desc: string; keys?: string; wide?: boolean; link?: boolean };
  const ROWS: Row[] = [
    { id: "theme", section: "appearance", card: "Look", label: "Theme", desc: "Studio is neutral grey for judging colour. Amber cuts blue light late at night.", keys: "dark light colour color mode midnight daylight studio amber", wide: true },
    { id: "uiScale", section: "appearance", card: "Look", label: "Interface size", desc: "Compact fits more on a laptop screen. TV is for a screen across the room.", keys: "zoom scale large small text font" },
    { id: "filmstrip", section: "appearance", card: "Layout", label: "Filmstrip", desc: "Where the strip of thumbnails docks. Each view remembers whether it's shown.", keys: "strip dock bottom left right hide" },
    { id: "tileLength", section: "appearance", card: "Layout", label: "Video length on tiles", desc: "A small badge with each video's length.", keys: "duration badge tile" },
    { id: "tileName", section: "appearance", card: "Layout", label: "File names on tiles", desc: "The file name under every tile.", keys: "caption filename tile" },
    { id: "stacks", section: "appearance", card: "Layout", label: "Stacks", desc: "RAW+JPEG pairs, and a shot with its edits and exports. Folded shows one tile per stack.", keys: "related raw jpeg fold collapse expand group" },

    { id: "autoplay", section: "playback", card: "Videos", label: "Play videos when opened", desc: "A clip starts playing as soon as it opens in Focus.", keys: "autoplay auto play" },
    { id: "minimalBar", section: "playback", card: "Videos", label: "Minimal video bar", desc: "The controls shrink to a thin line until you hover, so the picture stays edge to edge.", keys: "transport controls hover player" },
    { id: "glimpse", section: "playback", card: "Glimpse", label: "Glimpse speed", desc: "How fast Glimpse (Ctrl+Space) plays, as a multiple of real time: at 5×, a 20 s clip takes 4 s.", keys: "skim fast preview ctrl space speed" },

    { id: "prepare", section: "speed", card: "Previews", label: "Prepare this folder", desc: "Builds every Focus preview and video poster now, so stepping through later never waits. Worth it on SD cards and spinning disks; on an SSD there's nothing to gain.", keys: "build previews precache cache warm thumbnails sd card slow", wide: true },
    { id: "cache", section: "speed", card: "Previews", label: "Preview cache", desc: "Thumbnails, previews and filmstrips for this drive, kept in its _FoxCull folder. Anything missing is rebuilt when it's needed.", keys: "disk space size thumbs storage" },
    { id: "liveDecode", section: "speed", card: "Advanced", label: "Live scrubbing in Focus", desc: "Dragging across a clip shows the real frame under the cursor, at full resolution. Clips it can't decode fall back on their own. Turn off only to diagnose a problem.", keys: "scrub decode webcodecs sprites focus" },
    { id: "spriteFallback", section: "speed", card: "Advanced", label: "Sprite sheets for skimming", desc: "Also pre-builds frame sheets for clips the live decoder can't open. Costs minutes of work and disk space per folder.", keys: "sprite skim hover fallback live scrub" },

    { id: "deleteMode", section: "files", card: "Deleting", label: "When you delete", desc: "The in-app Trash keeps files on the same drive, to preview and restore. The system Trash is Finder's or Windows' bin.", keys: "delete recycle bin trash remove" },
    { id: "trash", section: "files", card: "Deleting", label: "Trash", desc: "Preview, play and restore anything before it's gone for good.", keys: "recycle restore deleted" },
    { id: "scanOnLaunch", section: "files", card: "Catalog", label: "Check the catalog at launch", desc: "Finds rated and tagged files that moved or were renamed outside FoxCull, and reconnects them.", keys: "scan integrity missing moved relink startup" },
    { id: "checkNow", section: "files", card: "Catalog", label: "Check for moved files now", desc: "The same check, on this drive, right away.", keys: "scan integrity missing moved relink" },
    { id: "excludes", section: "files", card: "Catalog", label: "Excluded folders", desc: "Folders FoxCull never scans, counts or shows.", keys: "ignore skip hide system folders", link: true },
    { id: "library", section: "files", card: "Where it's kept", label: "Library on this drive", desc: "Each drive keeps its own catalog, preview cache and Trash in a _FoxCull folder, so they travel with it.", keys: "catalog location path folder _foxcull", wide: true },

    { id: "shortcuts", section: "controls", card: "Keyboard", label: "Keyboard shortcuts", desc: "Every key, grouped. Press ? anywhere to see them.", keys: "keys hotkeys keyboard help" },
    { id: "mouseBack", section: "controls", card: "Mouse", label: "Back button", desc: "The thumb button on an MX Master, MX Anywhere or G-series mouse.", keys: "mouse thumb side button" },
    { id: "mouseForward", section: "controls", card: "Mouse", label: "Forward button", desc: "The other thumb button.", keys: "mouse thumb side button" },
    { id: "controller", section: "controls", card: "Game controller", label: "PS5 / PS4 controller", desc: "Cull from the sofa: pairing, a button tester and remapping.", keys: "gamepad dualsense dualshock pad joystick", link: true },

    { id: "updates", section: "about", card: "", label: "Updates", desc: "", keys: "version nightly stable release install", wide: true },
  ];

  // What the mouse's extra buttons can do (a small, curated subset).
  const MOUSE_CHOICES: [string, string][] = [
    ["viewBack", "Back to grid"],
    ["viewForward", "Open Focus"],
    ["toggleView", "Open / close Focus"],
    ["pick", "Pick"],
    ["reject", "Reject"],
    ["prev", "Previous item"],
    ["next", "Next item"],
    ["fullscreen", "Play mode (full screen)"],
    ["toggleFilmstrip", "Show / hide filmstrip"],
  ];

  // Colours copied from app.css so each tile previews its theme even while
  // another one is active.
  const THEMES: { id: Theme; name: string; bg: string; panel: string; elev: string; accent: string; line: string }[] = [
    { id: "neutral", name: "Studio", bg: "#17191d", panel: "#202329", elev: "#343a43", accent: "#78b9ef", line: "#3a4049" },
    { id: "dark", name: "Midnight", bg: "#0d1015", panel: "#141920", elev: "#27313c", accent: "#63b7f2", line: "#2d3742" },
    { id: "warm", name: "Amber", bg: "#1b1917", panel: "#24211e", elev: "#3b3630", accent: "#d8ad68", line: "#443e37" },
    { id: "light", name: "Daylight", bg: "#d9e0e7", panel: "#f7f9fb", elev: "#ffffff", accent: "#2d7fc2", line: "#cbd4dd" },
  ];
  const SCALES: [UiScale, string][] = [["compact", "Compact"], ["comfortable", "Standard"], ["distance", "TV"]];
  const DOCKS: [FilmstripPos, string][] = [["bottom", "Bottom"], ["left", "Left"], ["right", "Right"], ["hidden", "Off"]];

  let query = $state("");
  let searchEl: HTMLInputElement | undefined = $state();
  let bodyEl: HTMLElement | undefined = $state();
  let cache = $state<{ dir: string; bytes: number; files: number } | null>(null);
  let cacheLoading = $state(false);

  let section = $derived<Section>(page === "excludes" || page === "controller" ? SUBPAGES[page].parent : page);
  let words = $derived(query.trim().toLowerCase().split(/\s+/).filter(Boolean));
  let results = $derived(
    words.length
      ? ROWS.filter((r) => {
          const hay = `${r.label} ${r.desc} ${r.keys ?? ""} ${r.card} ${SECTIONS.find((s) => s.id === r.section)?.title}`.toLowerCase();
          return words.every((w) => hay.includes(w));
        })
      : [],
  );
  let prepJob = $derived(activity.jobs["prepare"]);
  let prepRunning = $derived(preparing && prepJob?.state === "running");

  function cardsOf(rows: Row[]): { card: string; rows: Row[] }[] {
    const out: { card: string; rows: Row[] }[] = [];
    for (const r of rows) {
      const last = out[out.length - 1];
      if (last && last.card === r.card) last.rows.push(r);
      else out.push({ card: r.card, rows: [r] });
    }
    return out;
  }

  function go(p: SettingsPage) {
    page = p;
    query = "";
    bodyEl?.scrollTo({ top: 0 });
  }

  async function loadCache() {
    cacheLoading = true;
    cache = await api.cacheUsage();
    cacheLoading = false;
  }
  // The cache is measured when its row is on screen, and again when a build
  // finishes (that's when the number moves).
  let cacheVisible = $derived(page === "speed" || results.some((r) => r.id === "cache"));
  $effect(() => {
    if (cacheVisible && !cache && !cacheLoading) void loadCache();
  });
  let lastPrepState: string | undefined;
  $effect(() => {
    const st = prepJob?.state;
    if (lastPrepState === "running" && st && st !== "running" && cacheVisible) void loadCache();
    lastPrepState = st;
  });

  function onkeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
      searchEl?.focus();
      searchEl?.select();
      e.preventDefault();
      return;
    }
    if (e.key !== "Escape") return;
    e.preventDefault();
    if (query) query = "";
    else if (page === "excludes" || page === "controller") go(SUBPAGES[page].parent);
    else onclose();
  }

  onMount(() => {
    void tick().then(() => searchEl?.focus({ preventScroll: true }));
  });

  const folderName = (p: string | null) => (p ? p.split(/[\\/]/).filter(Boolean).pop() ?? p : "");
  let excludeSummary = $derived.by(() => {
    const ex = settings.s.scanExcludes;
    const groups = [ex.windowsSystem, ex.macosSystem, ex.appData, ex.developer, ex.games].filter(Boolean).length;
    const own = ex.paths.length + ex.names.length;
    return `${groups} of 5 built-in groups${own ? ` · ${own} of yours` : ""}`;
  });
</script>

<svelte:window {onkeydown} />

<div class="scrim" role="presentation" onclick={onclose}></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label="Settings">
  <nav class="side">
    <div class="sideHead">
      <span class="sideTitle">Settings</span>
    </div>
    <label class="search">
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6.5" /><path d="m16 16 4.5 4.5" /></svg>
      <input bind:this={searchEl} type="search" placeholder="Search" bind:value={query} spellcheck="false" aria-label="Search settings" />
    </label>
    <div class="navList">
      {#each SECTIONS as s (s.id)}
        <button class="navItem" class:on={!words.length && section === s.id} onclick={() => go(s.id)}>
          <span class="navIco" data-s={s.id}>{@render icon(s.id)}</span>
          <span class="navLabel">{s.title}</span>
          {#if s.id === "about" && updates.available}<span class="navDot" title="An update is available"></span>{/if}
          {#if s.id === "speed" && prepRunning}<span class="navSpin" title="Building previews"></span>{/if}
        </button>
      {/each}
    </div>
    <div class="sideFoot">
      {#if updates.status?.current}FoxCull {updates.status.current}{:else}FoxCull{/if}
    </div>
  </nav>

  <main class="main">
    <header class="head">
      {#if words.length}
        <div class="titles">
          <h2>Search</h2>
          <p>{results.length ? `${results.length} setting${results.length === 1 ? " matches" : "s match"} “${query.trim()}”` : `Nothing matches “${query.trim()}”`}</p>
        </div>
      {:else if page === "excludes" || page === "controller"}
        <div class="titles">
          <button class="crumb" onclick={() => go(SUBPAGES[page as "excludes" | "controller"].parent)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m14.5 6-6 6 6 6" /></svg>
            {SECTIONS.find((s) => s.id === section)?.title}
          </button>
          <h2>{SUBPAGES[page].title}</h2>
          <p>{SUBPAGES[page].blurb}</p>
        </div>
      {:else}
        <div class="titles">
          <h2>{SECTIONS.find((s) => s.id === page)?.title}</h2>
          <p>{SECTIONS.find((s) => s.id === page)?.blurb}</p>
        </div>
      {/if}
      <button class="close" onclick={onclose} title="Close (Esc)" aria-label="Close settings">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11" /></svg>
      </button>
    </header>

    <div class="body" bind:this={bodyEl}>
      {#if words.length}
        {#each SECTIONS.filter((s) => results.some((r) => r.section === s.id)) as s (s.id)}
          <div class="resultHead">{s.title}</div>
          <div class="card">
            {#each results.filter((r) => r.section === s.id) as r (r.id)}{@render row(r)}{/each}
          </div>
        {/each}
        {#if !results.length}
          <p class="empty">Try another word: “dark”, “delete”, “controller”, “cache”…</p>
        {/if}
      {:else if page === "excludes"}
        <ExcludePanel />
      {:else if page === "controller"}
        <ControllerPanel />
      {:else}
        {#each cardsOf(ROWS.filter((r) => r.section === page)) as c (c.card)}
          {#if c.card}<div class="cardHead">{c.card}</div>{/if}
          <div class="card" class:bare={!c.card}>
            {#each c.rows as r (r.id)}{@render row(r)}{/each}
          </div>
        {/each}
      {/if}
    </div>
  </main>
</div>

{#snippet row(r: Row)}
  {#if r.link}
    <button class="srow link" onclick={() => go(r.id as SettingsPage)}>
      <span class="text"><span class="label">{r.label}</span><span class="desc">{r.desc}</span></span>
      <span class="ctl">
        <span class="linkVal">
          {#if r.id === "excludes"}{excludeSummary}{:else if r.id === "controller"}<span class:ok={pad.connected}>{pad.connected ? "Connected" : settings.s.padEnabled ? "Not connected" : "Off"}</span>{/if}
        </span>
        <svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m9.5 6 6 6-6 6" /></svg>
      </span>
    </button>
  {:else if r.id === "updates"}
    <div class="srow updatesRow"><UpdatePanel title="FoxCull" /></div>
  {:else}
    <div class="srow" class:wide={r.wide}>
      <span class="text"><span class="label">{r.label}</span><span class="desc">{r.desc}</span></span>
      <span class="ctl">{@render control(r.id)}</span>
    </div>
  {/if}
{/snippet}

{#snippet toggle(on: boolean, set: (v: boolean) => void, label: string)}
  <button class="switch" class:on role="switch" aria-checked={on} aria-label={label} onclick={() => set(!on)}><span class="knob"></span></button>
{/snippet}

{#snippet control(id: string)}
  {#if id === "theme"}
    <div class="themes" role="radiogroup" aria-label="Theme">
      {#each THEMES as t (t.id)}
        <button class="themeTile" class:on={settings.s.theme === t.id} role="radio" aria-checked={settings.s.theme === t.id} onclick={() => settings.set({ theme: t.id })}>
          <span class="mini" style="--b:{t.bg};--p:{t.panel};--e:{t.elev};--a:{t.accent};--l:{t.line}">
            <span class="miniSide"><i></i><i class="hl"></i><i></i></span>
            <span class="miniGrid"><i></i><i class="sel"></i><i></i><i></i><i></i><i></i></span>
          </span>
          <span class="themeName">{t.name}</span>
        </button>
      {/each}
    </div>
  {:else if id === "uiScale"}
    <div class="seg" role="radiogroup" aria-label="Interface size">
      {#each SCALES as [v, l] (v)}
        <button class:on={settings.s.uiScale === v} role="radio" aria-checked={settings.s.uiScale === v} onclick={() => settings.set({ uiScale: v })}>{l}</button>
      {/each}
    </div>
  {:else if id === "filmstrip"}
    <div class="seg" role="radiogroup" aria-label="Filmstrip">
      {#each DOCKS as [v, l] (v)}
        <button class:on={settings.s.filmstripPos === v} role="radio" aria-checked={settings.s.filmstripPos === v} onclick={() => onfilmstrip(v)}>
          <svg class="dock" viewBox="0 0 16 12" aria-hidden="true"><rect x="0.5" y="0.5" width="15" height="11" rx="2" />{#if v === "bottom"}<rect class="f" x="2" y="8" width="12" height="2" rx=".6" />{:else if v === "left"}<rect class="f" x="2" y="2" width="2.4" height="8" rx=".6" />{:else if v === "right"}<rect class="f" x="11.6" y="2" width="2.4" height="8" rx=".6" />{/if}</svg>
          {l}
        </button>
      {/each}
    </div>
  {:else if id === "tileLength"}
    {@render toggle(settings.s.tileInfo.duration, (v) => settings.set({ tileInfo: { ...settings.s.tileInfo, duration: v } }), "Video length on tiles")}
  {:else if id === "tileName"}
    {@render toggle(settings.s.tileInfo.name, (v) => settings.set({ tileInfo: { ...settings.s.tileInfo, name: v } }), "File names on tiles")}
  {:else if id === "stacks"}
    <div class="seg" role="radiogroup" aria-label="Stacks">
      <button class:on={settings.s.relatedMode === "expanded"} role="radio" aria-checked={settings.s.relatedMode === "expanded"} onclick={() => onstacks("expanded")}>Open</button>
      <button class:on={settings.s.relatedMode === "collapsed"} role="radio" aria-checked={settings.s.relatedMode === "collapsed"} onclick={() => onstacks("collapsed")}>Folded{foldableCount ? ` · ${foldableCount}` : ""}</button>
    </div>
  {:else if id === "autoplay"}
    {@render toggle(settings.s.videoAutoplay, (v) => settings.set({ videoAutoplay: v }), "Play videos when opened")}
  {:else if id === "minimalBar"}
    {@render toggle(settings.s.minimalVideoBar, (v) => settings.set({ minimalVideoBar: v }), "Minimal video bar")}
  {:else if id === "glimpse"}
    <span class="slider">
      <input type="range" min={GLIMPSE_MIN} max={GLIMPSE_MAX} step="1" value={settings.s.glimpseSpeed} oninput={(e) => settings.set({ glimpseSpeed: +e.currentTarget.value })} aria-label="Glimpse speed" />
      <span class="sliderVal">{settings.s.glimpseSpeed}×</span>
    </span>
  {:else if id === "prepare"}
    <div class="prep">
      {#if prepRunning && prepJob}
        <div class="prepTop">
          <span class="prepWhat">{prepJob.label}</span>
          <button class="btn sm" onclick={() => activity.cancel("prepare")}>Stop</button>
        </div>
        <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={prepJob.total} aria-valuenow={prepJob.done}>
          <i style="width:{prepJob.total ? (100 * prepJob.done) / prepJob.total : 0}%"></i>
        </div>
        <span class="prepNote">{prepJob.done.toLocaleString()} of {prepJob.total.toLocaleString()} · it carries on if you close Settings</span>
      {:else}
        <div class="prepTop">
          <span class="prepWhat">
            {#if currentDir}
              <b>{folderName(currentDir)}</b> · {folderCount.toLocaleString()} item{folderCount === 1 ? "" : "s"}
            {:else}
              Open a folder first
            {/if}
          </span>
          <button class="btn sm accent" disabled={!currentDir || !folderCount} onclick={onprepare}>Prepare</button>
        </div>
        {#if prepJob && prepJob.state !== "running"}
          <span class="prepNote">{prepJob.label}{prepJob.detail ? ` · ${prepJob.detail}` : ""}</span>
        {/if}
      {/if}
    </div>
  {:else if id === "cache"}
    <span class="pair">
      <span class="val">{cache ? `${fmtBytes(cache.bytes)} · ${cache.files.toLocaleString()} files` : cacheLoading ? "Measuring…" : "—"}</span>
      <button class="btn sm" disabled={!cache} onclick={() => cache && api.reveal(cache.dir)}>Show</button>
    </span>
  {:else if id === "liveDecode"}
    {@render toggle(settings.s.liveDecodeScrub, (v) => settings.set({ liveDecodeScrub: v }), "Live scrubbing in Focus")}
  {:else if id === "spriteFallback"}
    {@render toggle(settings.s.liveScrub, (v) => settings.set({ liveScrub: v }), "Sprite sheets for skimming")}
  {:else if id === "deleteMode"}
    <div class="seg" role="radiogroup" aria-label="When you delete">
      <button class:on={settings.s.deleteMode === "folder"} role="radio" aria-checked={settings.s.deleteMode === "folder"} onclick={() => settings.set({ deleteMode: "folder" })}>In-app Trash</button>
      <button class:on={settings.s.deleteMode === "recycle"} role="radio" aria-checked={settings.s.deleteMode === "recycle"} onclick={() => settings.set({ deleteMode: "recycle" })}>System Trash</button>
    </div>
  {:else if id === "trash"}
    <span class="pair">
      {#if trashCount}<span class="val">{trashCount.toLocaleString()} item{trashCount === 1 ? "" : "s"}</span>{/if}
      <button class="btn sm" disabled={!libInfo} onclick={onopentrash}>Open Trash</button>
    </span>
  {:else if id === "scanOnLaunch"}
    {@render toggle(settings.s.scanOnLaunch, (v) => settings.set({ scanOnLaunch: v }), "Check the catalog at launch")}
  {:else if id === "checkNow"}
    <button class="btn sm" disabled={scanning || !libInfo} onclick={oncheckcatalog}>{scanning ? "Checking…" : "Check now"}</button>
  {:else if id === "library"}
    {#if libInfo}
      <div class="libBox">
        <span class="libPath" title={libInfo.dir}>{libInfo.dir}</span>
        <span class="libTag" class:warn={!libInfo.on_drive}>{libInfo.on_drive ? `On ${driveLabel || "the drive"}` : "In app data: the drive is read-only"}</span>
        <button class="btn sm" onclick={() => libInfo && api.reveal(libInfo.catalog)}>Show</button>
      </div>
    {:else}
      <span class="val">Open a folder to see where its library lives.</span>
    {/if}
  {:else if id === "shortcuts"}
    <button class="btn sm" onclick={onshortcuts}>Show all <kbd>?</kbd></button>
  {:else if id === "mouseBack" || id === "mouseForward"}
    {@const key = id === "mouseBack" ? "mouseBack" : "mouseForward"}
    <select class="select" value={settings.s[key]} onchange={(e) => settings.set({ [key]: e.currentTarget.value })} aria-label={id === "mouseBack" ? "Back button" : "Forward button"}>
      {#each MOUSE_CHOICES as [v, l] (v)}<option value={v}>{l}</option>{/each}
    </select>
  {/if}
{/snippet}

{#snippet icon(id: Section)}
  <svg viewBox="0 0 24 24" aria-hidden="true">
    {#if id === "appearance"}
      <circle cx="12" cy="12" r="8.5" /><path d="M12 3.5v17a8.5 8.5 0 0 0 0-17z" class="fill" />
    {:else if id === "playback"}
      <rect x="3.5" y="5" width="17" height="14" rx="3" /><path d="m10.5 9.2 4.4 2.8-4.4 2.8z" class="fill" />
    {:else if id === "speed"}
      <path d="M13 3.5 5.5 13.5H12l-1 7 7.5-10H12z" />
    {:else if id === "files"}
      <path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v7.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z" />
    {:else if id === "controls"}
      <rect x="2.5" y="6.5" width="19" height="11" rx="3" /><path d="M6.5 10h1M10 10h1M13.5 10h1M17 10h.5M8 14h8" />
    {:else}
      <circle cx="12" cy="12" r="8.5" /><path d="M12 11v5.5M12 7.6v.2" />
    {/if}
  </svg>
{/snippet}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 300;
    background: rgba(0, 0, 0, 0.42);
    backdrop-filter: blur(3px);
    animation: fade 140ms ease-out;
  }
  .sheet {
    position: fixed;
    left: 50%;
    top: 50%;
    z-index: 301;
    transform: translate(-50%, -50%);
    width: min(940px, calc(100vw - 32px));
    height: min(660px, calc(100vh - 40px));
    display: grid;
    grid-template-columns: 216px minmax(0, 1fr);
    overflow: hidden;
    background: var(--bg-panel);
    border: 1px solid color-mix(in srgb, var(--border-strong) 70%, transparent);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow), 0 0 0 1px color-mix(in srgb, black 20%, transparent);
    animation: rise 170ms cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  @keyframes fade { from { opacity: 0; } }
  @keyframes rise { from { opacity: 0; transform: translate(-50%, calc(-50% + 8px)) scale(0.985); } }

  /* ── sidebar ── */
  .side {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 14px 10px 10px;
    background: color-mix(in srgb, var(--bg) 70%, var(--bg-panel));
    border-right: 1px solid var(--border-soft);
  }
  .sideHead { padding: 2px 8px 10px; }
  .sideTitle { font-family: var(--font-display); font-size: 15px; font-weight: 650; letter-spacing: -0.01em; }
  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 9px;
    margin-bottom: 10px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-soft);
    background: color-mix(in srgb, var(--bg-elev) 70%, transparent);
    color: var(--text-faint);
  }
  .search:focus-within { border-color: color-mix(in srgb, var(--accent) 70%, transparent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent); }
  .search svg { width: 14px; height: 14px; flex: none; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; }
  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    font-size: 12.5px;
    outline: none;
  }
  .search input::-webkit-search-cancel-button { filter: grayscale(1); opacity: 0.6; }
  .navList { display: flex; flex-direction: column; gap: 1px; overflow-y: auto; min-height: 0; }
  .navItem {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 32px;
    padding: 0 8px;
    border-radius: var(--radius-sm);
    color: var(--text-dim);
    font-size: 13px;
    font-weight: 520;
    text-align: left;
  }
  .navItem:hover { background: color-mix(in srgb, var(--bg-hover) 70%, transparent); color: var(--text); }
  .navItem.on { background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--text); }
  .navIco {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    flex: none;
    border-radius: 6px;
    color: #fff;
  }
  .navIco[data-s="appearance"] { background: linear-gradient(160deg, #8b7cf6, #6151d8); }
  .navIco[data-s="playback"] { background: linear-gradient(160deg, #f0717f, #d94a5d); }
  .navIco[data-s="speed"] { background: linear-gradient(160deg, #f5b84a, #e49422); }
  .navIco[data-s="files"] { background: linear-gradient(160deg, #5cb4f4, #2f8fd8); }
  .navIco[data-s="controls"] { background: linear-gradient(160deg, #6fcf97, #3fa86e); }
  .navIco[data-s="about"] { background: linear-gradient(160deg, #98a2ae, #6c7784); }
  .navIco svg { width: 15px; height: 15px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .navIco svg :global(.fill) { fill: currentColor; stroke: none; }
  .navLabel { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .navDot { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }
  .navSpin {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .sideFoot { margin-top: auto; padding: 10px 8px 2px; font-size: 11px; color: var(--text-faint); font-variant-numeric: tabular-nums; }

  /* ── content ── */
  .main { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .head {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 18px 22px 14px 26px;
  }
  .titles { flex: 1; min-width: 0; }
  h2 { margin: 0; font-family: var(--font-display); font-size: 19px; font-weight: 650; letter-spacing: -0.02em; }
  .titles p { margin: 3px 0 0; color: var(--text-dim); font-size: 12.5px; }
  .crumb {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin: -2px 0 4px -5px;
    padding: 2px 6px 2px 2px;
    border-radius: 6px;
    color: var(--accent);
    font-size: 12px;
    font-weight: 560;
  }
  .crumb:hover { background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .crumb svg { width: 15px; height: 15px; fill: none; stroke: currentColor; stroke-width: 2.2; stroke-linecap: round; stroke-linejoin: round; }
  .close {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    flex: none;
    border-radius: 50%;
    color: var(--text-dim);
    background: color-mix(in srgb, var(--bg-hover) 60%, transparent);
  }
  .close:hover { background: var(--bg-hover); color: var(--text); }
  .close svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; }
  .body { flex: 1; min-height: 0; overflow-y: auto; padding: 0 22px 24px 26px; }

  .cardHead, .resultHead {
    margin: 16px 0 7px 2px;
    font-size: 11.5px;
    font-weight: 650;
    color: var(--text-faint);
  }
  .cardHead:first-child, .resultHead:first-child { margin-top: 2px; }
  .card {
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--bg-elev) 82%, transparent);
    overflow: hidden;
  }
  .card.bare { border: 0; background: none; overflow: visible; }
  .srow {
    display: flex;
    align-items: center;
    gap: 18px;
    width: 100%;
    min-height: 52px;
    padding: 10px 14px;
    text-align: left;
  }
  .srow + .srow { border-top: 1px solid var(--border-soft); }
  .srow.wide { flex-direction: column; align-items: stretch; gap: 10px; }
  .srow.link:hover { background: color-mix(in srgb, var(--bg-hover) 55%, transparent); }
  .text { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .label { font-size: 13px; font-weight: 560; color: var(--text); }
  .desc { font-size: 11.75px; line-height: 1.45; color: var(--text-faint); max-width: 62ch; }
  .ctl { flex: none; display: flex; align-items: center; gap: 8px; }
  .wide .ctl { display: block; }
  .updatesRow { display: block; padding: 0; }
  .val, .linkVal { font-size: 12px; color: var(--text-dim); font-variant-numeric: tabular-nums; }
  .linkVal .ok { color: var(--pick); }
  .chev { width: 16px; height: 16px; fill: none; stroke: var(--text-faint); stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .pair { display: flex; align-items: center; gap: 10px; }
  .empty { color: var(--text-faint); font-size: 12.5px; margin: 6px 2px; }

  /* switch */
  .switch {
    position: relative;
    width: 38px;
    height: 22px;
    flex: none;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text-faint) 38%, var(--bg-elev));
    transition: background 140ms ease;
  }
  .switch .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
    transition: transform 160ms cubic-bezier(0.3, 0.7, 0.3, 1);
  }
  .switch.on { background: var(--accent); }
  .switch.on .knob { transform: translateX(16px); }

  /* segmented control */
  .seg {
    display: inline-flex;
    padding: 2px;
    gap: 2px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--bg) 75%, transparent);
    border: 1px solid var(--border-soft);
  }
  .seg button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 11px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 540;
    color: var(--text-dim);
    white-space: nowrap;
  }
  .seg button:hover:not(.on) { color: var(--text); }
  .seg button.on {
    background: var(--bg-elev);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.22), 0 0 0 1px var(--border-soft);
  }
  .dock { width: 15px; height: 12px; fill: none; stroke: currentColor; stroke-width: 1; }
  .dock .f { fill: currentColor; stroke: none; }
  .seg button.on .dock .f { fill: var(--accent); }

  /* theme tiles */
  .themes { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; }
  .themeTile { display: flex; flex-direction: column; align-items: center; gap: 7px; padding: 0; }
  .mini {
    display: flex;
    gap: 5px;
    width: 100%;
    aspect-ratio: 16 / 10;
    padding: 6px;
    border-radius: 10px;
    background: var(--b);
    box-shadow: 0 0 0 1px var(--l), 0 2px 8px rgba(0, 0, 0, 0.18);
    transition: box-shadow 120ms ease;
  }
  .themeTile:hover .mini { box-shadow: 0 0 0 1px var(--l), 0 0 0 4px color-mix(in srgb, var(--text-faint) 22%, transparent); }
  .themeTile.on .mini { box-shadow: 0 0 0 2px var(--accent), 0 0 0 5px color-mix(in srgb, var(--accent) 22%, transparent); }
  .miniSide { display: flex; flex-direction: column; gap: 3px; width: 26%; padding: 4px 3px; border-radius: 5px; background: var(--p); }
  .miniSide i { height: 4px; border-radius: 2px; background: var(--e); }
  .miniSide i.hl { background: var(--a); opacity: 0.75; }
  .miniGrid { flex: 1; display: grid; grid-template-columns: repeat(3, 1fr); gap: 3px; }
  .miniGrid i { border-radius: 3px; background: var(--e); }
  .miniGrid i.sel { box-shadow: inset 0 0 0 1.5px var(--a); }
  .themeName { font-size: 12px; font-weight: 540; color: var(--text-dim); }
  .themeTile.on .themeName { color: var(--text); }

  /* slider */
  .slider { display: flex; align-items: center; gap: 10px; }
  .slider input { width: 150px; }
  .sliderVal { min-width: 30px; text-align: right; font-size: 12px; color: var(--text-dim); font-variant-numeric: tabular-nums; }

  /* Build previews */
  .prep {
    display: flex;
    flex-direction: column;
    gap: 7px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--bg) 55%, transparent);
    border: 1px solid var(--border-soft);
  }
  .prepTop { display: flex; align-items: center; gap: 12px; }
  .prepWhat { flex: 1; min-width: 0; font-size: 12.5px; color: var(--text-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .prepWhat b { color: var(--text); font-weight: 600; }
  .prepNote { font-size: 11.5px; color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .bar { height: 5px; border-radius: 999px; background: color-mix(in srgb, var(--text-faint) 22%, transparent); overflow: hidden; }
  .bar i { display: block; height: 100%; border-radius: inherit; background: var(--accent); transition: width 200ms ease; }

  /* library location */
  .libBox {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 8px 8px 12px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--bg) 55%, transparent);
    border: 1px solid var(--border-soft);
  }
  .libPath { flex: 1; min-width: 0; font-size: 12px; color: var(--text-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .libTag { flex: none; font-size: 11px; padding: 2px 8px; border-radius: 999px; color: var(--pick); background: color-mix(in srgb, var(--pick) 13%, transparent); }
  .libTag.warn { color: var(--star); background: color-mix(in srgb, var(--star) 13%, transparent); }

  .select { height: 28px; padding: 0 8px; font-size: 12.5px; min-width: 190px; }
  kbd { margin-left: 4px; padding: 0 5px; border-radius: 4px; border: 1px solid var(--border); background: var(--bg-panel); font-size: 10.5px; }

  /* Narrow windows: the sidebar becomes a row of tabs above the content. */
  @media (max-width: 760px) {
    .sheet { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); height: calc(100vh - 24px); width: calc(100vw - 24px); }
    .side { flex-direction: row; flex-wrap: wrap; align-items: center; gap: 8px; padding: 10px; border-right: 0; border-bottom: 1px solid var(--border-soft); }
    .sideHead, .sideFoot { display: none; }
    .search { margin: 0; flex: 1 1 100%; }
    .navList { flex-direction: row; overflow-x: auto; gap: 4px; flex: 1 1 100%; }
    .navItem { flex: none; }
    .srow:not(.wide) { flex-wrap: wrap; }
  }
  @media (max-width: 520px) {
    .themes { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }
</style>
