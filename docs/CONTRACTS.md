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

| `shell_set_hit_rect` | `rect: {x,y,w,h}` (logical px, window-relative) | `()` |

`ShellConfig = { shortcut: string /* "Ctrl+Alt+Space" */, showPill: boolean, collapseOnBlur: boolean, launchAtLogin: boolean, hideInFullscreen: boolean }`

**Click-through (critical on Windows).** Transparent window pixels still swallow clicks,
so the invisible headroom around the pill would block e.g. the middle of a maximized
browser's tab strip. The UI reports the *visible* shape's bounding rect via
`shell_set_hit_rect` whenever it changes (ResizeObserver; also during/after morphs).
Rust samples the cursor (~30 Hz) and toggles `set_ignore_cursor_events` so clicks
outside that rect pass through to whatever is underneath. In `panel` mode, a click
outside the panel therefore lands on the other app → focus loss → collapse.

**Additions (panel-shell, as built):** `shell_get_mode → mode`; `shell_set_mode` returns
the mode actually applied (`pill`→`hidden` when the pill is off); Rust sends resolved
`expand`/`collapse` in `shell-request` (never `toggle`); `shell.ts` also exports
`onOpenTool(cb)` and `HitRect`; `Krovi.exe --quit` closes a running instance; the
bundled exe is `Krovi.exe`; capabilities are `core:default` only (Rust does all window
mutation). Alt+F4 collapses, never kills.

**Fullscreen**: when `hideInFullscreen` and the foreground window covers its whole
monitor (game, video, presentation), the pill hides; it returns when that ends.

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

Other data events (live-data): `notes-changed` (payload `Note[]`) is emitted by
`commands::create_note_from` (used by voice) — the Notes view must listen for it.

### Additional live-data commands (append-only, live-data)

| command | args | returns |
|---|---|---|
| `load_media` | — | `MediaState \| null` (latest hub sample, position advanced to now) |
| `weather_status` | `force?: bool` | `WeatherReply` — `{status:"ok"\|"offline"\|"needs-city", weather, stale, fetchedAt, place, error}` |
| `load_weather` | — | `WeatherData \| null` |
| `weather_search` | `query` | `{name,lat,lon}[]` (keyless geocode of the typed text) |
| `weather_set_place` | `name, lat, lon` | `WeatherReply` |
| `weather_set_city` | `query` | `WeatherReply` (best geocode match) |
| `load_currency_rates` | — | `{[code]: perUSD}` (empty offline with nothing cached) |
| `load_currency_info` | `force?: bool` | `{rates, base, date, updatedAt, stale, error}` |
| `load_calendar` / `add_calendar_event` / `remove_calendar_event` | — / `date,title,time?` / `id` | `{id,date,title,time?}[]` (chronological; persisted) |

Weather and FX are real keyless fetches cached in the store (weather 20 min, FX 6 h; an
out-of-date cache is served flagged `stale` when offline). `needs-city` means Windows
Location was unavailable: the UI asks for a city. Outbound hosts (audit): `api.open-meteo.com`
(coarse lat/long), `geocoding-api.open-meteo.com` (the typed city text only),
`api.frankfurter.dev` (base currency only). Persisted: clipboard (cap 200 unpinned),
notes, tasks, calendar, screen time, shelves, weather place + caches → `krovi.db` in the
app data dir; full-resolution copies of clipboard images are kept as `clipimg/<hash>.png`
beside it. Bridge additions (`src/lib/bridge.ts`): `loadWeatherStatus`, `searchPlaces`,
`setWeatherPlace`, `setWeatherCity`, `loadMediaState`, `loadCurrencyInfo`,
`addCalendarEvent`, `removeCalendarEvent`, `onNotesChanged`, `listen`, `invoke`;
`loadWeather`/`loadMedia` now resolve `null` when there is nothing to show.

### Browser demo / website embed
In the browser (`mode()==='demo'`) the app renders a desktop-like backdrop with the
pill at top-centre; clicking expands exactly like the native app. With `?embed=1` the
page background is transparent (the website supplies the wallpaper/mockup) and the
panel auto-expands ~700ms after load. The web build is `vite build --base=/demo/
--outDir web/demo` and is served at `/demo/` on the website.

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

