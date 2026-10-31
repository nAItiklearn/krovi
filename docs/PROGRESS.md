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
- ✅ Local persistence (`krovi-store`): `Store` trait + in-memory `MemStore` — 8 tests. **SQLite (`SqliteStore`, rusqlite bundled) deferred** — host is `x86_64-pc-windows-gnu` with 32-bit-only MinGW gcc ("64-bit mode not compiled in"); behind an off-by-default `sqlite` feature, needs an MSVC / 64-bit C toolchain to activate (see blockers).
- ✅ Unit tests ≥ core logic coverage  _(**61 passing**: 53 core + 8 store)_

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

## Phase 5 — Website (krovi.tech)  ✅ built & deployed (one toggle from public)
- ✅ Original pure-static site under `web/` (no trackers, system fonts) — hero, features, privacy, about, FAQ, download CTA
- ✅ `/compare` table (Krovi vs Vayli vs generics) — honest ✓/✗/Paid/Partial/Not-verified cells, legend, candid "where Krovi is behind" section (clean-room: Vayli unknowns = "Not verified")
- ✅ Full scaffold-style footer — every link resolves (0 dead `#`)
- ✅ Light + dark, responsive, accessible
- ✅ **Deployed to Vercel:** https://web-flame-kappa-83.vercel.app  ⚠️ currently gated by Vercel Authentication — see blockers to make it publicly viewable

## Phase 6 — Release engineering
- ⏳ Original icon set, MSI installer, CI, release workflow
- ✅ (authorised) Website deploy to **Vercel** — maintainer-approved 2026-11-01 (D-008)
- ✅ (authorised 2026-11-01, D-010) `git push`, tag, and GitHub Release — maintainer approved. The release installer is built by **GitHub Actions on a windows-latest (MSVC) runner** on a pushed `v*` tag, since the local toolchain can't build the native app.

## Open blockers / needs-maintainer
- **Website is deployed but not yet public.** The Vercel project has "Vercel Authentication"
  deployment protection ON, so visitors must log in. Turning it off was blocked by the local
  safety classifier ("Security Weaken"). To make https://web-flame-kappa-83.vercel.app public:
  either (a) Vercel dashboard → Project "web" → Settings → Deployment Protection → turn OFF
  "Vercel Authentication"; or (b) add a permission rule for
  `mcp__plugin_vercel_vercel__update_project` and I'll flip it. (Optional later: add the
  krovi.tech custom domain and rename the project to "krovi".)
- **64-bit C toolchain missing.** Host Rust is `x86_64-pc-windows-gnu` with a 32-bit-only MinGW
  gcc. This blocks (1) the SQLite backend (`--features sqlite`) and likely (2) a full native
  Tauri build. Fix: install **Visual Studio Build Tools (C++ workload)** and
  `rustup default stable-x86_64-pc-windows-msvc` (or install a 64-bit MinGW). Until then: core
  uses `MemStore`, and the panel UI ships as a runnable browser (Vite) demo.
- `git push`, tag, and GitHub Release are now **authorised** (D-010); the release installer is built
  by GitHub Actions on an MSVC `windows-latest` runner when a `v*` tag is pushed.
