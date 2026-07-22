<script lang="ts">
  // Controller setup: pairing guide, live status, and the press-to-bind
  // remapper. Bindings persist in settings (padBindings) and take effect
  // immediately — the polling loop in gamepad.svelte.ts reads them per frame.
  import { pad, PAD_ACTIONS, buttonName, type PadActionId } from "$lib/gamepad.svelte";
  import { settings } from "$lib/settings.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let binding = $state<PadActionId | null>(null);
  let cancelCapture: (() => void) | null = null;

  const GROUPS = ["Navigate", "Mark", "Label", "View", "Video"] as const;

  // What the mouse's extra buttons can do (a small, curated subset).
  const MOUSE_CHOICES: [string, string][] = [
    ["viewBack", "Back to grid"],
    ["viewForward", "Open Focus"],
    ["toggleView", "Open / close Focus"],
    ["pick", "Pick ⚑"],
    ["reject", "Reject ✕"],
    ["prev", "Previous item"],
    ["next", "Next item"],
    ["fullscreen", "Play mode (fullscreen)"],
    ["toggleFilmstrip", "Show / hide filmstrip"],
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
      <h3>Pairing</h3>
      <p class="note lede">
        A controller can only be paired to <b>one</b> device at a time, so you swap it between this PC
        and your console. Neither direction is destructive — it's the same two-minute dance each way.
      </p>

      <div class="cards">
        <article class="card">
          <div class="cardHead"><span class="badge pc">PC</span> Pair with this computer</div>
          <ol>
            <li>
              <b>Turn the controller off.</b>
              Hold <kbd>PS</kbd> ~10 s until the lights go out.
            </li>
            <li>
              <b>Enter pairing mode.</b>
              Hold <kbd>Create</kbd> <span class="dim">(PS5)</span> or <kbd>Share</kbd>
              <span class="dim">(PS4)</span> <b>+</b> <kbd>PS</kbd> together until the light bar
              <b>flashes rapidly</b> — a slow pulse is not pairing mode, keep holding.
            </li>
            <li>
              <b>Add it in Windows.</b>
              <span class="path">Settings → Bluetooth &amp; devices → Add device → Bluetooth</span>
              Pick <i>DualSense Wireless Controller</i> or <i>Wireless Controller</i>.
            </li>
            <li>
              <b>Press any button.</b>
              The status at the top of this panel turns green.
            </li>
          </ol>
          <p class="tip">
            <b>Skip all of it:</b> a USB-C cable works instantly, with no pairing and no unpairing from
            your PS5.
          </p>
        </article>

        <article class="card">
          <div class="cardHead"><span class="badge ps">PS5</span> Pair back with your console</div>
          <ol>
            <li>
              <b>Plug it into the PS5</b> with a USB-C cable, console powered on.
            </li>
            <li>
              <b>Press <kbd>PS</kbd>.</b>
              It re-pairs over the cable and reclaims the controller from this PC.
            </li>
            <li>
              <b>Unplug it.</b> It stays paired to the console wirelessly from then on.
            </li>
          </ol>
          <p class="tip">
            No cable to hand? Put the controller in pairing mode as in step 2 on the left, then on the
            console go
            <span class="path">Settings → Accessories → General → Bluetooth Accessories</span>
            and select it there.
          </p>
        </article>
      </div>

      <p class="note">
        Coming back to the PC later means repeating the left column — Windows remembers the controller,
        but the controller only remembers whichever device claimed it last.
      </p>
    </section>

    <section>
      <h3>Button tester</h3>
      <p class="note">
        Press anything — buttons, the PS button, the touchpad click, or a stick flick — and it shows up
        here. Whether a pad reports the PS button and the touchpad depends on the OS and the browser
        engine, so this answers it directly instead of leaving you to guess from a binding that seems
        dead.
      </p>
      <div class="tester" class:live={pad.pressedNow.length > 0}>
        {#if !pad.connected}
          <span class="dim">No controller connected.</span>
        {:else if pad.pressedNow.length === 0}
          <span class="dim">Nothing pressed.</span>
        {:else}
          {#each pad.pressedNow as b (b)}<span class="chip">{buttonName(b)} <i>#{b}</i></span>{/each}
        {/if}
      </div>
    </section>

    <section>
      <h3>Buttons</h3>
      <p class="note">
        Click <i>Rebind</i>, then press the controller button you want — a stick flick counts, so
        ratings and labels can live on the sticks. Binding a button that's in use moves it to the new
        action.
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
  /* ── pairing guide ──────────────────────────────────────────────────────
     Two cards side by side, because the two directions (to the PC, back to
     the console) are a pair of parallel procedures — running them together as
     one prose blob was the thing that made this section hard to read. Numbered
     steps lead with a bolded IMPERATIVE so the panel can be skimmed while
     you're holding the controller, not read. */
  .guide .lede {
    margin-bottom: 10px;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 12px;
  }
  .card {
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg-elev);
    padding: 12px 14px 10px;
  }
  .cardHead {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 650;
    margin-bottom: 8px;
  }
  .badge {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.4px;
    padding: 2px 7px;
    border-radius: 999px;
    background: var(--bg-hover);
    color: var(--text-dim);
  }
  .badge.pc {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    color: var(--accent);
  }
  .badge.ps {
    background: color-mix(in srgb, var(--pick) 20%, transparent);
    color: var(--pick);
  }
  .guide ol {
    margin: 0;
    padding-left: 18px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--text-dim);
  }
  .guide li {
    margin-bottom: 7px;
  }
  .guide li:last-child {
    margin-bottom: 0;
  }
  .guide li b {
    color: var(--text);
  }
  .guide .dim {
    color: var(--text-faint);
  }
  /* Menu paths get their own line — inline they read as part of the sentence
     and you lose them mid-scan. */
  .path {
    display: block;
    margin: 3px 0;
    font-size: 12px;
    color: var(--text-faint);
  }
  kbd {
    display: inline-block;
    padding: 0 5px;
    min-width: 18px;
    text-align: center;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 5px;
    background: var(--bg-panel);
    color: var(--text);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    line-height: 1.5;
  }
  .tip {
    margin: 9px 0 0;
    padding-top: 8px;
    border-top: 1px solid var(--border);
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-faint);
  }
  .tip b {
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
  .tester {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    min-height: 34px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-elev);
    font-size: 12.5px;
  }
  .tester.live {
    border-color: var(--pick);
  }
  .tester .dim {
    color: var(--text-faint);
  }
  .tester .chip {
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--bg-hover);
    color: var(--accent);
    font-weight: 600;
  }
  .tester .chip i {
    color: var(--text-faint);
    font-style: normal;
    font-weight: 400;
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
