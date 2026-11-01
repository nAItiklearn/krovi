# Krovi — Decision Log

Every non-trivial decision is recorded here, newest first. Format: `D-NNN · YYYY-MM-DD · title`.

---

## D-010 · 2026-11-01 · Push + Release authorised; final-delivery plan
Maintainer's final prompt authorises **`git push` and creating a GitHub Release** (previously gated),
and wants a working, downloadable app + finished website, all pushed, with the README tying it together.
Plan to finish unattended:
- **Push** `clean-main` to origin (non-destructive). Set the GitHub **default branch** to `clean-main`
  via `gh` — keep the old `main`; **no force-push / no history loss**.
- **Release/installer:** this host can't build the native app (`x86_64-pc-windows-gnu`, 32-bit gcc — D-009),
  so the **GitHub Actions workflow builds the Windows installer on a `windows-latest` (MSVC) runner** and
  attaches it to the Release, triggered by pushing a `v*` tag (tauri-action + the default `GITHUB_TOKEN`).
  That is how "ready to download & use" is delivered; local `cargo tauri build` is not relied on.
- **Website:** incorporate the maintainer's feature-presentation screenshots (conceptual, clean-room —
  approach only, original execution); redeploy to Vercel; retry disabling Vercel-Auth to make it public
  (if the safety classifier blocks it again, leave the one-click instruction in README + PROGRESS).
- **README** updated to tie together live site, download/release, features, and the privacy audit.
- Clean-room (D-002) and no-fabricated-claims remain absolute.

## D-009 · 2026-11-01 · Persistence shape + toolchain finding + website live
- **krovi-store** = a UI-agnostic `Store` trait (per-collection load/replace + a settings bag) with
  two impls: `MemStore` (in-memory, zero deps, always available) and `SqliteStore` (rusqlite
  "bundled", behind an **off-by-default `sqlite` feature**). The default build keeps zero
  third-party deps (error type hand-rolled, not `thiserror`).
- **Toolchain finding:** this host is `x86_64-pc-windows-gnu` with a 32-bit-only MinGW gcc, so
  bundled SQLite will not compile ("64-bit mode not compiled in"). Kept the suite green by
  feature-gating `SqliteStore` off; documented loudly, never silently degraded. The same missing
  64-bit C/MSVC compiler likely blocks a full native Tauri build — to be confirmed in Phase 2;
  mitigation is shipping the Vite browser demo as the clickable UI.
- **Website** built as an original pure-static site and **deployed to Vercel** (authorised, D-008):
  https://web-flame-kappa-83.vercel.app . Making it publicly viewable needs the project's
  Vercel-Authentication protection turned off — attempted via the API, blocked by the local safety
  classifier as a "Security Weaken"; surfaced to the maintainer (PROGRESS blockers). No app
  push/tag/release performed.

## D-008 · 2026-11-01 · Expanded scope: website + comparison table; Vercel deploy authorised
Maintainer directed (awake): build all phases; keep the product/site original; add a `/compare`
table demonstrating Krovi's advantages over Vayli and similar apps; ship a full scaffold-style
footer with every link working; **deploy the website to Vercel**; take design cues from the
maintainer's own site (https://scaffold-rho-dusky.vercel.app). All tests must stay green.
- **Website deployment to Vercel is now explicitly maintainer-authorised** (previously gated).
  Scope of this authorisation = the **website only**. Pushing the git repo, creating tags, and
  publishing a GitHub **Release of the app binary** remain **NOT** authorised until asked separately.
- If the Vercel CLI/MCP needs login or a token that isn't present, that is a `needs-maintainer`
  blocker: build the site, attempt deploy, and if credentials are missing, STOP and record how to
  finish (maintainer runs `vercel login` or supplies a token) rather than guessing.
- Comparison table obeys the clean-room rule (D-002): Vayli's column uses public listing info only;
  anything unconfirmed is "Not verified". A candid "where others are ahead" note is included.
