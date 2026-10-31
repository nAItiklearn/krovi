# Krovi — Product Specification

_Status: draft, living document. Last updated 2026-10-31._

Krovi is a free, open-source, Windows-first **"everything shelf"**: a persistent
quick-access panel that gathers small daily tools into one dropdown summoned by a global
hotkey or a screen-corner click.

**Provenance tags** used throughout:
- `[ref]` — capability inferred from the **public** product listing / user-supplied
  conceptual landing screenshots of the paid reference app. (Clean-room: public material
  only; no binary was inspected — see `DECISIONS.md` D-002.)
- `[brief]` — specified directly in the project brief.
- `[original]` — Krovi's own addition, not present in the reference.

---

## 1. Vision & positioning

A single panel that replaces a dozen tiny utilities and browser tabs. Krovi's identity
rests on three commitments that are also its differentiators:

1. **Free + open source, forever** `[brief]` — MIT, no key, no trial, no paywall. This is
   the explicit contrast with the reference app's free-trial-then-paid model.
2. **Verifiable privacy** `[brief]` — local-first; no account, ads, or telemetry. Unlike a
   closed app that merely *claims* privacy, Krovi documents every outbound call and ships
   the source so the claim is checkable (`docs/SECURITY-network-audit.md`).
3. **Original design** `[original]` — its own look (left rail, own palette/icons), not a
   clone of any existing panel.

## 2. Non-goals

- Not cross-platform yet (Windows only; architecture keeps the core portable).
- No cloud sync, accounts, or "pro" tier.
- No kitchen-sink feature bloat — additions must fit the "quick-access shelf" identity.
- No bundled third-party copyrighted assets or reverse-engineered material.

## 3. Design principles

- **Local-first:** all user data lives on disk in a local store; the app is fully
  functional offline (minus live weather/FX).
- **Glanceable:** a Home overview surfaces the most useful state at once.
- **Keyboard-driven:** summon, search, and switch tools without the mouse.
- **Auditable:** small dependency surface; network calls isolated behind one module.

## 4. Interaction model

- **Summon:** global hotkey (default `Ctrl+Alt+Space`, rebindable) **and** a configurable
  hot-corner click. `[ref]`
- **Panel anatomy** `[original layout]`:
  - **Left vertical tool rail** — icons to switch tools (reference uses a bottom rail; Krovi
    diverges to a left rail for its own identity — `DECISIONS.md` D-005).
  - **Header** — current tool name, date/clock, quick settings, close/pin.
  - **Content area** — the Home overview (card grid) or a focused tool view.
- **Dismiss:** click-away, `Esc`, or hotkey toggle. Optional pinned (stay-open) mode.
- **Drag affordances** `[ref]`: drag files out of the File Shelf into any app; drag
  clipboard/paste-stack items straight into the focused app.

## 5. Core features (reference-equivalent, rebuilt original)

| # | Feature | Tag | Summary |
|---|---------|-----|---------|
| C1 | **Home overview** | `[ref]` | Card grid summarising screen-time, system, weather, focus, clipboard, notes. |
| C2 | **Clipboard history** | `[ref][brief]` | Searchable history of recent copies (text/image/link), with pin & filter tabs. |
| C3 | **Timers & stopwatch** | `[ref][brief]` | Multiple countdown timers + a stopwatch; notifications on completion. |
| C4 | **File shelf** | `[ref][brief]` | Drag files in to park temporarily; drag out when needed. Named shelves. |
| C5 | **Media now-playing** | `[ref][brief]` | Reads the system media session; shows track + transport controls. |
| C6 | **Calendar & reminders** | `[brief]` | Month view + lightweight local reminders with notifications. |
| C7 | **Weather** | `[ref][brief]` | Local current + short forecast; **keyless** API + Windows location, no account. |
| C8 | **Quick notes** | `[ref][brief]` | Fast scratch notes with autosave. |
| C9 | **World clocks** | `[brief]` | Multiple time zones at a glance. |
| C10 | **Unit & currency conversion** | `[ref][brief]` | Offline unit conversion; currency via cached keyless rates. |
| C11 | **Screen-time tracking** | `[ref][brief]` | Local-only active-app/time tracking; never reported anywhere. |
| C12 | **Settings** | `[brief]` | Hotkey, hot-corner, startup, theme, per-tool toggles, data location. |

### Per-feature notes
- **C2 Clipboard:** ring buffer (configurable cap, default 200), dedup, pin-to-top,
  type filters (All/Text/Image/Link), substring search. Image entries stored as files
  referenced from the DB. Clearable; optional ignore-list for sensitive apps.
- **C4 File shelf:** stores references/copies of parked files under the Krovi data dir,
  with original name, size, added-at; drag-out writes to the drop target via OS DnD.
- **C5 Media:** Windows `GlobalSystemMediaTransportControlsSessionManager` (GSMTC) via
  `windows-rs` — read-only session info + play/pause/next/prev. No audio capture.
- **C7 Weather & C10 currency:** the **only** outbound calls. Documented in §7.
- **C11 Screen-time:** samples the foreground window's process name + duration locally;
  aggregates per-app/day in the local DB. No URLs, no titles, no cloud.

## 6. Beyond Vayli — Krovi originals `[original]`

A **focused** set chosen to deepen the shelf identity rather than sprawl (rationale in
`DECISIONS.md` D-006). Build order favours the strongest first.

| # | Feature | Why it fits | Priority |
|---|---------|-------------|----------|
| O1 | **Command Palette** | Quick-launch installed apps + run Krovi actions from one search box; the keyboard spine of the shelf. | High |
| O2 | **Paste Stack** | Numbered clipboard slots (1–9) you *actively* push/pop — distinct from passive history; great for repetitive paste workflows. | High |
| O3 | **Pomodoro focus** | A mode on the existing timer engine (work/break cycles, session count) — reuses C3, no new subsystem. | Med |
| O4 | **Calculator** | Natural companion to the conversion tools; expression + unit-aware. | Med |
| O5 | **To-do list** | Lightweight checkable tasks alongside notes/reminders. | Med |

**Explicitly deferred (anti-scope-creep):** system quick-toggles (Wi-Fi/Bluetooth/DND).
Windows exposes these inconsistently and several need elevated or vendor-specific APIs;
reconsider only after the core ships.

## 7. Privacy posture & network calls (to be verified in Phase 4)

Krovi makes **no** telemetry/analytics/account calls. Planned outbound calls, each to be
confirmed by the security review and listed in `docs/SECURITY-network-audit.md`:

| Call | Trigger | Endpoint type | Data sent | Opt-out |
|------|---------|---------------|-----------|---------|
| Weather fetch | Weather tool open / refresh | Keyless public weather API (e.g. Open-Meteo-class) | Coarse lat/long only | Disable weather tool |
| Currency rates | Currency conversion refresh | Keyless public FX rates API | None (just fetches rates) | Disable currency / use cached |
| Update check | Manual (not automatic) | GitHub releases | None | N/A (manual only) |

No other sockets. The security phase will grep the built binary's call sites and assert
this table is exhaustive.

## 8. Data & storage model

- Single local data dir under `%APPDATA%\Krovi\` (overridable in settings).
- `krovi.db` (SQLite via `rusqlite`) for structured data: clipboard entries, notes,
  todos, reminders, shelf items, screen-time aggregates, settings.
- `blobs/` for clipboard images and shelf file copies.
- No data leaves the machine. Export/clear controls provided.

## 9. Milestones

Tracked in `docs/PROGRESS.md`. Order: core engine (tested) → panel UI → integrations →
security audit → website → packaging. No release without explicit maintainer go-ahead.
