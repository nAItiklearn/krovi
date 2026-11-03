# Krovi v0.3 — Implementation Plan

_Derived from `FEATURE_AUDIT.md` (2026-11-03). Each milestone ships only when its acceptance tests pass._

## Decisions needed from the owner before the affected phase
| # | Conflict | Brief says | Existing rule | Needed |
|---|---|---|---|---|
| Q1 | Screen-awareness mode (Phase 4) | Opt-in continuous capture while active | AGENTS.md: "No passive/background screen capture, ever" | Confirm an explicit, indicator-on, user-started session is allowed; record as a new D-entry |
| Q2 | Cloud voice (Phase 2) | "Optional cloud voice services" | AGENTS.md: "No cloud STT" | Confirm cloud = **TTS only** (ElevenLabs already exists); STT stays Whisper-local |
| Q3 | System-aware eye expressions (Phase 5) | Battery/RAM/CPU drive expressions | D-017: expressions only from real app events | Approve amending D-017 (values are real OS readings, so honesty holds) |
| Q4 | Calculator removal (Phase 1) | Remove completely | Palette inline math shares `calc.ts` | Keep palette math, or remove both |
| Q5 | Toolchain | Build/test every milestone | Host is `x86_64-pc-windows-gnu` with 32-bit MinGW; native app builds only on CI | Install VS Build Tools + MSVC toolchain locally, or accept CI-only native verification |

