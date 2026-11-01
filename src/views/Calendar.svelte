<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import { holidayMap, fromISO, toISO } from '../components/agenda';
  import State from '../components/State.svelte';
  import { REGIONS, type Holiday } from '../lib/holidays';
  import { bridge } from '../lib/bridge';
  import { calendarTick, now, onNative, region, openTool } from '../lib/stores';
  import { arrive, depart, swap } from '../lib/motion';
  import {
    SNOOZE_CHOICES,
    addReminder,
    canDismiss,
    canSnooze,
    dismissReminder,
    environmentNote,
    isPastBy,
    loadReminders,
    normalizeEvents,
    reminderBadge,
    remindersStatus,
    setReminderAlert,
    snoozeReminder,
    type ReminderEvent,
  } from '../lib/reminders';

  const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
  const DOW = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  const base = new Date();
  let year = $state(base.getFullYear());
  let month = $state(base.getMonth());
  let selected = $state(toISO(base));
  let dir = $state(1);
  let draft = $state('');
  let draftTime = $state('');
  // a timed event is a reminder: the alert and its sound are on unless switched off here
  let draftRemind = $state(true);
  let draftSound = $state(true);
  let list = $state<ReminderEvent[]>([]);
  let evStatus = $state<'loading' | 'ready' | 'error'>('loading');
  let formError = $state('');
  // why a Windows pop-up may not show (Do Not Disturb, notifications off, ...); null = nothing known
  let envNote = $state<string | null>(null);
  async function refreshEnv() {
    try {
      const s = await remindersStatus();
      envNote = s ? environmentNote(s.environment) : null;
    } catch {
      envNote = null;
    }
  }

  const todayISO = $derived(toISO(new Date($now)));
  const draftAlert = $derived(!!draftTime && draftRemind);
  // the picked moment has already passed, so no alert could ring (the backend stores it with the alert off)
  const draftPast = $derived(draftAlert && isPastBy(selected, draftTime, $now));
  const regionName = $derived(REGIONS.find((r) => r.code === $region)?.name ?? $region);

  // 6 x 7 grid starting Monday
  const cells = $derived.by(() => {
    const first = new Date(year, month, 1);
    const offset = (first.getDay() + 6) % 7;
    return Array.from({ length: 42 }, (_, i) => {
      const d = new Date(year, month, 1 - offset + i);
      return { d, key: toISO(d), inMonth: d.getMonth() === month };
    });
  });

  const holidays = $derived(holidayMap(cells.map((c) => c.d.getFullYear()), $region));
  const events = $derived.by(() => {
    const m = new Map<string, ReminderEvent[]>();
    for (const e of list) {
      const list = m.get(e.date) ?? [];
      list.push(e);
      m.set(e.date, list);
    }
    return m;
  });

  const dayHolidays = $derived(holidays.get(selected) ?? []);
  const dayEvents = $derived(
    [...(events.get(selected) ?? [])].sort((a, b) => (a.time ?? '').localeCompare(b.time ?? '')),
  );
  const selLabel = $derived(
    fromISO(selected).toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' }),
  );
  const monthHolidays = $derived(
    [...holidays.values()].flat().filter((h) => h.date.startsWith(`${year}-${String(month + 1).padStart(2, '0')}`)).length,
  );

  function go(delta: number) {
    dir = delta;
    const d = new Date(year, month + delta, 1);
    year = d.getFullYear();
    month = d.getMonth();
  }
  function today() {
    const t = new Date();
    dir = year * 12 + month < t.getFullYear() * 12 + t.getMonth() ? 1 : -1;
    year = t.getFullYear();
    month = t.getMonth();
    selected = toISO(t);
  }
  function select(key: string) {
    selected = key;
    const d = fromISO(key);
    if (d.getMonth() !== month || d.getFullYear() !== year) {
      dir = d > new Date(year, month, 1) ? 1 : -1;
      year = d.getFullYear();
      month = d.getMonth();
    }
  }
  async function add() {
    if (!draft.trim()) return;
    formError = '';
    try {
      list = await addReminder({
        date: selected,
        title: draft,
        time: draftTime || null,
        remind: draftAlert && !draftPast,
        sound: draftSound,
      });
      draft = '';
      draftTime = '';
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    }
  }
  async function removeEvent(id: number) {
    try {
      list = normalizeEvents((await bridge.removeCalendarEvent(id)) as ReminderEvent[]);
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    }
  }
  async function load() {
    evStatus = 'loading';
    try {
      list = await loadReminders();
      evStatus = 'ready';
    } catch {
      evStatus = 'error';
    }
  }
  // alert / sound / snooze / dismiss: the backend returns the whole list
  async function act(run: () => Promise<ReminderEvent[]>) {
    formError = '';
    try {
      list = await run();
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    }
  }
  const toggleAlert = (e: ReminderEvent) => e.id !== undefined && act(() => setReminderAlert(e.id as number, { remind: !e.remind }));
  const toggleSound = (e: ReminderEvent) => e.id !== undefined && act(() => setReminderAlert(e.id as number, { sound: !e.sound }));
  const snooze = (id: number, minutes: number) => act(() => snoozeReminder(id, minutes));
  const dismiss = (id: number) => act(() => dismissReminder(id));

  function onGridKey(e: KeyboardEvent) {
    const step = e.key === 'ArrowLeft' ? -1 : e.key === 'ArrowRight' ? 1 : e.key === 'ArrowUp' ? -7 : e.key === 'ArrowDown' ? 7 : 0;
    if (!step) return;
    e.preventDefault();
    const d = fromISO(selected);
    d.setDate(d.getDate() + step);
    select(toISO(d));
    requestAnimationFrame(() => document.querySelector<HTMLButtonElement>('.day[aria-selected="true"]')?.focus());
  }

  const typeLabel = (h: Holiday) => (h.type === 'gazetted' ? 'Public holiday' : h.type === 'restricted' ? 'Restricted' : 'Observance');

  onMount(() => {
    void load();
    void refreshEnv();
    // a reminder the backend saved (voice): the event carries the whole, ordered list
    let off: (() => void) | undefined;
    let dead = false;
    void onNative<ReminderEvent[]>('calendar-changed', (l) => {
      list = normalizeEvents(l);
      evStatus = 'ready';
    }).then((u) => {
      if (dead) u();
      else off = u;
    });
    return () => {
      dead = true;
      off?.();
    };
  });

  // re-check Windows' notification state whenever a time is being picked (Do Not Disturb can change any time)
  $effect(() => {
    if (draftAlert) void refreshEnv();
  });

  // a reminder added by voice while this view is open
  let seenTick = $calendarTick;
  $effect(() => {
    const t = $calendarTick;
    if (t === seenTick) return;
    seenTick = t;
    loadReminders()
      .then((l) => (list = l))
      .catch(() => {});
  });
