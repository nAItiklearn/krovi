# Krovi v0.3 — Feature Audit (Phase 0)

_Date: 2026-11-03 · Branch `clean-main` (uncommitted work present, preserved) · Read-only audit._

**Method.** Source was read file by file and checked against the website (`web/`), README and `docs/SPEC.md`.
Shell access was unavailable for this session, so **no tests, builds or fresh measurements were run.**
Test status below comes from the code and from `docs/PROGRESS.md`. Performance figures come from
`docs/PERF.md` (2026-11-01, installed v0.2 build).

**Stack (actual).** Tauri 2 shell + Rust workspace (`krovi-core`, `krovi-store` [SQLite, on by default],
`krovi-sys` [windows-rs], `krovi-net` [ureq], `krovi-voice` [SAPI 5], `krovi-explain` [summariser + TTS],
`krovi-transfer` [tiny_http]) + Svelte 5 / Vite / TS frontend. Demo mode = no `__TAURI_INTERNALS__`.
Fixtures **cannot** reach the native path (`src/lib/bridge.ts:7,31-37`).

Status key: ✅ works natively · ⚠️ works with real gaps · 🟥 broken/missing vs advertised · ➖ not built

## Existing features

| Feature | Advertised behavior | Actual implementation | Status | Problem | Test needed |
|---|---|---|---|---|---|
| Home dashboard | Overview cards | `views/Home.svelte` → weather, calendar, live stats/media/clips | ✅ | — | Smoke: cards populate natively |
| Clipboard history | History, pin, search | `src-tauri/src/live.rs:194-238` polls seq no. every 250 ms; SQLite; PNG sidecars for images | ⚠️ | Dedup only against newest entry; older dupes reappear. 250 ms poll (could be `AddClipboardFormatListener`) | Dedup across history; image prune |
| Paste Stack (9-slot) | Brief lists it | **Removed 2026-11-02 (D-015)** | ➖ | Brief is out of date; must not be re-added | Confirm no residual site copy |
| Timers / stopwatch | Timers that ring | `krovi-core/timer.rs` pure logic, **no Tauri command**; runs in webview stores only | 🟥 | No native end-of-timer alert; a hidden/throttled webview can miss the end | Timer ends while panel folded / app hidden |
| Pomodoro | Work/break cycles | `krovi-core/pomodoro.rs`; `AppState.pomodoro` is dead code (`state.rs:51`) | 🟥 | UI-only, not persisted, no native alert | Same as timers + persistence across restart |
| File Shelf | Park files | `commands.rs:306` `load_shelves` only; UI writes to **localStorage** (`stores.ts:463-478`); `save_shelves` never called | 🟥 | Backend shelf is read-only; real data lives in webview storage | Add/remove/move round-trip through Rust store |
| Quick Transfer | LAN phone transfer | `krovi-transfer`: one private IPv4, random port, 160-bit token, optional PIN, 10 min expiry | ⚠️ | Plain HTTP (token sniffable on shared Wi-Fi); firewall rule needs elevated netsh | Transfer 5 MB; expiry; PIN |
| Media controls | Now playing + transport | `krovi-sys/media.rs` GSMTC, polls 500 ms | ✅ | Polls instead of session events (CPU, minor) | Track change latency |
| Calendar + reminders | Ringing reminders | `src-tauri/src/reminders.rs` condvar scheduler (≤30 s wake), toast + `PlaySoundW`, persisted `fired_at` before toast | ✅ | Toast Snooze/Dismiss buttons unverified on Windows; ≤30 s late after resume; dev toasts say "Windows PowerShell" | Fire while folded; sleep/resume; DND; missed; no duplicates |
| Weather | Keyless forecast | `commands.rs:464-595`, Open-Meteo, coarse location, cached | ✅ | Place label hard-coded "Current location" | Offline cache; place search |
| Notes / checklists | Notes + to-do | `commands.rs:201-300`, SQLite | ⚠️ | UI supplies timestamps; tasks have no delete/edit; Notes view also reads localStorage | CRUD round-trip; restart persistence |
| Explain / summarization | "Summarise anything" | `krovi-explain/summary.rs`: extractive TF-IDF + TextRank + centroid, MMR; Short/Medium/Points | 🟥 | Fixed cap (≤7 sentences/240 words) at any input length; top-1,200 candidate pre-filter drops off-topic sections; 1.5× lead bias; <4-word bullets never selectable; no section coverage; no action items/decisions/dates; no BYOK path; CJK breaks word counts; synchronous, no cancel | Long multi-section doc covers all sections; bullet lists; CJK; 60k-word input |
| Read aloud (TTS) | Speak summaries | `krovi-explain/win.rs` WinRT `SpeechSynthesizer` (voice + rate); ElevenLabs optional, consent-gated, key in Credential Manager | ✅ | Only reachable from Explain; no global voice settings page | Voice list; stop; rate |
| World clocks | Multiple zones | `views/WorldClocks.svelte`, localStorage first | ⚠️ | Native data never reaches Rust store | Persistence |
| Unit + currency conversion | Offline convert | `units.ts`; currency via Frankfurter, cached | ⚠️ | `fetch_currency_rates` uncached command still exposed | Offline convert |
| Calculator | "Expression + unit-aware" | `views/Calculator.svelte`, `lib/calc.ts` (arithmetic, %, sci notation — **no units**) | 🟥 | Claim inflated; **to be removed** per brief | See removal checklist below |
| Screen Time | Per-app usage | `krovi-sys/foreground.rs`, 5 s sampler, SQLite | 🟥 | No idle/lock detection (sleep credited to last app); **UTC day buckets, not local**; full table delete+rewrite every 5 s; unbounded growth | Lock screen not counted; local-midnight split; DB size over 30 days |
| Floating pill | Fixed top-centre | `shell.rs:551-563,657`; re-anchors on drift ~0.5 s | ✅ | Primary monitor only. Nothing moves it with the cursor (matches brief) | Cursor sweep: pill stationary |
| Idle eyes | Glance at cursor | Rust `GetCursorPos` at **33 ms always** (`shell.rs:897-1065`), emits ≤10 Hz within 400 px; `tanh` clamp; stepped transitions; blinks; doze | ⚠️ | Only tracks within 400 px; stepped not eased; sampler keeps 30 Hz polling while hidden | Pupils follow across whole screen; 0 emits when hidden |
| System-aware expressions | (brief, new) | None. Pill shows RAM/disk text; no battery code | ➖ | Needs `GetSystemPowerStatus`; conflicts with D-017 (see report) | — |
| Command palette | Ctrl+K | `components/CommandPalette.svelte`, includes inline calc row | ✅ | Inline math uses `calc.ts` | Palette after calculator removal |
| Six themes | graphite, porcelain, mochi, arcade, terracotta, tide | `lib/theme.ts:31-38`, token-enforced by `themes.test.ts` | ✅ | Not visually verified per view | Visual pass per theme |
| Fullscreen hiding | Hide for games/video | `shell.rs:416-450`, foreground covers monitor & `!IsZoomed` | ⚠️ | Secondary-monitor fullscreen hides primary pill; borderless "fill" apps count | Game/video/slideshow; maximized stays visible |
| Voice commands | On-device commands | SAPI 5 closed English grammar (`grammar.rs:64-190`), dictation only into Krovi notes | ⚠️ | Default mic only; no URL/web search; no discard-cancel; English only; no dictation into other apps | WAV round-trip tests exist |
| App & file search | Find apps/files | Voice-only (`krovi-voice/os.rs`, Windows Search index) | ⚠️ | No UI launcher; LIKE `_` unescaped (`os.rs:392-403`); `/select,"…"` quoting (`os.rs:314`) | Search with `_`; paths with `"` |
| Global shortcut / tray | Ctrl+Alt+Space | `shell.rs:1115,1142-1197` | ✅ | — | — |
| Privacy / network | Zero telemetry | Hosts: `api.open-meteo.com`, `geocoding-api.open-meteo.com`, `api.frankfurter.dev`, `api.elevenlabs.io` (opt-in). CSP blocks webview fetches | ✅ | Audit doc should be re-checked: it names `frankfurter.app`, code uses `.dev` | Binary-level network trace |

