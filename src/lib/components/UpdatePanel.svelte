<script lang="ts">
  // The update panel — identical in wispr-fox, FoxCull, Fox MD and Fox Mark.
  //
  // One screen answers the three questions a "check for updates" link never
  // could: what am I running, what is the newest stable, and is there a newer
  // nightly. One button moves between them; on Windows that button is the whole
  // job — download, silent install, relaunch — with no wizard to click through.
  //
  // ## Styling
  //
  // Self-contained on purpose. It reads `--accent` if the host app defines one
  // and otherwise falls back, and every surface is `color-mix(... currentColor
  // ...)` rather than a hard-coded grey, so it lands correctly in FoxCull's four
  // themes, Fox MD's light/dark, and Fox Mark's three without any app needing to
  // hand it tokens. Drop it in and it looks like it belongs.
  //
  // If you fix something here, fix it in all four — divergence is a bug.

  import { onMount, onDestroy } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    checkForUpdates,
    downloadAndInstall,
    onUpdateProgress,
    updates as updateStore,
    type ReleaseInfo,
    type UpdateProgress,
    type UpdateStatus,
  } from "$lib/updates.svelte";

  let {
    /** Product name for the prose. Falls back to whatever Rust reports. */
    title = "",
    /** Drop the explanatory footer where space is tight (a popover, say). */
    compact = false,
  }: { title?: string; compact?: boolean } = $props();

  let status = $state<UpdateStatus | null>(updateStore.status);
  let checking = $state(false);
  let error = $state<string | null>(null);
  let progress = $state<UpdateProgress | null>(null);
  /** Tag currently installing, so only that card shows a busy state. */
  let installing = $state<string | null>(null);
  let installError = $state<string | null>(null);
  let installNote = $state<string | null>(null);
  let unlisten: (() => void) | undefined;

  async function check() {
    checking = true;
    error = null;
    try {
      status = await checkForUpdates();
      updateStore.status = status;
      updateStore.available = status.update_available;
      updateStore.checked = true;
    } catch (e) {
      error = String(e);
    } finally {
      checking = false;
    }
  }

  onMount(async () => {
    // Re-check on open even when the background prime already ran: the panel is
    // the one place the answer is visible, and a stale "up to date" here is
    // worse than one extra call.
    void check();
    unlisten = await onUpdateProgress((p) => (progress = p));
  });
  onDestroy(() => unlisten?.());

  async function install(rel: ReleaseInfo) {
    installing = rel.tag;
    installError = null;
    installNote = null;
    progress = null;
    try {
      installNote = await downloadAndInstall(rel.tag);
      // On Windows the app is about to exit; anything rendered now is on screen
      // for a moment. Elsewhere the note is the actual instruction.
      if (!status?.can_self_install) installing = null;
    } catch (e) {
      installError = String(e);
      installing = null;
    }
  }

  function bytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
    return `${(n / 1048576).toFixed(1)} MB`;
  }

  /** "3 days ago" — the question is always how fresh a build is, never what
   *  o'clock it was published. */
  function age(iso: string | null): string {
    if (!iso) return "";
    const t = Date.parse(iso);
    if (Number.isNaN(t)) return "";
    const mins = Math.round((Date.now() - t) / 60000);
    if (mins < 1) return "just now";
    if (mins < 60) return `${mins} minute${mins === 1 ? "" : "s"} ago`;
    const hours = Math.round(mins / 60);
    if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
    const days = Math.round(hours / 24);
    if (days < 31) return `${days} day${days === 1 ? "" : "s"} ago`;
    return new Date(t).toLocaleDateString(undefined, {
      day: "numeric",
      month: "short",
      year: "numeric",
    });
  }

  let pct = $derived(
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : 0,
  );

  // "Up to date" has to mean up to date on the channel you are actually on. A
  // user running a nightly is not behind just because a stable exists with a
  // lower version number — which is why this reads `newer` rather than
  // comparing version strings here.
  let onLatest = $derived(!!status && !status.stable?.newer && !status.nightly?.newer);
  let name = $derived(title || status?.product || "This app");

  function phaseLabel(tag: string): string {
    if (installing !== tag) return "";
    switch (progress?.phase) {
      case "verifying":
        return "Verifying…";
      case "launching":
        return "Starting installer…";
      case "downloading":
        return `Downloading… ${pct}%`;
      default:
        return "Starting…";
    }
  }
</script>

