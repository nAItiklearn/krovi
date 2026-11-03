<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from '../../components/Icon.svelte';
  import State from '../../components/State.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { pushToast } from '../../lib/stores';
  import { formatBytes, relativeTime } from '../../lib/time';
  import * as og from '../../lib/organizer';

  // File organizer: rules like "In Downloads, move .pdf files older than 7 days to Documents\PDFs".
  //   1. Preview  lists the exact moves. Nothing changes.
  //   2. Confirm  you read the list and press the button. Krovi never runs a rule by itself.
  //   3. Run      every file is re-checked right before it moves; changed or linked files are left alone.
  //   Undo        puts back the files of the last run that nothing has touched since.
  // Moves only: nothing is deleted or replaced (a name clash becomes "name (2)"), only inside your
  // Downloads, Documents, Pictures, Desktop, Videos and Music folders, links are never followed.

  type Phase = 'idle' | 'previewing' | 'review' | 'running' | 'done';
  interface Draft {
    id?: string;
    enabled: boolean;
    folder: og.FolderId;
    ext: string;
    nameContains: string;
    days: string;
    destFolder: og.FolderId;
    destSubfolder: string;
  }

  let { demo }: { demo: boolean } = $props();

  const MOVES_SHOWN = 100;
  const FAILS_SHOWN = 20;

  let rules = $state<og.Rule[]>([]);
  let infos = $state<og.FolderInfo[]>([]);
  let loaded = $state(false);
  let draft = $state<Draft | null>(null);
  let saveError = $state('');
  let confirmingId = $state<string | null>(null);
  let phase = $state<Phase>('idle');
  let preview = $state<og.Preview | null>(null);
  let showAll = $state(false);
  let report = $state<og.RunReport | null>(null);
  let undoInfo = $state<og.UndoInfo | null>(null);
  let undoReport = $state<og.UndoReport | null>(null);
  let undoing = $state(false);
  let error = $state('');
  let announce = $state('');
  let nameEl: HTMLSelectElement | undefined = $state();
  let previewBtn: HTMLButtonElement | undefined = $state();
  let cancelBtn: HTMLButtonElement | undefined = $state();

  const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
  const busy = $derived(phase === 'previewing' || phase === 'running' || undoing);
  const enabledCount = $derived(rules.filter((r) => r.enabled).length);
  const moves = $derived(preview?.moves ?? []);
  const shownMoves = $derived(showAll ? moves : moves.slice(0, MOVES_SHOWN));
  const runSummary = $derived(report ? og.summarizeRun(report) : null);
  const undoSummary = $derived(undoReport ? og.summarizeUndo(undoReport) : null);

  /** The form's fields as a rule, or the sentence saying why they are not one yet. */
  const built = $derived.by<{ input: og.RuleInput | null; error: string | null }>(() => {
    if (!draft) return { input: null, error: null };
    const ext = og.parseExtensions(draft.ext);
    if (ext.error) return { input: null, error: ext.error };
    const days = draft.days.trim();
    const n = days === '' ? null : Number(days);
    if (n !== null && !Number.isFinite(n)) return { input: null, error: `The age must be a whole number of days, between 1 and ${og.LIMITS.maxAgeDays}.` };
    const input: og.RuleInput = {
      ...(draft.id ? { id: draft.id } : {}),
      enabled: draft.enabled,
      folder: draft.folder,
      extensions: ext.list,
      nameContains: draft.nameContains,
      olderThanDays: n,
      destFolder: draft.destFolder,
      destSubfolder: draft.destSubfolder,
    };
    return { input, error: og.validateRuleInput(input) };
  });
  const sentence = $derived(built.input && !built.error ? og.describeRule(og.normalizeRule(built.input)) : '');
  // Don't nag before anything is filled in.
  const hint = $derived(draft && (draft.ext.trim() || draft.nameContains.trim() || draft.days.trim() || draft.destSubfolder.trim()) ? built.error : null);

  async function load() {
    try {
      [rules, infos] = await Promise.all([og.rules(), og.folders()]);
      undoInfo = await og.undoInfo().catch(() => null);
    } catch (e) {
      error = msg(e);
    } finally {
      loaded = true;
    }
  }

  // ----------------------------------------------------------------- rules

  async function newRule() {
    saveError = '';
    confirmingId = null;
    draft = { enabled: true, folder: 'downloads', ext: '', nameContains: '', days: '', destFolder: 'documents', destSubfolder: '' };
    await tick();
    nameEl?.focus();
  }

  async function editRule(r: og.Rule) {
    saveError = '';
    confirmingId = null;
    draft = {
      id: r.id,
      enabled: r.enabled,
      folder: r.folder,
      ext: r.extensions.join(', '),
      nameContains: r.nameContains,
      days: r.olderThanDays ? String(r.olderThanDays) : '',
      destFolder: r.destFolder,
      destSubfolder: r.destSubfolder,
    };
    await tick();
    nameEl?.focus();
  }

  async function saveDraft(e: SubmitEvent) {
    e.preventDefault();
    if (!built.input || built.error || busy) return;
    saveError = '';
    try {
      rules = await og.saveRule(built.input);
      draft = null;
      discardPreview();
      pushToast('Rule saved', 1600);
    } catch (err) {
      saveError = msg(err);
    }
  }

  async function toggleRule(r: og.Rule, on: boolean) {
    try {
      rules = await og.saveRule({ ...r, enabled: on });
      discardPreview();
    } catch (e) {
      error = msg(e);
      rules = await og.rules().catch(() => rules);
    }
  }

  async function deleteRule(r: og.Rule) {
    try {
      rules = await og.removeRule(r.id);
      confirmingId = null;
      discardPreview();
    } catch (e) {
      error = msg(e);
    }
  }

  function draftKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      draft = null;
    }
  }

  // ----------------------------------------------------------------- preview -> confirm -> run

  function discardPreview() {
    if (phase === 'review' || phase === 'done') {
      preview = null;
      report = null;
      phase = 'idle';
    }
  }

  async function doPreview() {
    if (busy || enabledCount === 0) return;
    phase = 'previewing';
    error = '';
    report = null;
    undoReport = null;
    showAll = false;
    announce = 'Looking at your folders. Nothing is changed.';
    try {
      preview = await og.preview();
      phase = 'review';
      announce = preview.moves.length ? `${og.confirmText(preview.moves)} is ready to confirm.` : 'Nothing to move right now.';
      await tick();
      cancelBtn?.focus(); // the safe choice has focus, not the one that moves files
    } catch (e) {
      error = msg(e);
      phase = 'idle';
    }
  }

  function cancelReview() {
    preview = null;
    phase = 'idle';
    announce = 'Cancelled. Nothing was moved.';
    void tick().then(() => previewBtn?.focus());
  }

  async function confirmRun() {
    if (!preview || busy || preview.moves.length === 0) return;
    const id = preview.previewId;
    phase = 'running';
    error = '';
    announce = 'Moving files.';
    try {
      report = await og.run(id);
      undoInfo = await og.undoInfo().catch(() => null);
      announce = og.summarizeRun(report).headline;
      phase = 'done';
      preview = null;
    } catch (e) {
      const text = msg(e);
      error = text;
      announce = text;
      preview = null;
      phase = 'idle';
    }
  }

  async function doUndo() {
    if (busy || !undoInfo) return;
    undoing = true;
    error = '';
    try {
      undoReport = await og.undo();
      undoInfo = await og.undoInfo().catch(() => null);
      report = null;
      preview = null;
      phase = 'idle';
      announce = og.summarizeUndo(undoReport).headline;
    } catch (e) {
      error = msg(e);
    } finally {
      undoing = false;
    }
  }

  const baseName = (p: string) => og.fileName(p);
  /** `C:\Users\me\Documents\PDFs` -> `Documents\PDFs` (the part after the user's profile folder). */
  const shortDir = (dir: string) => dir.replace(/^.*\\Users\\[^\\]+\\/, '');
  const folderName = (id: og.FolderId) => og.FOLDER_LABEL[id];

  onMount(() => {
    void load();
  });
