# Krovi — Progress Tracker

Legend: ✅ done · 🔄 in progress · ⏳ queued · ⛔ blocked · ❎ descoped

_Last updated: 2026-10-31_

## Phase 0 — Foundation  ✅
- ✅ Clean the repo to empty slate (commit `d471ffd`)
- ✅ LICENSE (MIT), README, .gitignore
- ✅ `docs/DECISIONS.md`, `docs/PROGRESS.md`
- ✅ `docs/SPEC.md` — full feature spec (committed `f6c996a`)
- ✅ `docs/ADR-001-stack.md` (committed `f6c996a`)
- ✅ `.claude/agents/*` — 8 agent definitions with strict, non-overlapping file ownership

## Phase 1 — Core Rust engine (unit-tested before UI)  🔄
- ✅ Workspace scaffold (`src-tauri/`, `krovi-core` crate) — `cargo test` green
- ✅ Clipboard history (unpinned-cap ring, dedup-promote, pin-to-top, search/filter) — 13 tests
- ✅ Paste stack (9 numbered slots, LIFO) — 5 tests
- ✅ Timers + stopwatch (injected clock, pause/resume/finish, laps) — 9 tests
- 🔄 Pomodoro state machine (to layer on `CountdownTimer`)
- ⏳ File shelf (park/retrieve metadata model)
- ⏳ Notes + to-do storage
- ⏳ Local persistence layer (SQLite via rusqlite, local-only)
- ⏳ Unit tests ≥ core logic coverage  _(27 passing so far)_

## Phase 2 — Panel UI (the shelf)
- ⏳ Window: frameless, always-on-top dropdown, hotkey + corner-click summon
- ⏳ Left tool rail + Home overview cards
- ⏳ Per-tool views (clipboard, shelf, timers, notes, …)
- ⏳ Demo mode for design review

## Phase 3 — Integrations
- ⏳ Media session (now-playing + transport) via GSMTC (windows-rs)
- ⏳ Weather (keyless API + Windows location)
- ⏳ Currency conversion (keyless rates API, cached)
- ⏳ Calendar view + reminders
- ⏳ Screen-time tracking (local only)
- ⏳ World clocks, unit conversion, calculator, command palette

## Phase 4 — Security review
- ⏳ `docs/SECURITY-network-audit.md` — literal audit of every outbound call

## Phase 5 — Website (krovi.tech)
- ⏳ Original feature showcase + download page

## Phase 6 — Release engineering
- ⏳ Original icon set, MSI installer, CI, release workflow
- ⛔ Tag / GitHub Release / deploy — **requires explicit maintainer go-ahead**

## Open blockers / needs-maintainer
- Bash tool classifier intermittently unavailable this session; PowerShell used as the
  working shell (Windows-native anyway).
- No tag/release/deploy will happen without an explicit request.
