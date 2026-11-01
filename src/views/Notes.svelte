<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { flip } from 'svelte/animate';
  import Icon from '../components/Icon.svelte';
  import State from '../components/State.svelte';
  import Waveform from '../components/Waveform.svelte';
  import NoteEditor from '../components/NoteEditor.svelte';
  import { noteExcerpt, noteTitle, stats as listStats } from '../lib/checklist';
  import { bridge, isTauri } from '../lib/bridge';
  import { get } from 'svelte/store';
  import { native, noteTick, now, pushToast } from '../lib/stores';
  import { startVoice, stopVoice, voiceAvailability, voiceState } from '../lib/voice';
  import { relativeTime } from '../lib/time';
  import { arrive, depart } from '../lib/motion';
  import type { Note } from '../lib/types';

  const KEY = 'krovi.notes';
  let notes = $state<Note[]>([]);
  let selectedId = $state<number | null>(null);
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let saved = $state(false);
  let query = $state('');
  let editor: ReturnType<typeof NoteEditor> | undefined = $state();

  const byRecent = (l: Note[]) => [...l].sort((a, b) => b.updated_at - a.updated_at);
  const selected = $derived(notes.find((n) => n.id === selectedId) ?? null);
  const shown = $derived(
    query.trim() ? notes.filter((n) => n.body.toLowerCase().includes(query.trim().toLowerCase())) : notes,
  );

  const dictating = $derived($voiceState.mode === 'note' && ($voiceState.state === 'listening' || $voiceState.state === 'processing'));
  let dictationOk = $state(true);
  const voiceOff = $derived(!dictationOk || $voiceState.state === 'unavailable');
  const voiceErr = $derived($voiceState.state === 'error' ? ($voiceState.error ?? 'Voice input failed.') : '');

  const title = (n: Note) => noteTitle(n.body) || 'New note';
  const excerpt = (n: Note) => noteExcerpt(n.body);
  const progress = $derived(selected ? listStats(selected.body) : { done: 0, total: 0 });

  function persistLocal() {
    if (isTauri()) return; // the Rust store is the source of truth natively
    try {
      localStorage.setItem(KEY, JSON.stringify(notes));
    } catch {
      /* ignore */
    }
  }

  async function load(silent = false) {
    if (!silent) status = 'loading';
    try {
      let list: Note[];
      const raw = !isTauri() ? localStorage.getItem(KEY) : null;
      list = raw ? (JSON.parse(raw) as Note[]) : await bridge.loadNotes();
      notes = byRecent(list);
      if (!silent || selectedId === null || !notes.some((n) => n.id === selectedId)) selectedId = notes[0]?.id ?? null;
      status = 'ready';
    } catch {
      status = 'error';
    }
  }

  // a note added from elsewhere (a voice command) while this view is open
  let seenTick = get(noteTick);
  $effect(() => {
    const t = $noteTick;
    if (t === seenTick) return;
    seenTick = t;
    void load(true).then(() => {
      selectedId = notes[0]?.id ?? selectedId;
    });
  });

  async function create() {
    const t = Date.now();
    const res = await native<Note[]>('create_note', { body: '', now: t });
    if (res) {
      notes = byRecent(res);
      selectedId = res.reduce((a, n) => (n.id > a ? n.id : a), 0);
    } else {
      const n: Note = { id: t, body: '', created_at: t, updated_at: t };
      notes = [n, ...notes];
      selectedId = n.id;
      persistLocal();
    }
    await tick();
    editor?.focusFirst();
  }

  async function remove(id: number) {
    const idx = notes.findIndex((n) => n.id === id);
    const res = await native<Note[]>('delete_note', { id });
    notes = res ? byRecent(res) : notes.filter((n) => n.id !== id);
    if (selectedId === id) selectedId = (notes[idx] ?? notes[idx - 1] ?? notes[0])?.id ?? null;
    persistLocal();
  }

  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let flashTimer: ReturnType<typeof setTimeout> | null = null;
  async function persist(id: number, body: string, t: number) {
    await native<Note[]>('update_note', { id, body, now: t });
    saved = true;
    if (flashTimer) clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (saved = false), 1100);
  }
  function onEdit(body: string, immediate = false) {
    if (!selected) return;
    const id = selected.id;
    const t = Date.now();
    notes = byRecent(notes.map((n) => (n.id === id ? { ...n, body, updated_at: t } : n)));
    persistLocal();
    if (saveTimer) clearTimeout(saveTimer);
    // typing is debounced; ticking a checkbox is a deliberate act and saves right away
    if (immediate) void persist(id, body, t);
    else saveTimer = setTimeout(() => void persist(id, body, t), 450);
  }

  function toggleDictation() {
    if (dictating) void stopVoice();
    else void startVoice('note');
  }

  onMount(() => {
    void load();
    void voiceAvailability().then((a) => (dictationOk = a.dictation));
    // voice dictation creates notes in Rust and announces them here
    let off: (() => void) | undefined;
    let dead = false;
    void bridge.onNotesChanged((list) => {
      const had = new Set(notes.map((n) => n.id));
      notes = byRecent(list);
      const fresh = list.find((n) => !had.has(n.id));
      if (fresh) {
        selectedId = fresh.id;
        pushToast('Note saved from voice');
      } else if (selectedId !== null && !list.some((n) => n.id === selectedId)) {
        selectedId = notes[0]?.id ?? null;
      }
      status = 'ready';
    }).then((u) => (dead ? u() : (off = u)));
    return () => {
      dead = true;
      off?.();
      if (saveTimer) clearTimeout(saveTimer);
    };
  });