<div class="fxu">
  <div class="fxu-head">
    <div class="fxu-id">
      <div class="fxu-name">{name}</div>
      <div class="fxu-ver">
        v{status?.current ?? "…"}
        {#if status?.current_is_nightly}
          <span class="fxu-chan nightly">nightly</span>
        {:else if status}
          <span class="fxu-chan stable">stable</span>
        {/if}
      </div>
      {#if status && onLatest}
        <div class="fxu-sub ok">You're on the latest build.</div>
      {:else if status}
        <div class="fxu-sub new">An update is available below.</div>
      {/if}
    </div>
    <button class="fxu-btn ghost" onclick={check} disabled={checking}>
      {checking ? "Checking…" : "Check again"}
    </button>
  </div>

  {#if error}
    <p class="fxu-err">Couldn't reach GitHub — {error}</p>
  {/if}

  {#snippet channel(rel: ReleaseInfo | null, kind: "stable" | "nightly")}
    <article class="fxu-card" class:available={rel?.newer}>
      <div class="fxu-card-head">
        <span class="fxu-kind">{kind === "stable" ? "Latest stable" : "Latest nightly"}</span>
        {#if rel?.newer}<span class="fxu-badge">Available</span>{/if}
      </div>

      {#if !rel}
        <div class="fxu-tag muted">
          {kind === "stable" ? "None published yet" : "Nothing newer than stable"}
        </div>
        <p class="fxu-note">
          {kind === "nightly"
            ? "Nightlies appear here only when they're newer than the latest stable — otherwise installing one would move you backwards."
            : "No stable release has been published for this app yet."}
        </p>
      {:else}
        <div class="fxu-tag">
          {rel.tag}
          {#if age(rel.published_at)}<span class="fxu-date"> · built {age(rel.published_at)}</span>{/if}
        </div>
        {#if rel.summary}<p class="fxu-note">{rel.summary}</p>{/if}

        <div class="fxu-actions">
          {#if rel.newer && rel.asset}
            <button
              class="fxu-btn primary"
              onclick={() => install(rel)}
              disabled={installing !== null}
            >
              {#if installing === rel.tag}
                {phaseLabel(rel.tag)}
              {:else}
                {status?.can_self_install ? "Install" : "Download"} {rel.tag}
              {/if}
            </button>
          {:else if rel.newer && !rel.asset}
            <span class="fxu-note warn">No installer for this platform in that release.</span>
          {:else}
            <span class="fxu-note">You're on this or newer.</span>
          {/if}
          <button class="fxu-btn ghost" onclick={() => openUrl(rel.html_url)}>Release notes</button>
        </div>

        {#if rel.asset}
          <div class="fxu-file">{rel.asset.name} · {bytes(rel.asset.size)}</div>
        {/if}

        {#if installing === rel.tag && progress}
          <div class="fxu-prog" aria-label="Download progress">
            <div class="fxu-prog-fill" style="width: {pct}%"></div>
          </div>
          <div class="fxu-prog-text">
            {bytes(progress.downloaded)} of {bytes(progress.total)}
          </div>
        {/if}
      {/if}
    </article>
  {/snippet}

  <div class="fxu-grid">
    {@render channel(status?.stable ?? null, "stable")}
    {@render channel(status?.nightly ?? null, "nightly")}
  </div>

  {#if installError}
    <p class="fxu-err">{installError}</p>
  {/if}
  {#if installNote}
    <p class="fxu-ok">{installNote}</p>
  {/if}

  {#if !compact}
    <p class="fxu-foot">
      {#if status?.can_self_install}
        Install downloads the official build from this project's GitHub releases
        and runs it silently. {name} closes and reopens on the new version — your
        settings and data are untouched by an update.
      {:else}
        Download fetches the official build from this project's GitHub releases
        and opens it. These builds aren't code-signed, so the last step stays
        manual. Your settings and data are untouched by an update.
      {/if}
    </p>
    <p class="fxu-foot">
      <strong>Stable</strong> is the tested one and what you want by default.
      <strong>Nightly</strong> gets fixes and features first but has had less real
      use. You can move between them freely in either direction — both install
      over the top of each other.
    </p>
  {/if}

  <div class="fxu-links">
    <!-- The URL comes from Rust, which owns the repo constant. Deliberately no
         hard-coded fallback here: a literal would be the ONE thing in this file
         that differs between the four apps, and a wrong one would be silent. -->
    <button
      class="fxu-btn ghost"
      disabled={!status}
      onclick={() => status && openUrl(status.releases_url)}
    >All releases</button>
  </div>
</div>

<style>
  /* Every surface is mixed from `currentColor`, so this panel inherits whatever
     theme the host app is in — light, dark, warm, or the four FoxCull ships —
     without being handed a single token. `--accent` is used when the app
     defines one, with a fox-orange fallback when it doesn't. */
  .fxu {
    --fxu-accent: var(--accent, #e8833a);
    --fxu-line: color-mix(in srgb, currentColor 18%, transparent);
    --fxu-surface: color-mix(in srgb, currentColor 5%, transparent);
    --fxu-muted: color-mix(in srgb, currentColor 62%, transparent);
    display: flex;
    flex-direction: column;
    gap: 12px;
    font-size: 13px;
    line-height: 1.45;
    container-type: inline-size;
  }

  .fxu-head {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    justify-content: space-between;
  }
  .fxu-id { min-width: 0; }
  .fxu-name { font-weight: 650; font-size: 15px; }
  .fxu-ver {
    display: flex;
    align-items: center;
    gap: 8px;
    font-variant-numeric: tabular-nums;
    margin-top: 2px;
  }
  .fxu-chan {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 1px 6px;
    border-radius: 999px;
    border: 1px solid var(--fxu-line);
    color: var(--fxu-muted);
  }
  .fxu-chan.nightly {
    border-color: color-mix(in srgb, var(--fxu-accent) 55%, transparent);
    color: var(--fxu-accent);
  }
  .fxu-sub { margin-top: 4px; color: var(--fxu-muted); }
  .fxu-sub.new { color: var(--fxu-accent); }

  .fxu-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  /* Container query, not a media query: this panel lives inside a settings
     popover in one app and a full page in another, and the window width says
     nothing useful about how wide the panel itself is. */
  @container (max-width: 560px) {
    .fxu-grid { grid-template-columns: 1fr; }
  }

  .fxu-card {
    border: 1px solid var(--fxu-line);
    border-radius: 10px;
    padding: 10px 12px;
    background: var(--fxu-surface);
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .fxu-card.available {
    border-color: color-mix(in srgb, var(--fxu-accent) 55%, transparent);
    background: color-mix(in srgb, var(--fxu-accent) 8%, transparent);
  }
  .fxu-card-head {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: space-between;
  }
  .fxu-kind {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fxu-muted);
  }
  .fxu-badge {
    font-size: 10px;
    font-weight: 650;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--fxu-accent);
    color: #fff;
  }
  .fxu-tag {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
  .fxu-tag.muted { font-weight: 500; color: var(--fxu-muted); }
  .fxu-date { font-weight: 400; color: var(--fxu-muted); }
  .fxu-note { margin: 0; color: var(--fxu-muted); }
  .fxu-note.warn { color: var(--fxu-accent); }
  .fxu-file {
    font-size: 11px;
    color: var(--fxu-muted);
    overflow-wrap: anywhere;
  }

  .fxu-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }

  .fxu-btn {
    font: inherit;
    font-size: 12px;
    padding: 5px 11px;
    border-radius: 7px;
    border: 1px solid var(--fxu-line);
    background: transparent;
    color: inherit;
    cursor: pointer;
    white-space: nowrap;
  }
  .fxu-btn:hover:not(:disabled) {
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
  .fxu-btn:disabled { opacity: 0.55; cursor: default; }
  .fxu-btn.primary {
    background: var(--fxu-accent);
    border-color: var(--fxu-accent);
    color: #fff;
    font-weight: 600;
  }
  .fxu-btn.primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--fxu-accent) 85%, #000);
  }

  .fxu-prog {
    height: 5px;
    border-radius: 999px;
    background: color-mix(in srgb, currentColor 14%, transparent);
    overflow: hidden;
  }
  .fxu-prog-fill {
    height: 100%;
    background: var(--fxu-accent);
    transition: width 0.15s linear;
  }
  .fxu-prog-text {
    font-size: 11px;
    color: var(--fxu-muted);
    font-variant-numeric: tabular-nums;
  }

  .fxu-err,
  .fxu-ok,
  .fxu-foot {
    margin: 0;
    color: var(--fxu-muted);
  }
  .fxu-err { color: #d9534f; }
  .fxu-ok { color: var(--fxu-accent); }
  .fxu-foot { font-size: 12px; }
  .fxu-links { display: flex; gap: 8px; }
</style>
