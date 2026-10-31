# Krovi — Network / Privacy Audit

_Last updated: 2026-11-01. This document is the evidence behind Krovi's "verifiable
privacy" claim: a literal account of every place the app can open a network socket, what
triggers it, what is sent, and how to turn it off. It is maintained in-repo so the claim is
checkable against the source — and should be re-run against each released build._

## TL;DR

The Krovi **desktop app** makes **no telemetry, analytics, or account calls**. Only **two**
live network endpoints exist in the code, both **keyless** public APIs, plus a **manual-only**
update check. The **website** (`web/`) makes **zero** network requests (no trackers, no CDN,
no fonts-from-CDN) — it only contains `<a href>` links to the public GitHub repo.

## Method

Literal, repository-wide grep for anything that can open a socket or make a request:
`ureq`, `reqwest`, `hyper`, `isahc`, `TcpStream`, `UdpSocket`, `std::net::`, `fetch(`,
`XMLHttpRequest`, `navigator.geolocation`, and the endpoint hostnames. Every hit was traced to
its call site and classified below. (Build caches `target/` and `node_modules/` are excluded.)

## Declared outbound calls (exhaustive)

| # | Call | Trigger | Endpoint (keyless) | Data sent | Opt-out |
|---|------|---------|--------------------|-----------|---------|
| 1 | **Weather** | Weather tool open / manual refresh | `GET https://api.open-meteo.com/v1/forecast` | Coarse latitude/longitude only (coarsened to ~0.1° ≈ 11 km before the request). No identifiers. | Don't open the Weather tool / don't grant location; the whole app works offline without it. |
| 2 | **Currency rates** | Currency tool refresh | `GET https://api.frankfurter.app/latest` (ECB data) | The base currency code only (e.g. `USD`). No identifiers. | Use cached rates (conversion math is offline); disable the Currency tool. |
| 3 | **Update check** | **Manual only** (user clicks "check for updates") | GitHub Releases (public) | Nothing beyond a standard HTTPS GET. Never automatic. | N/A — only runs when the user asks. |

There are **no other sockets**. No API keys are shipped. Coarse location is used **only** for
weather. Screen-time stores process names + durations locally (never titles/URLs) and is never
transmitted. Clipboard/notes/shelf/to-do data never leaves the machine.

## Code evidence (where the socket actually is)

- The **single** HTTP call site in Krovi's own code is `src-tauri/crates/krovi-net/src/http.rs`
  (`ureq` GET over rustls), gated behind `#[cfg(feature = "net")]`.
- It is called only by:
  - `src-tauri/crates/krovi-net/src/weather.rs` — endpoint constant
    `https://api.open-meteo.com/v1/forecast`.
  - `src-tauri/crates/krovi-net/src/currency.rs` — endpoint constant
    `https://api.frankfurter.app`.
- With the `net` feature **off** (the default build), `fetch_forecast` / `fetch_rates` return
  `NetError::Disabled` — i.e. no socket at all.
- `krovi-core`, `krovi-store`, and `krovi-sys` contain **no** network code. `krovi-sys` touches
  the OS (media session, foreground window, coarse location) but opens **no** sockets.
- The frontend (`src/`) performs **no** direct network I/O: in the app it calls Tauri commands
  over local IPC (not the network); in the browser demo it uses in-memory fixtures.

## Honest caveat — transitive dependencies

`src-tauri/Cargo.lock` lists `reqwest`, `hyper`, and `hyper-util`. These are **transitive**
dependencies pulled in by the Tauri framework stack, **not** called by Krovi's code. Krovi does
**not** configure Tauri's auto-updater (no updater endpoint is set in `tauri.conf.json`), so no
background update traffic occurs. This audit covers Krovi's source; a belt-and-braces review
should also run a network trace against the **built binary** in CI to confirm the shipped app
opens only the sockets listed above. That binary-level check is the recommended next step once
CI produces a release build.

## Website (`web/`)

Static HTML/CSS/vanilla-JS. Grep confirms **no** `fetch`/`XMLHttpRequest`, no third-party
scripts, no analytics/trackers, no CDN fonts or images (all illustrations are inline SVG; fonts
are system stacks). The only external strings are `<a href>` links to
`https://github.com/nAItiklearn/krovi` (+ `/releases`), which are navigations the user clicks,
not requests the page makes.

## Comparison to the "private" claim

This is exactly what makes Krovi's privacy posture **verifiable** rather than merely asserted:
the outbound surface is two keyless GETs and a manual update check, isolated in one auditable
module, with the source public. Anyone can re-run the grep above or trace the built binary.
