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
security audit → website → packaging. No app-binary release without explicit maintainer go-ahead.

## 10. Website — krovi.tech `[brief]`

Static, privacy-first marketing + download site. **Original design** (not a clone of any site);
design *cues* only from the maintainer's own site (scaffold). Built under `web/`; **deployed to
Vercel** (maintainer-authorised 2026-11-01 — `DECISIONS.md` D-008).

**Sections:** hero (tagline + dual CTA: Download, Compare) → feature showcase (the shelf + each
tool) → local-first/verifiable-privacy value prop → "Free & open source forever" → `/compare` →
FAQ → final download CTA.

**Nav:** Features · Compare · Download · FAQ · GitHub.

**Footer (scaffold-style, every link must resolve — never a dead `#`):** multi-column —
- *Get started:* Download, Features, Compare, Keyboard shortcuts
- *Project:* GitHub repo, Roadmap, Changelog, FAQ
- *Privacy:* Network audit (→ the audit doc), Privacy, Security
- *Krovi:* About, License (MIT), Contributing
- Bottom bar: Krovi wordmark + tagline, trust line "No account · No ads · No tracking · No
  telemetry · Free & open source", "© 2026 Naitik Sahu & the Krovi contributors. Your data stays
  on your device."

**Comparison table (`/compare`) — honest + clean-room:**
- Columns: **Krovi** (featured, highlighted, first) vs **Vayli** vs 1–2 generic alternatives
  (e.g. "Typical paid utility suite", "A pile of separate free tools").
- Row groups: *Cost & ownership* (price / free, open source, no account, fully offline, local
  files, no telemetry) · *Daily tools* (clipboard history, paste stack, timers, Pomodoro, file
  shelf, media controls, weather, notes, to-do, world clocks, unit & currency, calculator,
  command palette, screen-time) · *Platform* (Windows, auto-start, global hotkey, hot-corner).
- Cells: ✓ / ✗ / "Paid" / "Partial" / **"Not verified"**, with a legend.
- **Clean-room honesty rule (D-002):** the Vayli column is filled from its **public listing only**;
  anything not publicly confirmable is **"Not verified"**, never guessed and never derived from its
  binary. Include a candid "Where others may be ahead today" note (Krovi is Windows-only for now,
  newer / less battle-tested). Honesty is the brand.

**Constraints:** no third-party trackers/analytics/ads on the site either; fast, accessible,
responsive, light + dark; highlighted Krovi column; sticky table header; mobile-friendly. The
Download button targets GitHub Releases as a **placeholder** until a real release exists.

## 11. v0.2 "Island" additions (2026-11-01) — features + ownership

Identity guardrails for everything below: no fluff, lightweight, local-first, actually
private. Each feature must hold the idle-RAM/CPU bar measured in `docs/PERF.md`.

| # | Feature | Owner agent | Files (see CONTRACTS.md §5) |
|---|---|---|---|
| 11.1 | Island window: pill ⇄ panel, tray, quit, click-through, fullscreen hide | panel-shell | `shell.rs`, `shell.ts`, `tauri.conf.json` |
| 11.2 | Live data: OS clipboard, GSMTC media, RAM/disk/CPU strip, SQLite persistence | live-data | `live.rs`, `commands.rs`, `krovi-sys`, `live.ts` |
| 11.3 | Premium black/white glass UI + spring motion | frontend-design | `App.svelte`, `components/**`, `views/**` |
| 11.4 | Holidays: IN (DoPT), US (OPM), GB/SCT/NIR (GOV.UK), offline | holidays | `src/lib/holidays/**` |
| 11.5 | Local voice: dictated notes + voice commands (SAPI 5, on-device) | voice | `krovi-voice`, `voice.rs`, `voice.ts` |
| 11.6 | Real inline checklists in Notes (`- [ ]` blocks mixed with text) | frontend-design | `views/Notes.svelte`, `lib/checklist.ts` |
| 11.7 | Quick Transfer: LAN-only QR file transfer desktop ⇄ phone, auto-expiring | transfer | `krovi-transfer`, `transfer.rs`, `views/Transfer.svelte` |
| 11.8 | **Theme system** (token-based; switchable; persists; applies to panel, pill, idle character) | personalization | `src/styles/themes/**`, `src/lib/theme.ts` |
| 11.9 | **Idle pill companion** ("screensaver"): after ~5.5s idle the collapsed pill becomes animated eyes that look around, blink, glance toward a nearby cursor; any hover/click/shortcut wakes it. Customizable styles/colors/personalities, free. | personalization (+ shell for the cursor signal) | `src/components/idle/**`, `src/lib/idle.ts`; `shell-cursor` event in `shell.rs` |
| 11.10 | **Voice v2**: point-and-ask context ("this"/"it" = current selection / focused file / window), voice-to-action across tools (reminders, shelf, pomodoro, timers), clean transcription (filler removal, casing/punctuation), explicit privacy toggles in Settings | voice | `krovi-voice`, `voice.rs`, `voice.ts` (+ helper fns in `commands.rs`) |
| 11.11 | Website polish: drifting tool-chain section, palette/type system, inline looping product clips | ui-polish (after web) | `web/**` |
| 11.12 | **Spike — "point anywhere"**: feasibility of cursor-as-context quick actions on Windows; verdict + scoped v1 before any build | ui-polish | `docs/research/point-anywhere.md` |

