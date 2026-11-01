// Minimal Island UI used ONLY for shell QA: it consumes the real src/lib/shell.ts API
// the way the design agent's Island does (shellMode store, onShellRequest, requestExpand,
// commitMode, setHitRect, quit, hold), so the Rust<->JS handshake is exercised end to end
// independent of the in-flux production frontend.
import { shellMode, onShellRequest, onOpenTool, requestExpand, requestCollapse, commitMode, setHitRect, quit, hold } from '../../../src/lib/shell';

const island = document.getElementById('island')!;
const logEl = document.getElementById('log')!;
const lines: string[] = [];
function log(s: string) {
  lines.unshift(`${new Date().toISOString().slice(14, 23)} ${s}`);
  logEl.textContent = lines.slice(0, 12).join('\n');
}
(window as any).__qaLog = () => lines.slice();

function report() {
  const r = island.getBoundingClientRect();
  void setHitRect({ x: r.left, y: r.top, w: r.width, h: r.height });
}
new ResizeObserver(report).observe(island);

shellMode.subscribe((m) => {
  island.classList.toggle('panel', m === 'panel');
  log(`mode=${m}`);
  report();
});

onShellRequest(async (a) => {
  log(`request ${a}`);
  if (a === 'expand') await requestExpand();
  else if (a === 'collapse') {
    await new Promise((r) => setTimeout(r, 420)); // the panel->pill morph
    await commitMode('pill');
  }
});

onOpenTool((t) => log(`open-tool ${t}`));

// shell-cursor capture (QA): timestamped payloads readable via CDP as window.__cursor
const cursorEvents: { t: number; dx: number; dy: number; dist: number }[] = [];
(window as any).__cursor = cursorEvents;
void import('@tauri-apps/api/event').then((ev) =>
  ev.listen<{ dx: number; dy: number; dist: number }>('shell-cursor', (e) => {
    cursorEvents.push({ t: performance.now(), ...e.payload });
    if (cursorEvents.length > 5000) cursorEvents.shift();
  }),
);

island.addEventListener('click', (e) => {
  const t = e.target as HTMLElement;
  if (t.id === 'q') return void quit();
  if (t.id === 'c') return void requestCollapse();
  if (t.id === 'h') return void hold(true);
  if (t.id === 'hx') return void hold(false);
  if (!island.classList.contains('panel')) void requestExpand();
});
window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') void requestCollapse();
  if (e.ctrlKey && e.key.toLowerCase() === 'q') void quit();
});
