<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import Island from './components/Island.svelte';
  import Backdrop from './components/Backdrop.svelte';
  import {
    activeTool,
    addNote,
    addToShelf,
    calendarTick,
    emitIntent,
    isEmbed,
    openTool,
    paletteOpen,
    pomodoroStart,
    closeVoiceSheets,
    pushActivity,
    pushToast,
    requestTransfer,
    showVoiceResults,
    voiceResults,
    startClock,
    timerAdd,
    timerControl,
  } from './lib/stores';
  import { bridge, mode } from './lib/bridge';
  import { snippet } from './components/pillModel';
  import { media, mediaTransport, startLive } from './lib/live';
  import { onVoiceCommand, onVoiceResult, voiceState, type VoiceResult } from './lib/voice';
  import { onOpenTool, quit, requestCollapse, shellMode } from './lib/shell';
  import { TOOL_ORDER, isToolId } from './lib/tools';
  import { markActivity } from './lib/idle';
  import { startTheme } from './lib/theme';
  import { setToaster, startTransfer } from './lib/transfer';
  import { startReminders } from './lib/reminders';
  import { createVoiceRouter, showFeedbackOnly, type Chip } from './components/voiceRouter';
  import { planResults } from './components/voiceModel';

  const demo = mode() === 'demo';
  // Theme attributes go on <html> during component init, i.e. before the first paint.
  const stopTheme = startTheme();

  // Say it where the user is looking: a toast in the open panel, a line on the collapsed pill.
  // The chip says what "this" was, e.g. “Quarterly revenue…”, report.pdf, 2 files.
  function notify(text: string, chip?: Chip) {
    pushToast(text, 2600);
    pushActivity('info', snippet(text, 44), undefined, chip ? 4200 : 3200, chip);
  }

  // Voice commands route into the same entry points as clicks and keys (see voiceRouter.ts).
  const route = createVoiceRouter({
    demo,
    isToolId,
    openTool,
    pomodoroStart,
    timerAdd,
    timerControl: (action, target) => timerControl(action, target),
    addNote,
    addToShelf,
    requestTransfer,
    notify,
    collapse: () => void requestCollapse(),
    addCalendarEvent: (ev) => bridge.addCalendarEvent(ev),
    calendarChanged: () => calendarTick.update((n) => n + 1),
    media: () => get(media),
    mediaTransport: (cmd) => void mediaTransport(cmd),
  });
  const routeVoice = (c: Parameters<typeof route>[0], r: Parameters<typeof route>[1]) => {
    markActivity();
    void route(c, r);
  };

  // Browser demo has no Rust side: a finished dictation becomes a note here.
  // (Natively Rust creates it and notes-changed refreshes the Notes view.)
  function onResult(r: VoiceResult) {
    showFeedbackOnly(r, notify); // e.g. "I didn't catch that" when nothing parsed to a command
    // Voice v3: search hits, a folder listing or ambiguous candidates need room, so open the panel
    // and show them as rows (Rust has already done the command; rows only offer the next step)
    const plan = planResults(r);
    if (plan) {
      showVoiceResults(plan.feedback, plan.rows);
      emitIntent('expand');
    } else voiceResults.set(null); // a newer answer replaces the old rows
    if (demo && r.mode === 'note' && r.text.trim()) {
      void addNote(r.text).then((ok) => {
        if (ok) pushToast('Note saved from dictation');
      });
    } else if (r.mode === 'note' && r.text.trim()) {
      pushToast('Note saved from dictation');
    }
  }

  onMount(() => {
    startClock();
    startLive();
    // Quick Transfer's events and toasts work even while its view is closed
    setToaster(pushToast);
    void startTransfer();
    // a reminder ringing (reminder-fired): a banner in the panel and a line on the pill, on top of
    // the real Windows toast the Rust scheduler shows (covers Focus Assist / Do Not Disturb)
    void startReminders({
      toast: pushToast,
      activity: (text) => pushActivity('info', snippet(text, 44), undefined, 5200),
      refresh: () => calendarTick.update((n) => n + 1),
    });
    const offVoice = onVoiceCommand(routeVoice);
    const offResult = onVoiceResult(onResult);
    // surface voice failures instead of leaving the user wondering
    let lastState = '';
    const offVState = voiceState.subscribe((s) => {
      if (s.state !== lastState) {
        if (s.state === 'error' || s.state === 'unavailable') pushToast(s.error ?? 'Voice input is not available.', 3200);
        lastState = s.state;
      }
    });

    // tray "Settings..." arrives as shell-open-tool {tool} after the shell expanded the window
    const offTool = onOpenTool((tool) => {
      if (isToolId(tool)) openTool(tool);
    });

    let embedTimer: ReturnType<typeof setTimeout> | null = null;
    if (demo && isEmbed) embedTimer = setTimeout(() => emitIntent('expand'), 700);

    return () => {
      stopTheme();
      offVoice();
      offResult();
      offVState();
      offTool();
      if (embedTimer) clearTimeout(embedTimer);
    };
  });

  function onKey(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    const open = get(shellMode) === 'panel';

    if (mod && key === 'k') {
      e.preventDefault();
      if (!open) {
        emitIntent('expand');
        paletteOpen.set(true);
      } else paletteOpen.update((v) => !v);
    } else if (mod && key === 'q') {
      e.preventDefault();
      void quit();
    } else if (e.key === 'Escape') {
      if (get(paletteOpen)) paletteOpen.set(false);
      else if (closeVoiceSheets()) return;
      else emitIntent('collapse');
    } else if (e.altKey && /^[1-9]$/.test(e.key) && open) {
      // Alt+1..9 jumps to the first nine tools
      e.preventDefault();
      activeTool.set(TOOL_ORDER[Number(e.key) - 1]);
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if demo && !isEmbed}
  <Backdrop />
{/if}
<Island />
