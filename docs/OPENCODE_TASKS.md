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
