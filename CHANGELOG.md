# Changelog

All notable changes to Krovi will be documented in this file.

## v0.3.0

### Added
- Agent Mode: intent recognition, planning, permission prompts, confirm cards, task registry and hotkey-driven workflow
- Circle to Ask: temporary visible screen awareness, per-monitor fullscreen support
- Read aloud: on-device text summarisation with optional bring-your-own AI key
- Settings → AI (BYOK): support for Anthropic, OpenAI, Gemini (requires user's own AI Studio key), Groq and Snowflake (experimental, not marketed)
- Snowflake: experimental AI provider — not marketed, use at your own discretion
- Clean My OS: scan, review real paths and sizes, confirm, move to Recycle Bin
- Extras: additional system utilities and configurations
- v0.3 product audit and CI workflow
- Native timers/pomodoro alarms so timers ring even when Krovi is hidden
- File Shelf and World Clocks now live in the Rust store (persisted across restarts)
- New themes: Obsidian, Linen, Pixel Meadow (renamed from Minecraft)
- Blocky structural shared across Voxel and Pixel Meadow themes
- Agent-driven voice commands and confirmation cards
- Circle to Ask per-monitor fullscreen detection
- System-aware moods from real battery, RAM and CPU readings
- Dictation pipeline: WASAPI capture, VAD, Whisper engine, clipboard-free typing
- Global dictation wiring — shortcut, lazy engine, model download, mic test
- Dictation settings, mic test and pill listening indicator
- Sectioned local summariser (Explain) that reads the whole document
- Pupils follow the cursor across the whole screen; idle sampler sleeps while hidden
- Verified on-demand Whisper model catalog and downloader
- Local calendar days, no sleep/lock/idle credit, incremental writes, 90-day retention
- Calculator removal: standalone tool removed; inline math kept in palette
- Windows native build workflow

### Changed
- BYOK AI providers: Gemini now requires the user's own AI Studio key and their consent
- Snowflake labelled experimental — not marketed
- DeepSeek API research documented (not included in release)
- Calculator tool removed; inline math retained in the command palette
- The Island morphs into a 19-tool panel (up from 15)
- Themes: Voxel and Pixel Meadow share _blocky.css structural layer
- Persistence: File Shelf and World Clocks moved to Rust store
- Voice: global dictation wiring and on-device pipeline overhaul
- Explain: sectioned summarisation with whole-document context
- Read aloud: optional BYOK key stored in Windows Credential Manager
- Agent: honest registry with permission-based confirm cards
- Circle to Ask: temporary screen awareness with per-monitor support
- Security audit: network calls documented and verified (docs/SECURITY-v03-audit.md)

### Security
- Network calls audit: all outbound calls grepped and documented in docs/SECURITY-v03-audit.md
- No DeepSeek integration
- Gemini requires user-owned AI Studio key — no automatic account creation
- Snowflake experimental: not marketed, use at your own discretion