### 11.8 Themes (proposal — 6 total)
All themes are pure token overrides (`[data-theme="<id>"]`) of one shared variable
vocabulary: surface/glass, text tiers, hairlines, shadow, one accent, plus `--eye-*`
tokens for the idle companion. Adding a theme = one CSS file + one registry entry.
1. **Graphite** (default dark) — near-black glass, white text, single cool accent.
2. **Porcelain** (default light) — near-white glass, ink text.
3. **Pastel / "Mochi"** — soft lilac-peach glass, rounded warmth, low contrast accents.
4. **Neon / "Arcade"** — deep indigo-black with one electric accent and faint glow.
5. **Earth / "Terracotta"** — warm sand/clay surfaces, olive accent, matte glass.
6. **Ocean / "Tide"** — slate-blue glass, seafoam accent.
Each theme ships light-safe contrast (WCAG AA for text) — verified, not assumed.
"Follow system" maps to Graphite/Porcelain.

### 11.9 Idle companion — constraints
- Original art: Krovi's own eye shapes (e.g. "Dot", "Pill", "Sleepy", "Pixel", "Visor"),
  colors from theme `--eye-*` tokens, personalities = timing profiles (curious, calm,
  sleepy, hyper). No Apple/Apollo assets or likeness.
- Wake on hover/click/shortcut/pulse instantly. Never steals focus. Off switch in Settings.
- **Cost bar**: CSS transform/opacity animations only, discrete saccades on timers
  (no continuous rAF loop), paused when hidden/fullscreen/reduced-motion. Measured idle
  CPU and RAM delta vs. companion-off is recorded in PERF.md; target ≈0% CPU, <5 MB RAM.
- Cursor glance uses a Rust `shell-cursor` event (≤10 Hz, only while the cursor is within
  ~400 px of the pill, only in pill mode) — the webview never polls.

### 11.10 Voice v2 — constraints
Fully on-device (SAPI 5); if any sub-feature can't be local, DECISIONS.md says so and it
is not built with a cloud API. Context capture reads only on explicit voice command and
only the minimum (selected text / focused Explorer item path / foreground app name);
nothing is stored unless the action stores it (a note, a reminder, a shelf item).

### 11.13 Supported voice commands (exact)

**Voice commands are a closed grammar, not open-ended natural language.** In command mode the
on-device recognizer (Windows SAPI 5) listens *only* for the phrasings in the tables below,
plus three places where free speech is allowed: the body of a note, the text of a reminder
and the search term of "find". Anything else is rejected or ignored; it is never interpreted,
guessed at, or sent anywhere. "Could you maybe pull up my stuff from last week" does nothing.
Folders and apps are a *closed set* too: only names Windows itself lists (below) are heard
and only those are acted on. The same wording also works when typed. Notation: `{a|b}` one of,
`[x]` optional, `<n>` a number from 1 to 99 (spoken as words or digits), `<text>` free speech.

