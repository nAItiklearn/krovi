# ADR-001 — Technology stack

- **Status:** Accepted
- **Date:** 2026-10-31
- **Deciders:** architect (autonomous), pending maintainer review
- **Related:** `DECISIONS.md` D-004

## Context

Krovi is a Windows-first, local-first desktop "everything shelf" with a small-but-rich UI
(panel + tool views), a non-trivial logic core (clipboard history, timers, shelf, notes,
screen-time), and a hard requirement for **verifiable privacy** and a **small, auditable
dependency surface**. The brief's default is "Tauri 2 + Rust core, consistent with
Windy/Krova unless ADR-001 finds a real reason to diverge."

## Decision

Adopt **Tauri 2** with a **Rust core**, a **Svelte 5 + Vite + TypeScript** frontend, and
**SQLite (`rusqlite`)** for local persistence. Native Windows APIs (media session,
foreground-window sampling, location) accessed via **`windows-rs`**.

Crate/workspace shape:
```
src-tauri/            Tauri shell + commands (thin)
  crates/krovi-core/  pure logic: clipboard, timers, shelf, notes, todo, screentime
  crates/krovi-store/ SQLite persistence + migrations
  crates/krovi-sys/   windows-rs wrappers (media, foreground window, location)
src/                  Svelte 5 frontend (panel UI)
```

The logic core is **UI-independent and unit-tested first** (brief requirement), so it can
be exercised without launching a window — and kept portable for a future cross-platform port.

## Rationale

- **Footprint & feel:** Tauri uses the OS WebView2 (already on Win11) instead of bundling
  Chromium — installers in the single-digit MB range and low idle memory, which matters for
  an always-available dropdown.
- **One language for logic:** Rust gives a fast, safe, testable core shared across tools;
  `windows-rs` gives first-party access to the exact OS APIs we need (GSMTC media session,
  `GetForegroundWindow`, Win location) without shelling out.
- **Auditability → privacy claim:** fewer/heavier runtimes = larger attack/audit surface.
  Tauri's small surface and Rust's explicit networking make "list every outbound call"
  tractable and believable — core to Krovi's positioning.
- **Svelte 5** for the frontend: compiles to tiny vanilla JS (no large runtime shipped),
  excellent for a snappy panel, and keeps the JS dependency count low (helps the audit).
  Vite gives fast dev + demo mode.
- **Consistency:** matches the brief's referenced Windy/Krova stack, easing shared patterns
  and CI.

## Alternatives considered

- **Electron + TS:** richest ecosystem, but bundles Chromium (100+ MB, high memory) and a
  large JS/native surface that undercuts the "small, auditable, light dropdown" goals. Rejected.
- **Native WinUI 3 / C#:** best-integrated Windows UX, but ties us to .NET/XAML, weaker core
  portability, and a heavier toolchain for contributors; diverges from the referenced stack
  without a compelling win here. Rejected (revisit only if WebView2 proves limiting).
- **Flutter desktop:** good UI, but Skia-rendered chrome fits a full app more than a
  lightweight OS-native dropdown, and Dart splits the logic language from the OS-API layer.
  Rejected.
- **egui / pure-Rust immediate-mode UI:** keeps everything in Rust, but immediate-mode
  styling for a polished, animated panel is costly; Tauri+Svelte gives better design
  velocity. Rejected for the UI (still great for internal tooling if needed).

## Consequences

- WebView2 is a runtime dependency (present on Win11; bootstrapper handles older systems).
- Two languages (Rust + TS) — mitigated by keeping `src-tauri` commands thin and all logic
  in Rust crates.
- Frontend assets must be audited too; kept minimal and bundled (no CDN at runtime).

## Revisit if

- WebView2 can't deliver a needed interaction (e.g. true OS drag-out) — then add a native
  Tauri-side path rather than switching frameworks.
- A cross-platform release is prioritised — the portable Rust core already anticipates this.
