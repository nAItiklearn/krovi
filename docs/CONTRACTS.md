# Krovi v0.2 — Integration Contracts

Single source of truth for every seam between agents. **Do not rename anything here
without updating this file.** If you need a new command/event, add it to your own
module and document it in your section below (append-only).

## 0. Build environment (Windows host)

The host has two gcc's; the 32-bit `C:\MinGW\bin` shadows the good one. **Every native
build must prepend WinLibs 64-bit MinGW to PATH**:

```powershell
$env:Path = "C:\Users\Lenovo\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;" + $env:Path
cd D:\krovi\src-tauri; cargo build          # default features = real integrations
cd D:\krovi; npx tauri build                # NSIS installer -> src-tauri/target/release/bundle/nsis
```

Default cargo features include the real integrations (`net`, `native`). A Tauri build
that silently returns fake data is a bug — there are **no** hardcoded fixtures on the
Tauri path. Fixtures exist only for the browser demo (`mode() === 'demo'`).

Cargo shares one target dir; if you see "Blocking waiting for file lock", another agent
is building — wait, don't delete locks.

## 1. Window model — "Island" (owner: panel-shell)

One Tauri window, label `main`: transparent, undecorated, always-on-top, skipTaskbar,
non-resizable, no native shadow. It is a transparent *canvas*; the visible shape
(pill or panel) is drawn by the frontend.

Both window sizes share one **anchor: top-centre of the primary monitor's work area,
`ISLAND_TOP = 6` logical px below the top edge**. Because the anchor is shared, the
pill never moves on screen when the window grows/shrinks.

| mode    | window (logical px)   | visible element inside window                  |
|---------|-----------------------|------------------------------------------------|
| `pill`  | `PILL_WIN  = 440×88`  | pill (≤ 400×44), centred horizontally, `top: 10px` |
| `panel` | `PANEL_WIN = 960×690` | panel `PANEL = 900×620`, centred, `top: 10px`  |
| `hidden`| window hidden         | — (tray + shortcut still work)                 |

Extra window area is shadow/hover headroom. Transparent regions must render nothing
(`html, body { background: transparent }` in Tauri mode). The constants live in
`src/lib/shell.ts` (`PILL_WIN`, `PANEL_WIN`, `PANEL`, `ISLAND_PAD = 10`) and must match
`shell.rs`.

### Transitions (no stuck states — ever)
- **Expand**: frontend calls `shell_set_mode("panel")` → Rust resizes to `PANEL_WIN`
  (same anchor), shows, focuses, emits `shell-mode {mode:"panel"}` → frontend plays the
  spring morph pill→panel.
- **Collapse**: frontend plays panel→pill morph (≈420ms) → then calls
  `shell_set_mode("pill")` → Rust resizes to `PILL_WIN`, emits `shell-mode`.
- **Rust-initiated** (global shortcut, tray click, focus lost, second instance launched):
  Rust emits `shell-request {action:"expand"|"collapse"|"toggle"}`; the frontend animates
  and then calls `shell_set_mode`. **Watchdog**: if the requested mode is not applied
  within 1500ms, Rust applies it itself and emits `shell-mode`. A frontend receiving
  `shell-mode` that disagrees with what it shows must snap to it.
- **Collapse triggers**: Esc (frontend), click outside / focus loss (Rust
  `WindowEvent::Focused(false)`), the shortcut again, the panel's close button.
  `shell_hold(true)` suppresses focus-loss collapse while a native dialog is open;
  `shell_hold(false)` restores it.
- **Quit** is always reachable: tray menu → "Quit Krovi", panel header → Quit,
  and `Ctrl+Q` while the panel is focused. Single-instance: launching Krovi again
  expands the existing panel instead of opening a second window.

### Tray (always present)
Left-click = toggle panel. Menu: `Open Krovi`, `Show pill` (checkable), `Settings…`,
separator, `Quit Krovi`.

### Shell commands / events
| command | args | returns |
|---|---|---|
| `shell_set_mode` | `mode: "pill"\|"panel"\|"hidden"` | `()` |
| `shell_hold` | `hold: bool` | `()` |
| `shell_quit` | — | never returns |
| `shell_get_config` | — | `ShellConfig` |
| `shell_set_config` | `config: ShellConfig` | `ShellConfig` (re-registers shortcut; errors if invalid) |

`ShellConfig = { shortcut: string /* "Ctrl+Alt+Space" */, showPill: boolean, collapseOnBlur: boolean, launchAtLogin: boolean }`

Events: `shell-mode {mode}`, `shell-request {action}`, `shell-open-tool {tool: ToolId}`
(tray "Settings…" emits `{tool:"settings"}` after expanding).

`src/lib/shell.ts` exports: `shellMode` (readable store: `'pill'|'panel'|'hidden'`),
`requestExpand()`, `requestCollapse()`, `commitMode(mode)` (called by the UI when its
animation finishes), `onShellRequest(cb)`, `quit()`, `hold(b)`, `getConfig()`,
`setConfig(c)`, the geometry constants, and `isIsland()` (true in Tauri). In the browser
demo the same API works without Tauri (state only).

## 2. Live data (owner: live-data)

