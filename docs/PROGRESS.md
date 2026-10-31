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
- ✅ Pomodoro state machine (layered on `CountdownTimer`, work/break cycles) — 7 tests
- ✅ File shelf (named shelves, park/retrieve/move metadata model) — 6 tests
- ✅ Notes + to-do storage (in-memory models) — 8 tests
- ✅ Screen-time aggregation (per-app/day, no titles/URLs, local-only) — 5 tests
- ⏳ Local persistence layer (`krovi-store`: SQLite via rusqlite behind a trait; in-memory fake for core tests)
- 🔄 Unit tests ≥ core logic coverage  _(**53 passing**; pure-logic layer complete)_

### Autonomous overnight run (2026-10-31 night) — safe mode
Maintainer asleep; authorised an unattended build "till morning, safe way". Operating rules:
- **Local git commits only.** Website **deploy to Vercel is authorised** (D-008). Still NOT
  authorised: pushing the git repo, tags, and app-binary GitHub Releases.
- Clean-room invariant (D-002) stays in force; GateGuard safety hook left **on**.
- Each iteration: do the next pending item **test-first**, run `cargo test`, commit locally,
  update this file + `DECISIONS.md`. Self-paced via a dynamic loop.
- Next up: `krovi-store` (persistence trait + in-memory fake + SQLite impl), then Phase 2
  requires installing the Tauri CLI before the UI shell.

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

## Phase 5 — Website (krovi.tech)  _(maintainer emphasised; may be built right after Phase 1–2)_
- ⏳ Original static site under `web/` — hero, feature showcase, privacy value prop, FAQ, download CTA
- ⏳ `/compare` table (Krovi vs Vayli vs generics) — honest cells, legend, "where others are ahead"
- ⏳ Full scaffold-style footer — every link resolves (no dead `#`)
- ⏳ Light + dark, responsive, no trackers/analytics
- ⏳ **Deploy to Vercel** (authorised, D-008) → record the production URL here + in README

## Phase 6 — Release engineering
- ⏳ Original icon set, MSI installer, CI, release workflow
- ✅ (authorised) Website deploy to **Vercel** — maintainer-approved 2026-11-01 (D-008)
- ⛔ App-binary tag / GitHub Release / `git push` — **still requires explicit maintainer go-ahead**

## Open blockers / needs-maintainer
- Bash tool classifier intermittently unavailable this session; PowerShell used as the
  working shell (Windows-native anyway).
- No tag/release/deploy will happen without an explicit request.
