// Krovi clip recorder helpers (ui-polish, internal tooling; not shipped).
//
// Purpose: record short product clips of the browser demo (http://localhost:5173/) with
// chrome-devtools `screencast_start` / `screencast_stop`. Model round-trips between tool calls
// are real wall-clock time, so every clip is driven by ONE scripted call that:
//   1. flashes the viewport magenta (start marker), 2. plays the choreography with a visible
//   fake cursor, 3. flashes magenta again (end marker).
// `encode-clips.ps1` finds the two marker flashes in the raw capture and trims to the content.
//
// Usage (in the page):   inject this file once (evaluate_script format:"script"), then
//   () => window.__clips.run('pill-expand')
// Re-record every clip once the final UI lands; selectors are by aria-label so they survive restyles.
(() => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  const find = (label, root = document) => {
    const l = label.toLowerCase();
    return [...root.querySelectorAll('button,[role=tab],[role=button],[role=menuitem],a,input,textarea,[aria-label]')].find(
      (e) => (e.getAttribute('aria-label') || e.textContent || '').trim().toLowerCase().startsWith(l),
    );
  };

  // ---- fake cursor (programmatic clicks have no pointer; viewers need to see intent) ----
  let cur;
  const cursor = () => {
    if (cur && cur.isConnected) return cur;
    cur = document.createElement('div');
    cur.style.cssText =
      'position:fixed;left:0;top:0;width:22px;height:22px;z-index:2147483000;pointer-events:none;' +
      'transform:translate(640px,420px);transition:transform .62s cubic-bezier(.22,1,.36,1);' +
      'filter:drop-shadow(0 2px 3px rgba(0,0,0,.55))';
    cur.innerHTML =
      '<svg viewBox="0 0 24 24" width="22" height="22"><path d="M4 2.5v17l4.6-4.1 3.1 6.9 2.6-1.2-3.1-6.8H18z" fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/></svg>';
    document.body.appendChild(cur);
    return cur;
  };
  const moveTo = async (el, dx = 0, dy = 0) => {
    const r = el.getBoundingClientRect();
    cursor().style.transform = `translate(${Math.round(r.left + r.width / 2 + dx)}px,${Math.round(r.top + r.height / 2 + dy)}px)`;
    await sleep(680);
  };
  const press = async (el) => {
    const c = cursor();
    const base = c.style.transform;
    c.style.transition = 'transform .09s ease-out';
    c.style.transform = base + ' scale(.82)';
    await sleep(110);
    el.click();
    c.style.transform = base;
    await sleep(120);
    c.style.transition = 'transform .62s cubic-bezier(.22,1,.36,1)';
  };
  const tap = async (label, after = 700, dx = 0, dy = 0) => {
    const el = find(label);
    if (!el) throw new Error('clip: missing control "' + label + '"');
    await moveTo(el, dx, dy);
    await press(el);
    await sleep(after);
    return el;
  };
  const waitFor = async (fn, ms = 15000, step = 120) => {
    const t0 = performance.now();
    while (performance.now() - t0 < ms) {
      const v = fn();
      if (v) return v;
      await sleep(step);
    }
    return null;
  };

  // ---- sync markers ----
  const flash = async (ms = 260) => {
    const d = document.createElement('div');
    d.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#ff00ff;';
    document.body.appendChild(d);
    await sleep(ms);
    d.remove();
  };

  // ---- choreographies ----
  const clips = {
    // Idle pill -> springs open to the Home panel -> collapses back. Loop-safe (starts/ends as pill).
    'pill-expand': async () => {
      await sleep(900);
      await tap('Open Krovi', 3400);
      await tap('Close panel', 1500);
    },

    // Clipboard view; the demo simulates a copy roughly every 12 s -> a new row slides in.
    'clipboard-live': async () => {
      await tap('Open Krovi', 1500);
      await tap('Clipboard', 600);
      const rows = () => document.querySelectorAll('[role=tabpanel] li, [role=tabpanel] [role=listitem], main li').length;
      const n0 = rows();
      await waitFor(() => rows() !== n0, 15000);
      await sleep(2200);
    },

    // Settings -> Theme: cycles the six themes (token-only overrides), ends back on Graphite.
    'theme-switch': async () => {
      await tap('Open Krovi', 1300);
      await tap('Settings', 900);
      const g = find('Graphite');
      if (g) g.scrollIntoView({ block: 'center', behavior: 'smooth' });
      await sleep(800);
      for (const t of ['Porcelain', 'Mochi', 'Arcade', 'Terracotta', 'Tide', 'Graphite']) {
        await tap(t, 1100);
      }
      await sleep(300);
    },

    // Collapsed pill left alone: after ~5.5 s it becomes the eyes (look around, blink); a hover wakes it.
    'idle-eyes': async () => {
      await sleep(10500);
      const pill = find('Open Krovi');
      if (pill) {
        await moveTo(pill, 140, 120);
        for (const type of ['pointerenter', 'pointermove', 'mouseenter', 'mouseover']) {
          pill.dispatchEvent(new PointerEvent(type, { bubbles: true, pointerType: 'mouse' }));
        }
      }
      await sleep(1800);
    },

    // Notes view, mic button: the demo simulates a dictation (partial words stream in, note is created).
    'voice-note': async () => {
      await tap('Open Krovi', 1300);
      await tap('Notes', 900);
      await tap('Start voice', 6500);
      await sleep(900);
    },
  };

  // Every clip starts from the collapsed pill so loops are seamless.
  const ensurePill = async () => {
    const c = find('Close panel');
    if (c) {
      const home = [...document.querySelectorAll('[role=tab]')].find((e) => (e.getAttribute('aria-label') || e.textContent || '').trim() === 'Home');
      if (home) home.click(); // the app remembers the last tool; every clip should start from Home
      await sleep(350);
      c.click();
      await sleep(1500);
    }
  };

  const run = async (name) => {
    const clip = clips[name];
    if (!clip) throw new Error('unknown clip ' + name + ' (have: ' + Object.keys(clips).join(', ') + ')');
    await ensurePill();
    cursor();
    await sleep(250);
    await flash();
    await sleep(450);
    await clip();
    await sleep(450);
    await flash();
    await sleep(150);
    return 'done ' + name;
  };

  window.__clips = { run, clips, find, tap, waitFor, sleep, flash };
})();