## Risky dependencies
- **whisper.cpp** (Phase 2): native DLL + model download (~75–150 MB for small/base q5). Must be lazy-loaded and user-initiated download; model host becomes a new network destination → audit doc update.
- **Audio capture** (cpal/WASAPI) + **Silero VAD** (ONNX runtime is heavy — prefer whisper.cpp's built-in VAD or energy VAD first).
- **Text injection into other apps** (SendInput / clipboard paste) — must snapshot+restore clipboard (existing `context.rs` code can be reused) and refuse elevated/secure windows.
- **OCR**: `Windows.Media.Ocr` (built-in WinRT, no dependency) is preferred over Tesseract.
- **OAuth integrations** (Gmail/Outlook): needs registered client IDs, loopback redirect, token storage in Credential Manager. Postpone until agent core is solid.
- **WebView2 RAM** is ~95% of idle footprint (136 MB). Every new always-loaded view raises it; new heavy surfaces (Circle overlay, launcher) should be separate on-demand windows.
- **`tauri-plugin-notification`** cannot set `scenario="reminder"`; persistent alarms need `tauri-winrt-notification` directly.

## Phase 1 — Reliability (first milestone, proposed)
**Milestone 1A — "Things that must ring, ring" + calculator removal** (small, high value, low risk)
1. Move timer / stopwatch / pomodoro end-events into Rust: wire `krovi-core::timer` + `pomodoro` through `AppState`, fire through the existing reminders toast/sound path, persist active timers. UI becomes a view of Rust state.
2. Remove Calculator per the audit checklist; "open calculator" → Windows Calculator app.
3. Fix website/README tool counts and label the hard-coded `27° / 61% RAM` demo values.
- Tests: `krovi-core` timer/pomodoro tests extended; new test that a timer due while the window is hidden produces exactly one alert; parser test "open calculator" → app launch; vitest for tool registry (15 tools); `svelte-check`, `vite build`.

**Milestone 1B — Data integrity**
- File Shelf and World Clocks persisted through the Rust store (migrate existing localStorage once); task delete/edit; clipboard full-history dedup.
- Screen Time: local-day buckets, idle/lock exclusion (`GetLastInputInfo` + session lock notifications), incremental upsert instead of full rewrite, 90-day retention.

**Milestone 1C — Summarisation rebuild** (`krovi-explain`)
- Section-aware extractive pipeline: split by headings/paragraph blocks → overlapping chunks → per-chunk selection with a budget proportional to input length → merge with coverage guarantee.
- Modes: Short, Detailed, Key points (bullets allowed <4 words), Action items, Decisions, Dates & deadlines (regex + local date parser from `krovi-voice/when.rs`), Open questions.
- Remove 1,200-candidate pre-filter (chunking replaces it); run off the UI thread with cancel.
- Optional BYOK generative path (map-reduce over the same chunks, results labelled "AI interpretation", source-checked for names/dates/numbers). Requires a new provider entry in the network audit.
- Tests: multi-section fixture where every section is represented; bullet-list fixture; 60k-word performance bound; CJK fixture; dates extraction.

## Phase 2 — Voice foundation
Whisper behind the existing `sapi.rs` seam (D-016 order), mic device selection + test, discard-cancel, push-to-talk/toggle global dictation into the active app with clipboard restore, filler removal + self-correction (local rules), custom vocabulary, voice settings page (voices/rate/preview/stop). Benchmark SAPI vs Whisper tiny/base/small on a fixed WAV set (incl. Indian-English, Hinglish) — publish WER, latency, RAM.

## Phase 3 — Agent mode
`REQUEST → INTENT → PLAN → PERMISSION → EXECUTE → RESULT` built on the existing parser/osintent. Deterministic intents first: open URL/site, Google/YouTube search, app/folder/file, timers, reminders, notes, read aloud. Confirmation card component. Integration registry interface; no OAuth integration until the core passes tests.

## Phase 4 — Circle to Ask (after Q1)
On-demand transparent overlay window, region capture (`Windows.Graphics.Capture`/GDI BitBlt of the region only), `Windows.Media.Ocr` locally, optional BYOK vision with disclosure. Capture held in memory, dropped on close.

## Phase 5 — Companion
Whole-screen gaze (remove 400 px gate, eased transitions, clamp; pill itself never moves), sampler drops to 0 Hz while hidden/fullscreen and to ~10 Hz idle, battery via `GetSystemPowerStatus`, expressions with hysteresis (after Q3), voxel / dark-minimal / clean-light themes, fullscreen check per pill monitor.

## Phase 6 — Clean My OS
Scanner (Recycle Bin via `SHQueryRecycleBin`, `%TEMP%` files older than N days, known app caches, crash dumps) → review list with paths/sizes/reasons → confirm → delete to Recycle Bin (`IFileOperation` with `FOFX_RECYCLEONDELETE`), locked files skipped and reported.

## Phase 7 — BYOK AI providers (replaces accounts/sync — owner decision 2026-11-03, D-022)
No Krovi account, sign-up, password, auth server, subscription, proxy, vault or multi-device sync in v1.
Positioning unchanged: "Use Krovi without an account. Bring your own AI key when you want AI-powered features."
Settings → AI: pick provider → enter key → **Test connection** → choose model (where the provider lists models) →
change/remove key. Keys live only in Windows Credential Manager (`Krovi/ai/<provider>`), never in SQLite, localStorage,
logs or backups. Requests go straight from the PC to the provider. Errors are classified separately: invalid key
(auth), billing/no credit, quota/rate limit, eligibility/region/age/policy (e.g. a Gemini key whose project has no
free-tier access), model not found, network. Modular provider trait so more providers can be added. A provider is
listed as supported only after its integration works end to end **and** its current terms (consumer-app use, age,
regions, data use) were checked and recorded in `docs/product/AI_PROVIDERS.md`. Local tools never need a key.
`ACCOUNTS_RESEARCH.md` stays as a future idea only; not a release dependency.

## Phase 8 — Extensions · Phase 9 — Website & release
As in the brief; website redesign last so it shows real screenshots of shipped features.

## Performance budget
Baseline (PERF.md, v0.2): 136 MB private idle (6.3 MB Rust + 129 MB WebView2), 0% idle CPU, 46–69 ms summon. Budget for v0.3: no regression >10% idle RAM with all new features **off**; models load only on use and unload after idle; re-measure at every phase end.