### Voice v2 (as built — see DECISIONS D-013)
`voice_start({mode, captureContext?})`, `voice_get_settings`, `voice_set_settings({settings})`.
`VoiceResult = {mode, text, command, context, feedback, executed}`;
`VoiceContext = {kind:'text',value,app?} | {kind:'files',value:string[],app?} | {kind:'app',value}`.
New `VoiceCommand` kinds: `add-reminder{date,time?,title}` (executed in Rust),
`shelf-add{paths,text?}`, `transfer-send{paths,text?}`, `media{command}` (executed in Rust),
`timer-control{action,target}`. `VoiceSettings = {localOnly:true, storeAudio:false (both locked),
keepTranscripts, contextCapture:'off'|'selection'|'selection+clipboard-fallback', cleanTranscription}`.
New event `calendar-changed` (`CalendarDto[]`) from `commands::add_calendar_event_from`.

## 4. Holidays (owner: holidays)

`src/lib/holidays/index.ts` exports
`holidaysFor(year: number, region?: string /* default 'IN' */): Holiday[]` and `REGIONS`.
`Holiday = { date: 'yyyy-mm-dd', name: string, type: 'gazetted'|'restricted'|'observance', region: string }`.
Bundled, offline dataset with a documented source per year — no network call.
Regions: `IN` (DoPT central-govt), `US` (OPM federal), `GB` (England & Wales),
`GB-SCT`, `GB-NIR` (GOV.UK bank holidays); years 2025–2027.
Extra exports: `nextHoliday(fromISO, region='IN', types=['gazetted']) → Holiday|null`,
`sourceFor(year, region) → source|null`, `availableYears(region)`. Unknown year/region → `[]`.

## 6. Quick Transfer (owner: transfer)

LAN-only file transfer desktop ⇄ phone. Drop a file → a QR appears → the phone scans
it and downloads over the same Wi-Fi; the same page can upload phone → desktop. No
cloud relay, no account. The local HTTP server exists **only while a share is open**,
binds to the active LAN interface (never 0.0.0.0 on public networks), serves only
unguessable token URLs, and auto-closes on expiry/stop/app quit.

| command | args | returns |
|---|---|---|
| `transfer_share` | `paths: string[]` | `Share` |
| `transfer_receive` | — | `Share` (an upload-only page; files land in `Downloads\Krovi`) |
| `transfer_stop` | `id?: string` | `()` |
| `transfer_status` | — | `{ active: Share[] }` |

`Share = { id, url, qrSvg, files: {name,size}[], mode: "send"|"receive", expiresAt, downloads, lanIp, port }`.
Events: `transfer-progress {id, file, bytes, total, direction}`, `transfer-done {id, file, direction, savedPath?}`,
`transfer-closed {id, reason: "expired"|"stopped"|"quit"}`.
Frontend: `src/lib/transfer.ts` + `src/views/Transfer.svelte` (ToolId `'transfer'`).

As built (additive): `transfer_pick_files() → string[]` (native dialog; wrapped in
`shell.hold`), `transfer_get_config()/transfer_set_config(config)` →
`{maxUploadBytes, ttlSecs, requirePin, receiveDir}`, `transfer_open_folder()`,
`transfer_reveal(path)`, `transfer_firewall_status() → {lanIp, adapter, ruleInstalled, profile, exe}`,
`transfer_firewall_allow()` (UAC; explicit click only). `transfer_share(paths, opts?)` /
`transfer_receive(opts?)` with `opts = {pin?, ttlSecs?}`. Share extra fields: `createdAt,
uploads, reached, pin|null, maxUploadBytes, receivedBytes, saveDir|null`. Events: direction is
`"send"|"receive"` (desktop's view); `transfer-done` has `size`; `transfer-closed.reason` may
be `"locked"`; new `transfer-reached {id}`, `transfer-failed {id, file, direction, message}`.
Boot: `App.svelte` calls `setToaster(pushToast); void startTransfer();` once.

## 7. Themes + idle companion (owner: personalization)

`src/lib/theme.ts`: `THEMES: {id, name, scheme:'dark'|'light', swatch:[bg,fg,accent]}[]`
(graphite, porcelain, mochi, arcade, terracotta, tide), `themeChoice` store
(`'system' | <id>`, persisted, default `'system'`), `resolvedTheme` readable, `startTheme()`
(idempotent; sets `data-theme=<id>` and `data-scheme=dark|light` on `<html>`, follows OS
when `'system'`). This **supersedes** `themePref/applyTheme` in `stores.ts`.
Themes are CSS files in `src/styles/themes/<id>.css` that only override the shared token
vocabulary defined by frontend-design in `src/styles/tokens.css` (+ `--eye-*` tokens).
UI code must never hardcode a colour — tokens only.

`src/lib/idle.ts`: `idleSettings` store (`{enabled, style, personality, delayMs: 5500}`,
persisted), `IDLE_STYLES`, `PERSONALITIES`, `isIdle` readable, `markActivity()` (the pill
calls it on hover/click/pulse/shortcut/mode change; any call wakes instantly).
`src/components/idle/IdleCompanion.svelte` — props `{ active: boolean }`; it renders the
eyes inside the pill's shape (fills the pill's content box), reads `shell-cursor` itself.
Event `shell-cursor {dx, dy, dist}` (logical px from pill centre; ≤10 Hz; only in pill
mode with the cursor within 400 px) — emitted by panel-shell. A final
`{dx:0,dy:0,dist:9999}` is sent once when the cursor leaves range or the panel/fullscreen
hides the pill; consumers recentre on `dist >= 9999`. Values are rounded integers; skip
threshold 2 px; silent while out of range.

