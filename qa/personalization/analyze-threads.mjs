// Per-thread busy time of the renderer that hosts the preview page, from a DevTools trace.
//   node qa/personalization/analyze-threads.mjs <trace.json>
import { readFileSync } from 'node:fs';
const j = JSON.parse(readFileSync(process.argv[2], 'utf8'));
const ev = Array.isArray(j) ? j : j.traceEvents;

const names = new Map();
for (const e of ev) if (e.ph === 'M' && e.name === 'thread_name') names.set(`${e.pid}:${e.tid}`, e.args?.name);
const main = ev.filter((e) => e.ph === 'M' && e.name === 'thread_name' && e.args?.name === 'CrRendererMain');
// the renderer that ran our scripts
const pid = process.env.PID ? Number(process.env.PID) : (main.find((m) => ev.some((e) => e.pid === m.pid && e.tid === m.tid && JSON.stringify(e.args ?? {}).includes('preview.html'))) ?? main[0]).pid;

const mine = ev.filter((e) => e.pid === pid && e.ph === 'X' && e.dur != null);
const ts = mine.map((e) => e.ts);
const lo = Math.min(...ts) + 2e6;
const hi = Math.max(...ts) - 2e6;
const win = (hi - lo) / 1e6;

// top-level busy time per thread = union of RunTask-ish events; use RunTask where present
const byThread = new Map();
for (const e of mine) {
  if (e.ts < lo || e.ts > hi) continue;
  if (e.name !== 'RunTask' && e.name !== 'ThreadControllerImpl::RunTask' && e.name !== 'ThreadPool_RunTask') continue;
  const k = `${e.pid}:${e.tid}`;
  byThread.set(k, (byThread.get(k) ?? 0) + e.dur);
}
console.log(`renderer pid ${pid}, window ${win.toFixed(1)}s`);
for (const [k, us] of [...byThread.entries()].sort((a, b) => b[1] - a[1]).slice(0, 8)) {
  console.log(`${(names.get(k) ?? '?').padEnd(28)} tid=${k.split(':')[1].padEnd(7)} busy ${(us / 1e3).toFixed(1).padStart(8)} ms  = ${((us / 1e6 / win) * 100).toFixed(2)}%`);
}

// what the main thread did, by event name (self-evident leaves only)
const mainKey = `${pid}:${main.find((m) => m.pid === pid).tid}`;
const agg = new Map();
for (const e of mine) {
  if (`${e.pid}:${e.tid}` !== mainKey || e.ts < lo || e.ts > hi) continue;
  agg.set(e.name, (agg.get(e.name) ?? 0) + e.dur);
}
console.log('\nmain thread, top events by total duration (nested events overlap):');
for (const [n, us] of [...agg.entries()].sort((a, b) => b[1] - a[1]).slice(0, 14)) console.log(`  ${n.padEnd(34)} ${(us / 1e3).toFixed(1).padStart(8)} ms`);
const count = (n) => ev.filter((e) => e.pid === pid && e.name === n && e.ts >= lo && e.ts <= hi).length;
console.log(`\nper second: DrawFrame ${(count('DrawFrame') / win).toFixed(1)}  BeginMainThreadFrame ${(count('BeginMainThreadFrame') / win).toFixed(1)}  Paint ${(count('Paint') / win).toFixed(1)}  UpdateLayoutTree ${(count('UpdateLayoutTree') / win).toFixed(1)}  TimerFire ${(count('TimerFire') / win).toFixed(2)}  Raster ${(count('RasterTask') / win).toFixed(1)}`);
