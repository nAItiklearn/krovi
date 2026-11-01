# Krovi — measured performance (real install)

Measured by the orchestrator on 2026-11-01 against the **installed** NSIS build
(`Krovi_0.1.0_x64-setup.exe`, v0.2 "Island" code), Windows 11, 1920×1080 @125%, 14 GB RAM.
Method is identical to Windy's PERF.md: private working set summed over the app process
**and every WebView2 child process it owns** (process tree by ParentProcessId), read from
`Win32_PerfFormattedData_PerfProc_Process`, after 75 s idle in pill mode (idle companion active).

## Idle RAM

| Process | Private working set |
|---|---|
| `Krovi.exe` (Rust: shell, clipboard/media/stats watchers, SQLite, voice, transfer) | **6.3 MB** |
| `msedgewebview2.exe` × 6 (50.8, 39.0, 27.9, 6.7, 3.1, 1.9) | 129.4 MB |
| **Total (3 samples, 10 s apart)** | **134.4 / 136.4 / 135.6 MB** |

Total working set including shared pages: ~433 MB.

**Verdict, stated plainly:** ~136 MB private. That is **higher than Windy's 103.4 MB and above
Windy's amended 120 MB budget**. ~95% of it is the WebView2 runtime that keeps the always-visible
pill rendered; the Rust process itself is 6.3 MB. Krovi's webview hosts far more UI (15 tools,
themes, idle companion) than Windy's popup. Options if this must come down: render the pill
natively (no webview while collapsed) and create the panel webview on demand — a real
architectural change, not done in v0.2.

## Idle CPU
`Krovi.exe` + WebView2 children: **0%** in each idle sample. Idle companion: ~0.7–1.2% of one
core while the eyes are awake, 0% once they doze (personalization agent, D-personalization).
Shell cursor sampler: 0.2–0.8% of one core (panel-shell phase D).

## Latency (real install)
| Path | Measured |
|---|---|
| Ctrl+Alt+Space → panel expanded | 46–69 ms (shell agent) |
| OS clipboard copy → shown in open panel | < 0.9 s (orchestrator; 155–237 ms at the watcher) |
| Media track change → shown in open panel | < 1.2 s (orchestrator; 68–488 ms at the worker) |
| Tray → Quit → process gone | ~199 ms (shell agent) |
| Quick Transfer 5 MB over LAN IP (curl) | 0.027 s |