A background "live hub" in Rust pushes events; the UI **never** polls `invoke` for these.

| event | payload | cadence |
|---|---|---|
| `clipboard-changed` | `{ entries: ClipEntry[], newId: number \| null }` | on every OS clipboard change (sequence-number poll ≤ 400ms) and on every mutation |
| `media-changed` | `MediaState \| null` | on change of track/status (poll ≤ 1000ms), plus every 5s while playing to resync position |
| `stats-tick` | `SystemStats` | every 2000ms |

```ts
interface MediaState {
  title: string; artist: string; album: string;
  playing: boolean;
  positionSec: number; durationSec: number;
  /** epoch ms when positionSec was sampled — UI interpolates while playing */
  sampledAt: number;
  sourceApp: string;          // e.g. "Spotify.exe", "chrome", "msedge"
  artwork: string | null;     // data:image/...;base64 thumbnail, or null
}
interface SystemStats {
  memUsed: number; memTotal: number;        // bytes
  diskUsed: number; diskTotal: number;      // bytes, system drive
  diskLabel: string;                        // "C:"
  cpu: number;                              // 0..100, best effort
}
```

| command | args | returns |
|---|---|---|
| `live_snapshot` | — | `{ clipboard: ClipEntry[], media: MediaState\|null, stats: SystemStats }` |
| `clipboard_copy` | `id` | `ClipEntry[]` — writes entry back to the OS clipboard; must NOT create a duplicate |
| `toggle_clip_pin` / `remove_clip` | `id` | `ClipEntry[]` |
| `clipboard_clear` | — | `ClipEntry[]` (keeps pinned) |
| `media_transport` | `command: "playpause"\|"next"\|"previous"` | `()` |

Frontend: `src/lib/live.ts` exports Svelte stores `clips`, `media`, `stats`, and
`pulse` (a counter that increments on each *new* clipboard entry — the pill glows on
change), plus `startLive()` (idempotent; called once from `App.svelte`). In demo mode
`startLive()` feeds fixtures and simulates a copy every ~12s so the website demo feels alive.

Privacy: entries flagged `ExcludeClipboardContentFromMonitorProcessing` (password
managers) are never stored. All data persists locally (SQLite in the app data dir).

## 3. Voice (owner: voice)

| command | args | returns |
|---|---|---|
| `voice_available` | — | `{ dictation: boolean, commands: boolean, engine: string }` |
| `voice_start` | `mode: "note"\|"command"` | `()` |
| `voice_stop` | — | `()` |
| `voice_transcribe_file` | `path: string, mode` | `VoiceResult` (QA hook: feed a WAV instead of the mic) |

Events: `voice-state {state:"idle"|"listening"|"processing"|"error", mode, error?}`,
`voice-partial {text}`, `voice-result VoiceResult`.

`VoiceResult = { mode, text, command: VoiceCommand | null }`,
`VoiceCommand = { kind: "open-tool", tool: ToolId } | { kind: "start-pomodoro" } | { kind: "start-timer", seconds: number } | { kind: "new-note", body: string } | { kind: "collapse" }`.

Frontend: `src/lib/voice.ts` exports `voiceState` store, `startVoice(mode)`,
`stopVoice()`, and `onVoiceCommand(cb)`. For `mode:"note"` the Rust voice module itself
creates the note (through the shared notes state) and emits `voice-result`; the UI
refreshes notes on that event.

## 4. Holidays (owner: holidays)

`src/lib/holidays/index.ts` exports
`holidaysFor(year: number, region?: string /* default 'IN' */): Holiday[]` and `REGIONS`.
`Holiday = { date: 'yyyy-mm-dd', name: string, type: 'gazetted'|'restricted'|'observance', region: string }`.
Bundled, offline dataset with a documented source per year — no network call.

## 5. File ownership (strict)

| agent | owns (may create/edit) |
|---|---|
| orchestrator | `docs/CONTRACTS.md`, `src-tauri/src/lib.rs` (except marked handler blocks), `src-tauri/Cargo.toml`, `package.json`, `vite.config.ts` |
| live-data | `src-tauri/src/{live,commands,state}.rs`, `src-tauri/crates/{krovi-core,krovi-store,krovi-net,krovi-sys}/**`, `src/lib/{bridge,live,types,fixtures}.ts`, `[live]` handler block in lib.rs |
| panel-shell | `src-tauri/src/shell.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/**`, `src-tauri/icons/**`, `src/lib/shell.ts`, `[shell]` handler block |
| voice | `src-tauri/crates/krovi-voice/**`, `src-tauri/src/voice.rs`, `src/lib/voice.ts`, `[voice]` handler block |
| frontend-design | `index.html`, `src/main.ts`, `src/App.svelte`, `src/components/**`, `src/views/**`, `src/styles/**`, `src/lib/{stores,tools,icons,motion}.ts` |
| holidays | `src/lib/holidays/**` |
| web | `web/**` (except `web/demo/`, which is build output), `scripts/build-web.*` |

Need a change in a file you don't own? Don't edit it — describe the exact change in
your final report and the orchestrator will apply it. Shared docs `docs/DECISIONS.md`
and `docs/PROGRESS.md`: **append** a section only; re-read immediately before editing.