Because the recognizer only matches phrases it was given, accuracy is high for the closed
parts and lower for free text (note bodies, reminder titles, search terms). Measured with
synthesized speech and the real recognizer, see DECISIONS D-013/D-014; real microphones differ.
A phrase heard with low confidence does nothing when it would launch an app ("I am not sure I
heard that right ... Say it again.").

#### Krovi tools
| Template | Examples |
|---|---|
| `{open|show|show me|go to|switch to|launch|take me to|bring up|view} [the|my] <tool>` | "open the calendar", "show me clipboard", "go to settings" |

`<tool>`: home / home screen / dashboard / overview; clipboard / clipboard history / copy history /
clips; paste stack / paste / paste slots; timers / timer / stopwatch / countdown / countdowns;
pomodoro / pomodoros / pomodoro timer / focus / focus timer / focus mode / focus session; shelf /
file shelf / files; media / music / now playing / player / media controls; calendar / agenda /
schedule / events (also "reminder(s)"); weather / forecast; notes / note / notebook; clocks /
clock / world clocks / world clock / time zones / time zone; convert / converter / conversion /
currency / currency converter / unit converter / units / exchange rates; calculator / calc; screen
time / screentime / app usage / usage; transfer / quick transfer / send to phone / phone transfer /
file transfer (Quick Transfer; "send to phone" is the tool only on its own - "send **this** to my phone"
is the selection action below, and `files` / `file shelf` stay the Shelf); explain / explainer / summarize / summarizer / summary / read
aloud (the on-device summarizer and read-aloud tool; it only *opens* the tool, voice does not summarize
text itself); settings / preferences / options / configuration.
A Krovi tool wins over a folder or app of the same name for `open`/`show`/`go to`... ("open settings"
is Krovi's Settings). Say **launch** to get the installed Windows app ("launch settings"), or
**folder** to get the Windows folder ("open music folder").

#### Timers, pomodoro, close
| Template | Examples |
|---|---|
| `{start|set|begin} [a|an|the] timer for <n> {second|seconds|minute|minutes|hour|hours}` | "start a timer for five minutes" |
| `timer for <n> {seconds|minutes|hours}` | "timer for ten minutes" |
| `{start|set|begin} [a|an|the] <n> {second|minute|hour} timer` | "set a ten minute timer" |
| `{start|set|begin} [a|an|the] timer for {an hour|half an hour|a minute|a quarter of an hour}` | "start a timer for half an hour" |
| `{start|set|begin} [a|an|the] timer` | opens Timers |
| `{start|begin|run} [a|the] {pomodoro|focus} [timer|session|mode]` | "start a pomodoro" |
| `{pause|resume|stop|cancel} [the] {timer|pomodoro|stopwatch|countdown}` | "pause the timer" |
| `{close|hide|dismiss|collapse|minimize} [the] [panel|krovi|window|shelf]`; `go away`; `never mind`; `that is all` | "close" |

Timers are 1 second to 100 hours.

#### Notes
| Template | Examples |
|---|---|
| `{new note|take a note|make a note|add a note|create a note|note to self} <text>` | "new note buy milk" -> a note "Buy milk" |
| the same prefix alone | opens Notes |
| `{save this as a note|save that as a note|save it as a note|make this a note|turn this into a note|add this to my notes|copy this to a new note|put this in a note|write this down|note this down|note this}` | saves the selected text; needs a selection |

#### Reminders (voice-to-action)
| Template | Examples |
|---|---|
| `{add a reminder|set a reminder|create a reminder|make a reminder|new reminder|remind me} <when and what>` | "set a reminder for tomorrow at nine thirty in the morning to renew the passport" |
| also accepted as spoken openings: `remind me {to|at|in|on|about|today|tonight|tomorrow|this morning|this afternoon|this evening}`, `{set|add|create} a reminder {for|to}` | "remind me at five pm to call mom" |

The short opening "remind me" is the weakest for the recognizer; "set a reminder for ..." held up best in testing.

`<when and what>` is free speech. The date and time words are found by a local parser and removed
from the title; what is left is the title. Understood time phrases (today is the day you say it):

| You say | It means |
|---|---|
| `at 5pm`, `at 5 p m`, `at 5:30 pm`, `at five thirty`, `at 17:00` | that clock time; with no am/pm, 1-6 means PM and 7-11 means AM |
| `at 5:30 in the evening`, `at 6 in the morning`, `at 9 at night`, `at 3 o'clock` | am/pm from the daypart; "o'clock" keeps the default above |
| `at noon`, `at midnight`, `half past three`, `quarter past four`, `quarter to five` | 12:00, 00:00, 15:30, 16:15, 16:45 (hour with no am/pm as above) |
| `in 20 minutes`, `in two hours`, `in an hour`, `in half an hour`, `in a quarter of an hour`, `in an hour and a half` | now plus that, to the minute |
| `in 3 days`, `in two weeks`, `in a month` | that many days/weeks/months ahead (no time) |
| `today`, `tonight`, `tomorrow`, `the day after tomorrow` | that day; "tonight" = 20:00 unless a time is given (`tonight at 8` = 20:00) |
| `friday`, `on friday`, `this friday`, `next friday` | next such day after today; "this" may be today; "next" = that day in the *following Monday-Sunday week* |
| `tomorrow at 9`, `tomorrow morning`, `friday afternoon at 3`, `this evening at 7` | day plus time; dayparts: morning 09:00, afternoon 15:00, evening 18:00, night 20:00 (an hour with no am/pm follows the daypart: `tonight at 8` is 8 PM) |
| `on friday at noon`, `december twenty fifth`, `march 3rd`, `the 5th` | named day / month and day / day of this month |
| a time with no day ("at 9") | today if still ahead, else tomorrow |

The confirmation states the exact resolved date and time, for example
`Reminder set for today, Sun 1 Nov, 5:00 PM: Call mom` or
`Reminder set for Fri 6 Nov, 12:00 PM: Send the report`. With no time spoken the entry is a calendar
item with no alert and says so (`..., no alert time: ...`).

#### Selection actions ("this" = the selection captured when you started the command)
| Template | Examples |
|---|---|
| `{copy this to the file shelf|copy this to the shelf|put this on the shelf|put this on the file shelf|put it on the shelf|add this to the shelf|add this to the file shelf|save this to the shelf|move this to the shelf|drop this on the shelf|shelve this}` | files or text go on the shelf |
| `{send this to my phone|send it to my phone|send that to my phone|send this to the phone|send this to phone|share this with my phone|transfer this to my phone}` | routes the selected files to Quick Transfer |

Both need something selected when you start; otherwise the answer says nothing is selected.

#### Media
`{next track|next song|skip track|skip song|skip this song|skip this track|play the next track|previous track|previous song|last track|last song|pause music|pause the music|pause playback|play music|resume music|resume playback|toggle music|what's playing}`.
When typed, bare "pause", "skip", "play" and the longer "what song is this" forms also work.

#### Open a folder, list a folder (Voice v3)
| Template | Examples |
|---|---|
| `{open|show|show me|go to|browse|take me to|bring up} [the|my] <folder> [folder]` | "open downloads", "show my pictures", "open krovi downloads" |
| `{what's in|what is in|what's inside|what is inside|tell me about|list|list the contents of|show me what's in} [the|my] <folder> [folder]` | "what's in downloads", "tell me about the desktop" |

`<folder>` is one of exactly these (nothing else is understood):
**Desktop, Documents, Downloads, Pictures, Music, Videos** (the Windows Known Folders, wherever
OneDrive or you moved them), **Krovi downloads** (`Downloads\Krovi`, where Quick Transfer saves),
and the **folders in Explorer's Quick Access** (pinned and frequent), with their Explorer names
(e.g. "Projects"). Spoken aliases: "docs" (Documents), "pics" (Pictures), "movies" (Videos).
Opening shows the folder in Explorer. "What's in" reads the folder with the file system only: item
count, files and folders, total size of the files directly inside, the newest three by name, and up
to 50 rows (newest first) in the panel; it does not look inside subfolders and skips hidden/system items.

#### Launch an app (Voice v3)
| Template | Examples |
|---|---|
| `{open|launch|start|run|start up} [the] <app>` | "open notepad", "launch spotify", "start firefox", "launch visual studio code" |

`<app>` is one of the apps in your Start menu (Windows' own app list plus Start-menu shortcuts), by
the name shown there. Brand prefixes are optional ("chrome" for Google Chrome, "edge" for Microsoft
Edge) and a few aliases are built in ("vs code", "cmd", "explorer", "ppt"). A name that matches
several apps equally ("photo" for "Photo Editor Pro" and "Photo Viewer Plus") launches nothing and
lists the candidates. Web links, documents and uninstallers in the Start menu are not apps and are
never launched.

#### Find a file by name (Voice v3)
| Template | Examples |
|---|---|
| `{find file|find files|find the file|find me|search for|search my files for} <text>` | "search for tax return", "find the file budget", "find me boarding pass", "find file invoice" |

A bare "find ..." / "look for ..." / "locate ..." is deliberately not in the spoken grammar (it made the recognizer
swallow noisy audio that was really another command, see DECISIONS D-014); when *typed* they work too ("find invoice").

Searches the **Windows Search index** (file *names* that contain the words, folders included), newest
first, 20 results, shown as rows in the panel. A row opens its folder (folders) or shows the file
selected in Explorer (files); a found file is never run. No disk scan: only what Windows has indexed
is searched. It skips `AppData`.

#### Not supported (yet)
- Open-ended natural language, questions, chit-chat, or several commands in one sentence ("open notes and start a timer").
- Folders and apps outside the sets above: any other path or drive, network shares, subfolders
  ("downloads slash invoices"), recent *files*, websites and URLs (voice never opens a web page).
- Searching file *contents*, inside one folder only, by type/date/size ("pdfs from last week"),
  non-indexed locations, the web, email or messages.
- Doing anything to a found file or folder by voice: open, run, copy, move, rename, delete, send.
- Quitting or closing apps and windows, killing processes, and system actions (shut down, volume,
  brightness, Wi-Fi, lock).
- Typing or dictating into other apps (dictation only creates a Krovi note).
- Editing, deleting, snoozing or listing reminders by voice; recurring reminders ("every Monday");
  years in dates ("march 3rd 2027"); "next year".
- Languages other than English (US), and names that cannot be pronounced from their letters
  (e.g. "Ünïcode") may not be heard.
- Hands-free "always listening" or wake words: voice only listens after you start it.