## Website / docs accuracy issues
- Tool count inconsistent: "15 tools" (`web/index.html:8,210,249`) vs "Sixteen" (`:431`) vs docs "16"; list at `:252` has 14.
- Unlabelled hard-coded values: Home explorer panel "27° Clear", "61% RAM · C: 43%" (`index.html:462-465`); hero "27° Clear" (`:168`) only captioned "A preview".
- Section heading says "RAM and disk" (`:328`) but widget shows simulated CPU.
- README/SPEC call the calculator "unit-aware" — false.
- Other simulations (clipboard, pomodoro ×30, voice, palette, calendar) **are** labelled.

## Calculator removal checklist (Phase 1)
Remove: `src/views/Calculator.svelte`; `tools.ts:30`; `types.ts:191` (`ToolId`); `icons.ts:20`; `Panel.svelte:25,49`;
`voiceHelp.ts:27`; Rust `ToolId::Calculator` (`krovi-voice/types.rs:47,69,89,733`), `parser.rs:72` + tests `:941,942,1057`,
`osintent.rs:479` assertion → retarget so "open calculator" launches **Windows Calculator**; site `index.html:60,241,252,450,589-596,750`,
`docs.html:42,199,223`, `compare.html:42,150`, `changelog.html` (add removal entry, keep history), icon symbols in `download.html`, `404.html`,
`scripts/build-web.header.html`; README `:78,133`; SPEC `:97,161,259`.
**Decision needed:** `calc.ts` is also used by the palette's inline math. Keep it (palette math is a small, real feature) or remove both.
Keep: references to the Windows Calculator *app* (`voice.ts:372`, `catalog.rs:452`, `grammar.rs:365`, `osintent.rs:400,485`, `win.rs:333`).

## Brief items with no implementation yet
Universal dictation, Whisper, mic selection, voice settings page, agent mode / confirmation cards / URL opening, integrations/OAuth, MCP,
background tasks, Circle to Ask, OCR/vision, screen-awareness mode, system-aware expressions, voxel/dark-minimal/clean-light themes,
Clean My OS, quick launcher, snippets, file organizer, workflow builder, BYOK LLM summarisation.
