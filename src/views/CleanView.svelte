<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import Icon from '../components/Icon.svelte';
  import State from '../components/State.svelte';
  import { isTauri } from '../lib/bridge';
  import { swap } from '../lib/motion';
  import { formatBytes } from '../lib/time';
  import * as cl from '../lib/clean';

  // Clean My OS: scan -> review the real paths and sizes -> confirm -> Recycle Bin.
  //   * Krovi only looks in a fixed list of places (see the list below the Scan button).
  //   * Nothing is moved until you confirm, and everything goes to the Recycle Bin, never deleted
  //     for good. The Recycle Bin itself is only measured, never emptied.
  //   * In the browser demo everything on screen is made up and labelled DEMO.

  type Phase = 'idle' | 'scanning' | 'review' | 'deleting' | 'done';

  const demo = !isTauri();

  let phase = $state<Phase>('idle');
  let infos = $state<cl.CategoryInfo[]>([]);
  let scan = $state<cl.Scan | null>(null);
  let selected = $state<Set<string>>(new Set());
  let open = $state<Set<string>>(new Set());
  let shown = $state<Record<string, number>>({});
  let seen = $state<Record<string, cl.ScanProgress>>({});
  let current = $state<string | null>(null);
  let scannedAt = $state(Date.now());
  let confirmOpen = $state(false);
  let report = $state<cl.DeleteReport | null>(null);
  let moving = $state<cl.DeleteProgress | null>(null);
  let bin = $state<cl.RecycleBinSize | null>(null);
  let error = $state<string | null>(null);
  let announce = $state('');
  let stopping = $state(false);
  let allFailures = $state(false);

  let cancelBtn: HTMLButtonElement | undefined = $state();
  let scanBtn: HTMLButtonElement | undefined = $state();
  let dialogEl: HTMLElement | undefined = $state();

  const FAILURES_SHOWN = 100;
  const reducedMotion = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const dur = reducedMotion ? 0 : 200;

  const infoById = $derived(new Map(infos.map((i) => [i.id, i])));
  const totals = $derived(scan ? cl.selectionTotals(scan, selected) : { items: 0, files: 0, bytes: 0 });
  const breakdown = $derived(scan ? cl.selectionBreakdown(scan, selected) : []);
  const confirmLabel = $derived(cl.confirmText(totals.items, totals.bytes));
  const yourFiles = $derived(breakdown.filter((b) => infoById.get(b.id)?.badge).reduce((n, b) => n + b.items, 0));
  const found = $derived(
    scan ? scan.categories.filter((c) => !infoById.get(c.id)?.readOnly).reduce((n, c) => n + c.bytes, 0) : 0,
  );
  const summary = $derived(report ? cl.summarizeReport(report) : null);
  const scanTitle = $derived(infoById.get(current ?? '')?.title ?? '');
  const lookedAt = $derived(Object.values(seen).reduce((n, p) => n + p.files, 0));
  const lookedBytes = $derived(Object.values(seen).reduce((n, p) => n + p.bytes, 0));
  const busy = $derived(phase === 'scanning' || phase === 'deleting');

  const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

  // ----------------------------------------------------------------- scanning

  async function startScan() {
    if (busy) return;
    phase = 'scanning';
    error = null;
    report = null;
    bin = null;
    seen = {};
    current = null;
    stopping = false;
    confirmOpen = false;
    announce = 'Scanning. Nothing is changed while Krovi looks.';
    try {
      const s = await cl.scan([], (p) => {
        seen = { ...seen, [p.category]: p };
        current = p.category;
      });
      scan = s;
      scannedAt = Date.now();
      selected = cl.defaultSelection(s, infos);
      open = new Set();
      shown = {};
      allFailures = false;
      phase = 'review';
      const t = cl.selectionTotals(s, selected);
      announce = s.cancelled
        ? 'Scan stopped. What was found so far is listed.'
        : `Scan finished. ${cl.plural(t.items, 'item')} (${formatBytes(t.bytes)}) ticked.`;
    } catch (e) {
      error = msg(e);
      announce = error;
      phase = scan ? 'review' : 'idle';
    } finally {
      stopping = false;
    }
  }

  async function stop() {
    if (stopping) return;
    stopping = true;
    announce = 'Stopping.';
    try {
      await cl.cancel();
    } catch (e) {
      error = msg(e);
    }
  }

  // ----------------------------------------------------------------- review

  function toggleOpen(id: string) {
    const next = new Set(open);
    if (!next.delete(id)) next.add(id);
    open = next;
  }

  function limit(id: string): number {
    return shown[id] ?? cl.PAGE;
  }

  function showMore(id: string) {
    shown = { ...shown, [id]: limit(id) + cl.PAGE };
  }

  function tickCategory(c: cl.CategoryResult) {
    selected = cl.toggleCategory(selected, c);
  }

  function tickItem(id: string) {
    selected = cl.toggleItem(selected, id);
  }

  /** Keeps a category checkbox's "some ticked" dash in sync (there is no HTML attribute for it). */
  function indeterminate(node: HTMLInputElement, on: boolean) {
    node.indeterminate = on;
    return {
      update(next: boolean) {
        node.indeterminate = next;
      },
    };
  }

  // ----------------------------------------------------------------- confirm + move

  async function askConfirm() {
    if (totals.items === 0 || busy) return;
    confirmOpen = true;
    await tick();
    cancelBtn?.focus(); // the safe choice has focus, not the destructive one
  }

  async function closeConfirm() {
    confirmOpen = false;
    await tick();
    scanBtn?.focus();
  }

  function dialogKeys(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      void closeConfirm();
      return;
    }
    if (e.key !== 'Tab' || !dialogEl) return;
    const items = Array.from(dialogEl.querySelectorAll<HTMLElement>('button:not([disabled])'));
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  async function move() {
    if (!scan || totals.items === 0 || busy) return;
    const id = scan.scanId;
    const ids = [...selected];
    confirmOpen = false;
    phase = 'deleting';
    moving = { done: 0, total: totals.items };
    error = null;
    stopping = false;
    announce = `Moving ${cl.plural(totals.items, 'item')} to the Recycle Bin.`;
    try {
      report = await cl.deleteItems(id, { itemIds: ids }, (p) => (moving = p));
      phase = 'done';
      announce = cl.summarizeReport(report).headline;
      bin = await cl.recycleBinSize().catch(() => null);
    } catch (e) {
      const text = msg(e);
      error = text;
      announce = text;
      if (/out of date|no scan to clean/i.test(text)) {
        scan = null;
        selected = new Set();
        phase = 'idle';
      } else {
        phase = 'review';
      }
    } finally {
      moving = null;
      stopping = false;
    }
  }

  function categoryNotes(c: cl.CategoryResult): string[] {
    return cl.skipNotes(c);
  }

  function metaLine(c: cl.CategoryResult, info: cl.CategoryInfo): string {
    if (info.readOnly) {
      return c.rootsFound > 0 ? `${formatBytes(c.bytes)} · ${cl.plural(c.files, 'item')} (size only)` : 'Windows did not report a size';
    }
    if (c.items.length === 0) return c.rootsFound === 0 ? 'Not found on this PC' : 'Nothing to clean';
    return `${formatBytes(c.bytes)} · ${cl.plural(c.files, 'file')}`;
  }

  onMount(() => {
    void cl
      .categories()
      .then((list) => (infos = list))
      .catch((e) => (error = msg(e)));
  });