</script>

<div class="view">
  {#if status === 'loading'}
    <div class="layout">
      <div class="col"><div class="skel" style="height:34px"></div><div class="skel" style="height:56px"></div><div class="skel" style="height:56px"></div></div>
      <div class="skel" style="height:100%;border-radius:20px"></div>
    </div>
  {:else if status === 'error'}
    <State tone="error" title={'Couldn’t open your notes'} text="Your notes could not be read from this device." action={{ label: 'Try again', run: load }} />
  {:else if notes.length === 0}
    <State icon="notes" title="No notes yet" text="Jot something down, or press the mic and just say it. Notes stay on this device.">
      <div class="firstrun">
        <button class="btn primary" onclick={create}><Icon name="plus" size={15} /> New note</button>
        <button class="btn" onclick={toggleDictation} disabled={voiceOff}><Icon name="mic" size={15} /> Dictate</button>
      </div>
    </State>
  {:else}
    <div class="layout">
      <aside class="col" aria-label="Notes">
        <div class="tools">
          <button class="btn primary grow" onclick={create}><Icon name="plus" size={15} /> New note</button>
        </div>
        <label class="find">
          <Icon name="search" size={14} />
          <input placeholder="Search notes" bind:value={query} aria-label="Search notes" />
        </label>
        <ul class="list scroll" aria-label="Note list">
          {#each shown as n (n.id)}
            <li animate:flip={{ duration: 380 }} in:arrive out:depart>
              <button class="nrow" class:on={selectedId === n.id} aria-current={selectedId === n.id} onclick={() => (selectedId = n.id)}>
                <span class="nt ellipsis">{title(n)}</span>
                <span class="ne ellipsis faint">{excerpt(n) || 'No additional text'}</span>
                <span class="nw faint">
                  {relativeTime(n.updated_at, $now)}
                  {#if listStats(n.body).total > 0}
                    <span class="nchk"><Icon name="check" size={10} />{listStats(n.body).done}/{listStats(n.body).total}</span>
                  {/if}
                </span>
              </button>
            </li>
          {:else}
            <li class="faint nomatch">No notes match &ldquo;{query}&rdquo;</li>
          {/each}
        </ul>
      </aside>

      <section class="editor card" aria-label="Editor">
        {#if selected}
          <div class="eh">
            <span class="faint small">Edited {relativeTime(selected.updated_at, $now)}</span>
            <span class="saved" class:show={saved}><Icon name="check" size={13} /> Saved</span>
            {#if progress.total > 0}
              <span class="prog" class:all={progress.done === progress.total} title="Checklist progress">
                <span class="pbar"><i style:width="{(progress.done / progress.total) * 100}%"></i></span>
                {progress.done} of {progress.total}
              </span>
            {/if}
            <span class="grow"></span>
            <button class="btn sm" onclick={() => editor?.addChecklistItem()} title="Add checklist item (Ctrl+Shift+7 on a line)">
              <Icon name="listChecks" size={14} /> Checklist
            </button>
            <button
              class="btn sm"
              class:live={dictating}
              onclick={toggleDictation}
              disabled={voiceOff}
              aria-pressed={dictating}
              title={voiceOff ? 'Voice is not available on this device' : 'Dictate a new note'}>
              {#if dictating}<Waveform height={12} /> Stop{:else}<Icon name={voiceOff ? 'micOff' : 'mic'} size={14} /> Dictate{/if}
            </button>
            <button class="btn sm icon ghost danger" aria-label="Delete note" onclick={() => remove(selected.id)}><Icon name="trash" size={14} /></button>
          </div>
          <div class="edscroll scroll">
            <NoteEditor bind:this={editor} body={selected.body} noteId={selected.id} onchange={onEdit} />
          </div>
          {#if dictating && $voiceState.partial}
            <p class="partial" aria-live="polite">{$voiceState.partial}</p>
          {:else if voiceErr}
            <p class="partial verr" role="alert">{voiceErr}</p>
          {/if}
          <div class="ef faint">{selected.body.length} characters</div>
        {:else}
          <State compact icon="notes" title="Select a note" text="Pick one from the list or start a new one." />
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 252px 1fr;
    gap: 16px;
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }
  .tools {
    display: flex;
  }
  .grow {
    flex: 1;
  }
  .find {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 11px;
    border-radius: var(--r-2);
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text-3);
  }
  .find:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .find input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    outline: none;
    color: var(--text);
    font-size: var(--t-sm);
    font-weight: 500;
  }
  .list {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-right: -6px;
    padding-right: 6px;
  }
  .nrow {
    width: 100%;
    display: grid;
    gap: 1px;
    padding: 10px 12px;
    border-radius: var(--r-3);
    border: 1px solid transparent;
    text-align: left;
    transition:
      background var(--dur-1),
      border-color var(--dur-1),
      transform 0.4s var(--spring);
  }
  .nrow:hover {
    background: var(--surface-1);
  }
  .nrow:active {
    transform: scale(0.985);
  }
  .nrow.on {
    background: var(--surface-2);
    border-color: var(--line);
  }
  .nt {
    font-size: var(--t-sm);
    font-weight: 650;
    letter-spacing: -0.005em;
  }
  .ne {
    font-size: var(--t-xs);
  }
  .nw {
    font-size: var(--t-micro);
    margin-top: 2px;
  }
  .nomatch {
    padding: 14px 8px;
    font-size: var(--t-sm);
  }

  .editor {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 12px 6px 10px 16px;
    border-radius: var(--r-4);
  }
  .editor:focus-within {
    border-color: var(--line-strong);
  }
  .eh {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-right: 10px;
    flex: none;
  }
  .small {
    font-size: var(--t-xs);
  }
  .saved {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-2);
    opacity: 0;
    transform: translateY(2px);
    transition:
      opacity var(--dur-2),
      transform var(--dur-3) var(--ease-out);
  }
  .saved.show {
    opacity: 1;
    transform: none;
  }
  .btn.live {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
    background: var(--accent-soft);
  }
  .edscroll {
    flex: 1;
    min-height: 0;
    margin-top: 8px;
  }
  .prog {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 22px;
    padding: 0 9px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-2);
    font-size: var(--t-xs);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .pbar {
    width: 30px;
    height: 4px;
    border-radius: 4px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .pbar i {
    display: block;
    height: 100%;
    border-radius: 4px;
    background: var(--text-2);
    transition: width 0.5s var(--ease-out);
  }
  .prog.all {
    color: var(--text);
  }
  .prog.all .pbar i {
    background: var(--accent);
  }
  .nchk {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    margin-left: 6px;
    font-variant-numeric: tabular-nums;
  }
  .partial {
    margin: 0 10px 6px 0;
    padding: 8px 12px;
    border-radius: var(--r-2);
    background: var(--accent-soft);
    color: var(--text);
    font-size: var(--t-sm);
    font-style: italic;
  }
  .partial.verr {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    font-style: normal;
    color: var(--danger);
  }
  .ef {
    flex: none;
    padding: 6px 10px 0 0;
    font-size: var(--t-xs);
    border-top: 1px solid var(--line);
  }
  .firstrun {
    display: flex;
    justify-content: center;
    gap: 8px;
    margin-top: 10px;
  }
</style>
