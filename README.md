# Krovi

**A free, open-source "everything shelf" for Windows.** Krovi is a quick-access panel —
summoned by a global hotkey or a screen-corner click — that gathers the small daily tools you
keep reaching for into one place: clipboard history, a paste stack, timers & Pomodoro, a file
shelf, media controls, weather, notes, to-dos, world clocks, unit & currency conversion, a
calculator, a command palette, and local screen-time.

- **Free + open source forever** — MIT, no account, no trial, no paywall.
- **Verifiable privacy** — local-first; no ads, no telemetry. Every outbound call is documented
  in [`docs/SECURITY-network-audit.md`](docs/SECURITY-network-audit.md) (spoiler: two keyless
  calls — weather and currency — and a manual update check, nothing else).
- **Original design** — its own look (a left tool rail, its own palette and icons).

> Status: **early (v0.1.0)**, built in the open. Core logic + persistence: **99 Rust tests**;
> panel UI: **26 frontend tests** — all green.

## Website

🌐 **https://web-flame-kappa-83.vercel.app** — features, a live interactive preview, docs, and an
honest comparison vs. alternatives.

## Download

Grab the Windows installer from **[GitHub Releases](https://github.com/nAItiklearn/krovi/releases)**.
Installers (NSIS + MSI) are built on Windows/MSVC by GitHub Actions when a `v*` tag is pushed.
Until the first tagged build finishes, the releases page is a placeholder.

## Try the UI right now (browser demo, no backend)

The panel runs in a plain browser in demo mode (fixtures, no Rust side needed):

```sh
git clone https://github.com/nAItiklearn/krovi
cd krovi
npm install
npm run dev        # → http://localhost:5173
```

Every tool view is interactive in the demo (the calculator computes, timers/clocks tick,
clipboard filters, the converter does real math, Ctrl/Cmd+K opens the command palette).

## Features

| | |
|---|---|
| **Clipboard history** | searchable, pin-to-top, type filters, capped ring |
| **Paste stack** | 9 explicit numbered slots (push/pop) |
| **Timers & stopwatch** | multiple countdowns + laps |
| **Pomodoro** | work/break cycles on the timer engine |
| **File shelf** | park files on named shelves, drag out |
| **Media now-playing** | reads the Windows media session; transport controls |
| **Weather** | keyless API + coarse location |
| **Quick notes / To-do** | fast scratch notes; checkable tasks |
| **World clocks** | several zones at a glance |
| **Unit & currency** | offline units; cached keyless FX |
| **Calculator** | expression + unit-aware |
| **Command palette** | keyboard-first launcher |
| **Screen-time** | local-only per-app/day (no titles/URLs, never sent) |

## Architecture

```
src/                     Svelte 5 + Vite + TS panel UI (demo-mode capable)
src-tauri/               Tauri 2 shell
  src/                   thin #[tauri::command] bridge (no business logic)
  crates/krovi-core/     pure logic: clipboard, timers, pomodoro, notes, todo, shelf, screentime
  crates/krovi-store/    persistence: Store trait + in-memory + SQLite (feature-gated)
  crates/krovi-net/      the ONLY outbound calls — keyless weather + currency
  crates/krovi-sys/      windows-rs wrappers (media, foreground window, location)
web/                     the krovi.tech marketing/docs site (static)
docs/                    SPEC, ADR, decision log, progress, network audit
```
See [`docs/ADR-001-stack.md`](docs/ADR-001-stack.md) and [`docs/SPEC.md`](docs/SPEC.md).

## Build from source

Frontend: `npm install && npm run build` (and `npm test`, `npm run check`).

Rust core (pure logic, no native toolchain needed):

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p krovi-core -p krovi-store -p krovi-net -p krovi-sys
```

Full native app / installer: needs a **64-bit MSVC C toolchain** (Visual Studio Build Tools, C++
workload) with `rustup default stable-x86_64-pc-windows-msvc`. Build with the integrations
enabled:

```sh
cargo tauri build --features net,native
```

> Note: a `x86_64-pc-windows-gnu` host with only 32-bit MinGW cannot compile the native app
> (it fails in `dlltool`/bundled-C); CI uses MSVC, which is why the downloadable installer is
> produced by GitHub Actions rather than requiring a local native build.

## Privacy

Krovi sends nothing about you anywhere. The complete, grep-backed accounting of every network
call lives in [`docs/SECURITY-network-audit.md`](docs/SECURITY-network-audit.md). The source is
public so the claim is checkable — that is the whole point.

## Contributing

Issues, ideas, and pull requests are welcome as the core stabilises. Please keep changes
local-first and privacy-preserving, and add tests.

## License

[MIT](LICENSE) © 2026 Naitik Sahu and the Krovi contributors.
