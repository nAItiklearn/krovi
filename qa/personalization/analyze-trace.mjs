// Summarise a Chrome DevTools trace for the idle-companion cost comparison.
//   node qa/personalization/analyze-trace.mjs <trace.json> [<trace.json> ...]
// Reports, for the page's renderer main thread over the recorded window:
//   main-thread busy % (sum of top-level RunTask durations), task wakeups/s, TimerFire/s,
//   frames produced (compositor DrawFrame + BeginMainThreadFrame), and the JS heap range.
import { readFileSync } from 'node:fs';

function load(path) {
  const j = JSON.parse(readFileSync(path, 'utf8'));
  return Array.isArray(j) ? j : j.traceEvents;
}

function summarise(path) {
  const ev = load(path);
  // renderer main threads
  const mains = ev.filter((e) => e.ph === 'M' && e.name === 'thread_name' && e.args?.name === 'CrRendererMain');
  const score = new Map();
  for (const e of ev) {
    if (e.name === 'RunTask' && mains.some((m) => m.pid === e.pid && m.tid === e.tid)) {
      score.set(`${e.pid}:${e.tid}`, (score.get(`${e.pid}:${e.tid}`) ?? 0) + 1);
    }
  }
  // the page under test is the main thread that ran our scripts
  let best = null;
  for (const m of mains) {
    const key = `${m.pid}:${m.tid}`;
    const ours = ev.some(
      (e) => e.pid === m.pid && e.tid === m.tid && JSON.stringify(e.args ?? {}).includes('preview.html'),
    );
    if (ours && (!best || (score.get(key) ?? 0) > (score.get(best) ?? 0))) best = key;
  }
  if (!best) best = [...score.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
  const [pid, tid] = best.split(':').map(Number);

  const tasks = ev.filter((e) => e.ph === 'X' && e.name === 'RunTask' && e.pid === pid && e.tid === tid);
  const t0 = Math.min(...tasks.map((e) => e.ts));
  const t1 = Math.max(...tasks.map((e) => e.ts + e.dur));
  // trim 2s at each end (trace start/stop overhead and the harness' own evaluate call)
  const lo = t0 + 2_000_000;
  const hi = t1 - 2_000_000;
  const win = (hi - lo) / 1e6;
  const inWin = (e) => e.ts >= lo && e.ts <= hi;

  const busy = tasks.filter(inWin).reduce((a, e) => a + e.dur, 0) / 1e3; // ms
  const byName = (name, filter = () => true) => ev.filter((e) => e.name === name && inWin(e) && filter(e));
  const timers = byName('TimerFire', (e) => e.pid === pid).length;
  const mainFrames = byName('BeginMainThreadFrame', (e) => e.pid === pid).length;
  const drawFrames = byName('DrawFrame', (e) => e.pid === pid).length;
  const activate = byName('ActivateLayerTree', (e) => e.pid === pid).length;
  const paints = byName('Paint', (e) => e.pid === pid && e.tid === tid).length;
  const layouts = byName('UpdateLayoutTree', (e) => e.pid === pid && e.tid === tid).length;
  const fn = byName('FunctionCall', (e) => e.pid === pid && e.tid === tid);
  const fnMs = fn.reduce((a, e) => a + (e.dur ?? 0), 0) / 1e3;
  const heaps = ev
    .filter((e) => e.name === 'UpdateCounters' && e.pid === pid && inWin(e))
    .map((e) => e.args?.data?.jsHeapSizeUsed)
    .filter((v) => typeof v === 'number');
  const nodes = ev
    .filter((e) => e.name === 'UpdateCounters' && e.pid === pid && inWin(e))
    .map((e) => e.args?.data?.nodes)
    .filter((v) => typeof v === 'number');

  const mb = (b) => (b / 1048576).toFixed(2);
  return {
    file: path.split(/[\\/]/).pop(),
    windowSec: +win.toFixed(1),
    mainThreadBusyPct: +((busy / 1000 / win) * 100).toFixed(3),
    mainThreadBusyMs: +busy.toFixed(1),
    tasksPerSec: +(tasks.filter(inWin).length / win).toFixed(2),
    timerFiresPerSec: +(timers / win).toFixed(2),
    js_FunctionCallMs: +fnMs.toFixed(2),
    framesBeginMain: mainFrames,
    framesDraw: drawFrames,
    framesActivated: activate,
    fps_draw: +(drawFrames / win).toFixed(2),
    paintsPerSec: +(paints / win).toFixed(2),
    styleRecalcsPerSec: +(layouts / win).toFixed(2),
    heapMB: heaps.length ? { min: mb(Math.min(...heaps)), max: mb(Math.max(...heaps)), last: mb(heaps[heaps.length - 1]) } : 'n/a',
    domNodes: nodes.length ? { min: Math.min(...nodes), max: Math.max(...nodes) } : 'n/a',
  };
}

for (const p of process.argv.slice(2)) console.log(JSON.stringify(summarise(p), null, 2));