- Panel-UI phase re-views the user-supplied conceptual screenshots in
  `D:\krovi-reference\vayli-landing\` (9 PNGs; not committed; conceptual only). See SPEC §10.

## D-007 · 2026-10-31 · Core engine design invariants (deterministic, UI-independent)
Decisions made while building the first `krovi-core` modules (clipboard, paste stack,
timers/stopwatch):
- **Injected clock, not wall-clock.** Every time-dependent query takes a caller-supplied
  `now` (epoch ms). The core holds no clock, so all logic is deterministic and unit-testable
  without sleeping or mocking system time. Real time is applied at the integration boundary.
- **Clipboard capacity applies to *unpinned* entries only**; pinned items are never evicted
  and float to the top. Re-copying existing content **promotes** it (refreshes timestamp,
  keeps pin) instead of creating a duplicate. Default cap 200 (SPEC C2).
- **Paste stack is a bounded 9-slot LIFO** (SPEC O2), slot 1 = top; pushing past 9 drops the
  oldest. Kept deliberately distinct from the passive clipboard history.
- **Zero third-party dependencies** in `krovi-core` so far — supports the small-surface /
  verifiable-privacy posture. New deps require justification.
- Workspace rooted at `src-tauri/` with crates under `src-tauri/crates/`; the Tauri app crate
  and `krovi-store`/`krovi-sys`/`krovi-net` join `members` as later phases scaffold them.
First green suite: 27 unit tests (commit recorded in PROGRESS).

## D-006 · 2026-10-31 · "Beyond Vayli" original feature set (focused, not scope-creep)
From the candidate list we commit to a **focused** set that fits the "quick-access shelf"
identity and build the strongest few first:
- **Command Palette** (quick-launch apps + Krovi actions) — primary original surface.
- **Paste Stack** (numbered multi-slot clipboard, distinct from passive history).
- **Pomodoro** mode layered onto the existing Focus timer (reuses timer engine).
- **Calculator** (fits the conversion tools).
- **To-do list** (fits notes/reminders).
Deferred to avoid scope-creep: system quick-toggles (wifi/BT/DND) — Windows exposes these
inconsistently and some require elevated or vendor APIs; revisit after core ships. Full
rationale in `SPEC.md` §"Beyond Vayli".

## D-005 · 2026-10-31 · Design divergence from reference
Krovi must be visually original. Decision: **left vertical tool rail** (reference uses a
bottom horizontal rail), Krovi's own accent colour and iconography, and a distinct card
system. Reference screenshots are conceptual only and are **not** committed to the repo.

## D-004 · 2026-10-31 · Stack = Tauri 2 + Rust core + Svelte 5 / Vite / TypeScript
See `ADR-001-stack.md`. Chosen for small footprint, a reusable testable Rust core, native
Windows API access (windows-rs), and a small dependency surface that supports the
verifiable-privacy positioning.

## D-003 · 2026-10-31 · Clean slate executed
Removed stale tracked files (`llm_client.py`, `main.py`, old `README`/`.gitignore`) in
commit `d471ffd`. Working tree confirmed empty except `.git` before any new content.

## D-002 · 2026-10-31 · Declined reverse-engineering / licence-cracking of the paid reference app
The kickoff brief contained instructions to decompile, disassemble, extract assets from,
reverse-engineer, and analyse/crack the paid reference app's licence check. **Declined and
omitted.** Reasons: (a) legally risky; (b) it directly contradicts the brief's own
clean-room requirement and would poison Krovi's "100% original" claim. All Krovi work is
clean-room: public listing/marketing copy, user-supplied conceptual screenshots, and
general domain knowledge only — never the competitor's binary. This is a hard project
invariant, not a one-off.

## D-001 · 2026-10-31 · Repo target = existing local D:\krovi
Confirmed by maintainer. Remote verified as `github.com/nAItiklearn/krovi.git`. Work
proceeds in place on branch `clean-main`; no re-clone.

## D-012 · 2026-11-01 · Voice: 100% on-device speech via Windows SAPI 5; grammar-first commands
**Requirement (maintainer):** "Use Windows' built-in speech recognition APIs where possible to keep
this fully local/private … research and note in DECISIONS.md if a local-only approach isn't feasible."
No cloud speech API, ever. **Outcome: a local-only approach IS feasible**, for both dictation notes and
voice commands, with the accuracy limits recorded below. Implemented in `krovi-voice` (engine +
parser), `src-tauri/src/voice.rs` (Tauri layer) and `src/lib/voice.ts` (UI API + demo simulation).

**Engines considered (verified on this machine: Windows 11 10.0.26300, en-US):**
| Option | Verdict |
|---|---|
| **SAPI 5 in-proc recognizer** (`SpInprocRecognizer`, token `MS-1033-80-DESK`, "Microsoft Speech Recognizer 8.0 for Windows (English - US)") | **Chosen** for both modes. In-process COM engine, documented API, dictation grammar + custom grammars, mic or file input. No network code path, no privacy toggle involved. |
| WinRT `Windows.Media.SpeechRecognition` | **Rejected.** Free-form dictation (topic constraint) uses Microsoft's *online* service and needs the "Online speech recognition" privacy setting: unacceptable. Its list/SRGS constraints are on-device, but (a) it offers no dictation, (b) it can only listen to the microphone, never a file, so nothing could be verified repeatably, and (c) this machine has never accepted the online-speech consent (no `HKCU\Software\Microsoft\Speech_OneCore\Settings\OnlineSpeechPrivacy` key). *Not re-tested here:* WinRT is not drivable from Windows PowerShell 5.1, so the WinRT behaviour above is Microsoft's documented behaviour, not something I measured. |
| OneCore DNN engine (`MS-1033-110-WINMO-DNN`, "Embedded DNN v11.1") hosted *through SAPI* | **Tested, not viable.** SAPI does not enumerate it (SPCAT_RECOGNIZERS lists only `MS-1033-80-DESK`); an explicit token id is accepted by `SetRecognizer`/`CreateRecoContext`, but `LoadDictation` fails (`0x8004503A`) and a dynamic command grammar fails at activation (`SPERR_UNINITIALIZED`, `0x80045001`). It also depends on an undocumented registry layout. Not used. |
| Cloud APIs (Azure/Google/OpenAI/Windows Voice typing "Win+H") | **Forbidden** (privacy). |
| Optional local Whisper (whisper.cpp) model, user-initiated download | **Documented upgrade path, not built.** Would improve dictation accuracy markedly (esp. female voices, noise, proper nouns) at the cost of a tens-to-hundreds-of-MB model download (the *only* network access, must be explicit opt-in, hash-checked, and added to `docs/SECURITY-network-audit.md`) plus a C++ build dependency that is awkward on this host's MinGW toolchain. Revisit if SAPI dictation accuracy proves insufficient for users. |

**Design.**
- *Notes (`mode:"note"`)*: SAPI dictation (`LoadDictation`) only. Hypotheses stream as `voice-partial`; finished phrases
  join into the note text; spoken punctuation ("period", "comma") is handled by the engine.
- *Commands (`mode:"command"`)*: a **closed, dynamically-built context-free grammar** (thousands of phrases: 2,636 when the number slot is sampled at six
  values, more with all 99; generated from `grammar::command_templates`: open/show/go to + any tool name, start pomodoro/focus, "start a timer for <1-99> <unit>",
  "<n> <unit> timer", new note, close/hide/dismiss) with SAPI's embedded dictation (`*+`) only for the body of
  "new note ...". The recognised text goes through a **pure parser** (`parser::parse_command`: filler, casing, digits vs
  number words, "an hour and a half", tool synonyms, near-miss spellings; `None` on garbage). A test expands every grammar
  phrase and requires the parser to accept it, so grammar and parser cannot drift (it caught a real bug: "now playing"
  lost its "now" as a filler word).
- *Why grammar-first and not "dictation + parse"?* Measured on the first 10-command set: with dictation and the grammar both
  active, dictation outscored the grammar on short commands ("open timers" -> "Up and timers", "close" -> "Plus"):
  male voice 9/10, female voice 3/10. Grammar-only with embedded dictation: 10/10 and 7/10 (before tuning).
- *Threading*: one dedicated MTA thread per session (`CoInitializeEx(COINIT_MULTITHREADED)`), notification via a Win32
  event handle, no message pump, so nothing can deadlock against Tauri's STA UI thread. `start()` returns once the mic is open.
- *Microphone*: bound explicitly (`SpMMAudioIn`, device id `WAVE_MAPPER`). **Bug found by testing:** relying on SAPI's implicit
  default input produced a session that said "listening" but received *no audio at all* (0 audio-level events vs ~8/s once
  bound explicitly). Preflight reports "No microphone found" (`waveInGetNumDevs`) and "Microphone access is turned off"
  (ConsentStore `Deny`, user or machine, store or desktop-app switch) as clear `error` states instead of silent failure.
- *Endpointing* (pure, unit-tested state machine, `session.rs`): note mode ends after ~2.5 s of silence (2.0 s + the
  engine's own ~0.5 s utterance close), no speech for 8 s, 120 s cap; command mode ends ~0.9 s after the phrase, 6 s no-speech,
  20 s cap. `voice_stop` finalises the phrase in flight (up to a 1.5 s grace; falls back to the last hypothesis), never hangs.
  A session whose thread panics still marks itself done, so the controller cannot wedge.

**Tuning, measured (synthesised speech, Windows `System.Speech` voices David/Zira; 18 commands + 7 deliberately
command-like chatter lines + 4 dictation sentences per scenario; scenarios: David 16 kHz, Zira 16 kHz, David 22.05 kHz
(format conversion), David+noise, Zira+noise):**
- Bare one-word tool names in the grammar ("timer", "notebook") caused false accepts on chatter (up to 4/7): **a verb is required**
  in the grammar (the parser still accepts bare names in typed text).
- `CFGConfidenceRejectionThreshold` sweep (engine default 60): at 60 commands recognised 76/90 (84%) with 0/35 false accepts;
  at **40: 87/90 (97%) with 4/35 (11%) false accepts, all on adversarial chatter that contains command words** ("the timer on the
  oven is broken" -> start timer 1 min); at 30: 87/90 with 8/35. **40 chosen** (the knee). Silence, room noise, a loud noise and a
  thump produced **0/8** invented commands or text after the dictation gate below.
- Dictation noise gate: a lone word hallucinated from room noise scored engine confidence 0.09 versus 0.55-0.75 for real
  speech; results scoring < 0.15 are discarded (also drops some garbage like "Final thought..." for a noisy sentence, which is better
  than a wrong note; the user is told nothing was recognised).
- Final numbers (settings as shipped): commands correct / chatter false accepts / dictation word-error-rate:
  David 18/18, 1/7, 5%; Zira 17/18, 1/7, 30%; David 22.05 kHz 18/18, 0/7, 12%; David+noise 17/18, 1/7, 37%;
  Zira+noise 17/18, 1/7, 89% (two of four sentences correctly withheld as unintelligible).
  Weak spot: the free-text body after "new note" (e.g. "buy milk" -> "by mail to" for Zira); the closed part is reliable.

**Privacy analysis.**
- `krovi-voice` has **no network code**: its entire dependency tree is `serde` and `windows` (OS bindings) (`cargo tree`).
  Recognition happens in Windows' own SAPI engine loaded in-process. Neither this crate nor the Tauri layer opens a socket.
- The "Online speech recognition" Windows setting does not apply to this path (SAPI does not use it) and is not required.
- Audio is never written to disk or retained (`SPAO_RETAIN_AUDIO` is not enabled). The only persisted artefact is the note the
  user dictated (note mode), through the same store as typed notes. File input is read only via the explicit QA hook.
- Recognizer **adaptation is turned off** (`AdaptationOn=0`; the engine otherwise learns from and persists dictated text into
  the user's Windows speech profile, which would survive deleting a note). Cost: no per-user accuracy gain over time.
- The microphone is open only while a session is active (stopped on silence, timeouts or `voice_stop`); Windows' own mic
  indicator shows while it is. `KROVI_VOICE_TRACE=1` prints recognizer diagnostics to the local stderr only.

**Honest limits.** (1) SAPI 5 desktop recognition is an older generation than cloud or Whisper: accuracy is voice-dependent
(female synthetic voice: dictation WER 30% vs 5% for the male voice) and drops with noise; real microphones are noisier than TTS,
so figures above are an upper bound, not a promise. (2) English (US) only, and only if that recognizer is installed
(Settings > Time & language > Speech). Other languages would need their SAPI recognizer; `voice_available` reports
`dictation:false, commands:false` when none exists. (3) Dictation yields digits for numbers and little punctuation unless dictated.
(4) The live-microphone recognition path was verified with acoustic loopback only (laptop speakers into the laptop mic at low
level: partial hypotheses streamed, silence endpointing and noise gating worked); accuracy numbers come from WAV input through the
same recognizer, not from a human speaking.


## D-shell - 2026-11-01 - Island window shell: the "can't close it" bug and how it is made impossible
**Bug (v0.1):** window config was `center:true, decorations:false, skipTaskbar:true` with no tray and no close control,
so the installed app was an undismissable fixed 1080x700 window. **Root fix** (src-tauri/src/shell.rs, tauri.conf.json):
- Rust owns the window. Every Rust-initiated change (shortcut, tray, focus loss, 2nd launch) emits `shell-request`
  and a 1500 ms **watchdog** forces the mode if the UI never calls `shell_set_mode`; verified with a literally blank
  frontend (no JS) on a real install. Quit / shortcut / click-outside never depend on the UI.
- Quit: tray "Quit Krovi" -> `app.exit(0)` (+ hard `process::exit` after 3 s as a fail-safe, tray icon removed first so no
  ghost). `Krovi.exe --quit` (second-instance hand-off) closes a running copy with no UI at all. `mainBinaryName = "Krovi"` so
  Task Manager shows "Krovi", not "krovi-app". Alt+F4 only collapses the panel (CloseRequested is prevented).
- Pill never takes focus: window is created `focusable:false` (WS_EX_NOACTIVATE) and only made focusable while the panel is open.
  Shown with tao `show()` (no raw ShowWindow) so tao's visibility flags stay truthful. Focus returns to the previously active
  window on collapse.
- Click-through: ~30 Hz sampler (raw user32 FFI, no new crate) toggles `set_ignore_cursor_events` only on transitions;
  default hit rect = contract pill/panel rect until the UI reports its own. A click outside the panel collapses it even if no
  focus event ever fires (button-edge detection), plus a foreground-lost net. `shell_hold` is a 5 min lease so a crashed UI can
  not disable collapse-on-blur forever.
- Geometry = SetWindowPos in physical px from SPI_GETWORKAREA + GetDpiForMonitor, same top-centre anchor for pill and panel;
  re-anchored if the work area / DPI drifts. Verified at 125%: pill 685,8 550x110; panel 360,8 1200x863 (1920x1080).
- Capabilities cut to `core:default`: the webview no longer needs (or gets) window-mutation permissions.
- Fullscreen auto-hide treats a *maximised* window as not fullscreen (maximised Chrome overhangs the monitor by 8 px).
- NSIS `installMode: currentUser` (no UAC). Silent uninstall of a RUNNING app was verified: the template kills the process,
  removes files + registry. App data (`%APPDATA%\tech.krovi.app`) is intentionally kept.


## D-013 · 2026-11-01 · Voice v2: point-and-ask context, voice-to-action, clean transcription, privacy settings (all local)
**Scope (SPEC §11.10).** Extends D-012; same rule: fully on-device, no cloud API. **Every sub-feature below is feasible
locally and is built locally.** Nothing in this decision adds a network path (the crate's dependency tree is still `serde`
+ `windows`; `cargo tree` unchanged apart from extra `windows` feature flags).

**1. Point-and-ask context (what "this" means) - read only on an explicit command, never in the background.**
- *When:* only when the UI starts a **command** session with `captureContext:true` AND the user's setting is not `off`. It
  is captured once, at the moment the session starts (before Krovi takes focus), because the panel stealing focus would
  leave nothing to read. It is never polled, never kept after the command, never sent anywhere, and the result records
  exactly what was captured (`VoiceResult.context`).
- *Selected text:* UI Automation (`IUIAutomation::GetFocusedElement` -> `TextPattern.GetSelection`, walking up to 5
  ancestors, same process only). **Password fields are refused** (`CurrentIsPassword`): the hint says so and nothing is read.
  Normalised (CRLF->LF, trailing whitespace trimmed) and capped at 20,000 characters.
- *Selected files:* Explorer only, via the Shell COM objects (`IShellWindows` -> top-level `IShellBrowser` ->
  `IFolderView2::Items(SVGIO_SELECTION)`), at most 100 items, on a short-lived STA thread with a 2.5 s timeout. If nothing is
  selected the context degrades to the foreground app name only.
- *Foreground app:* just the executable's base name (`QueryFullProcessImageNameW`), e.g. `notepad`. Not the window title
  (titles often carry document names and URLs).
- *Clipboard fallback (opt-in, `selection+clipboard-fallback`, off by default):* many apps (browsers, Electron, WinForms)
  expose no UIA text pattern. Only if UIA yields nothing AND the user chose this level, Krovi sends Ctrl+C and reads the
  clipboard. Safeguards: the clipboard is first snapshotted **byte-exactly** (every format as raw bytes) and restored
  afterwards (verified for text+HTML+RTF, an image - identical raw DIB SHA-256 - and an empty clipboard); refused for
  terminals (Ctrl+C would interrupt the process: "windowsterminal is a terminal; Ctrl+C would interrupt it, so it was not
  used"), when a modifier key is held, when focus changes during the copy, for non-restorable formats and for clipboards
  over 64 MB; done under Krovi's own clipboard-I/O lock and acknowledged to the clipboard change-gate so Krovi's history does
  not record it. **Known residual risk:** Windows' own clipboard history (Win+V) and third-party clipboard managers may
  record the transient copy; Krovi cannot prevent that. This is exactly why the level is opt-in.
- *Not supported (honest):* selection inside apps that expose neither UIA text nor allow the fallback (many WinForms
  controls, some games/terminals) returns the app name only and the user is told "Nothing is selected in X to ...".
  Selected files in non-Explorer file managers, and desktop icons, are not read.

**2. Voice-to-action (Krovi's own tools only).** The recognizer only produces words; a pure local parser (`intent.rs`) with a
local date/time resolver (`when.rs`: today/tomorrow/weekdays/next week/in N units/month+day/clock times/dayparts; no network, no
locale services) turns them into typed commands. Data actions whose state lives in Rust are executed in Rust and reported
with `executed:true` + a human `feedback` line: `add-reminder` -> `commands::add_calendar_event_from` (validated by the same
`Calendar::add`, persisted, `calendar-changed` emitted) and `media` -> `live::media_transport` / the cached media session.
Everything whose state lives in the UI (shelf is browser-side localStorage; timers/pomodoro; opening a tool; collapsing;
creating a note from `new-note`; Quick Transfer) is only reported (`executed:false`) and the UI acts on it. Nothing is sent
to the phone by voice alone: `transfer-send` only routes the already-selected paths to the Transfer tool.
- Grammar/accuracy trade-off, measured: adding media phrases made ordinary speech trigger media actions ("what time is it in
  Tokyo" was heard as "what song is this" / "what is playing" / "what's playing right now"). The grammar therefore only
  contains the short "what's playing" for the question and multi-word forms for transport ("next track", "pause the
  music"); the text parser still accepts the long forms and bare words when typed. Note and reminder prefixes share one
  embedded-dictation slot (each embedded dictation word costs a language-model load in the recognizer).
- Reminder dates/times are resolved with the PC clock and time zone at the moment of the command. Time-only phrases that have
  already passed today roll to tomorrow; bare weekdays mean the next one strictly after today ("this friday" includes
  today, "next friday" is the following week). Ambiguous clock hours without am/pm default 1-6 -> PM, 7-11 -> AM.

**3. Clean transcription (dictated notes; setting `cleanTranscription`, default on).** Rule-based, deterministic, local,
unit-tested (`clean.rs`, no model): removes fillers (um/uh/er/erm, "you know" and "like" only at clause edges / clearly
filler), repeated words and restarted phrases ("I want to I want to buy milk" -> "I want to buy milk"), applies spoken
punctuation ("period", "comma", "question mark", "new line", "new paragraph"), capitalisation (sentence starts, I,
weekdays/months), sentence-final punctuation (statement -> ".", question words -> "?"), and number sanity ("330" ->
"3:30", "p m" -> "PM"). It never rewrites meaning or adds words. Real recognizer output, before -> after:
"The meeting is is moved to 330" -> "The meeting is moved to 3:30."; "I want to I want to buy milk and eggs" -> "I want to buy
milk and eggs."; "What time is a meeting" -> "What time is a meeting?". **Limits:** the SAPI engine writes a spoken "um/uh" as
the word "a" or drops it, so the filler rule rarely fires on real SAPI output; it is useful for typed/other-engine text and
for the repeats/restarts the engine does keep. "It was you know really good" is deliberately left alone (no commas inserted).
Turning the setting off keeps the engine's raw text (only trimmed and first-letter capitalised).

**4. Privacy settings (persisted in the local settings bag, key `voice.settings`; never sent anywhere).**
`{localOnly:true (locked), storeAudio:false (locked), keepTranscripts (default true), contextCapture:'off'|'selection'|
'selection+clipboard-fallback' (default 'selection'), cleanTranscription (default true)}`. `localOnly` and `storeAudio`
are re-imposed on every read/write (`normalized()`), on both the Rust and TypeScript side, so no stored value, UI bug or
crafted call can unlock them. `keepTranscripts:false` means a **command leaves no transcript trace**: `VoiceResult.text` is
empty and no live `voice-partial` events are emitted for command sessions; the parsed command and its `feedback` are still
reported. (Dictated notes are by definition kept: the note is the product.) `PRIVACY_FACTS` (voice.ts) lists the plain-language
guarantees for the Settings page; each statement is verifiable in this repo.

**Measured (synthesised speech through the real recognizer; same harness as D-012):**
- v1 regression set after the v2 grammar growth: David 18/18 commands, chatter false accepts 1/7, dictation WER 5%;
  Zira 16/18, 1/7, 30%; David 22.05 kHz 18/18, 0/7, 12%; David+noise 17/18, 2/7 (varies run to run), 37%; Zira+noise 16/18,
  1/7, 89%; non-speech (silence/room noise/thump) 0/8 invented results.
- v2 intents (15 phrases: reminders with relative dates, "this" actions, timers, pause timer, media, calendar, close):
  **David 14/15, Zira 12/15.** Misses are recognizer errors, not parser errors: e.g. "...to call mom" heard as "...to call
  of". The body of a reminder/note is free dictation, so it is the weakest part, as in D-012.
- Real context capture (Notepad + SendKeys selection, Explorer, password box, clipboard fallback), in a real session: Notepad
  selection read in ~76 ms; Explorer selection (2 files / 1 file / none) in ~60-68 ms; `off` reads nothing; password field
  refused with a hint; fallback restored the clipboard exactly; Windows Terminal refused. A normal WinForms TextBox returned
  app-only (WinForms exposes no UIA text pattern) - the documented limitation above, not a bug.
- Session start latency ("listening") was 0.45-0.95 s in the last runs versus ~0.1 s earlier, but the machine was at ~91%
  CPU from parallel builds and note mode (no command grammar at all) showed the same figure; treat as load, re-measure on an
  idle machine. Not a grammar regression (merging the dictation slot made no measurable difference).

**Honest limits.** Everything in D-012 still applies (SAPI desktop recognition is an older generation; real microphones are
noisier than TTS; en-US only). Voice-to-action depends on the free-text body being recognised correctly (the reminder
title). The clipboard fallback cannot guarantee that Windows clipboard history stayed clean. Selection reading is best-effort
by app; the user is always told when nothing could be read.


## D-transfer - 2026-11-01 - Quick Transfer: local-network only, unguessable URLs, optional PIN
**Requirement (maintainer):** "Keep it local-network-only for v1 - no third-party cloud relay, no account signup ...
drop file -> QR appears -> scan -> file received." Implemented in `krovi-transfer` (server, tokens, expiry, filename safety,
firewall helpers), `src-tauri/src/transfer.rs` (Tauri layer), `src/lib/transfer.ts` + `src/views/Transfer.svelte`.
Full security surface: `docs/SECURITY-network-audit.md` > "Quick Transfer".

- **Server only while a share is open**, on ONE private IPv4 (`GetAdaptersAddresses`, virtual/VPN adapters excluded, never
  `0.0.0.0`), random port 49152-65534, one server for all shares. No private LAN -> refuse, no wider fallback.
- **Tokens 160-bit from the OS CSPRNG**; everything not on an exact token URL is the same plain 404. `POST` only on receive tokens.
- **PIN: optional, OFF by default.** The token is already unguessable and the QR is on the user's own screen, so a mandatory PIN
  taxes the core flow for little. It protects against the *link* leaking (screenshot, screen-share), not against sniffing; 5 wrong
  tries close the share.
- **Plain HTTP, no TLS.** A self-signed certificate would put a browser warning in front of every phone, defeating "scan -> file".
  Mitigations: short life (10 min default), stop button, live countdown. Documented as a limitation, not hidden.
- **`tiny_http` (as briefed) verified from its source to lack read timeouts, header caps, a thread cap and `Connection: close`
  control.** Accepted for v1 and written down: a hostile device on the same LAN can stall the share server while a share is open. If
  that threat matters, replace the loop with a stricter in-house one (httparse + std::net, ~300 lines). Also found and fixed: its
  default switches responses over 32 KB to chunked encoding (no Content-Length, no progress/resume on phones); we force identity.
- **Expiry lets a transfer already running finish** (a 4 GB file must not die at minute 10); stop, PIN lockout and quit abort.
- **Firewall: never silent.** Krovi does not touch it. After 20 s with no request the UI explains (same Wi-Fi; allow Private) and offers
  one button that, behind UAC, deletes this exe's inbound rules and adds a single rule: this exe, TCP, Private profile, local
  subnet only. Found on the dev PC: the Wi-Fi is classified **Public**, where Windows blocks other devices; the Private-only rule
  does not help there, so the UI detects and says so.
- **Received files get the Internet-zone mark** (`Zone.Identifier`, ZoneId=3) so SmartScreen/Office treat them as downloads; names are
  sanitised (no traversal, no device names, no bidi tricks) and never overwrite (`name (1).ext`, atomic reservation).
- **Native file dialog via `rfd` inside the crate** (parented to the Island window) instead of `tauri-plugin-dialog`, so no
  Cargo.toml/capabilities change was needed outside the transfer files; the webview `<input type=file>` cannot give real paths.
- QR is rendered by a ~30-line run-length SVG writer (4 KB vs 10 KB for the stock per-module renderer, no seams when scaled).
- Verified with a real Android 14 emulator as the phone (SHA-256 both directions); a physical-phone scan and the Windows Firewall
  behaviour for a truly separate device remain for the maintainer.

## D-personalization - 2026-11-01 - Theme tokens + idle pill companion (cost-bar driven)
**Requirement (maintainer):** "Architect this as a token/variable system so adding future themes is cheap ... Keep this
lightweight - it must not add meaningfully to idle CPU/RAM, verify this when done." Files: `src/lib/theme.ts`,
`src/lib/idle.ts`, `src/styles/themes/**`, `src/components/idle/**`; how-to in `src/styles/themes/README.md`.

- **A theme = one css file + one registry line.** Pure token overrides under `:root[data-theme='<id>']`; the files are
  pulled in by a glob in `theme.ts`. `startTheme()` stamps `data-theme=<id>` AND `data-scheme=dark|light`; anything that
  needs light-vs-dark keys off `data-scheme`, never a theme id.
- **Cross-fade without per-element rules:** every colour token is `@property`-registered in `themes/_base.css`, so a switch
  is ONE transition on `:root` (armed ~150 ms after launch, colour only, no layout). Reduced motion zeroes it.
- **Contrast is tested, not assumed.** `themes.test.ts` composites the translucent glass like the browser and checks 70
  pairs per theme (text tiers on panel + 3 surface layers + popover, pill text, eyes, accent, danger, buttons) over the
  theme's own page AND the native glass over pure black and pure white wallpaper. All six pass AA.
- **Graphite/Porcelain are monochrome with the ember accent** (maintainer correction). Graphite `#ff8a4c`. Porcelain uses
  `#bc3f09`, one notch deeper than the brand `#c8440a`, because `#c8440a` measures 4.21:1 on the panel and 3.6-4.0:1 on its
  own `--accent-soft` tint (tag text) - short of AA. Swap back is one line if the maintainer prefers the exact hex.
- **Companion cost, learned the hard way (Edge 156 = WebView2's engine, no DevTools attached, renderer + GPU summed):**
  smooth 150-400 ms transitions cost 4.8% of a core while the eyes are showing, because the GPU process pays ~4 ms for
  every presented frame and a compositor animation keeps ticking while it is alive. So every move is short and STEPPED
  (a glance is a single jump, a blink is 3 states, poses are 3 steps), lids only ever carry Web Animations, and the
  moving parts keep `will-change` layers (removing them was worse: 1.55% vs 1.09%).
- **Attention span:** the cadence relaxes up to 3x over 45 s and after `dozeAfterMs` (curious 60 s, calm 90 s, sleepy 25 s,
  hyper 40 s) the eyes close and the scheduler returns `Infinity`: no timer, no frames (measured 0.0 ms CPU / 30 s). A
  `shell-cursor` event wakes them. Under `prefers-reduced-motion` the companion never appears at all.
- **Measured (30 s windows, 10 s after the eyes appear, one core = 100%):** companion off 0.05-0.10%; curious 0.72%;
  calm 0.93%; hyper 1.19%; asleep 0.0%. Renderer working set 111.0 MB off vs 108.8-109.3 MB on (no measurable RAM). Raw
  scripts: `qa/personalization/{measure-edge.ps1,sample-cpu.ps1,analyze-trace.mjs}`.

## D-014 · 2026-11-01 · Voice v3: local OS actions from a closed grammar (open folder/app, find, list; exact reminder times)
**Requirement (maintainer, via the orchestrator):** exactness, not vague "natural language"; fully on-device SAPI, no cloud.
**Outcome: everything is local and built.** User-facing contract: docs/SPEC.md 11.13 (every phrasing, plus "Not supported").
Network evidence: docs/SECURITY-network-audit.md "Voice v3". Files: `krovi-voice/src/{catalog,osintent,actions,listing,os}.rs`,
`src-tauri/src/voice.rs`, `src/lib/voice.ts`.

**Design.**
- *A catalog, not a search.* SAPI can only recognise words it was given, so the folders and apps are enumerated from Windows and
  become words of the command grammar when a session starts (`Catalog` -> `grammar::command_templates`): Known Folders
  (`SHGetKnownFolderPath`, so OneDrive-redirected Desktop/Documents/Pictures resolve correctly - on this PC Desktop is
  `OneDrive\Pictures\Desktop`) + "Krovi downloads" (`Downloads\Krovi`) + Quick Access (Shell namespace
  `shell:::{679f85cb-...}`, works: pinned and frequent folders incl. drive roots); apps from `shell:AppsFolder`
  (`FOLDERID_AppsFolder`) + Start-menu `.lnk`. The parser resolves what was heard back to exactly one entry (exact name > name without
  brand prefix > spacing variants > all words contained, fewer extra words wins > one typo in a long word), or to "not found",
  or to "ambiguous" (equal best matches: **nothing is done**, the candidates are listed). Nothing is ever resolved against the disk.
- *Clean-up of the app list:* 236 entries on the test PC before, 230 after dropping web links (`https://gitforwindows.org/faq`) and
  documents (`ReleaseNotes.html`); uninstallers/readmes/help entries are dropped too. A spoken name can therefore never open a web
  page. Capped at 400 apps / 80 folders.
- *Free text only where needed:* the search term, like note and reminder bodies, uses the SAPI embedded-dictation slot (all three
  prefixes share ONE slot - each costs the recognizer a language-model load).
- *Launch/open* with `ShellExecuteW` on `shell:AppsFolder\<id>` / the directory / the `.lnk` (what `explorer.exe shell:AppsFolder\...`
  does, without a second process or command-line quoting), on its own STA thread (the speech thread is an MTA). `open_folder` and
  `launch_app` re-validate their input (existing directory; AppsFolder id or existing `.lnk`).
- *Find* = OLE DB `Search.CollatorDSO` through windows-rs: `SELECT TOP 20 System.ItemPathDisplay, System.ItemName FROM SystemIndex WHERE
  scope='file:' AND System.FileName LIKE '%term%' AND NOT ... '%\AppData\%' ORDER BY System.DateModified DESC`. Typical 0.4-1.3 s
  (no-hit queries too). The term is quote-doubled, `%` stripped. Honest limit: it finds what Windows has indexed - on the test PC new
  files in Downloads and Documents were still not in the index after 9 minutes (indexer idle), so tests used already-indexed files.
- *List* = `std::fs`, one directory, hidden/system skipped, count + total size of the files directly inside + newest 3 in the sentence
  + 50 rows; scan cap 20,000 entries ("at least ...").
- *Cost:* enumerating 230 apps takes 1.7-2.7 s (machine under load), so the catalog lives in `VoiceHub`: warmed 4 s after launch,
  refreshed in the background when 10 min old, and a session NEVER waits for it (before the first scan finishes only the Known Folders
  are offered). The grammar is rebuilt per session from the cached catalog; if the engine rejects a name the grammar falls back to the
  built-in commands only.
- *Precedence (so a spoken word means one thing):* a Krovi tool called exactly that wins for `open/show/go to/...` ("open settings" =
  Krovi Settings, "open music" = Media tool); `launch/start/run` prefer the installed app ("launch settings" = Windows Settings,
  "launch notepad"); a trailing "folder"/"app" forces the kind ("open music folder"). "notepad" was removed from the Notes tool's
  synonyms so "open notepad" reaches the Windows app.
- *Clicking a result row* (`voice_open_result`): only paths of the latest result are accepted; folder = open, file = reveal in Explorer
  (`/select,`), never run; app = launch.

**Grammar cost found by measurement (search prefixes).** The Phase 1 regression suite is the guard rail. Putting a bare "find *+" /
"look for *+" / "locate *+" path (embedded dictation after a one-word opening) into the grammar made the recognizer swallow noisy audio
that was really another command: female synthetic voice + white noise went from 16/18 commands understood to 9/18 (clean voices
unchanged). Sets tried (zira-noisy / david-noisy commands understood, out of 18): none 16/17; "find"+"search for" 9/17; "find"+"search for"
+"look for" 9/17; {find file, find files, find the file, search for, search my files for} 15/17; that set + "look for"+"locate" 14/16;
**that set + "find me" 15/17 (chosen)**. So the spoken search openings are `find file|find files|find the file|find me|search for|search my
files for`; the parser still takes bare "find ..." typed. The same suite also showed adding the longer "remind me at/in/..." openings
brought nothing, so they were not kept.

**Safety gate found by measurement (app launches).** In command mode the closed grammar always snaps to the nearest allowed phrase,
so ordinary sentences can come back as commands ("start the car" -> "start the Claude", "launch the rocket" -> "launch the Reddit"):
launching a program is the one false accept with a visible side effect. SAPI's per-phrase confidence separates them for a clear
voice: genuine "launch X"/"open X" scored 0.92-0.95 (male synthetic voice) and chatter snapped to an app scored 0.54-0.73. Apps now
launch only at confidence >= 0.75; below that nothing is launched and the answer says so ("I am not sure I heard that right, so I did
not launch Notepad. Say it again."). Cost: the female synthetic voice, which this recognizer scores lower overall (0.62-0.95 on
correct phrases), is asked to repeat 4 of 10 app names. Folder opens, listing and search have no gate (no lasting side effect).

**Reminders.** Time phrases are parsed locally (SPEC 11.13); every documented example is a unit test. Real bugs the new tests found and
fixed: "tonight at 8" resolved to 08:00 (a daypart word now decides AM/PM for an hour spoken without am/pm; "tomorrow morning at 3" is
03:00); "in half an hour" / "in a quarter of an hour" / "in an hour and a half" were not understood. The confirmation states the exact
resolved date and time ("Reminder set for today, Sun 1 Nov, 5:00 PM: Call mom"; no time -> "..., no alert time: ..."). The reminders
scheduler is woken by `add_calendar_event_from` itself (it goes through `reminders::add_event`).

**Measured (synthesized speech through the real recognizer + this PC's real catalog: 12 folders, 230 apps; 41 phrases per voice).**

| | David | Zira |
|---|---|---|
| open folder (8) | 8/8 | 8/8 |
| what's in folder (4) | 4/4 | 4/4 |
| open/launch app (10) | 10/10 | 6/10 (4 correctly heard but held by the 0.75 gate) |
| find (5) | 5/5 | 4/5 ("search for tax return" not recognised) |
| reminder time phrases (8), date+time right | 7/8 | 1/8 |
| reminder time phrases, also title exact | 6/8 | 1/8 |
| chatter (8) correctly ignored | 5/8 | 6/8 |

Before the gate: David chatter 4 false accepts incl. an app launch, Zira 3 incl. an app launch. Chatter that still gets through only
opens a Krovi tool or lists a folder ("what is in the fridge" -> "what is in the drive" -> Google Drive listing). Reminder failures are
recognizer errors in free text ("call mom" -> "call a", "nine" -> "night") and, for the female voice, the short opening "remind me"
being heard as "find"/"run"/"add a note": the one test phrase that opens with "set a reminder for ..." was recognised correctly by both
voices (n=1, so only a hint), and adding longer "remind me at/in/on/tomorrow ..." openings to the grammar did not help the female voice
(they were removed again).
A closed grammar for the time words themselves (so only the title is free) would likely raise this; not built.
Real executions (no GUI): "open downloads" opened a real Explorer window, "open notepad" launched Notepad (both closed afterwards),
"what's in downloads" returned 1026 items / 7.7 GB / 50 rows, "find invoice" 4 hits, "find krovi" 1 hit (Krovi.lnk).

**Limits.** SAPI accuracy limits from D-012 apply; app names are read from letters (odd spellings may be misheard); the female-voice
numbers show the engine's weakness more than a real microphone would. Everything in SPEC 11.13 "Not supported (yet)" is out of scope
on purpose (no recents/files, no subfolders, no content search, no acting on found files, no system actions).

## D-reminders · 2026-11-01 · Real reminders and alarms: one Rust scheduler, honest about Do Not Disturb

**Problem.** A calendar entry with a time was stored and never rang. **Decision.** A timed event *is* a reminder; a Rust background
thread (`reminders.rs`) rings it at its time with a real Windows toast, whatever the window mode (pill, panel, hidden).

- **Data.** `CalendarEvent`/`CalendarDto` gain `remind` (default on when a time is set), `sound` (default on), `firedAt`, `snoozedUntil`,
  `missed`. SQLite schema v3 adds the columns in place (existing rows kept; timed rows become reminders). A time already past by > 90 s is
  stored with the alert off (it could never ring).
- **No busy-poll.** The thread waits on a condvar for `min(time to the next alert, 30 s)`; add / remove / snooze / dismiss wake it at once
  (`reminders::wake_scheduler`). The 30 s cap is what picks up clock changes, DST jumps and sleep/resume (worst case ~30 s late after a resume).
- **DST-correct, testable.** Local wall clock -> UTC uses the OS zone (`SystemTimeToTzSpecificLocalTime`) through a `ZoneOffset` trait; the
  gap (02:30 on a spring-forward night rings at 03:30) and overlap (rings once, at the first 01:30) rules and every state transition live in
  `krovi_core::calendar` with tests, because the app crate's own test binary cannot start on Windows.
- **Never twice.** `collect_due` marks a reminder fired in the same step it hands it out, and the mark is **persisted before the toast is
  shown**: a crash can lose a toast, never repeat one (a failed save is retried on the next pass).
- **Closed at the time.** On the first pass anything already due fires once, `missed:true` when > 60 s late; > 24 h overdue is only marked;
  4+ at once share one summary toast.
- **Sound.** `PlaySoundW("Notification.Reminder")` (fallback `SystemAsterisk`); the toast itself is silent so it never doubles. Skipped while
  Do Not Disturb / alarms-only is on: that is the user's explicit request to Windows.
- **Do Not Disturb is detected, not guessed.** `SHQueryUserNotificationState` keeps saying "accepts notifications" while Do Not Disturb is on
  (observed on the maintainer's PC), so the Focus Assist WNF state (`WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED`) is read instead. With it on,
  Windows files the toast in the notification centre with no banner and no sound; `reminders_status.environment` says so, the Calendar form
  shows a one-line warning, and the in-app banner (`reminder-fired`) still appears in the panel and on the pill.
- **Toast identity.** The plugin sets the AppUserModelID to the identifier (`tech.krovi.app`) unless the exe is in a `target\debug|release`
  folder. Installed: the NSIS Start-menu shortcut carries that AUMID (checked on this machine) so toasts read "Krovi". Dev: "Windows PowerShell".
  Windows also accepts a toast under an unregistered `tech.krovi.*` id (portable exe), filed under that raw id.
- **Toast buttons** (Snooze 5 / 10, Dismiss; body click opens the Calendar) are handled in Rust through the plugin's `on_action`. The plugin
  reports a click only while the banner is still on screen (not from the notification centre), so the Calendar always offers Snooze 5/10 for 30 min.

**Limits (known, by design of the plugin/Windows).** Banners last ~5 s (`duration=short`); a toast that stays until dismissed, or that breaks
through "alarms only", needs `scenario="reminder"/"alarm"`, which `tauri-plugin-notification` does not expose on Windows (a direct dependency on
`tauri-winrt-notification` would). Verified: toast delivery, exact-time firing (8-25 ms late), persistence, missed/stale/grouped, snooze,
dismiss, voice path, migration of a real v2 database. Not verified live: the toast *buttons* and the on-screen *banner* (both need Do Not Disturb off).