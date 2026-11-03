# Small tasks for OpenCode (v0.3 release)

Rules for every task:
- Touch ONLY the files listed for that task. Do not reformat, rename or "tidy" anything else.
- Do not touch `src-tauri/`, `web/`, `.github/`, `package.json` or any lock file.
- Never use `muse-spark-*` models (AGENTS.md). Free tier first.
- Run the listed check before finishing. One commit per task, message prefix given below.
- Do not push. Claude (the main agent) pushes and releases.

---

## T1 — Rename the "minecraft" theme to "Pixel Meadow" (trademark safety)

Files (only these):
- `src/styles/themes/minecraft.css` → rename the file to `src/styles/themes/pixel-meadow.css`
- `src/styles/themes/_blocky.css`
- `src/styles/themes/polish-themes.test.ts`
- `src/lib/theme.ts`
- `src/components/idle/IdleCompanion.svelte`
- wherever `minecraft.css` is imported (grep `minecraft` inside `src/` only)

Do:
- Theme id `minecraft` → `pixel-meadow`; label → "Pixel Meadow".
- A saved setting with the old id `minecraft` must still load as `pixel-meadow`. Add that mapping in `theme.ts` and a test for it.
- No remaining "minecraft"/"Minecraft" string anywhere in `src/` (comments included).

Check: `npx vitest run src/styles src/lib/theme` and `npx svelte-check` with 0 errors.
Commit: `refactor(theme): rename minecraft theme to Pixel Meadow`

## T2 — Tool count 15 → 19 in docs

Files: `README.md`, `docs/product/FEATURE_AUDIT.md`, `docs/PERF.md`
(Leave DECISIONS.md, IMPLEMENTATION_PLAN.md and research docs alone; they are history.)

The v0.3 tools are Home, Clipboard, Timers, Pomodoro, File Shelf, Quick Transfer, Media, Calendar, Weather, Notes, Explain, World Clocks, Convert, Screen Time, Agent, Circle to Ask, Clean My OS, Extras and Settings (19).
Mention the new ones in README in one line each. Mark Agent, Circle to Ask and Clean My OS as "new in v0.3".
Use AI wording only as follows: "bring your own key (optional)". No account, no sign-up, no sync claims.

Commit: `docs: v0.3 tool list`

## T3 — CHANGELOG entry for v0.3.0

File: `CHANGELOG.md` (create it if missing). Base the entry on `git log clean-main..feat/krovi-v03-autonomous --oneline`.
Group the changes as Added / Changed / Security.
Rules:
- Honest wording only. Snowflake is "experimental" and is not marketed.
- Gemini requires the user's own AI Studio key and their consent.
- No DeepSeek.

Commit: `docs: changelog for v0.3.0`

## T4 — Version bump to 0.3.0

Files: `package.json`, `package-lock.json` (only the top-level `"version"` fields), `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` (`[package] version`) and `src-tauri/Cargo.lock` (only the `krovi-app` entry's version line).
Check: `npm run check` passes, and `cargo +stable-x86_64-pc-windows-gnu metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --no-deps` runs without error.
Commit: `chore: version 0.3.0`

## T5 — CI: move off the deprecated Node 20 actions

Files: `.github/workflows/*.yml` only. GitHub warns that actions/checkout@v4, actions/setup-node@v4 and actions/upload-artifact@v4 target Node 20.
Bump each to the latest major that runs on Node 24. Check each action's README on GitHub for the version. Change nothing else in the workflows.
Commit: `ci: actions on Node 24`

## T6 — IMPLEMENTATION_STATUS for v0.3

File: `docs/product/IMPLEMENTATION_STATUS.md`. Add a v0.3 section from `git log clean-main..HEAD --oneline` covering:
- Agent Mode, Talk to Agent, Circle to Ask and screen awareness
- Clean My OS, Extras and Settings → AI (BYOK)
- companion themes and Read aloud
- the security audit (`docs/SECURITY-v03-audit.md`)
For each item, state plainly what is unit-tested and what is not yet verified on a real desktop. Don't claim anything that wasn't run.
Commit: `docs: implementation status v0.3`

Order: T1, T2, T3, T4, T5, T6, then stop. Do not touch `src-tauri/src/`, `src-tauri/crates/` or `web/`. Claude is fixing the native build there.
