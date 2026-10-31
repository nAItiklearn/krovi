# Krovi — Decision Log

Every non-trivial decision is recorded here, newest first. Format: `D-NNN · YYYY-MM-DD · title`.

---

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