## 8. Reminders & alarms (owner: reminders)

A reminder is a calendar event with a `time` (local wall-clock). A Rust background
scheduler (`src-tauri/src/reminders.rs`) fires a real Windows toast
(`tauri-plugin-notification`, from Rust — no webview permission) plus an optional sound
AT that time, whatever the window mode (pill, panel, hidden). Each fire is persisted
(`firedAt`) so it never double-fires; reminders missed while Krovi was closed fire once
on next start, marked "missed". Snooze 5/10 min. Event `reminder-fired {id, title, date, time, missed}`.
Voice path: voice parses the phrase → `commands::add_calendar_event_from` (same store) →
`voice-result.feedback` echoes the exact resolved time ("Reminder set for today 5:00 PM: …").

### Reminders — as built (append-only, reminders; see DECISIONS D-reminders)

**Data.** `CalendarEvent` / `CalendarDto` gained `remind` (alert at `time`; default **true** when a time
is set, never for all-day), `sound` (default true), `firedAt?` and `snoozedUntil?` (epoch ms) and
`missed` — camelCase on the wire. SQLite schema **v3** adds the columns in place (existing rows kept;
timed rows become reminders, all-day rows do not). A reminder whose time already passed (> 90 s) is
stored with `remind:false` (it could never ring).

**Scheduler** (`src-tauri/src/reminders.rs`, thread `krovi-reminders`): sleeps on a condvar for
`min(time to next alert, 30 s)`, wakes at once on `reminders::wake_scheduler(&AppHandle)` (called by
add/remove/snooze/dismiss). Due times = local wall clock + the OS zone (DST-correct, tested in
`krovi_core::calendar`). When due: persist `firedAt` **first**, then toast
(`tauri-plugin-notification`) + sound (`PlaySoundW` "Notification.Reminder") + `reminder-fired`. Missed
while closed: fires once with `missed:true`; > 24 h overdue is only marked; 4+ at once share one summary
toast. Sound is skipped while Do Not Disturb / alarms-only is on (the toast then waits in the
notification centre; the in-app banner still shows).

**Commands:** `reminders_status → {now, nowLocal, utcOffsetMinutes, pending[], next, scheduler{…lastFire},
environment{toastIdentity, appUserModelId, toasts, focus, toastMayBeHidden}, persistenceError}`,
`reminder_add(date,title,time?,remind?,sound?)`, `reminder_set(id,remind?,sound?)`,
`reminder_snooze(id, minutes: f64)` (5/10 in the UI; fractions for QA), `reminder_dismiss(id)`,
`reminder_test_fire(id, ignoreFocus?)` (QA) — each mutator returns the whole `CalendarDto[]`.
`add_calendar_event` / `add_calendar_event_from` (voice) now create reminders and wake the scheduler.

**Events:** `reminder-fired {id,title,date,time?,missed,firedAt,sound,test}`; `calendar-changed` also
after the scheduler marks a reminder fired or a toast button is used.

**Frontend:** `src/lib/reminders.ts` (`addReminder`, `setReminderAlert`, `snoozeReminder`,
`dismissReminder`, `remindersStatus`, `startReminders(sink)` — called once from `App.svelte`).

