<script lang="ts">
  // Settings → Excluded folders. Built-in groups (pre-ticked) plus the user's own
  // folders and name patterns. Every change writes straight through to the
  // settings store, so it persists on this device; the page re-applies the rules
  // to the tree and the open folder when the panel closes (see `onclose`).
  import { api } from "$lib/api";
  import { settings, defaultScanExcludes } from "$lib/settings.svelte";
  import type { ScanExcludes } from "$lib/types";

  let { onclose }: { onclose: () => void } = $props();

  type GroupKey = "windowsSystem" | "macosSystem" | "appData" | "developer" | "games";

  // Keep these lists in step with the matchers in src-tauri/src/commands.rs
  // (`is_windows_root_dir`, `is_macos_root_dir`, ...). They are shown to the user
  // so they can see exactly what a group hides.
  const GROUPS: { key: GroupKey; title: string; where: string; names: string[] }[] = [
    {
      key: "windowsSystem",
      title: "Windows system folders",
      where: "Directly inside a drive such as C:\\ or D:\\, plus Windows update leftovers anywhere",
      names: ["Windows", "Program Files", "Program Files (x86)", "ProgramData", "Recovery", "PerfLogs", "Intel", "AMD", "NVIDIA", "Drivers", "OneDriveTemp", "inetpub", "Windows.old", "$Windows.~BT", "$Windows.~WS", "$SysReset", "$WinREAgent", "$GetCurrent", "Config.Msi", "MSOCache"],
    },
    {
      key: "macosSystem",
      title: "macOS system folders",
      where: "Directly inside your Mac's startup disk (/). External drives are left alone",
      names: ["System", "Library", "Applications", "private", "usr", "bin", "sbin", "opt", "cores", "dev", "etc", "var", "tmp", "Network", "Developer"],
    },
    {
      key: "appData",
      title: "App data & app bundles",
      where: "Library and Applications inside a home folder; AppData and app bundles anywhere",
      names: ["~/Library", "~/Applications", "AppData", "WindowsApps", "*.app", "*.framework", "*.bundle", "*.plugin", "*.kext", "*.xpc", "*.appex", "*.lproj", "*.xcassets"],
    },
    {
      key: "developer",
      title: "Developer folders",
      where: "Anywhere",
      names: ["node_modules", "bower_components", "__pycache__", "site-packages", "venv", "DerivedData"],
    },
    {
      key: "games",
      title: "Game libraries",
      where: "Anywhere. Thousands of game textures look exactly like photos",
      names: ["steamapps", "SteamLibrary", "XboxGames", "Epic Games", "Riot Games", "GOG Games"],
    },
  ];

  let ex = $derived(settings.s.scanExcludes);
  let draft = $state("");
  let error = $state("");

  function save(patch: Partial<ScanExcludes>) {
    settings.set({ scanExcludes: { ...ex, ...patch } });
  }

  const isAbsolute = (s: string) => s.startsWith("/") || /^[A-Za-z]:[\\/]/.test(s);
  const has = (list: string[], v: string) => list.some((x) => x.toLowerCase() === v.toLowerCase());

  function addPath(p: string) {
    if (has(ex.paths, p)) return;
    save({ paths: [...ex.paths, p] });
  }

  async function addFolder() {
    const picked = await api.pickFolder();
    if (picked) addPath(picked);
  }

  function addDraft() {
    const v = draft.trim();
    error = "";
    if (!v) return;
    if (isAbsolute(v)) {
      addPath(v);
    } else if (/[\\/]/.test(v)) {
      error = "A name can't contain / or \\. To exclude one specific folder, use Add folder… or paste its full path.";
      return;
    } else if (!has(ex.names, v)) {
      save({ names: [...ex.names, v] });
    }
    draft = "";
  }

  function reset() {
    settings.set({ scanExcludes: defaultScanExcludes() });
    error = "";
  }

  let isDefault = $derived(JSON.stringify(ex) === JSON.stringify(defaultScanExcludes()));
  let customCount = $derived(ex.paths.length + ex.names.length);
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="backdrop" onclick={onclose} role="presentation"></div>
<div class="panel" role="dialog" aria-label="Excluded folders">
  <header>
    <div>
      <h2>Excluded folders</h2>
      <p class="sub">FoxCull never scans, counts or shows these. Saved on this device.</p>
    </div>
    <span class="grow"></span>
    <button class="x" onclick={onclose} title="Close (Esc)" aria-label="Close">✕</button>
  </header>

  <div class="scroll">
    <section>
      <h3>Built-in <span class="count">recommended — all on by default</span></h3>
      <div class="groups">
        {#each GROUPS as g (g.key)}
          <label class="group" class:off={!ex[g.key]}>
            <input type="checkbox" checked={ex[g.key]} onchange={(e) => save({ [g.key]: (e.currentTarget as HTMLInputElement).checked })} />
            <div class="gBody">
              <div class="gTitle">{g.title}</div>
              <div class="gWhere">{g.where}</div>
              <div class="names">
                {#each g.names as n (n)}<code>{n}</code>{/each}
              </div>
            </div>
          </label>
        {/each}
      </div>
    </section>

    <section>
      <h3>Your exclusions {#if customCount}<span class="count">{customCount}</span>{/if}</h3>
      <div class="add">
        <input
          type="text"
          placeholder="Folder name, e.g. Proxy or *_cache — or a full path"
          bind:value={draft}
          onkeydown={(e) => { if (e.key === "Enter") addDraft(); }}
          spellcheck="false"
        />
        <button class="btn" onclick={addDraft} disabled={!draft.trim()}>Add</button>
        <button class="btn" onclick={addFolder}>Add folder…</button>
      </div>
      {#if error}<p class="error">{error}</p>{/if}

      {#if customCount === 0}
        <p class="empty">
          Nothing yet. Right-click any folder in the sidebar and choose <b>Exclude from scans</b>, or add one above.
          A <b>name</b> is skipped wherever it appears (<code>*</code> matches anything); a <b>folder</b> is skipped with
          everything inside it.
        </p>
      {:else}
        <ul class="rules">
          {#each ex.paths as p (p)}
            <li>
              <span class="kind">Folder</span>
              <span class="val" title={p}>{p}</span>
              <button class="rm" onclick={() => save({ paths: ex.paths.filter((x) => x !== p) })} title="Stop excluding" aria-label="Remove {p}">✕</button>
            </li>
          {/each}
          {#each ex.names as n (n)}
            <li>
              <span class="kind name">Name</span>
              <span class="val">{n}</span>
              <button class="rm" onclick={() => save({ names: ex.names.filter((x) => x !== n) })} title="Stop excluding" aria-label="Remove {n}">✕</button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <p class="always">
      Always skipped, whatever is ticked: hidden folders, FoxCull's own library and Trash, the Recycle Bin,
      System Volume Information, and <code>/Volumes</code> when scanning a Mac's startup disk (other drives have their
      own entries in the sidebar).
    </p>
  </div>

  <footer>
    <button class="btn" onclick={reset} disabled={isDefault}>Reset to defaults</button>
    <span class="grow"></span>
    <button class="btn accent" onclick={onclose}>Done</button>
  </footer>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.66);
    backdrop-filter: blur(6px);
    z-index: 100;
  }
  .panel {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(680px, calc(100vw / var(--ui-scale) - 32px));
    max-height: calc(100vh / var(--ui-scale) - 48px);
    z-index: 101;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: color-mix(in srgb, var(--bg-panel) 97%, transparent);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow);
  }
  header, footer {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 18px;
  }
  header { border-bottom: 1px solid var(--border-soft); }
  footer { border-top: 1px solid var(--border-soft); }
  h2 { margin: 0; font-family: var(--font-display); font-size: 17px; letter-spacing: -0.015em; }
  .sub { margin: 3px 0 0; color: var(--text-dim); font-size: 12.5px; }
  .grow { flex: 1; }
  .x { width: 30px; height: 30px; border-radius: 7px; color: var(--text-dim); font-size: 13px; }
  .x:hover { background: var(--bg-hover); color: var(--text); }

  .scroll { flex: 1; overflow-y: auto; padding: 6px 18px 16px; user-select: text; }
  section { margin-top: 14px; }
  h3 {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0 0 9px;
    color: var(--text-faint);
    font-size: 10.5px;
    font-weight: 720;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }
  .count { color: var(--text-faint); font-size: 11px; font-weight: 500; letter-spacing: 0; text-transform: none; }

  .groups { display: flex; flex-direction: column; gap: 7px; }
  .group {
    display: flex;
    gap: 11px;
    padding: 11px 13px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--bg-elev) 45%, transparent);
    cursor: pointer;
    transition: opacity 120ms ease, border-color 120ms ease;
  }
  .group:hover { border-color: var(--border); }
  .group.off { opacity: 0.62; }
  .group input { margin-top: 2px; accent-color: var(--accent); width: 15px; height: 15px; flex: none; }
  .gBody { min-width: 0; }
  .gTitle { color: var(--text); font-size: 13px; font-weight: 620; }
  .gWhere { margin-top: 2px; color: var(--text-dim); font-size: 12px; }
  .names { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 7px; }
  code {
    padding: 1px 6px;
    border-radius: 5px;
    background: color-mix(in srgb, var(--bg-hover) 70%, transparent);
    color: var(--text-dim);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, monospace;
    font-size: 11px;
  }

  .add { display: flex; gap: 7px; }
  .add input { flex: 1; min-width: 0; min-height: var(--control-h); padding: 5px 10px; font-size: 12.5px; user-select: text; }
  .error { margin: 7px 0 0; color: var(--reject); font-size: 12px; }
  .empty { margin: 10px 0 0; color: var(--text-dim); font-size: 12.5px; line-height: 1.6; }
  .empty b { color: var(--text); font-weight: 600; }

  .rules { list-style: none; margin: 10px 0 0; padding: 0; display: flex; flex-direction: column; gap: 5px; }
  .rules li {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 6px 6px 6px 10px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--bg-elev) 45%, transparent);
  }
  .kind {
    flex: none;
    padding: 1px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    font-size: 10.5px;
    font-weight: 650;
  }
  .kind.name { background: color-mix(in srgb, var(--stack) 16%, transparent); color: var(--stack); }
  .val { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); font-size: 12.5px; }
  .rm { width: 26px; height: 26px; border-radius: 6px; color: var(--text-faint); font-size: 11px; }
  .rm:hover { background: var(--bg-hover); color: var(--reject); }

  .always { margin: 18px 0 0; color: var(--text-faint); font-size: 11.5px; line-height: 1.6; }
</style>
