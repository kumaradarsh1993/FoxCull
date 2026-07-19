<script lang="ts">
  // Controller setup: pairing guide, live status, and the press-to-bind
  // remapper. Bindings persist in settings (padBindings) and take effect
  // immediately — the polling loop in gamepad.svelte.ts reads them per frame.
  import { pad, PAD_ACTIONS, buttonName, type PadActionId } from "$lib/gamepad.svelte";
  import { settings } from "$lib/settings.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let binding = $state<PadActionId | null>(null);
  let cancelCapture: (() => void) | null = null;

  const GROUPS = ["Navigate", "Mark", "View", "Video"] as const;

  // What the mouse's extra buttons can do (a small, curated subset).
  const MOUSE_CHOICES: [string, string][] = [
    ["viewBack", "Back to grid"],
    ["viewForward", "Open Focus"],
    ["pick", "Pick ⚑"],
    ["reject", "Reject ✕"],
    ["prev", "Previous item"],
    ["next", "Next item"],
    ["fullscreen", "Play mode (fullscreen)"],
  ];

  function startBind(action: PadActionId) {
    stopBind();
    binding = action;
    cancelCapture = pad.captureNextButton((btn) => {
      binding = null;
      cancelCapture = null;
      void pad.bind(action, btn);
    });
  }
  function stopBind() {
    cancelCapture?.();
    cancelCapture = null;
    binding = null;
  }
  function close() {
    stopBind();
    onclose();
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && (binding ? stopBind() : close())} />

<div class="backdrop" onclick={close} role="presentation"></div>
<div class="panel" role="dialog" aria-label="Controller setup">
  <header>
    <h2>🎮 Controller</h2>
    {#if pad.connected}
      <span class="status on">● {pad.name.replace(/\s*\(.*\)$/, "") || "Controller"} connected</span>
    {:else}
      <span class="status">○ No controller detected — connect one and press any button</span>
    {/if}
    <span class="grow"></span>
    <label class="entoggle">
      <input
        type="checkbox"
        checked={settings.s.padEnabled}
        onchange={(e) => settings.set({ padEnabled: (e.currentTarget as HTMLInputElement).checked })}
      />
      Enabled
    </label>
    <button class="x" onclick={close} title="Close (Esc)">✕</button>
  </header>

  <div class="scroll">
    <section class="guide">
      <h3>Connect a PS5 / PS4 controller</h3>
      <ol>
        <li>
          Turn the controller off (hold the <b>PS</b> button, choose Turn Off — or just have it idle),
          then hold <b>Create</b> (PS5) / <b>Share</b> (PS4) + <b>PS</b> together until the light bar
          flashes rapidly — that's pairing mode.
        </li>
        <li>
          On the laptop: <b>Settings → Bluetooth → Add device</b>, pick
          <i>DualSense Wireless Controller</i> (PS5) or <i>Wireless Controller</i> (PS4).
        </li>
        <li>Press any button — the status above turns green. A USB-C cable also works, no pairing needed.</li>
      </ol>
      <p class="note">
        A controller paired to a PlayStation re-pairs to the console with one press of its PS button
        later — pairing here doesn't unpair it there permanently, you just repeat the hold-to-pair when
        switching back.
      </p>
    </section>

    <section>
      <h3>Buttons</h3>
      <p class="note">
        Click <i>Rebind</i>, then press the controller button you want. Binding a button that's in use
        moves it to the new action.
      </p>
      {#each GROUPS as g (g)}
        <div class="grp">
          <div class="grpName">{g}</div>
          {#each PAD_ACTIONS.filter((a) => a.group === g) as a (a.id)}
            <div class="row">
              <span class="lbl">{a.label}</span>
              <span class="btnname" class:none={pad.buttonFor(a.id) < 0}>
                {#if binding === a.id}<span class="listening">press a button…</span>{:else}{buttonName(pad.buttonFor(a.id))}{/if}
              </span>
              {#if binding === a.id}
                <button class="b" onclick={stopBind}>Cancel</button>
              {:else}
                <button class="b" onclick={() => startBind(a.id)} disabled={!pad.connected}>Rebind</button>
                {#if pad.buttonFor(a.id) >= 0}
                  <button class="b ghost" onclick={() => void pad.unbind(a.id)} title="Unbind">✕</button>
                {/if}
              {/if}
            </div>
          {/each}
        </div>
      {/each}
      <div class="resetrow">
        <button class="b" onclick={() => void pad.resetBindings()}>Reset to PS5 defaults</button>
      </div>
    </section>

    <section>
      <h3>Mouse extra buttons</h3>
      <p class="note">The thumb Back/Forward buttons on an MX Master / MX Anywhere / G-series mouse.</p>
      <div class="row">
        <span class="lbl">Back button</span>
        <select
          class="sel"
          value={settings.s.mouseBack}
          onchange={(e) => settings.set({ mouseBack: (e.currentTarget as HTMLSelectElement).value })}
        >
          {#each MOUSE_CHOICES as [v, l] (v)}<option value={v}>{l}</option>{/each}
        </select>
      </div>
      <div class="row">
        <span class="lbl">Forward button</span>
        <select
          class="sel"
          value={settings.s.mouseForward}
          onchange={(e) => settings.set({ mouseForward: (e.currentTarget as HTMLSelectElement).value })}
        >
          {#each MOUSE_CHOICES as [v, l] (v)}<option value={v}>{l}</option>{/each}
        </select>
      </div>
    </section>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    z-index: 100;
  }
  .panel {
    position: fixed;
    inset: 6% 22%;
    min-width: 560px;
    z-index: 101;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  @media (max-width: 1100px) {
    .panel { inset: 5% 8%; min-width: 0; }
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
  }
  header h2 {
    margin: 0;
    font-size: 15px;
  }
  .status {
    font-size: 12.5px;
    color: var(--text-dim);
  }
  .status.on {
    color: var(--pick);
  }
  .grow {
    flex: 1;
  }
  .entoggle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--text-dim);
  }
  .x {
    width: 30px;
    height: 30px;
    border-radius: 7px;
    color: var(--text-dim);
    font-size: 13px;
  }
  .x:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 14px 18px 20px;
  }
  section {
    margin-bottom: 18px;
  }
  h3 {
    margin: 0 0 6px;
    font-size: 13.5px;
  }
  .guide ol {
    margin: 6px 0;
    padding-left: 20px;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--text-dim);
  }
  .note {
    margin: 4px 0 8px;
    font-size: 12px;
    color: var(--text-faint);
    line-height: 1.5;
  }
  .grp {
    margin-bottom: 8px;
  }
  .grpName {
    font-size: 10.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-faint);
    margin: 8px 0 3px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 0;
  }
  .lbl {
    flex: 0 0 190px;
    font-size: 12.5px;
  }
  .btnname {
    flex: 1;
    font-size: 12.5px;
    color: var(--accent);
    font-weight: 600;
  }
  .btnname.none {
    color: var(--text-faint);
    font-weight: 400;
  }
  .listening {
    color: var(--pick);
    animation: pulse 1s ease-in-out infinite;
  }
  @keyframes pulse {
    50% { opacity: 0.45; }
  }
  .b {
    padding: 4px 10px;
    border-radius: 7px;
    border: 1px solid var(--border);
    background: var(--bg-elev);
    color: var(--text);
    font-size: 12px;
  }
  .b:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .b:disabled {
    opacity: 0.45;
  }
  .b.ghost {
    color: var(--text-faint);
  }
  .resetrow {
    margin-top: 10px;
  }
  .sel {
    background: var(--bg-elev);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 4px 6px;
    font-size: 12.5px;
  }
</style>