**Toast identity.** The plugin sets the AppUserModelID to the identifier (`tech.krovi.app`) unless the
exe sits in a `target\debug|release` folder. Installed builds: the NSIS Start-menu shortcut carries that
AUMID (verified on the maintainer's machine), toasts read "Krovi". Dev builds: "Windows PowerShell".
Toast body click opens the Calendar; the toast's Snooze 5 / Snooze 10 / Dismiss buttons work while the
banner is on screen (a limit of the plugin: a click after the toast moved to the notification centre is
not delivered to the app).

## 9. Voice v3 — local OS actions (owner: voice)

Command intents `open-folder`, `open-app`, `search-files`, `list-folder` (see SPEC §11.13 for
the exact phrasings). Folders resolve only against a known set (Known Folders + Quick
Access/recent folders + pinned), apps against Start-menu entries, search uses the Windows
Search index (SystemIndex), listing uses std::fs. Results shown in-panel via
`voice-result` (`results: [...]`). **Zero network calls** in this flow — verified by a
socket capture recorded in the security audit.

As built (D-014): `voice_known_targets() → {ready, folders:[{name,path}], apps:[string]}`,
`voice_open_result({path}) → string` (only paths from the latest result; folders open, files are
revealed never run, apps launch). `VoiceCommand += open-folder{name,path} | open-app{name,path} |
search-files{query} | list-folder{name,path}`. `VoiceResult += results: {kind:'folder'|'app'|'file',
name, path, detail?}[]`. All four are `executed:true` in Rust; the UI shows `feedback` + rows only.
App launches are gated at SAPI confidence ≥ 0.75 (below: "say it again", nothing launched).

## 10. Explain (owner: explain)

Paste any text → **summary** (on-device, extractive) + **read aloud**. Maintainer decision
(2026-11-01, revised): **summary only for now — no LLM explanation** (no Ollama, no cloud model).
- Summary: on-device only, no network.
- Read aloud: Windows built-in on-device voices by default; **optional** ElevenLabs with each
  user's own key (easy setup: link, paste, Test, voice picker, Remove), labelled "sends text to
  ElevenLabs"; automatic fallback to the Windows voice on failure. No relay, no shared key.
- Keys stored in Windows Credential Manager (never plaintext, never in SQLite/localStorage).
Commands: `explain_summarize`, `explain_explain`, `explain_speak`, `explain_stop_speaking`,
`explain_get_settings`, `explain_set_settings`, `explain_set_key`, `explain_engines`.
Frontend: `src/lib/explain.ts` + `src/views/Explain.svelte` (ToolId `'explain'`).

## 5. File ownership (strict)

| agent | owns (may create/edit) |
|---|---|
| orchestrator | `docs/CONTRACTS.md`, `src-tauri/src/lib.rs` (except marked handler blocks), `src-tauri/Cargo.toml`, `package.json`, `vite.config.ts` |
| live-data | `src-tauri/src/{live,commands,state}.rs`, `src-tauri/crates/{krovi-core,krovi-store,krovi-net,krovi-sys}/**`, `src/lib/{bridge,live,types,fixtures}.ts`, `[live]` handler block in lib.rs |
| panel-shell | `src-tauri/src/shell.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/**`, `src-tauri/icons/**`, `src/lib/shell.ts`, `[shell]` handler block |
| voice | `src-tauri/crates/krovi-voice/**`, `src-tauri/src/voice.rs`, `src/lib/voice.ts`, `[voice]` handler block |
| frontend-design | `index.html`, `src/main.ts`, `src/App.svelte`, `src/components/**`, `src/views/**`, `src/styles/**`, `src/lib/{stores,tools,icons,motion}.ts` |
| holidays | `src/lib/holidays/**` |
| explain | `src-tauri/crates/krovi-explain/**`, `src-tauri/src/explain.rs`, `[explain]` handler block, `src/lib/explain.ts`, `src/views/Explain.svelte`, Explain sections of `docs/SECURITY-network-audit.md` + `docs/DECISIONS.md` |
| reminders | `src-tauri/src/reminders.rs`, `[reminders]` handler block, reminder fields in `crates/krovi-core/src/calendar.rs` + `krovi-store` calendar persistence, the reminder UI parts of `src/views/Calendar.svelte`, `src/lib/reminders.ts` |
| personalization | `src/lib/theme.ts`, `src/lib/idle.ts`, `src/styles/themes/**`, `src/components/idle/**` |
| ui-polish | `docs/research/**`, `qa/polish/**`; `web/**` only after the web agent hands off |
| transfer | `src-tauri/crates/krovi-transfer/**`, `src-tauri/src/transfer.rs`, `src/lib/transfer.ts`, `src/views/Transfer.svelte`, `[transfer]` handler block, the Quick Transfer section of `docs/SECURITY-network-audit.md` |
| web | `web/**` (except `web/demo/`, which is build output), `scripts/build-web.*` |

Need a change in a file you don't own? Don't edit it — describe the exact change in
your final report and the orchestrator will apply it. Shared docs `docs/DECISIONS.md`
and `docs/PROGRESS.md`: **append** a section only; re-read immediately before editing.
