<script lang="ts">
  import { onMount } from 'svelte';
  import { bridge } from '../lib/bridge';
  import Icon from '../components/Icon.svelte';
  import type { CalendarEvent } from '../lib/types';

  let events = $state<CalendarEvent[]>([]);
  const base = new Date();
  let year = $state(base.getFullYear());
  let month = $state(base.getMonth()); // 0-11
  let selected = $state(iso(base));
  let draft = $state('');

  const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
  const DOW = ['Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su'];

  function iso(d: Date): string {
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }
  const todayIso = iso(base);

  // Build a 6x7 grid starting Monday.
  const cells = $derived.by(() => {
    const first = new Date(year, month, 1);
    const startOffset = (first.getDay() + 6) % 7; // Mon=0
    const gridStart = new Date(year, month, 1 - startOffset);
    return Array.from({ length: 42 }, (_, i) => {
      const d = new Date(gridStart.getFullYear(), gridStart.getMonth(), gridStart.getDate() + i);
      return { date: d, key: iso(d), inMonth: d.getMonth() === month };
    });
  });

  const byDate = $derived.by(() => {
    const m = new Map<string, CalendarEvent[]>();
    for (const e of events) {
      const list = m.get(e.date) ?? [];
      list.push(e);
      m.set(e.date, list);
    }
    return m;
  });

  const selectedEvents = $derived(
    (byDate.get(selected) ?? []).slice().sort((a, b) => (a.time ?? '').localeCompare(b.time ?? '')),
  );

  function prevMonth() {
    if (month === 0) {
      month = 11;
      year -= 1;
    } else month -= 1;
  }
  function nextMonth() {
    if (month === 11) {
      month = 0;
      year += 1;
    } else month += 1;
  }
  function addReminder() {
    const title = draft.trim();
    if (!title) return;
    events = [...events, { date: selected, title }];
    draft = '';
  }

  const selLabel = $derived(
    new Date(selected + 'T00:00:00').toLocaleDateString(undefined, {
      weekday: 'long',
      day: 'numeric',
      month: 'long',
    }),
  );

  onMount(async () => {
    events = await bridge.loadCalendar();
  });
</script>

<div class="layout">
  <div class="cal">
    <div class="cal-head">
      <button class="btn icon ghost" onclick={prevMonth} title="Previous month"><Icon name="chevronLeft" size={16} /></button>
      <strong>{MONTHS[month]} {year}</strong>
      <button class="btn icon ghost" onclick={nextMonth} title="Next month"><Icon name="chevronRight" size={16} /></button>
    </div>
    <div class="dow">
      {#each DOW as d (d)}<span>{d}</span>{/each}
    </div>
    <div class="grid">
      {#each cells as c (c.key)}
        <button
          class="day"
          class:dim={!c.inMonth}
          class:today={c.key === todayIso}
          class:sel={c.key === selected}
          onclick={() => (selected = c.key)}>
          <span class="dn">{c.date.getDate()}</span>
          {#if byDate.has(c.key)}<span class="evdot"></span>{/if}
        </button>
      {/each}
    </div>
  </div>

  <aside class="agenda">
    <p class="eyebrow"><Icon name="calendar" size={13} /> {selLabel}</p>
    <ul class="evlist scroll">
      {#each selectedEvents as e, i (i)}
        <li>
          {#if e.time}<span class="etime mono">{e.time}</span>{:else}<span class="etime faint">all day</span>{/if}
          <span class="etitle">{e.title}</span>
        </li>
      {:else}
        <li class="faint empty">No events. Add a reminder below.</li>
      {/each}
    </ul>
    <div class="add">
      <input class="input" placeholder="New reminder…" bind:value={draft} onkeydown={(e) => e.key === 'Enter' && addReminder()} />
      <button class="btn accent" onclick={addReminder}><Icon name="plus" size={16} /></button>
    </div>
  </aside>
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 1fr 260px;
    gap: 18px;
  }
  .cal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
    font-size: 15px;
  }
  .dow,
  .grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 4px;
  }
  .dow {
    margin-bottom: 4px;
  }
  .dow span {
    text-align: center;
    font-size: 11px;
    color: var(--text-faint);
    font-weight: 600;
  }
  .day {
    aspect-ratio: 1;
    border: 1px solid transparent;
    background: var(--surface-2);
    border-radius: 9px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    color: var(--text);
    font-size: 13px;
    position: relative;
  }
  .day:hover {
    border-color: var(--border);
  }
  .day.dim {
    color: var(--text-faint);
    background: transparent;
  }
  .day.today .dn {
    color: var(--accent);
    font-weight: 700;
  }
  .day.sel {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .evdot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
  }
  .agenda {
    border-left: 1px solid var(--border);
    padding-left: 16px;
    display: flex;
    flex-direction: column;
  }
  .evlist {
    list-style: none;
    margin: 0 0 12px;
    padding: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 7px;
    max-height: 300px;
  }
  .evlist li {
    display: flex;
    gap: 10px;
    align-items: baseline;
    padding: 9px 11px;
    background: var(--surface-2);
    border-radius: 9px;
    border: 1px solid var(--border);
  }
  .evlist li.empty {
    background: transparent;
    border: none;
    padding: 6px 0;
  }
  .etime {
    font-size: 12px;
    color: var(--accent);
    min-width: 44px;
  }
  .etitle {
    font-size: 13.5px;
  }
  .add {
    display: flex;
    gap: 7px;
  }
  @media (max-width: 680px) {
    .layout {
      grid-template-columns: 1fr;
    }
    .agenda {
      border-left: none;
      border-top: 1px solid var(--border);
      padding-left: 0;
      padding-top: 14px;
    }
  }
</style>