</script>

<div class="view">
  <div class="layout">
    <section class="cal" aria-label="Month">
      <div class="head">
        <div class="mt">
          {#key `${year}-${month}`}
            <h2 in:swap={{ y: 8 * dir, duration: 260 }}>{MONTHS[month]} <span class="faint">{year}</span></h2>
          {/key}
          <p class="faint">{monthHolidays} {monthHolidays === 1 ? 'holiday' : 'holidays'} · {regionName}</p>
        </div>
        <div class="nav">
          <button class="btn sm ghost" onclick={today} disabled={selected === todayISO && month === base.getMonth() && year === base.getFullYear()}>Today</button>
          <button class="btn icon sm" aria-label="Previous month" onclick={() => go(-1)}><Icon name="chevronLeft" size={15} /></button>
          <button class="btn icon sm" aria-label="Next month" onclick={() => go(1)}><Icon name="chevronRight" size={15} /></button>
        </div>
      </div>

      <div class="dow" aria-hidden="true">{#each DOW as d (d)}<span>{d}</span>{/each}</div>

      <div class="gridwrap">
        {#key `${year}-${month}`}
          <div class="grid" role="grid" aria-label={`${MONTHS[month]} ${year}`} tabindex="-1" onkeydown={onGridKey} in:swap={{ y: 14 * dir, duration: 340 }}>
            {#each cells as c (c.key)}
              {@const hol = holidays.get(c.key)}
              {@const evs = events.get(c.key)}
              <button
                type="button"
                role="gridcell"
                class="day"
                class:dim={!c.inMonth}
                class:today={c.key === todayISO}
                class:sel={c.key === selected}
                class:hol={!!hol}
                class:gaz={hol?.some((h) => h.type === 'gazetted')}
                aria-selected={c.key === selected}
                tabindex={c.key === selected ? 0 : -1}
                aria-label={`${c.d.toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' })}${hol ? `, ${hol.map((h) => h.name).join(', ')}` : ''}${evs ? `, ${evs.length} ${evs.length === 1 ? 'event' : 'events'}` : ''}`}
                onclick={() => select(c.key)}>
                <span class="dn">{c.d.getDate()}</span>
                <span class="marks" aria-hidden="true">
                  {#if hol}<i class="m-hol"></i>{/if}
                  {#if evs}<i class="m-ev"></i>{/if}
                </span>
              </button>
            {/each}
          </div>
        {/key}
      </div>

      <div class="legend faint" aria-hidden="true">
        <span><i class="m-hol"></i> Holiday</span>
        <span><i class="m-ev"></i> Reminder</span>
        <button class="linkbtn" onclick={() => openTool('settings')}>Region: {regionName}</button>
      </div>
    </section>

    <aside class="agenda" aria-label="Agenda">
      <p class="eyebrow"><Icon name="calendar" size={13} /> {selLabel}</p>

      <ul class="list scroll">
        {#each dayHolidays as h (h.name + h.date)}
          <li class="item hol" in:arrive out:depart>
            <span class="ico"><Icon name="sparkles" size={14} /></span>
            <div class="tx">
              <strong>{h.name}</strong>
              <span class="faint">{typeLabel(h)} · {h.region}</span>
            </div>
          </li>
        {/each}
        {#each dayEvents as e (e.id ?? e)}
          {@const rid = e.id}
          {@const badge = reminderBadge(e, $now)}
          <li class="item ev" in:arrive out:depart>
            <div class="row">
              <span class="time num">{e.time ?? 'All day'}</span>
              <strong class="ellipsis">{e.title}</strong>
              {#if rid !== undefined}
                <button class="x" aria-label={`Delete ${e.title}`} onclick={() => removeEvent(rid)}><Icon name="trash" size={13} /></button>
              {/if}
            </div>
            {#if e.time && rid !== undefined}
              <div class="sub">
                <span class="badge {badge.tone}" class:live={badge.state === 'upcoming'}>{badge.text}</span>
                <button
                  type="button"
                  class="tog"
                  class:on={e.remind}
                  aria-pressed={e.remind}
                  aria-label={e.remind ? `Turn off the alert for ${e.title}` : `Alert me for ${e.title}`}
                  title={e.remind ? 'Alert on — click to turn off' : 'Alert off — click to turn on'}
                  onclick={() => toggleAlert(e)}><Icon name="bell" size={13} /></button>
                <button
                  type="button"
                  class="tog"
                  class:on={e.remind && e.sound}
                  aria-pressed={e.sound}
                  disabled={!e.remind}
                  aria-label={e.sound ? `Mute the sound for ${e.title}` : `Play a sound for ${e.title}`}
                  title={e.sound ? 'Sound on — click to mute' : 'Sound off — click to turn on'}
                  onclick={() => toggleSound(e)}><Icon name="volume" size={13} /></button>
              </div>
              {#if canSnooze(e, $now) || canDismiss(e)}
                <div class="acts">
                  {#if canSnooze(e, $now)}
                    <span class="faint">Snooze</span>
                    {#each SNOOZE_CHOICES as m (m)}
                      <button type="button" class="btn sm" onclick={() => snooze(rid, m)}>{m} min</button>
                    {/each}
                  {/if}
                  {#if canDismiss(e)}
                    <button type="button" class="btn sm ghost" onclick={() => dismiss(rid)}>Dismiss</button>
                  {/if}
                </div>
              {/if}
            {/if}
          </li>
        {/each}
        {#if evStatus === 'loading'}
          <li class="skel" style="height:46px"></li>
        {:else if evStatus === 'error'}
          <li>
            <State compact tone="error" title={'Couldn’t load reminders'} text="Your saved reminders could not be read." action={{ label: 'Try again', run: load }} />
          </li>
        {:else if !dayHolidays.length && !dayEvents.length}
          <li class="none">
            <span class="glyph"><Icon name="calendar" size={18} /></span>
            <strong>Nothing on this day</strong>
            <span class="faint">Add a reminder below.</span>
          </li>
        {/if}
      </ul>

      {#if formError}<p class="ferr" role="alert">{formError}</p>{/if}
      <form class="add" onsubmit={(e) => { e.preventDefault(); void add(); }}>
        <input class="input" placeholder="New reminder" bind:value={draft} aria-label="Reminder title" maxlength="80" />
        <div class="add-row">
          <input class="input time" type="time" bind:value={draftTime} aria-label="Time (optional)" />
          <button
            type="button"
            class="btn icon tog-btn"
            class:on={draftAlert}
            aria-pressed={draftAlert}
            disabled={!draftTime}
            aria-label="Remind me at this time"
            title={draftTime ? (draftRemind ? 'Remind me — notification at this time' : 'No alert') : 'Pick a time to get an alert'}
            onclick={() => (draftRemind = !draftRemind)}><Icon name="bell" size={15} /></button>
          <button
            type="button"
            class="btn icon tog-btn"
            class:on={draftAlert && draftSound}
            aria-pressed={draftAlert && draftSound}
            disabled={!draftAlert}
            aria-label="Play a sound with the alert"
            title={draftSound ? 'Sound on' : 'Sound off'}
            onclick={() => (draftSound = !draftSound)}><Icon name="volume" size={15} /></button>
          <button class="btn primary icon" type="submit" aria-label="Add reminder" disabled={!draft.trim()}><Icon name="plus" size={15} /></button>
        </div>
        {#if draftPast}
          <p class="hint warn" role="status">That time has already passed, so it will be saved without an alert.</p>
        {:else if draftTime}
          <p class="hint">{draftAlert ? `You'll get a notification${draftSound ? ' and a sound' : ''} at ${draftTime}.` : 'Saved without an alert.'}</p>
        {:else}
          <p class="hint">Add a time to get a notification.</p>
        {/if}
        {#if envNote && draftAlert}
          <p class="hint warn" role="status">{envNote}</p>
        {/if}
      </form>
    </aside>
  </div>
</div>

<style>
  .layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr 264px;
    gap: 22px;
  }
  .cal {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    margin-bottom: 14px;
    flex: none;
  }
  .mt h2 {
    font-family: var(--font-display);
    font-size: var(--t-2xl);
    font-weight: 650;
    letter-spacing: -0.03em;
    line-height: 1.1;
  }
  .mt h2 .faint {
    font-weight: 500;
  }
  .mt p {
    font-size: var(--t-xs);
    margin-top: 3px;
  }
  .nav {
    display: flex;
    gap: 4px;
  }
  .dow,
  .grid {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    gap: 4px;
  }
  .dow span {
    text-align: center;
    padding-bottom: 6px;
    font-size: var(--t-micro);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .gridwrap {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .grid {
    grid-auto-rows: 1fr;
    height: 100%;
    max-height: 372px;
  }
  .day {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: 14px;
    border: 1px solid transparent;
    color: var(--text);
    font-size: var(--t-md);
    font-weight: 560;
    transition:
      background var(--dur-1) var(--ease-out),
      border-color var(--dur-1),
      transform 0.45s var(--spring);
  }
  .day:hover {
    background: var(--surface-2);
    transform: scale(1.05);
  }
  .day:active {
    transform: scale(0.96);
  }
  .day.dim {
    color: var(--text-3);
    font-weight: 450;
  }
  .dn {
    display: grid;
    place-items: center;
    min-width: 28px;
    height: 28px;
    padding: 0 3px;
    border-radius: 50%;
    font-variant-numeric: tabular-nums;
  }
  /* public holidays read as a tinted date; restricted/observances only get the dot */
  .day.gaz:not(.today) .dn {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 700;
  }
  .day.today .dn {
    background: var(--invert-bg);
    color: var(--invert-fg);
    font-weight: 700;
  }
  .day.sel {
    border-color: var(--line-strong);
    background: var(--surface-2);
    box-shadow: inset 0 1px 0 var(--card-hi);
  }
  .marks {
    display: flex;
    gap: 3px;
    height: 5px;
  }
  .m-hol,
  .m-ev {
    display: inline-block;
    width: 5px;
    height: 5px;
    border-radius: 50%;
  }
  .m-ev {
    background: var(--text-2);
  }
  .m-hol {
    background: transparent;
    border: 1.5px solid var(--accent);
    width: 6px;
    height: 6px;
    margin-top: -0.5px;
    border-radius: 2px;
    transform: rotate(45deg) scale(0.9);
  }
  .legend {
    display: flex;
    align-items: center;
    gap: 16px;
    padding-top: 8px;
    font-size: var(--t-xs);
    flex: none;
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .linkbtn {
    margin-left: auto;
    color: var(--text-2);
    font-size: var(--t-xs);
    font-weight: 600;
    text-decoration: underline;
    text-decoration-color: var(--line-strong);
    text-underline-offset: 3px;
  }
  .linkbtn:hover {
    color: var(--text);
  }

  .agenda {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
    padding-left: 22px;
    border-left: 1px solid var(--line);
  }
  .list {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-right: -8px;
    padding-right: 8px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 46px;
    padding: 8px 8px 8px 12px;
    border-radius: var(--r-3);
    background: var(--surface-1);
    border: 1px solid var(--line);
  }
  .item strong {
    font-size: var(--t-sm);
    font-weight: 600;
    flex: 1;
    min-width: 0;
  }
  .time {
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-2);
    min-width: 46px;
  }
  .item.hol {
    background: var(--accent-soft);
    border-color: color-mix(in srgb, var(--accent) 30%, transparent);
    align-items: flex-start;
  }
  .item.hol .ico {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 8px;
    color: var(--accent);
    background: var(--surface-1);
    flex: none;
  }
  .item.hol .tx {
    display: grid;
    gap: 1px;
    min-width: 0;
  }
  .item.hol .tx strong {
    flex: none;
    white-space: normal;
  }
  .item.hol .tx span {
    font-size: var(--t-xs);
  }
  .x {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 8px;
    color: var(--text-3);
    opacity: 0;
    transition:
      opacity var(--dur-1),
      color var(--dur-1),
      background var(--dur-1);
  }
  .item:hover .x,
  .x:focus-visible {
    opacity: 1;
  }
  .x:hover {
    color: var(--danger);
    background: var(--surface-3);
  }
  .none {
    display: grid;
    justify-items: center;
    gap: 3px;
    padding: 30px 0;
    text-align: center;
    font-size: var(--t-sm);
  }
  .none .glyph {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    margin-bottom: 4px;
    border-radius: var(--r-3);
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text-2);
  }
  .ferr {
    color: var(--danger);
    font-size: var(--t-xs);
    margin: -4px 2px 0;
  }
  .add {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: none;
  }
  .add-row {
    display: flex;
    gap: 6px;
  }
  .add .input.time {
    flex: 1;
    min-width: 0;
    padding: 0 6px 0 8px;
  }
  .add .input:first-child {
    min-width: 0;
  }
  .add-row .btn {
    flex: none;
  }
  /* the alert / sound switches read as on when tinted */
  .tog-btn.on,
  .tog.on {
    background: var(--accent-soft);
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .hint {
    margin: 0 2px;
    font-size: var(--t-xs);
    line-height: 1.35;
    color: var(--text-3);
  }
  .hint.warn {
    color: var(--danger);
  }

  /* an event row: the line, then (for a timed one) its alert status and switches */
  .item.ev {
    flex: none; /* rows keep their height; the list scrolls instead of squashing them */
    flex-direction: column;
    align-items: stretch;
    gap: 4px;
    padding-top: 6px;
    padding-bottom: 6px;
  }
  .item.ev .row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 28px;
  }
  .sub,
  .acts {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .sub .badge {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--text-3);
  }
  .sub .badge.accent {
    color: var(--accent);
  }
  .sub .badge.warn {
    color: var(--danger);
  }
  .tog {
    display: grid;
    place-items: center;
    flex: none;
    width: 26px;
    height: 26px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--surface-1);
    color: var(--text-3);
    transition:
      background var(--dur-1),
      color var(--dur-1),
      border-color var(--dur-1),
      opacity var(--dur-1);
  }
  .tog:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  .tog.on:hover:not(:disabled) {
    color: var(--accent);
  }
  .tog:disabled {
    opacity: 0.38;
  }
  .acts {
    flex-wrap: wrap;
    font-size: var(--t-xs);
  }
  .acts .btn.sm {
    height: 24px;
    padding: 0 9px;
  }
</style>
