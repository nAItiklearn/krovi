<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import State from './State.svelte';
  import { isTauri } from '../lib/bridge';
  import { pushToast } from '../lib/stores';
  import * as ai from '../lib/ai';

  // Settings -> AI (bring your own key). The section body; Settings.svelte supplies the heading.
  //
  //   * A key goes into a password field, is sent to Rust once and the field is cleared. It is never
  //     shown again: a saved key shows "Key saved in Windows Credential Manager" and its last four
  //     characters, and a Remove button.
  //   * Nothing is sent until a button asks for it (Test connection, Show models). Opening this page
  //     contacts nobody.
  //   * Gemini is off until its notice is accepted. Snowflake is experimental.
  //   * In the browser demo nothing can be saved or tested: the cards show what exists.

  const desktop = isTauri();

  let status = $state<ai.AiStatus | null>(null);
  let loadError = $state('');

  // the key fields (cleared after a successful save) and the Snowflake address
  let keyDraft = $state<Record<string, string>>({});
  let accountDraft = $state<Record<string, string>>({});

  let busy = $state<Record<string, string>>({});
  let problem = $state<Record<string, ai.AiFailure | null>>({});
  let reports = $state<Record<string, ai.TestReport | null>>({});
  let lists = $state<Record<string, ai.ModelList | null>>({});
  let customModel = $state<Record<string, string>>({});
  let customOpen = $state<Record<string, boolean>>({});

  // the Gemini notice
  let noticeOpen = $state(false);
  let checks = $state<ai.ConsentChecks>({
    ...ai.EMPTY_CHECKS,
    region: ai.guessConsentRegion(typeof navigator !== 'undefined' ? navigator.language : null),
  });
  const noticeReady = $derived(ai.consentComplete(checks));

  const stateOf = (id: ai.ProviderId): ai.AiProviderState | undefined => status?.providers.find((p) => p.id === id);

  async function refresh() {
    try {
      status = await ai.loadAi();
      loadError = '';
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  onMount(() => {
    void refresh();
  });

  async function run(id: ai.ProviderId, what: string, job: () => Promise<void>) {
    if (busy[id]) return;
    busy[id] = what;
    problem[id] = null;
    try {
      await job();
    } catch (e) {
      problem[id] = ai.toFailure(e);
    } finally {
      busy[id] = '';
    }
  }

  function saveKey(id: ai.ProviderId) {
    void run(id, 'save', async () => {
      if (id === 'snowflake') {
        const raw = accountDraft[id] ?? stateOf(id)?.accountUrl ?? '';
        const chk = ai.checkAccountUrl(raw);
        if (!chk.ok) throw new Error(chk.error);
        status = await ai.setAccountUrl(id, chk.url);
      }
      status = await ai.saveKey(id, keyDraft[id] ?? '');
      keyDraft[id] = ''; // the field never keeps it
      reports[id] = null;
      pushToast('Key saved');
    });
  }

  function saveAccount(id: ai.ProviderId) {
    void run(id, 'account', async () => {
      const chk = ai.checkAccountUrl(accountDraft[id] ?? '');
      if (!chk.ok) throw new Error(chk.error);
      status = await ai.setAccountUrl(id, chk.url);
      accountDraft[id] = '';
      pushToast('Address saved');
    });
  }

  function removeKey(id: ai.ProviderId) {
    void run(id, 'remove', async () => {
      status = await ai.removeKey(id);
      keyDraft[id] = '';
      reports[id] = null;
      lists[id] = null;
      pushToast('Key removed');
    });
  }

  function test(id: ai.ProviderId) {
    void run(id, 'test', async () => {
      reports[id] = null;
      reports[id] = await ai.testConnection(id);
      await refresh(); // the usage counters
    });
  }

  function showModels(id: ai.ProviderId) {
    void run(id, 'models', async () => {
      lists[id] = await ai.loadModels(id);
    });
  }

  function choose(id: ai.ProviderId, model?: string) {
    void run(id, 'select', async () => {
      status = await ai.selectProvider(id, model);
    });
  }

  function onModelPick(id: ai.ProviderId, value: string) {
    if (value === '__other') {
      customOpen[id] = true;
      return;
    }
    customOpen[id] = false;
    choose(id, value);
  }

  function useCustomModel(id: ai.ProviderId) {
    const m = (customModel[id] ?? '').trim();
    if (!m) return;
    choose(id, m);
  }

  function openKeyPage(id: ai.ProviderId) {
    void run(id, 'page', async () => {
      await ai.openKeyPage(id);
    });
  }

  async function acceptNotice() {
    if (!noticeReady) return;
    try {
      status = await ai.setGeminiConsent(true);
      noticeOpen = false;
      checks = { ...ai.EMPTY_CHECKS, region: checks.region };
      pushToast('Gemini is on');
    } catch (e) {
      problem['gemini'] = ai.toFailure(e);
    }
  }

  async function withdrawNotice() {
    try {
      status = await ai.setGeminiConsent(false);
      reports['gemini'] = null;
    } catch (e) {
      problem['gemini'] = ai.toFailure(e);
    }
  }

  function when(ms: number | null): string {
    return ms ? new Date(ms).toLocaleDateString() : '';
  }
</script>

<div class="ai">
  {#if !status && !loadError}
    {#each Array(3) as _, i (i)}<div class="skel" style="height:64px;margin-bottom:10px;border-radius:14px"></div>{/each}
  {:else if loadError || !status}
    <State tone="error" title={'Couldn’t read AI settings'} text={loadError || 'The AI module did not answer.'} action={{ label: 'Try again', run: refresh }} />
  {:else}
    <p class="intro faint"><Icon name="shield" size={13} /> {ai.PRIVACY_LINE}</p>
    {#if !desktop}
      <p class="demo" role="note"><Icon name="info" size={13} /> {ai.DESKTOP_ONLY} In this browser demo you can look at the providers, but nothing can be saved or sent.</p>
    {/if}

    <div class="group card">
      {#each ai.CATALOG as c (c.id)}
        {@const st = stateOf(c.id)}
        {#if st}
          {@const isSelected = status.selected === c.id}
          {@const report = reports[c.id]}
          {@const failure = problem[c.id]}
          {@const list = lists[c.id]}
          {@const working = busy[c.id] ?? ''}
          <section class="prov" aria-label={c.name}>
            <header class="phead">
              <div class="who">
                <strong>{c.name}</strong>
                {#if c.experimental}<span class="tag accent" title="Not verified against a live account yet">Experimental</span>{/if}
                {#if isSelected}<span class="tag accent">In use</span>{/if}
              </div>
              <span class="tier" data-tier={c.tier}>{c.label}</span>
            </header>
            <p class="blurb">{c.blurb}</p>
            {#if c.note}<p class="note faint"><Icon name="info" size={12} /> {c.note}</p>{/if}

            {#if c.gated && desktop && !status.gemini.accepted}
              <!-- Gemini: off until the notice is accepted -->
              {#if st.hasKey}
                <div class="saved">
                  <span class="ok"><Icon name="lock" size={13} /> {ai.KEY_STORED_TEXT}{st.keyHint ? ` (ends in ${st.keyHint})` : ''}. Gemini is off, so this key is not used.</span>
                  <span class="grow"></span>
                  <button class="btn sm ghost danger" type="button" onclick={() => removeKey(c.id)} disabled={working !== ''}><Icon name="trash" size={13} /> Remove key</button>
                </div>
              {/if}
              {#if !noticeOpen}
                <div class="row">
                  <button class="btn" type="button" onclick={() => (noticeOpen = true)}><Icon name="eye" size={14} /> Review the notice to turn Gemini on</button>
                  <span class="faint small">Gemini is off until you accept it.</span>
                </div>
              {:else}
                {@render notice()}
              {/if}
            {:else}
              {#if c.gated && desktop && status.gemini.accepted}
                <p class="accepted small">
                  <Icon name="circleCheck" size={13} /> You accepted the Gemini notice{status.gemini.acceptedAt ? ` on ${when(status.gemini.acceptedAt)}` : ''}.{st.hasKey ? '' : ' Paste your key below.'}
                  <button class="linkbtn" type="button" onclick={withdrawNotice}>Withdraw</button>
                </p>
              {/if}

              {#if c.id === 'snowflake'}
                <div class="field">
                  <label class="flab" for="acct-{c.id}">Account URL</label>
                  <div class="inline">
                    <input
                      id="acct-{c.id}"
                      class="input"
                      type="text"
                      autocomplete="off"
                      spellcheck="false"
                      placeholder="https://myorg-myaccount.snowflakecomputing.com"
                      value={accountDraft[c.id] ?? st.accountUrl ?? ''}
                      oninput={(e) => (accountDraft[c.id] = e.currentTarget.value)}
                      disabled={!desktop || working !== ''} />
                    {#if st.accountUrl && (accountDraft[c.id] ?? st.accountUrl) !== st.accountUrl}
                      <button class="btn sm" type="button" onclick={() => saveAccount(c.id)} disabled={!desktop || working !== ''}>Save address</button>
                    {/if}
                  </div>
                  <span class="faint small">{st.accountUrl ? 'Saved. Only this address is used.' : 'https only, ending in .snowflakecomputing.com. Saved with your token.'}</span>
                </div>
              {/if}

              {#if st.hasKey}
                <div class="saved">
                  <span class="ok"><Icon name="lock" size={13} /> {ai.KEY_STORED_TEXT}{st.keyHint ? ` (ends in ${st.keyHint})` : ''}</span>
                  <span class="grow"></span>
                  <button class="btn sm" type="button" onclick={() => test(c.id)} disabled={!desktop || working !== '' || !st.ready}>
                    <Icon name={working === 'test' ? 'loader' : 'refresh'} size={13} /> {working === 'test' ? 'Testing…' : 'Test connection'}
                  </button>
                  {#if !isSelected}
                    <button class="btn sm primary" type="button" onclick={() => choose(c.id)} disabled={!desktop || working !== '' || !st.ready}>Use this provider</button>
                  {/if}
                  <button class="btn sm ghost danger" type="button" onclick={() => removeKey(c.id)} disabled={!desktop || working !== ''}><Icon name="trash" size={13} /> Remove key</button>
                </div>
                {#if st.gate === 'account-required'}
                  <p class="warn small"><Icon name="alert" size={13} /> Add your account URL above to use {c.name}.</p>
                {/if}
                <details class="change">
                  <summary>Change key</summary>
                  {@render keyForm(c, st)}
                </details>
              {:else}
                {@render keyForm(c, st)}
              {/if}

              {#if isSelected}
                {@render modelRow(c, st, list)}
              {/if}
            {/if}

            {#if failure}
              {@const copy = ai.failureCopy(failure.kind)}
              <div class="problem" role="alert">
                <Icon name="alert" size={14} />
                <span>
                  {failure.message}
                  {#if copy.advice && failure.kind !== 'other'}<br /><small>{copy.advice}</small>{/if}
                  {#if failure.detail}<br /><small class="detail">{failure.detail}</small>{/if}
                </span>
              </div>
            {/if}

            {#if report}
              {@render result(report)}
            {/if}

            {#if ai.usageLine(st.usage) || st.usage.lastError}
              <p class="usage faint small">
                {ai.usageLine(st.usage) ?? ''}{#if st.usage.lastError}{ai.usageLine(st.usage) ? '. ' : ''}Last error: {ai.failureCopy(st.usage.lastError.kind).title.toLowerCase()}.{/if}
              </p>
            {/if}
          </section>
        {/if}
      {/each}
    </div>

    <p class="foot faint small"><Icon name="info" size={12} /> Every AI answer in Krovi is labelled “{ai.AI_OUTPUT_LABEL}” Check anything that matters.</p>
  {/if}
</div>

{#snippet keyForm(c: ai.CatalogEntry, st: ai.AiProviderState)}
  {@const working = busy[c.id] ?? ''}
  {@const draftKey = keyDraft[c.id] ?? ''}
  {@const shapeProblem = draftKey ? ai.keyShapeProblem(c.id, draftKey) : null}
  <form
    class="field"
    onsubmit={(e) => {
      e.preventDefault();
      if (!shapeProblem && draftKey) saveKey(c.id);
    }}>
    <label class="flab" for="key-{c.id}">{c.keyName}</label>
    <div class="inline">
      <input
        id="key-{c.id}"
        class="input"
        type="password"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        placeholder={c.keyPlaceholder}
        value={draftKey}
        oninput={(e) => (keyDraft[c.id] = e.currentTarget.value)}
        disabled={!desktop || working !== ''} />
      <button class="btn primary" type="submit" disabled={!desktop || working !== '' || !draftKey || shapeProblem !== null || (c.id === 'snowflake' && !ai.checkAccountUrl(accountDraft[c.id] ?? st.accountUrl ?? '').ok)}>
        <Icon name={working === 'save' ? 'loader' : 'check'} size={14} /> {working === 'save' ? 'Saving…' : 'Save key'}
      </button>
      <button class="btn" type="button" onclick={() => openKeyPage(c.id)} disabled={!desktop || working !== ''}>
        <Icon name="externalLink" size={14} /> {c.keyPageLabel}
      </button>
    </div>
    {#if shapeProblem}<span class="warn small" role="status">{shapeProblem}</span>{/if}
    <span class="faint small">
      {#if desktop}It is stored in Windows Credential Manager on this PC and cleared from this box once saved. Krovi never shows it again.{:else}{ai.DESKTOP_ONLY}{/if}
    </span>
  </form>
{/snippet}

{#snippet modelRow(c: ai.CatalogEntry, st: ai.AiProviderState, list: ai.ModelList | null | undefined)}
  {@const working = busy[c.id] ?? ''}
  {@const options = ai.modelOptions(list?.models ?? [], st.model, st.defaultModel)}
  <div class="field">
    <label class="flab" for="model-{c.id}">Model</label>
    <div class="inline">
      <select
        id="model-{c.id}"
        class="input sel"
        value={st.model}
        disabled={!desktop || working !== ''}
        onchange={(e) => onModelPick(c.id, e.currentTarget.value)}>
        {#each options as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
        <option value="__other">Another model…</option>
      </select>
      <button class="btn sm" type="button" onclick={() => showModels(c.id)} disabled={!desktop || working !== ''}>
        <Icon name={working === 'models' ? 'loader' : 'refresh'} size={13} /> {c.id === 'snowflake' ? 'Show suggested models' : 'Show available models'}
      </button>
    </div>
    {#if customOpen[c.id]}
      <div class="inline">
        <input
          class="input"
          type="text"
          autocomplete="off"
          spellcheck="false"
          aria-label="Model id"
          placeholder="Type a model id"
          value={customModel[c.id] ?? ''}
          oninput={(e) => (customModel[c.id] = e.currentTarget.value)}
          onkeydown={(e) => {
            if (e.key === 'Enter') useCustomModel(c.id);
          }} />
        <button class="btn sm" type="button" onclick={() => useCustomModel(c.id)} disabled={!(customModel[c.id] ?? '').trim()}>Use it</button>
      </div>
    {/if}
    <span class="faint small">
      {#if list?.listed}
        {list.models.length} {list.models.length === 1 ? 'model' : 'models'} from {c.name}.
      {:else if list?.error}
        Couldn’t load the list ({ai.failureCopy(list.error.kind).title.toLowerCase()}), so only the default is shown. You can still type a model id.
      {:else}
        Pressing the button asks {c.name} what your key can use. Nothing is sent until you do.
      {/if}
      {#if st.visionModel !== st.model}Pictures use {st.visionModel}, {c.name}’s image model.{/if}
    </span>
  </div>
{/snippet}

{#snippet result(r: ai.TestReport)}
  {@const rows = ai.statusRows(r)}
  <div class="result" class:good={r.ok} role="status">
    <p class="rhead">
      <Icon name={r.ok ? 'circleCheck' : 'alert'} size={14} />
      <strong>{r.ok ? 'Connection works' : r.kind ? ai.failureCopy(r.kind).title : 'The test did not pass'}</strong>
    </p>
    <p class="rmsg">{r.message}</p>
    {#if !r.ok && r.kind}<p class="radv faint small">{ai.failureCopy(r.kind).advice}</p>{/if}
    <ul class="rows" aria-label="What was checked">
      {#each rows as row (row.id)}
        <li class="rrow" data-state={row.state}>
          <span class="dot" aria-hidden="true"></span>
          <span class="rlab">{row.label}</span>
          <span class="rtxt faint">{row.text}</span>
        </li>
      {/each}
    </ul>
    {#each r.notes as n (n)}<p class="small faint"><Icon name="info" size={12} /> {n}</p>{/each}
    {#if r.detail}<p class="small faint detail">{r.detail}</p>{/if}
    {#if r.ok && (r.inputTokens !== null || r.outputTokens !== null)}
      <p class="small faint">The test used {ai.formatTokens((r.inputTokens ?? 0) + (r.outputTokens ?? 0))} tokens, with model {r.model}.</p>
    {/if}
  </div>
{/snippet}

{#snippet notice()}
  {@const n = ai.GEMINI_NOTICE}
  <div class="notice" role="group" aria-label={n.title}>
    <h3>{n.title}</h3>
    <p>{n.body}</p>
    <label class="chk"><input type="checkbox" bind:checked={checks.adult} /> <span>{n.adult}</span></label>
    <label class="chk"><input type="checkbox" bind:checked={checks.terms} /> <span>{n.terms}</span></label>
    <label class="chk"><input type="checkbox" bind:checked={checks.freeTier} /> <span>{n.freeTier}</span></label>
    <fieldset class="region">
      <legend>{n.regionLabel}</legend>
      <label class="radio"><input type="radio" name="gemini-region" value="restricted" bind:group={checks.region} /> {n.regionRestricted}</label>
      <label class="radio"><input type="radio" name="gemini-region" value="elsewhere" bind:group={checks.region} /> {n.regionElsewhere}</label>
    </fieldset>
    {#if checks.region === 'restricted'}
      <label class="chk"><input type="checkbox" bind:checked={checks.billing} /> <span>{n.billing}</span></label>
    {/if}
    <p class="faint small">{n.footnote}</p>
    <div class="row">
      <button class="btn primary" type="button" onclick={acceptNotice} disabled={!noticeReady}><Icon name="check" size={14} /> Accept and turn on Gemini</button>
      <button class="btn ghost" type="button" onclick={() => (noticeOpen = false)}>Not now</button>
    </div>
  </div>
{/snippet}

<style>
  .ai {
    display: grid;
    gap: 10px;
  }
  .intro,
  .demo,
  .foot {
    display: flex;
    gap: 7px;
    align-items: flex-start;
    margin: 0 4px;
    font-size: var(--t-xs);
    line-height: 1.5;
  }
  .demo {
    padding: 8px 12px;
    border-radius: var(--r-2);
    background: var(--accent-soft);
    margin: 0;
  }
  .group {
    border-radius: var(--r-4);
  }
  .prov {
    display: grid;
    gap: 10px;
    padding: 14px 16px;
  }
  .prov + .prov {
    border-top: 1px solid var(--line);
  }
  .phead {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 6px 14px;
  }
  .who {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .who strong {
    font-size: var(--t-md);
    font-weight: 600;
    letter-spacing: -0.005em;
  }
  .tier {
    font-size: var(--t-xs);
    font-weight: 560;
    color: var(--text-2);
  }
  .blurb,
  .note {
    margin: 0;
    font-size: var(--t-xs);
    line-height: 1.5;
    max-width: 62ch;
  }
  .note {
    display: flex;
    gap: 6px;
    align-items: flex-start;
  }
  .small {
    font-size: var(--t-xs);
    line-height: 1.45;
  }
  .grow {
    flex: 1;
  }
  .row,
  .saved,
  .inline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .inline .input {
    flex: 1;
    min-width: 200px;
  }
  .inline .sel {
    flex: 0 1 auto;
    min-width: 220px;
  }
  .field {
    display: grid;
    gap: 6px;
  }
  .flab {
    font-size: var(--t-sm);
    font-weight: 600;
  }
  .ok {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .warn {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    color: var(--danger);
  }
  .accepted {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 0;
    color: var(--text-2);
  }
  .linkbtn {
    color: var(--text-2);
    text-decoration: underline;
    font-size: var(--t-xs);
  }
  .linkbtn:hover {
    color: var(--text);
  }
  .change summary {
    cursor: pointer;
    font-size: var(--t-xs);
    color: var(--text-2);
    margin-bottom: 8px;
  }
  .problem {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--r-2);
    border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    font-size: var(--t-sm);
    font-weight: 560;
    line-height: 1.45;
  }
  .problem small {
    font-weight: 450;
  }
  .detail {
    opacity: 0.85;
    user-select: text;
  }
  .result {
    display: grid;
    gap: 6px;
    padding: 10px 12px;
    border-radius: var(--r-2);
    background: var(--surface-1);
    border: 1px solid var(--line);
  }
  .result.good {
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .rhead,
  .rmsg {
    margin: 0;
  }
  .rhead {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .rmsg {
    font-size: var(--t-sm);
    line-height: 1.5;
    user-select: text;
  }
  .radv {
    margin: 0;
  }
  .rows {
    display: grid;
    gap: 3px;
    margin: 2px 0;
    padding: 0;
    list-style: none;
  }
  .rrow {
    display: grid;
    grid-template-columns: 10px minmax(150px, auto) 1fr;
    align-items: center;
    gap: 8px;
    font-size: var(--t-xs);
  }
  .rlab {
    font-weight: 600;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--line-strong);
  }
  .rrow[data-state='ok'] .dot {
    background: var(--accent);
  }
  .rrow[data-state='problem'] .dot {
    background: var(--danger);
  }
  .rrow[data-state='problem'] .rtxt {
    color: var(--danger);
  }
  .usage {
    margin: 0;
  }
  .notice {
    display: grid;
    gap: 10px;
    padding: 12px 14px;
    border-radius: var(--r-3);
    border: 1px solid var(--line-strong);
    background: var(--surface-1);
  }
  .notice h3 {
    margin: 0;
    font-size: var(--t-md);
  }
  .notice p {
    margin: 0;
    font-size: var(--t-sm);
    line-height: 1.5;
  }
  .chk {
    display: flex;
    gap: 9px;
    align-items: flex-start;
    font-size: var(--t-sm);
    line-height: 1.5;
  }
  .chk input {
    margin-top: 4px;
    flex: none;
  }
  .region {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
    margin: 0;
    padding: 0;
    border: 0;
    font-size: var(--t-sm);
  }
  .region legend {
    padding: 0;
    margin-bottom: 4px;
    font-weight: 600;
  }
  .radio {
    display: inline-flex;
    gap: 7px;
    align-items: center;
  }
</style>