</script>

<div class="tab">
  <div class="sr" role="status" aria-live="polite">{announce}</div>
  <p class="lede">
    Rules move files out of one of your folders into a folder you name. You always see the exact list first, and nothing moves until
    you confirm. Nothing is deleted or replaced, links are never followed, and nothing runs in the background.
  </p>

  {#if error}
    <div class="problem" role="alert"><Icon name="alert" size={15} /> <span>{error}</span></div>
  {/if}

  <div class="body scroll">
    <!-- ------------------------------------------------------------ rules -->
    <section aria-label="Rules">
      <div class="shead">
        <h3 class="eyebrow"><Icon name="listChecks" size={13} /> Rules</h3>
        <button class="btn sm" type="button" onclick={newRule} disabled={!!draft || busy || rules.length >= og.LIMITS.rules}>
          <Icon name="plus" size={13} /> Add rule
        </button>
      </div>

      {#if draft}
        <!-- Escape closes the editor from any field in it -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <form class="card editor" onsubmit={saveDraft} onkeydown={draftKey} aria-label={draft.id ? 'Edit rule' : 'New rule'}>
          <div class="grid">
            <label>
              <span class="lbl">Look in</span>
              <select class="input" bind:this={nameEl} bind:value={draft.folder}>
                {#each og.FOLDER_IDS as id (id)}<option value={id}>{folderName(id)}</option>{/each}
              </select>
            </label>
            <label>
              <span class="lbl">File types <span class="faint">(for example pdf, docx)</span></span>
              <input class="input" bind:value={draft.ext} placeholder="pdf, docx" autocomplete="off" spellcheck="false" />
            </label>
            <label>
              <span class="lbl">Name contains <span class="faint">(optional)</span></span>
              <input class="input" bind:value={draft.nameContains} maxlength={og.LIMITS.containsChars} placeholder="invoice" autocomplete="off" />
            </label>
            <label>
              <span class="lbl">Older than, in days <span class="faint">(optional)</span></span>
              <input class="input" inputmode="numeric" bind:value={draft.days} placeholder="7" autocomplete="off" />
            </label>
            <label>
              <span class="lbl">Move into</span>
              <select class="input" bind:value={draft.destFolder}>
                {#each og.FOLDER_IDS as id (id)}<option value={id}>{folderName(id)}</option>{/each}
              </select>
            </label>
            <label>
              <span class="lbl">Folder inside it</span>
              <input class="input" bind:value={draft.destSubfolder} placeholder="PDFs" autocomplete="off" spellcheck="false" />
            </label>
          </div>
          {#if sentence}
            <p class="sentence"><Icon name="info" size={13} /> {sentence}</p>
          {/if}
          {#if hint || saveError}
            <p class="error" role="alert"><Icon name="alert" size={14} /> {saveError || hint}</p>
          {/if}
          <div class="actions">
            <button class="btn" type="button" onclick={() => (draft = null)}>Cancel</button>
            <button class="btn primary" type="submit" disabled={!!built.error || !built.input}><Icon name="check" size={14} /> Save rule</button>
          </div>
        </form>
      {/if}

      {#if !loaded}
        <p class="faint">Loading...</p>
      {:else if rules.length === 0 && !draft}
        <State
          icon="folder"
          title="No rules yet"
          text="Add a rule such as: in Downloads, move .pdf files older than 7 days to Documents\PDFs."
          compact
          action={{ label: 'Add rule', run: newRule }} />
      {:else}
        <ul class="rules">
          {#each rules as r (r.id)}
            <li class="rule card" class:off={!r.enabled}>
              <Toggle checked={r.enabled} label="Use this rule: {og.describeRule(r)}" disabled={busy} onchange={(on) => toggleRule(r, on)} />
              <span class="rtext">{og.describeRule(r)}</span>
              <div class="acts">
                {#if confirmingId === r.id}
                  <span class="faint sure">Delete the rule?</span>
                  <button class="btn sm danger" type="button" onclick={() => deleteRule(r)}>Delete</button>
                  <button class="btn sm ghost" type="button" onclick={() => (confirmingId = null)}>Keep</button>
                {:else}
                  <button class="btn sm icon ghost" type="button" aria-label="Edit rule" onclick={() => editRule(r)} disabled={!!draft || busy}>
                    <Icon name="pencil" size={13} />
                  </button>
                  <button class="btn sm icon ghost danger" type="button" aria-label="Delete rule" onclick={() => (confirmingId = r.id)} disabled={!!draft || busy}>
                    <Icon name="trash" size={13} />
                  </button>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <!-- ------------------------------------------------------------ preview / confirm / result -->
    <section aria-label="Preview and run">
      <div class="shead">
        <h3 class="eyebrow"><Icon name="eye" size={13} /> Preview</h3>
        <div class="acts">
          {#if undoInfo && phase !== 'running'}
            <button class="btn sm" type="button" onclick={doUndo} disabled={busy}>
              <Icon name="reset" size={13} /> Undo last run ({undoInfo.count} {undoInfo.count === 1 ? 'file' : 'files'}, {relativeTime(undoInfo.atMs, Date.now())})
            </button>
          {/if}
          <button class="btn sm primary" type="button" bind:this={previewBtn} onclick={doPreview} disabled={busy || enabledCount === 0}>
            <Icon name={phase === 'previewing' ? 'loader' : 'search'} size={13} />
            {phase === 'previewing' ? 'Looking...' : 'Preview moves'}
          </button>
        </div>
      </div>

      {#if enabledCount === 0 && loaded}
        <p class="faint">Switch on at least one rule to preview what it would move.</p>
      {/if}

      {#if phase === 'review' && preview}
        <div class="card review" role="group" aria-label="Moves waiting for your confirmation">
          {#if moves.length === 0}
            <p class="none"><Icon name="circleCheck" size={15} /> Nothing to move right now. Nothing was changed.</p>
          {:else}
            <p class="count">
              <b>{moves.length}</b> {moves.length === 1 ? 'file' : 'files'} would move ({formatBytes(og.totalBytes(moves))}). Check the list, then confirm.
            </p>
            <ul class="moves">
              {#each shownMoves as m (m.id)}
                <li class="move">
                  <span class="mname ellipsis" title={m.from}>{baseName(m.from)}</span>
                  <span class="arrow" aria-label="moves to"><Icon name="arrowUpRight" size={12} /></span>
                  <span class="mto ellipsis" title={`${m.toDir}\\${m.toName}`}>
                    {shortDir(m.toDir)}{#if m.toName !== baseName(m.from)} <b class="renamed">as {m.toName}</b>{/if}
                  </span>
                  <span class="msize faint">{formatBytes(m.size)}</span>
                </li>
              {/each}
            </ul>
            {#if moves.length > shownMoves.length}
              <button class="btn sm ghost" type="button" onclick={() => (showAll = true)}>Show all {moves.length}</button>
            {/if}
          {/if}
          {#if preview.notes.length}
            <ul class="notes">
              {#each preview.notes as n (n)}<li><Icon name="info" size={12} /> <span>{n}</span></li>{/each}
            </ul>
          {/if}
          {#if demo}<p class="faint small">DEMO: these are made-up files. Confirming moves them only inside this preview.</p>{/if}
          <div class="actions">
            <button class="btn" type="button" bind:this={cancelBtn} onclick={cancelReview}>Cancel</button>
            {#if moves.length > 0}
              <button class="btn primary" type="button" onclick={confirmRun}>{og.confirmText(moves)}</button>
            {/if}
          </div>
        </div>
      {:else if phase === 'running'}
        <div class="card review"><p class="none"><Icon name="loader" size={15} /> Moving files...</p></div>
      {/if}

      {#if phase === 'done' && runSummary && report}
        <div class="card review result" class:clean={runSummary.clean}>
          <p class="count"><Icon name={runSummary.clean ? 'circleCheck' : 'info'} size={15} /> <b>{runSummary.headline}</b>{#if report.demo} <span class="tag accent">DEMO</span>{/if}</p>
          {#each runSummary.lines as line (line)}<p class="small">{line}</p>{/each}
          {#if report.failed.length}
            <ul class="notes">
              {#each report.failed.slice(0, FAILS_SHOWN) as f (f.path)}
                <li><Icon name="alert" size={12} /> <span><b>{baseName(f.path)}</b>: {f.reason}</span></li>
              {/each}
            </ul>
            {#if report.failed.length > FAILS_SHOWN}<p class="faint small">and {report.failed.length - FAILS_SHOWN} more.</p>{/if}
          {/if}
        </div>
      {/if}

      {#if undoSummary && undoReport}
        <div class="card review result" class:clean={undoSummary.clean}>
          <p class="count"><Icon name={undoSummary.clean ? 'circleCheck' : 'info'} size={15} /> <b>{undoSummary.headline}</b></p>
          {#each undoSummary.lines as line (line)}<p class="small">{line}</p>{/each}
          {#if undoReport.failed.length}
            <ul class="notes">
              {#each undoReport.failed.slice(0, FAILS_SHOWN) as f (f.path)}
                <li><Icon name="alert" size={12} /> <span><b>{baseName(f.path)}</b>: {f.reason}</span></li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    </section>

    <!-- ------------------------------------------------------------ where Krovi may look -->
    <details class="places">
      <summary>Folders Krovi may use</summary>
      <ul>
        {#each infos as f (f.id)}
          <li><b>{f.label}</b> <span class="mono faint ellipsis" title={f.path ?? ''}>{f.path ?? 'Not found on this PC'}</span></li>
        {/each}
      </ul>
      <p class="faint small">{demo ? 'DEMO paths. ' : ''}Krovi reads and writes nowhere else, and only inside these folders.</p>
    </details>
  </div>
</div>

<style>
  .tab {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .lede {
    flex: none;
    margin: 0;
    color: var(--text-2);
    font-size: var(--t-xs);
    line-height: 1.5;
  }
  .problem {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    border-radius: var(--r-2);
    border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    align-content: start;
    gap: 18px;
    padding-right: 4px;
  }
  section {
    display: grid;
    gap: 8px;
    min-width: 0;
  }
  .shead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }
  .editor {
    display: grid;
    gap: 10px;
    padding: 14px 16px;
    border-radius: var(--r-4);
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  label {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .lbl {
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-2);
  }
  .sentence {
    display: flex;
    gap: 7px;
    align-items: flex-start;
    margin: 0;
    padding: 8px 10px;
    border-radius: var(--r-2);
    background: var(--accent-soft);
    font-size: var(--t-sm);
    font-weight: 560;
    overflow-wrap: anywhere;
  }
  .sentence :global(svg) {
    flex: none;
    margin-top: 3px;
  }
  .error {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 0;
    color: var(--danger);
    font-size: var(--t-sm);
    overflow-wrap: anywhere;
  }
  .error :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .rules {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .rule {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 12px;
    border-radius: var(--r-3);
  }
  .rule.off .rtext {
    color: var(--text-3);
  }
  .rtext {
    flex: 1;
    min-width: 0;
    font-size: var(--t-sm);
    font-weight: 560;
    overflow-wrap: anywhere;
  }
  .acts {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .sure {
    font-size: var(--t-xs);
    margin-right: 2px;
  }
  .review {
    display: grid;
    gap: 10px;
    padding: 14px 16px;
    border-radius: var(--r-4);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .result {
    border-color: var(--line);
  }
  .result.clean {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .count,
  .none {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: var(--t-sm);
  }
  .count b {
    font-size: var(--t-md);
  }
  .small {
    margin: 0;
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .moves {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
    max-height: 190px;
    overflow: auto;
  }
  .move {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 14px minmax(0, 1.4fr) 56px;
    align-items: center;
    gap: 8px;
    padding: 4px 6px;
    border-radius: var(--r-2);
    font-size: var(--t-xs);
  }
  .move:nth-child(odd) {
    background: var(--surface-1);
  }
  .mname {
    font-weight: 600;
  }
  .arrow {
    color: var(--text-3);
    display: grid;
    place-items: center;
  }
  .mto {
    color: var(--text-2);
  }
  .renamed {
    color: var(--text);
  }
  .msize {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .notes {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .notes li {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    overflow-wrap: anywhere;
  }
  .notes li :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .places {
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .places summary {
    cursor: pointer;
    font-weight: 600;
  }
  .places ul {
    list-style: none;
    margin: 8px 0 4px;
    padding: 0;
    display: grid;
    gap: 3px;
  }
  .places li {
    display: flex;
    gap: 10px;
    align-items: baseline;
  }
  .places li b {
    flex: none;
    min-width: 76px;
    color: var(--text);
  }
  .mono {
    font-family: var(--font-mono);
    font-size: var(--t-xs);
    min-width: 0;
  }
</style>
