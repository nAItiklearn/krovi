# Spike: "point anywhere, Krovi shows results"

_Owner: ui-polish. Written 2026-11-01. SPEC §11.12. Nothing in this document is built into the app._

## Verdict

**PARTIALLY FEASIBLE.**

- A **scoped v1 is FEASIBLE** and cheap: on an explicit trigger, read the item under the cursor
  (text, an Explorer file, the window) through Windows' own accessibility and shell APIs, with no
  admin rights, no injection into other processes, and no network. Measured end to end it takes
  roughly **15-45 ms** per probe once warm.
- **"Anywhere" is not literally achievable.** Four classes of target stay opaque: elevated windows,
  games and other pixel-rendered UIs, editors that do not publish their text (VS Code's Monaco, by
  default), and any app whose accessibility tree is off. Krovi must say so on the card instead of
  guessing. A clipboard-copy fallback recovers some of the text cases, at a cost described below.

Prototype (outside the repo, standalone): `D:\krovi-scratch\point-spike\` (Rust, `windows` 0.58,
about 600 lines, builds with the WinLibs recipe in `docs/CONTRACTS.md` §0 and its own
`--target-dir`). Test drivers: `test-notepad.ps1`, `test-explorer.ps1`, `test-chrome.ps1`,
`test-vscode.ps1`. The spike never moves the user's mouse: it probes explicit coordinates
(`--at X,Y`), a countdown (`--wait`), or a hotkey (`--hotkey`, Ctrl+Alt+Q).

## What was proven, API by API

| Capability | API | Result on this machine (Windows 11 26300, no admin) |
|---|---|---|
| Element under cursor | `IUIAutomation::ElementFromPoint` | Works. Returns name, control type, class, automation id, framework id, bounding rect, owning pid. |
| Word under cursor | `TextPattern.RangeFromPoint` → `ExpandToEnclosingUnit(Word)` → `GetText` | Works in Notepad ("in"), Chrome ("brown", "1,250.00"). Not available in Explorer cells or VS Code's editor. |
| Selected text | `GetFocusedElement` → `TextPattern.GetSelection` | Works in Notepad (132 chars) and Chrome (134 chars). Returns 0 chars in VS Code. |
| Edit-field value | `ValuePattern.CurrentValue` | Works (Notepad, Chrome textbox, Explorer cells). **Skipped when `IsPassword`.** |
| Explorer selection | Shell COM: `IShellWindows` → `IServiceProvider` → `IShellBrowser` → `QueryActiveShellView` → `IFolderView2::GetSelection` → `IShellItemArray` | Works. Full path, size, type name ("PNG File"), modified time. Picks the right Win11 tab by matching the visible `ShellTabWindowClass`. |
| Explorer **hovered** item | UIA: climb from the cell to its row (`ListItem`), then match its display name against `IFolderView2` items | Works (hovered file resolved to a path, not just the selected one). O(items); capped at 5,000. |
| Foreground / under-cursor window | `GetForegroundWindow`, `WindowFromPoint` + `GetAncestor(GA_ROOT)`, `QueryFullProcessImageNameW` | Works; this is what `krovi-sys::foreground` already does for screen time. |
| Elevation detection | `OpenProcessToken(TOKEN_QUERY)` + `TokenElevation` | Works for same-user processes, so Krovi can say "this window runs as administrator" instead of failing silently. |
| Copy fallback | `SendInput(Ctrl+C)` → poll `GetClipboardSequenceNumber` → read → restore | Works: Notepad 44 ms, VS Code 47 ms (137 chars, text restored). Guarded: refuses if the foreground is not the window under the cursor, and refuses consoles/terminals. |

### Measured latency (single process, `Instant`, release build)

`ElementFromPoint` dominates; everything else is a few milliseconds. "Cold" is the first call from a
freshly started UIA client; Krovi would keep one `IUIAutomation` alive, so cold is paid once per
target app, not per probe. Ranges are from two to three runs each.

| Target | ElementFromPoint cold | ElementFromPoint warm (median / p95) | 8 property reads | Text or value pattern | Selection read | Notes |
|---|---|---|---|---|---|---|
| Notepad (Win11, RichEdit D2D) | 33-85 ms | 4.7-10.7 ms / 6-34 ms | 2-3 ms | 2-3 ms | 5-16 ms | Full text exposed. Copy fallback 44 ms. |
| Explorer (DirectUI + XAML tabs) | 8-12 ms | 5-10 ms / 9-14 ms | 3-4 ms | 13-19 ms (cell values) | Shell COM selection 26-43 ms | Hovered-item path resolve 32 ms with 4 items. |
| Chrome (Chromium) | **112-295 ms, and only the top-level pane (no web content)** | 11-17 ms / 20-45 ms | 4-5 ms | 4 ms | 14-29 ms | Needs a first UIA contact to switch accessibility on; see "limits". |
| VS Code (Electron + Monaco) | 50-273 ms | 7-13 ms / 11-43 ms | 3-4 ms | 5 ms | 7-10 ms but **0 chars** | Editor text not published; copy fallback 47 ms works. |

Rule of thumb for the product: budget **<= 60 ms** for the card to appear on a warm app, and show a
"reading..." state for the first probe of a browser or Electron app (up to ~300 ms). Use a UIA cache
request to batch the property reads (not done in the spike) and a worker thread with a hard
timeout (see "design requirements").

## Limits, measured or documented

- **Elevated windows (UIPI).** Microsoft's UI Automation security note is explicit: a client
  without `UIAccess` "starts with medium IL and cannot access elevated process UI". `UIAccess`
  needs an Authenticode signature, an install under `Program Files`, and (for elevated UI) an
  admin-launched process, so it is out of scope for a free, unsigned, non-admin app. Krovi can
  detect it (above) and show an honest "can't read this window" card. _Not exercised here: it
  needs an admin launch, which the spike deliberately avoids._
- **Chromium and Electron.** Accessibility is off until an assistive technology touches the
  browser. The first probe returned only the top-level pane (Chrome, VS Code); after that, Chrome
  exposed the DOM (document, textbox, word, selection) and the field latency fell to 11-17 ms.
  Two consequences: (1) the first hover over a browser needs a retry or a spinner; (2) **Krovi
  switching accessibility on makes that browser build and maintain its accessibility tree for the
  rest of its session**, which has a CPU and memory cost in the target app. That is a real side
  effect and has to be disclosed in Settings, and only ever happens on an explicit trigger.
- **VS Code / Monaco.** Even warm, the editor publishes "The editor is not accessible at this
  time" and no text. The clipboard-copy fallback is the only route (47 ms, 137 chars worked).
- **Password fields.** Native/Win32 fields set `IsPassword`; the spike refuses to read them.
  Chromium is subtler: hovering a password `<input>` returned the **document** element with
  `IsPassword=false`, while `RangeFromPoint` returned the masked bullets, never the secret. So the
  rule must be layered: never read when `IsPassword` is true; never keep a "word" made only of
  `•`/`*`; never use the copy fallback when the focused element is a password field.
- **Games, DirectX/OpenGL full-screen, Qt/Flutter/Skia canvases without semantics, emulators and
  remote-desktop viewers.** The tree stops at the window. The Android emulator on this machine
  (`qemu-system`, Qt) returned only the window. Krovi already hides in full-screen apps
  (`hideInFullscreen`); the card should do the same.
- **Terminals.** `Ctrl+C` in a console is SIGINT, not copy. The spike refuses
  `ConsoleWindowClass`, `CASCADIA_HOSTING_WINDOW_CLASS`, `mintty`. Windows Terminal does expose a
  UIA text pattern, so it does not need the fallback; plain conhost is skipped.
- **Wrong-app danger.** An early spike run, before the guard existed, sent Ctrl+C to the Android
  emulator because it was the foreground app. The final code only fires when the foreground root
  window equals the window under the cursor. This is a hard requirement for the product.
- **Not tested:** hung target apps (UIA calls cross processes and can block, so the product needs a
  worker thread and a timeout), multi-monitor mixed DPI (the spike sets Per-Monitor-V2 awareness,
  coordinates matched on one display), Edge, Office, Teams and other Electron apps (expected to
  behave like Chrome), Java apps (use Java Access Bridge, not UIA), the `--hotkey` mode.

## The clipboard-copy fallback and its downsides

`SendInput` Ctrl+C, wait for the clipboard sequence number to change (16-22 ms measured), read,
restore. It is the only way to get text out of apps like VS Code, but:

1. **It writes to the user's clipboard.** Text is restored in the spike; **images, files and rich
   formats are not** (a faithful restore means enumerating and copying every `HGLOBAL`-backed
   format, and delay-rendered formats cannot be copied). In the spike's Notepad run there was no
   text on the clipboard to restore (it held nothing, or non-text data), and a non-text payload
   would have been lost.
2. **It pollutes every clipboard manager**, Krovi's own history included, unless the sequence
   number is gated and the restore uses the "exclude from monitor processing" format.
3. **It needs the target to be the foreground app** and fails silently against elevated windows
   (UIPI also blocks `SendInput`).
4. **Wrong semantics in some apps** (terminals), and a user who is still holding a modifier key
   turns Ctrl+C into something else.

Recommendation: ship v1 on UIA + Shell COM only. Make the copy fallback an off-by-default
"Also try Copy for apps that hide their text" setting, text-only, with the foreground guard.

## Windows 11 "Click to Do", for comparison

Click to Do is Microsoft's own cursor-as-context feature. To my knowledge (I could not retrieve the
current support page, so treat these as unverified): it runs on **Copilot+ PCs only** (the developer
guide requires an NPU of 40+ TOPS), works from a snapshot of the screen using on-device text and
image recognition, is invoked with Win+click or Win+Q, offers actions such as copy text, search and
AI rewrite/summarise, and exposes **no public API** for third parties. Krovi's approach differs on
purpose: it reads **structure** (UIA, Shell) instead of pixels, so it works on any Windows 10/11
PC, returns exact text with no OCR errors, takes no screenshot, and keeps everything in-process.
The price is the "opaque" classes above, which pixel-based tools can read. If that gap ever matters,
the bounded answer is the built-in `Windows.Media.Ocr` on a small captured region around the
cursor; it needs a screen capture and so belongs behind its own explicit opt-in, not in v1.

## Privacy: how it stays inside Krovi's stance

- **Explicit trigger only.** A global hotkey (Krovi already registers one) or a hold-to-peek key.
  No hooks, no event subscriptions, no polling when idle. "Hold" polls `GetCursorPos` at ~20 Hz
  only while the key is physically down.
- **Minimum data, in memory only.** Element name/type, at most the word under the cursor or the
  selection (capped, e.g. 4,000 chars), file path and metadata, and the app's exe name. **Window
  titles and URLs are not read**, matching the screen-time rule in SPEC C11. Nothing is stored
  unless the user picks an action that stores it (note, shelf, reminder).
- **No network, no capture API.** Pure local calls; no `Windows.Graphics.Capture` (so no capture
  border or consent prompt), no OCR in v1.
- **Never for secrets:** skip `IsPassword`, masked text, known password-manager processes, and the
  copy fallback on password fields.
- **Add to `docs/SECURITY-network-audit.md`:** "Point/peek performs zero network calls" and the
  disclosure that probing a browser or Electron app can switch on its accessibility tree.

## Proposed scoped v1: "What's this?" peek

**Trigger.** A rebindable shortcut (default `Ctrl+Alt+/`); optionally hold-to-peek (the card follows
the cursor while the keys are down, releases on key-up). Krovi's main window is untouched; a small
second window (`peek`) shows the card above the cursor. It is `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`
and **never takes focus**, otherwise the selection it is trying to read would disappear.

**Card by what is under the cursor** (all actions local):

| Target | Shows | Quick actions |
|---|---|---|
| Text (selection, else the word under cursor) | characters / words / lines; detected entity (number, date, URL, email, amount) | Copy clean (trim, normalise whitespace and quotes, strip tracking params from URLs) · Add to note · Search clipboard history · If numeric: convert units/currency (existing `krovi-core` units and cached rates) · If a date: add to calendar · If a duration: start a timer |
| File in Explorer (hovered, else selected) | name, size, type, modified time | Send via Quick Transfer · Add to File Shelf · Copy path |
| Any other window | app name (exe → friendly name) and screen time today (existing screen-time aggregate) | Open Screen Time |
| Unreadable (elevated, game, password) | an honest reason | none |

**Shared with Voice v2 (SPEC 11.10).** "This"/"it" in a voice command resolves through the same
`capture_at()` function, so the "context capture reads only on explicit command, minimum data" rule
is implemented once.

### Who builds what (file ownership per `docs/CONTRACTS.md` §5)

| Piece | Owner | Files |
|---|---|---|
| `capture_at(point) -> PointContext` (UIA + Shell COM + elevation + password rules; worker thread; timeouts; UIA cache request) | live-data | `src-tauri/crates/krovi-sys/src/point.rs`, `krovi-sys/Cargo.toml` (features: `Win32_UI_Accessibility`, `Win32_UI_Shell`, `Win32_System_Com`, `Win32_System_Ole`, `Win32_System_Variant`, `Win32_Security`) |
| Pure classification and clean-copy logic (word/char counts, entity detection, URL cleaning), with tests | core-engine | `krovi-core` (new module, no OS calls) |
| Peek window, hotkey and hold mode, `peek_capture` / `peek_hide` commands, fullscreen/elevated gating | panel-shell | new `src-tauri/src/peek.rs`, `tauri.conf.json`, `capabilities/**`; `[peek]` handler block in `lib.rs` (orchestrator) |
| Card UI (reuses theme tokens), `src/lib/peek.ts` | frontend-design | `src/peek/**`, `src/lib/peek.ts` |
| Actions glue (add to note, shelf, transfer, reminder) | live-data + transfer | `commands.rs` helpers |
| Settings toggle + disclosure text; audit line | frontend-design; security-reviewer | `views/Settings.svelte`; `docs/SECURITY-network-audit.md` |
| Voice "this"/"it" reuse | voice | `krovi-voice`, `voice.rs` |
| Test matrix: Notepad, Explorer, Chrome/Edge, VS Code, Word, terminal, elevated, full-screen, mixed DPI, hung app | qa-tester | `qa/**` |

### Effort

| Slice | Estimate |
|---|---|
| `point.rs` productionised from the spike (STA/MTA threading, timeouts, cache request, password/terminal rules, fake-backed unit tests) | 1.5-2 days |
| Peek window + hotkey (+ hold mode) without stealing focus, DPI-correct placement | 1.5 days |
| Card UI, 4 card types, empty/unreadable states, reduced motion | 1-1.5 days |
| Actions, settings, audit and privacy copy | 1 day |
| QA matrix and a latency/idle-cost check recorded in `docs/PERF.md` | 1 day |
| **Total** | **about 6-7 working days; 4 without hold mode and without the copy fallback** |

**Risks to price in:** Chromium accessibility side effect (disclose it), the hung-app timeout, the
tab-matching logic for Explorer on future Windows builds, and keeping the copy fallback off by
default. **Not recommended for v1:** OCR, continuous hover, window titles, anything that stores
what was read.

## Reproducing

```powershell
$env:Path = "C:\Users\Lenovo\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;" + $env:Path
cd D:\krovi-scratch\point-spike
cargo build --release --target-dir D:\krovi-scratch\target-point
.\test-notepad.ps1; .\test-explorer.ps1; .\test-chrome.ps1; .\test-vscode.ps1   # each opens and closes its own window
```
