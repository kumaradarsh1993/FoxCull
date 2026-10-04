<script lang="ts">
  // Settings → Files & catalog → Excluded folders: a page inside the Settings
  // sheet. Built-in groups (pre-ticked) plus the user's own folders and name
  // patterns. Every change writes straight through to the settings store, so it
  // persists on this device; the page re-applies the rules to the tree and the
  // open folder when Settings closes (see closeSettings in +page.svelte).
  import { api } from "$lib/api";
  import { settings, defaultScanExcludes } from "$lib/settings.svelte";
  import type { ScanExcludes } from "$lib/types";

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

<div class="excl">
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

  <div class="foot">
    <button class="btn sm" onclick={reset} disabled={isDefault}>Reset to defaults</button>
  </div>
</div>

<style>
  .excl { user-select: text; }
  .foot { display: flex; justify-content: flex-end; margin-top: 16px; }
  section + section { margin-top: 20px; }
  h3 {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0 0 7px 2px;
    color: var(--text-faint);
    font-size: var(--fs-sm);
    font-weight: var(--fw-semibold);
  }
  .count { color: var(--text-faint); font-size: var(--fs-xs); font-weight: var(--fw-medium); }

  .groups { display: flex; flex-direction: column; gap: 7px; }
  .group {
    display: flex;
    gap: 11px;
    padding: 11px 13px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--bg-elev) 82%, transparent);
    cursor: pointer;
    transition: opacity 120ms ease, border-color 120ms ease;
  }
  .group:hover { border-color: var(--border); }
  .group.off { opacity: 0.62; }
  .group input { margin-top: 2px; accent-color: var(--accent); width: 15px; height: 15px; flex: none; }
  .gBody { min-width: 0; }
  .gTitle { color: var(--text); font-size: var(--fs-md); font-weight: var(--fw-semibold); }
  .gWhere { margin-top: 2px; color: var(--text-dim); font-size: var(--fs-sm); }
  .names { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 7px; }
  code {
    padding: 1px 6px;
    border-radius: var(--radius-xs);
    background: color-mix(in srgb, var(--bg-hover) 70%, transparent);
    color: var(--text-dim);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, monospace;
    font-size: var(--fs-xs);
  }

  .add { display: flex; gap: 7px; }
  .add input { flex: 1; min-width: 0; min-height: var(--control-h); padding: 5px 10px; font-size: var(--fs-md); user-select: text; }
  .error { margin: 7px 0 0; color: var(--reject); font-size: var(--fs-sm); }
  .empty { margin: 10px 0 0; color: var(--text-dim); font-size: var(--fs-md); line-height: 1.6; }
  .empty b { color: var(--text); font-weight: var(--fw-semibold); }

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
    font-size: var(--fs-xs);
    font-weight: var(--fw-semibold);
  }
  .kind.name { background: color-mix(in srgb, var(--stack) 16%, transparent); color: var(--stack); }
  .val { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); font-size: var(--fs-md); }
  .rm { width: 26px; height: 26px; border-radius: var(--radius-xs); color: var(--text-faint); font-size: var(--fs-xs); }
  .rm:hover { background: var(--bg-hover); color: var(--reject); }

  .always { margin: 18px 0 0; color: var(--text-faint); font-size: var(--fs-sm); line-height: 1.6; }
</style>
