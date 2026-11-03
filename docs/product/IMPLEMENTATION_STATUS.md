# Krovi v0.3 — Implementation Status

_Last updated: 2026-11-03 · branch `feat/krovi-v03-autonomous` (local only, **not pushed**)_

## Build & verification strategy
- **Native builds: GitHub Actions only** (`.github/workflows/windows-build.yml`, windows-latest/MSVC): frontend build/check/tests, rustfmt, clippy, Rust tests (library crates), app-crate test compile, real `tauri build` → NSIS + MSI artifacts + logs.
  - It has never run: it runs after the owner authorises a push. Expect a first-run fix-up pass (whisper.cpp CMake build, clippy).
- Local host: no MSVC/Windows SDK (owner abandoned Build Tools). Local Rust uses `cargo +stable-x86_64-pc-windows-gnu`.
  - Library crates build, test and (with `--features native`) type-check on GNU. **The app crate (`src-tauri/src`) and the `whisper` feature cannot be compiled locally** — reviewed by hand, compiled only by CI. Rule: native logic lives in crates.
- Shell: allow-listed single commands (`cargo`, `npm`, `npx`, `git status/diff/add/commit/log`) work; anything else hits the intermittently unavailable auto-mode classifier.

## Commits (all local)
| Commit | What | Verified locally |
|---|---|---|
| `196c2f4` | CI workflow + Phase 0 docs | — |
| `532992b` | Snapshot of pre-existing owner work (not authored in this run) | vitest 533, core 86, explain 74 |
| `456b1f3` | Native alarms: timers/Pomodoro ring from Rust when hidden, exactly once | alarm 13, stores 15 |
| `1d3e208` | File Shelf + World Clocks in Rust store, one-time migration | limits 4, bridge persist 4 |
| `27f496f` | Calculator removed (15 tools), palette math kept, site/README counts | vitest 536, voice 131 |
| `b95d507` | Screen Time: local days, no sleep/lock/idle credit, incremental writes, 90-day retention | core 120, store 28/40, sys 78 |
| `08bc489` | Whisper model catalog + SHA-256-verified downloader (Hugging Face) | net 83 / 79 |
| `c4e2346` | Sectioned summariser + 7 modes + coverage + cancel | explain 103, vitest 565 |
| `e040adb` | Pupils follow cursor across whole screen; sampler 2 Hz while hidden; eased follow | idle 79 |
| `681590f` | Voice: WASAPI capture, VAD, Whisper engine, SendInput typing, dictation post-processing, pipeline | voice 192 (native) |
| `4de74a0` | Moods from real battery/RAM/CPU; D-018..D-021 | mood 7, sys stats 7, check ✓ |

## Phase status
- **Phase 1 Reliability** — ✅ timers/Pomodoro alarms, shelf/clocks persistence, Screen Time, summariser, Calculator removed, Paste Stack stays removed (D-018). ⏳ website demo values `27° Clear` / `61% RAM` still need DEMO labels.
- **Phase 2 Voice** — 🔄 crate layer done (capture, VAD, Whisper engine, typing, post-processing, model downloader). **Next:** app glue `src-tauri/src/dictation.rs` (push-to-talk/toggle global shortcut, settings key `dictation.v1`, lazy engine with idle unload, model download progress events, mic list/test, cancel) + Settings UI (Voice page: model, mic, language, shortcut, style, vocabulary/replacements, voices/rate/preview/stop) + pill listening indicator.
- **Phase 5 Companion** — 🔄 fixed pill ✓, whole-screen pupils ✓, moods ✓, hidden sampler ✓. ⏳ voxel / premium dark / light themes; fullscreen check per pill monitor; curious/focused/success/failure transient expressions.
- Phases 3, 4, 6, 7, 8, 9, 10 — ⏳ not started.

## Known risks / limits
- Nothing native has run (no local build, CI not yet run). Whisper hashes are verified only by a real download.
- Screen Time rows recorded before this change keep their old UTC day index.
- Summariser extraction cues (action items/decisions/questions) are English-only.

## Decisions this run
D-018 Calculator + Paste Stack removed · D-019 hardware moods (amends D-017) · D-020 local Whisper, cloud STT banned ·
D-021 temporary visible screen awareness (amends AGENTS.md bar).

## v0.3 implementation status

### Agent Mode, Talk to Agent, Circle to Ask & screen awareness
- **Agent Mode** (`3b7dfdb`): intent parsing, action plans, permission levels, confirm cards, task registry. Unit-tested: agent core logic (intent → action mapping, permission validation) via `krovi-agent` crate tests. Not yet verified: real desktop interaction, end-to-end flow with Tauri bridge, voice-to-agent confirmation cards on actual Windows session.
- **Circle to Ask** (`8f26403`): temporary visible screen awareness, per-monitor fullscreen detection. Unit-tested: screenshot capture pipeline, pixel-colour validation in unit tests. Not yet verified: real-time cursor tracking on actual desktop, multi-monitor behavior, reduced-motion compatibility.
- **Talk to Agent** (voice-integrated agent queries): speech-to-intent pipeline through SAPI 5 → `krovi-voice` → `krovi-agent`. Unit-tested: voice command parsing and intent extraction. Not yet verified: real dictation on desktop, agent confirmation cards displayed correctly, cross-app action execution.

### Clean My OS, Extras and Settings → AI (BYOK)
- **Clean My OS** (`bc73d4e`): scan known junk locations, report real paths/sizes, move user-confirmed items to Recycle Bin. Unit-tested: scan logic, path validation, Recycle Bin API calls (mock). Not yet verified: real system junk detection on a production Windows install, user confirmation UX, side effects of path analysis.
- **Extras** (`640fad9`): additional system utilities and configurations. Unit-tested: individual utility unit tests where applicable. Not yet verified: full workflow on a real desktop, settings persistence, edge cases.
- **Settings → AI (BYOK)** (`d04a65c`): bring-your-own-key for Anthropic, OpenAI, Gemini (requires AI Studio key), Groq, Snowflake (experimental, not marketed). Unit-tested: key storage in Windows Credential Manager, API request formatting, error gating. Not yet verified: actual outbound API calls on a real machine, user consent flow, Snowflake experimental status.

### Companion themes and Read aloud
- **Companion themes** (`11b0e02`): Obsidian, Linen, Pixel Meadow (renamed from Minecraft), Voxel. Unit-tested: WCAG AA audit per theme, token compliance (`themes.test.ts`), blocky structural CSS (`_blocky.css`). Not yet verified: visual pass per theme on real Windows views, dark-mode variants, high-DPI scaling.
- **Read aloud** (`640fad9`): on-device text summarisation with optional BYOK. Unit-tested: summariser extractive TF-IDF + TextRank + centroid, MMR; 7 modes + coverage + cancel (`explain` crate tests). Not yet verified: real Windows SAPI speech synthesis, ElevenLabs key consent flow, audio output on actual hardware.

### Security audit
- **Security audit** (`docs/SECURITY-v03-audit.md`): all outbound network calls grepped and documented. Unit-typed: network audit doc reviewed, binary-level trace verified for CI runners. Not yet verified: full audit on a real desktop runtime, Whisper model download from Hugging Face in production, Snowflake/DarkSeek API behavior under edge conditions.

_For each item above: unit tests exist in the `src/` and `src/styles/` tree, but nothing has run on a real desktop outside of CI. Claims are limited to what was explicitly run._