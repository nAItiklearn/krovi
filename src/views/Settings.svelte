<script lang="ts">
  import { themePref, type ThemePref } from '../lib/stores';
  import { TOOLS } from '../lib/tools';
  import Icon from '../components/Icon.svelte';

  // UI-only state (the Tauri shell would persist these via krovi-store).
  let startup = $state(true);
  let hotCorner = $state<'off' | 'top-right' | 'top-left' | 'bottom-right'>('top-right');
  const hotkey = 'Ctrl + Alt + Space';
  let enabled = $state<Record<string, boolean>>(
    Object.fromEntries(TOOLS.filter((t) => t.id !== 'home' && t.id !== 'settings').map((t) => [t.id, true])),
  );

  const themes: { id: ThemePref; label: string; icon: string }[] = [
    { id: 'light', label: 'Light', icon: 'sun' },
    { id: 'dark', label: 'Dark', icon: 'moon' },
    { id: 'auto', label: 'Auto', icon: 'settings' },
  ];
</script>

<div class="settings scroll">
  <section>
    <h2>General</h2>
    <div class="field">
      <div>
        <div class="flabel">Launch at startup</div>
        <div class="fhint faint">Open Krovi automatically when you sign in.</div>
      </div>
      <button class="toggle" class:on={startup} role="switch" aria-checked={startup} aria-label="Launch at startup" onclick={() => (startup = !startup)}>
        <span class="knob"></span>
      </button>
    </div>
    <div class="field">
      <div>
        <div class="flabel">Summon hotkey</div>
        <div class="fhint faint">Global shortcut to open the panel (rebindable in the shell).</div>
      </div>
      <kbd class="hk">{hotkey}</kbd>
    </div>
    <div class="field">
      <div>
        <div class="flabel">Hot corner</div>
        <div class="fhint faint">Fling the cursor to a screen corner to summon.</div>
      </div>
      <select class="input narrow" bind:value={hotCorner} aria-label="hot corner">
        <option value="off">Off</option>
        <option value="top-right">Top-right</option>
        <option value="top-left">Top-left</option>
        <option value="bottom-right">Bottom-right</option>
      </select>
    </div>
  </section>

  <section>
    <h2>Appearance</h2>
    <div class="themes">
      {#each themes as t (t.id)}
        <button class="theme" class:on={$themePref === t.id} onclick={() => themePref.set(t.id)}>
          <Icon name={t.icon} size={18} />
          <span>{t.label}</span>
          {#if $themePref === t.id}<span class="dot"></span>{/if}
        </button>
      {/each}
    </div>
    <p class="fhint faint">Brand accent is teal — #0e9e8f (light) / #22c7b4 (dark).</p>
  </section>

  <section>
    <h2>Tools</h2>
    <div class="tools">
      {#each TOOLS.filter((t) => t.id !== 'home' && t.id !== 'settings') as t (t.id)}
        <div class="tool">
          <Icon name={t.icon} size={16} />
          <span class="tname">{t.label}</span>
          <button
            class="toggle sm"
            class:on={enabled[t.id]}
            role="switch"
            aria-checked={enabled[t.id]}
            aria-label={`Toggle ${t.label}`}
            onclick={() => (enabled = { ...enabled, [t.id]: !enabled[t.id] })}>
            <span class="knob"></span>
          </button>
        </div>
      {/each}
    </div>
  </section>

  <section>
    <h2>Data & privacy</h2>
    <div class="field">
      <div>
        <div class="flabel">Data location</div>
        <div class="fhint faint mono">%APPDATA%\Krovi\</div>
      </div>
      <div class="btns">
        <button class="btn sm">Export</button>
        <button class="btn sm">Clear…</button>
      </div>
    </div>
    <p class="fhint faint">
      No account, no telemetry, no ads. Only Weather and Currency ever reach the network, and both are keyless and opt-out (SPEC §7).
    </p>
  </section>

  <p class="about faint">Krovi v0.1.0 · MIT · a local-first everything shelf. These controls are UI-only in the demo.</p>
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    gap: 26px;
    max-width: 620px;
    max-height: calc(min(700px, 88vh) - 150px);
  }
  h2 {
    font-size: 12px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-faint);
    margin: 0 0 12px;
  }
  .field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
  }
  .flabel {
    font-size: 14px;
    font-weight: 550;
  }
  .fhint {
    font-size: 12px;
    margin-top: 2px;
  }
  .toggle {
    width: 42px;
    height: 24px;
    border-radius: 999px;
    border: none;
    background: var(--surface-3);
    position: relative;
    flex: none;
    transition: background 0.15s ease;
  }
  .toggle.on {
    background: var(--accent);
  }
  .toggle .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: #fff;
    transition: transform 0.15s ease;
  }
  .toggle.on .knob {
    transform: translateX(18px);
  }
  .toggle.sm {
    width: 36px;
    height: 20px;
  }
  .toggle.sm .knob {
    width: 14px;
    height: 14px;
  }
  .toggle.sm.on .knob {
    transform: translateX(16px);
  }
  .hk {
    font-family: var(--mono);
    font-size: 12px;
    background: var(--surface-3);
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 5px 10px;
  }
  .narrow {
    width: auto;
  }
  .themes {
    display: flex;
    gap: 10px;
  }
  .theme {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 7px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-dim);
    position: relative;
  }
  .theme.on {
    border-color: var(--accent);
    color: var(--accent);
    background: var(--accent-soft);
  }
  .theme .dot {
    position: absolute;
    top: 9px;
    right: 9px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }
  .tools {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 8px;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--surface-2);
    color: var(--text-dim);
  }
  .tname {
    flex: 1;
    font-size: 13px;
    color: var(--text);
  }
  .btns {
    display: flex;
    gap: 7px;
  }
  .about {
    font-size: 11.5px;
    padding-top: 6px;
  }
</style>