</script>

<div class="view" role="presentation">
  <header class="view-head" inert={confirmOpen}>
    <p class="lede">
      Find junk you can safely clear. Krovi lists every folder and file with its real path and size, and moves nothing until
      you confirm. Everything goes to the Recycle Bin, so you can undo it.
    </p>
    {#if phase === 'scanning'}
      <button class="btn" onclick={stop} disabled={stopping}><Icon name="square" size={13} /> {stopping ? 'Stopping' : 'Stop'}</button>
    {:else if phase === 'deleting'}
      <button class="btn" onclick={stop} disabled={stopping}><Icon name="square" size={13} /> {stopping ? 'Stopping' : 'Stop'}</button>
    {:else if phase === 'idle'}
      <button class="btn primary" bind:this={scanBtn} onclick={startScan} disabled={infos.length === 0}><Icon name="search" size={14} /> Scan this PC</button>
    {:else}
      <button class="btn" bind:this={scanBtn} onclick={startScan}><Icon name="refresh" size={13} /> Scan again</button>
    {/if}
  </header>

  {#if demo}
    <div class="demo" role="note">
      <span class="tag accent">DEMO</span>
      <span>Made-up files, folders and sizes for this preview. Nothing on your computer is scanned, moved or changed.</span>
    </div>
  {/if}

  {#if error}
    <div class="problem" role="alert" inert={confirmOpen}><Icon name="alert" size={15} /> <span>{error}</span></div>
  {/if}

  <div class="body scroll" inert={confirmOpen}>
    {#key phase}
      <div class="stage" in:swap>
        {#if phase === 'idle'}
          <State
            compact
            icon="shield"
            title="Nothing has been scanned yet"
            text="Press Scan this PC. Krovi only reads while it scans. It looks in the places below and nowhere else, and never follows shortcuts or links." />
          <h2 class="eyebrow gap">Where Krovi looks</h2>
          <ul class="places">
            {#each infos as info (info.id)}
              <li class="card place">
                <div class="ptitle">
                  <b>{info.title}</b>
                  {#if info.badge}<span class="tag accent">{info.badge}</span>{/if}
                </div>
                <p class="faint">{info.description}</p>
              </li>
            {/each}
          </ul>
        {:else if phase === 'scanning'}
          <section class="card scanning" aria-live="off">
            <div class="srow">
              <span class="spin"><Icon name="loader" size={18} /></span>
              <div class="stack">
                <b>{scanTitle ? `Looking at ${scanTitle}` : 'Starting'}</b>
                <small class="faint">{cl.plural(lookedAt, 'file')} looked at so far · {formatBytes(lookedBytes)}</small>
              </div>
            </div>
            <ul class="steps">
              {#each infos.filter((i) => !i.readOnly) as info (info.id)}
                {@const p = seen[info.id]}
                <li class="step" class:now={current === info.id}>
                  <Icon name={p && current !== info.id ? 'circleCheck' : current === info.id ? 'loader' : 'clock'} size={14} />
                  <span class="ellipsis">{info.title}</span>
                  <span class="grow"></span>
                  <span class="faint num">{p ? `${cl.formatCount(p.files)} files · ${formatBytes(p.bytes)}` : ''}</span>
                </li>
              {/each}
            </ul>
          </section>
        {:else if phase === 'review' && scan}
          {#if scan.cancelled}
            <p class="banner" role="status"><Icon name="info" size={14} /> The scan was stopped early. This list is correct but may be missing things.</p>
          {/if}
          <p class="found" aria-live="off">
            <b class="num">{formatBytes(found)}</b> found that could be moved to the Recycle Bin. Tick what you want, then press the button at the bottom.
          </p>

          <div class="cats">
            {#each scan.categories as c (c.id)}
              {@const info = infoById.get(c.id)}
              {#if info}
                {@const st = cl.categoryState(selected, c)}
                {@const isOpen = open.has(c.id)}
                {@const notes = categoryNotes(c)}
                <section class="cat card" class:ro={info.readOnly} aria-labelledby="cl-h-{c.id}">
                  <div class="chead">
                    {#if info.readOnly}
                      <span class="tickspace" aria-hidden="true"><Icon name="lock" size={15} /></span>
                    {:else}
                      <input
                        type="checkbox"
                        class="tick"
                        aria-label="Select everything in {info.title}"
                        checked={st === 'all'}
                        use:indeterminate={st === 'some'}
                        disabled={c.items.length === 0}
                        onchange={() => tickCategory(c)} />
                    {/if}
                    <h3 class="ctitle" id="cl-h-{c.id}">
                      <button class="expand" aria-expanded={isOpen} aria-controls="cl-d-{c.id}" onclick={() => toggleOpen(c.id)}>
                        <span class="tline">
                          <span class="tname">{info.title}</span>
                          {#if info.badge}<span class="tag accent">{info.badge}</span>{/if}
                        </span>
                        <span class="meta num">{metaLine(c, info)}</span>
                        <Icon name={isOpen ? 'chevronUp' : 'chevronDown'} size={16} />
                      </button>
                    </h3>
                  </div>
                  <p class="risk faint">{info.riskNote}</p>

                  {#if isOpen}
                    <div class="detail" id="cl-d-{c.id}">
                      <p class="desc">{info.description}</p>
                      {#each notes as n}
                        <p class="note faint"><Icon name="info" size={12} /> <span>{n}</span></p>
                      {/each}
                      {#if c.items.length}
                        <ul class="items">
                          {#each c.items.slice(0, limit(c.id)) as it (it.id)}
                            {@const parts = cl.splitPath(it.path)}
                            {@const age = cl.itemAge(it, scannedAt)}
                            <li class="item">
                              <label>
                                <input type="checkbox" class="tick" checked={selected.has(it.id)} onchange={() => tickItem(it.id)} />
                                <span class="ipath">
                                  <span class="iname ellipsis" title={it.path}>{cl.itemTitle(it)}</span>
                                  <span class="idir ellipsis faint" title={it.path}>{it.label ? it.path : parts.dir}</span>
                                </span>
                                <span class="iside">
                                  <span class="isize num">{formatBytes(it.bytes)}</span>
                                  <span class="imeta faint">{cl.itemDetail(it)}{age ? ` · ${age}` : ''}</span>
                                </span>
                              </label>
                            </li>
                          {/each}
                        </ul>
                        {#if c.items.length > limit(c.id)}
                          <button class="btn sm more" onclick={() => showMore(c.id)}>
                            Show {Math.min(cl.PAGE, c.items.length - limit(c.id))} more ({cl.formatCount(c.items.length - limit(c.id))} not shown yet)
                          </button>
                        {/if}
                      {:else if !info.readOnly}
                        <p class="faint none">Nothing here that Krovi can safely move.</p>
                      {/if}
                    </div>
                  {/if}
                </section>
              {/if}
            {/each}
          </div>
        {:else if phase === 'deleting'}
          <section class="card scanning">
            <div class="srow">
              <span class="spin"><Icon name="loader" size={18} /></span>
              <div class="stack">
                <b>Moving to the Recycle Bin</b>
                <small class="faint num">
                  {moving ? `${cl.formatCount(moving.done)} of ${cl.formatCount(moving.total)} items checked and moved` : 'Starting'}
                </small>
              </div>
            </div>
            <div
              class="bar"
              role="progressbar"
              aria-label="Moving items"
              aria-valuemin="0"
              aria-valuemax={moving?.total ?? 0}
              aria-valuenow={moving?.done ?? 0}>
              <span style:width="{moving && moving.total ? Math.round((moving.done / moving.total) * 100) : 0}%"></span>
            </div>
            <p class="faint small">Krovi checks each item again just before moving it, and skips anything that changed.</p>
          </section>
        {:else if phase === 'done' && report && summary}
          <section class="card result" aria-labelledby="cl-res">
            <div class="rhead">
              <span class="ricon" class:ok={summary.clean}><Icon name={summary.clean ? 'circleCheck' : 'info'} size={20} /></span>
              <h2 id="cl-res">{summary.headline}</h2>
              {#if report.demo}<span class="tag accent">DEMO</span>{/if}
            </div>
            {#each summary.lines as line}
              <p class="rline">{line}</p>
            {/each}
            {#if bin}
              <p class="rline faint">
                <Icon name="trash" size={12} /> The Recycle Bin now holds {formatBytes(bin.bytes)} in {cl.plural(bin.items, 'item')}. Krovi never empties
                it: open the Recycle Bin on your desktop to restore things or empty it.
              </p>
            {/if}

            {#if report.failed.length}
              <h3 class="eyebrow gap">Could not be moved</h3>
              <ul class="fails">
                {#each allFailures ? report.failed : report.failed.slice(0, FAILURES_SHOWN) as f}
                  <li class="fail">
                    <span class="fpath ellipsis" title={f.path}>{f.path}</span>
                    <span class="freason">{f.reason}</span>
                  </li>
                {/each}
              </ul>
              {#if !allFailures && report.failed.length > FAILURES_SHOWN}
                <button class="btn sm more" onclick={() => (allFailures = true)}>
                  Show all {cl.formatCount(report.failed.length)}
                </button>
              {/if}
            {/if}
            <div class="ractions">
              <button class="btn primary" onclick={startScan}><Icon name="refresh" size={14} /> Scan again</button>
            </div>
          </section>
        {/if}
      </div>
    {/key}
  </div>

  {#if phase === 'review' && scan}
    <footer class="foot" inert={confirmOpen}>
      <span class="sel" aria-live="polite">
        <b class="num">{formatBytes(totals.bytes)}</b> in {cl.plural(totals.items, 'item')} ticked
        {#if yourFiles > 0}<span class="tag accent">includes {cl.plural(yourFiles, 'of your own file')}</span>{/if}
      </span>
      <span class="grow"></span>
      <button class="btn primary" onclick={askConfirm} disabled={totals.items === 0}>
        <Icon name="trash" size={14} /> Move to Recycle Bin
      </button>
    </footer>
  {/if}

  <p class="sr-only" role="status" aria-live="polite">{announce}</p>

  <!-- ============================ the confirm step ============================ -->
  {#if confirmOpen && scan}
    <div class="scrim" role="presentation" onclick={closeConfirm} transition:fade={{ duration: dur }}></div>
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="cl-c-h"
      aria-describedby="cl-c-d"
      tabindex="-1"
      bind:this={dialogEl}
      onkeydown={dialogKeys}
      transition:fly={{ y: 12, duration: dur }}>
      <h2 id="cl-c-h"><Icon name="trash" size={16} /> {confirmLabel}</h2>
      <div id="cl-c-d">
        <p>
          These items are moved to the Recycle Bin, not deleted for good. You can restore them from there, and disk space only comes
          back once you empty the Recycle Bin yourself. Krovi checks each one again just before moving it and skips anything that changed.
        </p>
        <ul class="mix">
          {#each breakdown as b (b.id)}
            <li>
              <span class="ellipsis">{infoById.get(b.id)?.title ?? b.id}</span>
              <span class="num faint">{cl.plural(b.items, 'item')} · {formatBytes(b.bytes)}</span>
            </li>
          {/each}
        </ul>
        {#if yourFiles > 0}
          <p class="warn"><Icon name="alert" size={14} /> <span>This includes {cl.plural(yourFiles, 'of your own file')} from your Downloads folder.</span></p>
        {/if}
        {#if demo}
          <p class="faint">This is a demo: nothing real will be moved.</p>
        {/if}
      </div>
      <div class="actions">
        <button class="btn" bind:this={cancelBtn} onclick={closeConfirm}>Cancel</button>
        <button class="btn primary" onclick={move}>{confirmLabel}</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .view-head {
    align-items: center;
  }
  .lede {
    margin: 0;
    max-width: 58ch;
    color: var(--text-2);
    font-size: var(--t-sm);
    line-height: 1.5;
  }
  .grow {
    flex: 1;
  }
  .body {
    flex: 1;
    min-height: 0;
    padding-right: 4px;
  }
  .gap {
    margin: 18px 0 8px;
  }
  .demo {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;
    padding: 8px 12px;
    border-radius: var(--r-2);
    border: 1px dashed color-mix(in srgb, var(--accent) 55%, var(--line));
    background: var(--accent-soft);
    color: var(--text);
    font-size: var(--t-xs);
    line-height: 1.4;
  }
  .problem {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
    padding: 9px 12px;
    border-radius: var(--r-2);
    border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 10px;
    padding: 8px 12px;
    border-radius: var(--r-2);
    background: var(--surface-2);
    color: var(--text);
    font-size: var(--t-xs);
  }
  .found {
    margin: 0 0 12px;
    color: var(--text-2);
    font-size: var(--t-sm);
    line-height: 1.5;
  }
  .found b {
    color: var(--text);
    font-size: var(--t-lg);
  }

  /* ---- idle ---- */
  .places {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }
  .place {
    padding: 10px 14px;
  }
  .ptitle {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--t-sm);
  }
  .place p {
    margin: 3px 0 0;
    font-size: var(--t-xs);
    line-height: 1.5;
  }

  /* ---- scanning / moving ---- */
  .scanning {
    display: grid;
    gap: 12px;
    padding: 16px 18px;
  }
  .srow {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .stack {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .stack b {
    font-size: var(--t-md);
    font-weight: 650;
  }
  .stack small {
    font-size: var(--t-xs);
  }
  .spin :global(svg) {
    animation: cl-spin 1.1s linear infinite;
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .step {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--t-xs);
    color: var(--text-3);
  }
  .step.now {
    color: var(--text);
  }
  .step.now :global(svg) {
    animation: cl-spin 1.1s linear infinite;
  }
  .bar {
    height: 6px;
    border-radius: 999px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width var(--dur-2) var(--ease-out);
  }
  .small {
    margin: 0;
    font-size: var(--t-xs);
  }

  /* ---- review ---- */
  .cats {
    display: grid;
    gap: 10px;
    padding-bottom: 6px;
  }
  .cat {
    padding: 4px 4px 10px;
  }
  .chead {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px 0 14px;
  }
  .tick {
    flex: none;
    width: 17px;
    height: 17px;
    margin: 0;
    accent-color: var(--invert-bg);
    cursor: pointer;
  }
  .tick:disabled {
    cursor: default;
    opacity: 0.4;
  }
  .tickspace {
    display: grid;
    place-items: center;
    width: 17px;
    height: 17px;
    color: var(--text-3);
  }
  .ctitle {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: inherit;
    font-weight: inherit;
  }
  .expand {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 34px;
    padding: 4px 8px;
    border-radius: var(--r-2);
    color: var(--text);
    text-align: left;
    transition: background var(--dur-1);
  }
  .expand:hover {
    background: var(--surface-2);
  }
  .tline {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px 8px;
  }
  .tname {
    font-size: var(--t-md);
    font-weight: 650;
  }
  .meta {
    flex: none;
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .risk {
    margin: 0;
    padding: 2px 16px 0 44px;
    font-size: var(--t-xs);
    line-height: 1.5;
  }
  .ro .risk {
    padding-left: 44px;
  }
  .detail {
    display: grid;
    gap: 6px;
    margin: 8px 8px 0 14px;
    padding-top: 8px;
    border-top: 1px solid var(--line);
  }
  .desc {
    margin: 0 0 2px;
    padding-left: 30px;
    font-size: var(--t-xs);
    line-height: 1.5;
    color: var(--text-2);
  }
  .note {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 0;
    padding-left: 30px;
    font-size: var(--t-xs);
    line-height: 1.45;
  }
  .note :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .none {
    margin: 0;
    padding-left: 30px;
    font-size: var(--t-xs);
  }
  .items {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    display: grid;
  }
  .item label {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 8px;
    border-radius: var(--r-1);
    cursor: pointer;
  }
  .item label:hover {
    background: var(--surface-1);
  }
  .ipath {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 1px;
  }
  .iname {
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .idir {
    font-size: var(--t-xs);
    font-family: var(--font-mono);
  }
  .iside {
    flex: none;
    display: grid;
    justify-items: end;
    gap: 1px;
  }
  .isize {
    font-size: var(--t-sm);
    font-weight: 600;
  }
  .imeta {
    font-size: var(--t-micro);
    white-space: nowrap;
  }
  .more {
    justify-self: start;
    margin: 4px 0 0 30px;
  }

  /* ---- footer bar ---- */
  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }
  .sel {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 10px;
    font-size: var(--t-sm);
    color: var(--text-2);
  }
  .sel b {
    color: var(--text);
    font-size: var(--t-lg);
  }

  /* ---- result ---- */
  .result {
    display: grid;
    gap: 8px;
    padding: 18px 20px;
  }
  .rhead {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .rhead h2 {
    font-family: var(--font-display);
    font-size: var(--t-lg);
    font-weight: 650;
  }
  .ricon {
    display: grid;
    place-items: center;
    color: var(--text-2);
  }
  .ricon.ok {
    color: var(--text);
  }
  .rline {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 0;
    font-size: var(--t-sm);
    line-height: 1.5;
    color: var(--text-2);
  }
  .rline :global(svg) {
    flex: none;
    margin-top: 3px;
  }
  .fails {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
  }
  .fail {
    display: grid;
    gap: 1px;
    padding: 6px 10px;
    border-radius: var(--r-1);
    background: var(--surface-1);
  }
  .fpath {
    font-family: var(--font-mono);
    font-size: var(--t-xs);
  }
  .freason {
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .ractions {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }

  /* ---- confirm dialog ---- */
  .scrim {
    position: absolute;
    inset: 0;
    z-index: 40;
    background: var(--scrim);
    border-radius: inherit;
  }
  .dialog {
    position: absolute;
    z-index: 50;
    left: 50%;
    top: 50%;
    translate: -50% -50%;
    width: min(480px, calc(100% - 32px));
    max-height: calc(100% - 32px);
    overflow-y: auto;
    display: grid;
    gap: 12px;
    padding: 18px 20px;
    border-radius: var(--r-4);
    border: 1px solid var(--line-strong);
    background:
      linear-gradient(var(--pop-bg), var(--pop-bg)),
      var(--pop-bg);
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    box-shadow: var(--shadow-pop);
  }
  .dialog h2 {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--t-lg);
    font-weight: 650;
  }
  .dialog p {
    margin: 0;
    font-size: var(--t-sm);
    line-height: 1.55;
    color: var(--text-2);
  }
  #cl-c-d {
    display: grid;
    gap: 10px;
  }
  .mix {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
  }
  .mix li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 10px;
    border-radius: var(--r-1);
    background: var(--surface-1);
    font-size: var(--t-xs);
  }
  .warn {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    color: var(--text) !important;
  }
  .warn :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--accent);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 8px;
  }

  @keyframes cl-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin :global(svg),
    .step.now :global(svg) {
      animation: none;
    }
    .bar span {
      transition: none;
    }
  }
</style>
