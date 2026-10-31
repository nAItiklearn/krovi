# Krovi

**Your everything-shelf for Windows.** A persistent, quick-access panel that bundles
the small daily tools — clipboard history, timers, a file shelf, notes, weather,
media controls and more — into one dropdown you summon with a hotkey or a corner click.

- **Free and open source, forever.** MIT licensed. No licence key, no trial, no paywall.
- **Local-first and private by design.** No account, no ads, no telemetry. The only
  network calls Krovi makes are the data fetches you'd expect (weather, currency rates),
  and every one of them is documented and independently verifiable. See
  `docs/SECURITY-network-audit.md` *(added in the security phase)*.
- **Windows first.** Built with [Tauri 2](https://tauri.app) + a Rust core.

> Status: **early development.** See [`docs/PROGRESS.md`](docs/PROGRESS.md) for the live build status
> and [`docs/SPEC.md`](docs/SPEC.md) for the full feature specification.

## Why Krovi

Krovi is an original, clean-room "everything shelf" in the spirit of menu-bar / notch
dropdown utilities. It is **not** a fork, re-skin, or reverse-engineering of any existing
product — the name, design language, icons, copy and all code are original. Its explicit
differentiator is being free **and** open source with a privacy posture you can verify by
reading the code, not just a marketing claim.

## Project layout

```
docs/          Specs, ADRs, decision log, progress tracker, security audit
.claude/       Per-task agent definitions (strict file ownership)
```

Source directories (`src-tauri/`, `src/`, `web/`) are added as each phase lands.

## License

[MIT](LICENSE). Contributions welcome once the core stabilises.
