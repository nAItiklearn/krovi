# Krovi v0.3 - Security and Privacy Pre-release Audit

- Date: 2026-11-03
- Reviewer: security-reviewer (static review, read-only over source)
- Tree reviewed: working tree on branch feat/krovi-v03-autonomous (includes uncommitted edits to agent.rs, dictation.rs, AgentPanel.svelte, agent.ts, Settings.svelte)
- Method: file-by-file read of the surfaces below, plus repo-wide greps (sockets, Command::new, emit, logging, @html, key-shaped strings). Nothing was compiled or run; no test results are claimed.
- Not covered: git history, web/, krovi-transfer, krovi-voice, the Whisper download, krovi-store, extras.rs (not wired into lib.rs yet; one note in I-3). Only 5 of the 10 root-level screenshots were opened.

## Verdict

| Severity | Count |
|----------|-------|
| CRITICAL | 0 |
| HIGH     | 0 |
| MEDIUM   | 4 |
| LOW      | 14 |
| INFO     | 3 |

No finding lets a key, a screenshot or a file leave or disappear without the user acting. The BYOK key handling, the Agent permission model, the mail flow and the Clean My OS delete path held up under review.

Release advice:

1. Fix M-4 (the network audit says "no other outbound sockets" but ElevenLabs text-to-speech is one). The audit table and the code must agree before sign-off.
2. Fix or reword M-1 and M-2 before release, because both contradict statements the UI makes to the user ("always visible indicator", "nothing is saved").
3. M-3 is defence in depth; schedule it, do not block on it.

Severity scale: CRITICAL = key/data exfiltration or irreversible loss without user action; HIGH = a documented guard can be bypassed by ordinary use; MEDIUM = a stated privacy promise or a defence-in-depth layer is not enforced; LOW = hardening, latent, or narrow edge case; INFO = no action required.

---

## MEDIUM

### M-1  The "screen awareness is ON" indicator can be hidden while a session runs (D-021)

- Where: src-tauri/src/shell.rs:118 (hide_in_fullscreen defaults to true), :637-641 (show_pill=false forces Mode::Hidden), :667-669 and :688-693 (the main window is hidden), :1031-1038 (fullscreen detection). The indicator is drawn only inside that window: src/components/Island.svelte:457-466. The only other cue is the tray tooltip (src-tauri/src/circle.rs:913-926), which needs a hover. aware_start (circle.rs:954-1005) never checks that anything is visible. shell.rs has no reference to the awareness session.
- Scenario: the user starts a 30 minute session, then watches a fullscreen video on the primary monitor. With the default setting the window, the dot and the one-click Stop disappear, and the session stays live until it times out. Today a capture still needs the user to press Ask in the panel, so no passive capture happens, but D-021 says the indicator is always visible and nothing in the backend enforces it. A future hotkey or automation calling aware_ask would capture with no visible cue.
- Minimal fix: in apply_mode_main / sync_visibility do not hide the window (fullscreen or Mode::Hidden) while Circle.aware is Some; or call end_session(app, None, "stopped") when the window is hidden. Also make aware_ask return "inactive" if the main window is not visible.

### M-2  "Copy" on screen text puts it in the persistent clipboard history, contradicting "nothing is saved"

- Where: src/circle/TextActions.svelte:40, src/circle/AskPanel.svelte:100, src/circle/AwarePanel.svelte:108 -> src/lib/stores.ts:570-577 (navigator.clipboard.writeText) -> the Krovi clipboard watcher src-tauri/src/live.rs:198-209 and :170-196 -> src-tauri/src/commands.rs:118 (persist_clipboard, SQLite). Only Rust-initiated copies are acknowledged to the watcher (krovi-sys/src/clipboard.rs, ClipGate::acknowledge); a webview write is not. The claims: src/circle/AwarePanel.svelte:16 and :132 ("nothing is saved").
- Scenario: during a session on a banking page the user presses Copy on the OCR text (or on an AI answer about it). The text is now stored in the clipboard history database, and any backup of it, until the user deletes the entry.
- Minimal fix: route these copies through a Rust command that sets the ExcludeClipboardContentFromMonitorProcessing / CanIncludeInClipboardHistory=0 formats (the same formats clipboard.rs already honours when reading), or acknowledges the change token. Failing that, reword the UI: "Copy also adds it to your clipboard history."

### M-3  Every app command is callable from every window; the overlay window has the whole surface

- Where: src-tauri/build.rs:1-3 (plain tauri_build::build(), no app manifest, so generate_handler! commands are not subject to per-window permissions), src-tauri/capabilities/default.json:5 (windows main and circle, core:default only), src-tauri/tauri.conf.json:14 (withGlobalTauri true), src-tauri/src/lib.rs:48-198 (about 120 commands).
- Scenario: the circle window (always on top, transparent, built at run time, circle.rs:506) needs three commands (circle_overlay_ready, circle_cancel, circle_capture) but can call ai_set_key, clean_delete, agent_run, transfer_share and the rest. If script ever runs in a bundled page (the CSP is the only barrier), the attacker gets all of it. In particular circle_start + circle_capture (circle.rs:562, :588-619) need no proof of a user selection, and the capture is clipped to the whole virtual screen (krovi-sys/src/capture.rs:318-321), not the armed monitor.
- No exploit path was found: the pages load only bundled assets, script-src falls back to self, there is no eval, and the two @html sites render static icon markup (Icon.svelte:23) and a Rust-generated QR SVG (Transfer.svelte:222).
- Minimal fix: add an app manifest (tauri_build::Attributes::new().app_manifest(AppManifest::new().commands(...))) with per-window permission sets, giving circle its own capability with only the three commands; set withGlobalTauri to false (src/lib/bridge.ts:47,57 already uses the ES module API); clip circle_capture to Overlay.monitor.

### M-4  docs/SECURITY-network-audit.md says "no other outbound sockets", but ElevenLabs TTS is one

- Where: src-tauri/crates/krovi-explain/src/eleven.rs:17 (https://api.elevenlabs.io), :327-359 (ureq agent), called from src-tauri/src/explain.rs:289-350 and :390-510. The audit table (lines 34-49 of the audit doc) lists rows 1-5 and then states there are no other outbound sockets; "eleven" appears nowhere in that file. DECISIONS D-020 does accept ElevenLabs as optional cloud text-to-speech.
- The ElevenLabs code itself is clean: key in the xi-api-key header only, redirects off, no environment proxy, error text never echoes the body, voice id validated before it reaches a URL path, explicit confirm_cloud flag on explain_speak (explain.rs:470-500), audio kept in memory.
- Failure scenario: a reader trusts the "exhaustive" table, but the text the user asks to have read aloud goes to a sixth host. The promise is verifiability; it is currently not true.
- Minimal fix: add a row 6 (trigger: Explain, read aloud with the ElevenLabs engine and a saved key; hosts api.elevenlabs.io/v1/text-to-speech/<voice> and /v2/voices; data: the sentences being read, the key in a header; opt-out: do not save an ElevenLabs key) and correct the "no other outbound sockets" sentence. That file belongs to the security-reviewer role; it was left unedited because this task was scoped to this document.

---

## LOW

### L-1  Clean My OS: a TEMP value inside the profile is accepted as a cleaning root
- src-tauri/crates/krovi-clean/src/categories.rs:193 (root taken from the TEMP variable), :318, :221-230 (acceptable_root refuses a root only if it is, or contains, a guard folder: inside_or_same(guard, root)); the test at :585-624 does not cover a sub-folder of the profile.
- Scenario: TEMP set to the Documents folder of the profile (misconfiguration, or user-level malware writing HKCU Environment). The default-ticked "Temporary files" category then lists every file older than 24 hours in Documents. Recoverable from the Recycle Bin and the user sees the real paths, so LOW.
- Fix: require the temp root final component to be Temp or Tmp, or to equal the GetTempPath2W result, and refuse roots equal to a known user folder (Documents, Desktop, Pictures, Videos, Music, OneDrive).

### L-2  Clean My OS: time-of-check to time-of-use window of up to one batch
- krovi-clean/src/delete.rs:181-247 (validate: symlink_metadata, canonical equality, re-walk), :345-372 (up to 32 items are validated, then recycle is called once for the batch, :262-263). A directory item is re-walked during validation, so the gap can be seconds. The module comment admits "a single batch".
- Scenario: a same-user process, or a lower-integrity sandboxed process that can write under the temp folder, swaps a directory for a junction after item 3 is validated and before the shell call, so Krovi moves files that process could not touch. Recoverable from the Recycle Bin.
- Fix: validate-and-move with batch size 1 for directory items, or run a second cheap pass (symlink_metadata + canonical equality) over every path in the batch immediately before SHFileOperationW.

### L-3  Clean My OS: Windows may delete permanently when the Recycle Bin limit is unknown
- krovi-clean/src/native.rs:118-120 (MaxCapacity is read only if the registry value exists), :166 (FOF_ALLOWUNDO | FOF_NOCONFIRMATION, no FOF_WANTNUKEWARNING), delete.rs:35 (2 GiB per-item cap), :91-95.
- Scenario: on a small volume (under about 20 GB) with a default bin size, a 1.9 GiB cache folder is larger than the bin allows. With NOCONFIRMATION the shell deletes it for good. Narrow, but "Recycle Bin only" is the headline promise.
- Fix: when MaxCapacity is absent, cap the item at 10 percent of the volume size (GetDiskFreeSpaceExW total) in bin_facts_for_root.

### L-4  ai_complete sends arbitrary text with no consent flag
- src-tauri/src/ai.rs:583-619; the wrapper src/lib/ai.ts:722-736 has no confirm parameter. Compare circle_ask_vision, which refuses without confirm: true (circle.rs:674). No UI calls complete() yet (grep: only the helper).
- Scenario: a future feature (or any code in the webview) summarises clipboard or note text through the user provider with no disclosure step. It is the user own account, so this is a privacy-honesty gap, not exfiltration.
- Fix: add confirm: bool (default false returns consent-required) and a disclosure string naming provider and model, like Disclosure in circle.rs.

### L-5  eleven::Request derives Debug and holds the API key in its headers
- krovi-explain/src/eleven.rs:106-112. Any Debug print of a request shows xi-api-key. Nothing prints it today (grep of Debug format strings in eleven.rs, speak.rs, summary.rs finds tests only). The AI HttpRequest correctly has a redacting Debug (krovi-net/src/ai.rs:1088-1100).
- Fix: replace the derive with a manual Debug that prints header names only.

### L-6  Banned-model matcher is hyphen-only and ignores display names
- krovi-net/src/ai.rs:68-86 matches muse-spark, muse-wonder, muse-glimmer as lower-case substrings of the id. muse_spark_1_3, muse.spark and MuseSpark pass. ModelInfo.name (:1696-1704) is never checked.
- Scenario: a provider (or a user typing a custom model id) uses a different separator; the hard ban in AGENTS.md is bypassed.
- Fix: normalise before matching (lower-case, drop every non-alphanumeric) and compare against musespark, musewonder, museglimmer, deepseek; apply it to id and display name; mirror it in src/lib/ai.ts isExcludedModel.

### L-7  CSP and window hardening
- src-tauri/tauri.conf.json:37 sets default-src self; style-src self unsafe-inline; img-src self data:; font-src self data:; connect-src self ipc: http://ipc.localhost (keywords are single-quoted in the file). Good baseline: no script relaxation, no remote connect. Missing: base-uri none, form-action none, frame-ancestors none (these do not fall back to default-src). withGlobalTauri: true (:14) exposes window.__TAURI__ needlessly (see M-3).
- Fix: append base-uri, form-action and frame-ancestors with the value none (single-quoted); set withGlobalTauri to false.

### L-8  MCP registry stores command arguments and URLs as plain text and echoes them
- krovi-agent/src/registry.rs:321-326 (arguments), :332-338 (an https URL with a query string is accepted), persisted by src-tauri/src/agent.rs:209-214 into the settings table (and therefore into backups), echoed into the integration list at registry.rs:177-181.
- Scenario: a user pastes an argument like --api-key=... or a URL like https://host/mcp?token=... . The secret lands in SQLite and in the UI, against the "keys only in Credential Manager" rule. Nothing connects to a server today, so LOW; it becomes MEDIUM the day execution lands.
- Fix: reject URL query strings and arguments that look like key= or token=, show a "do not put secrets here" note, and do not echo arguments back.

### L-9  Voice-originated plans run Safe steps with no card, including launching any Start-menu app
- krovi-agent/src/plan.rs:5-14 (open app, timer, note are Safe), src-tauri/src/agent.rs:403-416 (voice_plan runs with confirmed = []), src-tauri/src/dictation.rs:12-15 (the Agent shortcut is off by default).
- Scenario: a mis-heard or injected sentence ("open powershell") launches a shell (no arguments) or saves a note, with no confirmation. Launch only, no arguments, and the shortcut is opt-in, so LOW.
- Fix: for voice-originated plans downgrade OpenApp of shells and script hosts (cmd, powershell, pwsh, wt, regedit, mmc, wscript, cscript) to Confirm.

### L-10  check_openable accepts more mailto: than the builder can produce
- krovi-agent/src/url.rs:283-306 allows ?, = and & and any parameter name, so a link like mailto:a@b.co?attach= followed by a percent-encoded UNC path passes, although its doc comment says "only the characters crate::mail emits". Only mail::compose_url output reaches open_link today (agent.rs:260-263; plans are held server-side), so this is latent.
- Fix: parse the query and allow only subject and body.

### L-11  API keys stay in Credential Manager after uninstall
- src-tauri/tauri.conf.json:53-60 (the NSIS config has no uninstall hook); krovi-explain/src/secrets.rs:48 (CRED_PERSIST_LOCAL_MACHINE). Targets: Krovi/ai/<id> for the five providers, and Krovi/Explain/ElevenLabs.
- Fix: an NSIS uninstall hook running cmdkey /delete:<target> for each, or an in-app "Remove all keys" button plus a line in the uninstall docs.

### L-12  Demo script and ignore-file hygiene
- qa/polish/clips/cortex-answers.mjs: verified that the token is never written. The output object holds only generatedAt, note and per-answer id, model, prompt, answer (:75-95), and answers.json does not exist in the tree. Remaining nits: SNOWFLAKE_ACCOUNT_URL is not checked against *.snowflakecomputing.com before the Bearer token is sent (:28, :57-66); fetch follows redirects (Node strips Authorization cross-origin, but redirect: "error" is safer); the error path prints up to 160 characters of the response body and redacts only the "Bearer ..." form (:68).
- .gitignore ignores .env, .env.*, *.key, *.pem only. snowflake.env, other *.env files and the .krovi-demo folder are not covered. The real file lives outside the repo, so nothing is exposed today.
- Fix: add *.env and .krovi-demo/ to .gitignore; validate the host suffix and set redirect: "error" in the script.

### L-13  Third-party screenshots sit untracked in the repo root
- Ten files named "Screenshot 2026-11-0*.png" in the repo root (untracked and not ignored; .gitignore covers qa/**/*.png and reference/ but not the root). The five opened show competitor marketing pages (VoiceOS and similar). D-002 permits user-supplied conceptual screenshots but never as shipped material; one "git add -A" would commit third-party imagery. None of the five opened contains a key or personal data (the masked key field is a mock-up on the competitor page).
- Fix: move them under reference/ (already ignored) or add /Screenshot*.png to .gitignore.

### L-14  "Zeroed, never stored" covers only the raw BGRA buffer
- krovi-sys/src/capture.rs:190-200 wipes Image.bgra on drop. Copies made on the way out (bgra_to_rgba at :211, the PNG bytes, the base64 String at circle.rs:683-687 and :1089, and the JSON request body in HttpRequest.body) are ordinary heap allocations, freed but not wiped. The comment at circle.rs:1097 ("zeroed, never stored") and the module docs overstate this. Nothing is written to disk or logged (checked: no fs::, File, temp or print calls in capture.rs, ocr.rs, circle.rs).
- Fix: say "best effort, the raw frame only", or wrap the PNG and base64 buffers in a zeroing guard.

---

## INFO

- I-1  src-tauri/tauri.conf.json:5 still says version 0.2.0. Bump before building the v0.3 installer.
- I-2  The Snowflake host check blocks SSRF and internal addresses (suffix .snowflakecomputing.com, lower-case labels, no port/path/userinfo; krovi-net/src/ai.rs:459-507), but any Snowflake customer can own a valid *.snowflakecomputing.com name. If the stored account URL were tampered with, the token would go to that account. That needs local write access to ai.v1; no action.
- I-3  rundll32.exe url.dll,FileProtocolHandler is spawned by bare name in agent.rs:227 (and extras.rs:724, not wired yet) while krovi-sys/src/web.rs:119-142 uses ShellExecuteW directly. Both receive only validated http(s)/mailto text. Prefer one opener (ShellExecuteW) and avoid the LOLBin pattern that EDR products flag. Note for later: extras.rs os_open_file opens file paths through the same call and relies on a deny-list of extensions (krovi-extras/src/fsx.rs:45-53) that omits, for example, .chm, .settingcontent-ms, .search-ms, .library-ms, .xll, .appref-ms, .py, .pyw, .iso; use an allow-list when that module is wired in.

---

## Verified clean

### 1. AI keys and outbound hosts
- Keys are stored, read and deleted only through Credential Manager targets Krovi/ai/<id> (krovi-net/src/ai.rs:142-213, src-tauri/src/ai.rs:64-95, krovi-explain/src/secrets.rs:32-95). AiSettings (the ai.v1 row) has no key field and a unit test pins the field list (ai_settings.rs:390-413); circle.v1 carries no key.
- The webview learns only hasKey and the last four characters, and only when the key is at least 20 characters (secrets.rs:11-17). No emit payload in ai.rs, circle.rs, agent.rs, clean.rs or explain.rs carries a key (every emit call was checked).
- Error strings: key problems return fixed sentences that never echo input (krovi-net/src/ai.rs:357-413, :486-507); provider bodies are scrubbed of the key before classification (:1770) and only a 240-character provider message is kept as detail. Credentials and HttpRequest have redacting Debug impls (:1053-1060, :1088-1100). Transport errors carry only the failure kind (http.rs:90).
- No logging of requests: no log or tracing dependency in these paths; the only eprintln! calls are shortcut errors and Clean My OS counts (circle.rs:862, dictation.rs:540, clean.rs:173, :193, :271).
- Keys travel in headers only (Authorization, x-api-key, x-goog-api-key), never in a URL (ai.rs:92-101, :1236-1247); header values cannot contain CR or LF because keys must be ASCII-graphic (:362).
- Hosts are exactly api.anthropic.com, api.openai.com, generativelanguage.googleapis.com, api.groq.com, plus *.snowflakecomputing.com under /api/v2/cortex/ (ai.rs:1115-1133). The gate runs in the builders (debug_assert) and again in exchange() before any socket call (:1753). request_json disables redirects (a key header can never be replayed to another host), cookies and the environment proxy (http.rs:58-75). TLS uses ureq/rustls defaults; no certificate-verifier override exists anywhere in src-tauri (grep).
- Snowflake account URL: https only, lower-case, no path, query, fragment, port, userinfo or backslash; host must end in .snowflakecomputing.com with valid labels; trailing dot rejected (ai.rs:486-507). It is validated on save, on load (ai_settings.rs:187-190) and on every request (origin_for, ai.rs:1251-1268).
- Banned and excluded models are filtered from provider lists (parse_models, ai.rs:1714-1731), from Snowflake static list (static_models, :546-558), refused by clean_model (:437), by AiSettings::select and on load (ai_settings.rs:119-132, :173-202). Provider::from_id returns None for deepseek and muse-spark-1.3. (See L-6 for the matching weakness.)
- Gemini consent gates ai_set_key, ai_select, ai_test, ai_models, ai_complete, circle_ask_vision, aware_start and aware_ask (src-tauri/src/ai.rs:233-243, :275-278; circle.rs:327-333, :1069-1071).
- Release builds have no DevTools (tauri features are tray-icon and image-png only; Cargo.toml:42).
- src/lib/ai.ts and AiSettings.svelte keep the key in a local draft that is cleared after save or remove (AiSettings.svelte:79-80, :99), use a password input with autocomplete off, and normalise every answer field by field (no key can reach a store).

### 2. Agent Mode
- Only http and https links open: validate_inner rejects file:, javascript:, data:, ms-*, vbscript:, drive paths, UNC, credentials, backslashes, non-ASCII, bad hosts and ports (krovi-agent/src/url.rs:193-262, tests :500-572); check_openable runs again right before the OS call (agent.rs:241-245). Typed addresses are Confirm; a known site is Safe only for a path-less address that matches a table host (parse.rs:982-995).
- The webview cannot smuggle an action: it sends only a plan id and step ids; plans live in Rust memory (16 at most); Blocked steps never dispatch; a Confirm step dispatches only if its exact id is listed; nothing runs twice (krovi-agent/src/session.rs:114-162 and its tests). Input is capped at 1000 characters (parse.rs:42). The Action enum has no shell, file or delete variant.
- Apps: launched only if the target is in the cached Start-menu scan (agent.rs:128-132, :264-268) and WinHost::launch_app accepts it (an AppsFolder entry or an existing .lnk; krovi-voice/src/os.rs:353-361); no arguments; no cmd or shell is involved anywhere in agent.rs.
- Voice: handle_voice calls voice_plan, which runs run_plan with an empty confirmed list, so only Safe steps run and Confirm steps wait as cards; the frontend does not auto-confirm (AgentPanel.svelte:87-98, agent.ts:361-364); workflows and the app launcher go through the same agent_run (workflowRun.ts, launcher.ts:97-105). The Agent shortcut is off by default.
- Mail: compose_url validates addresses (no commas, CRLF, percent signs or question marks), collapses and percent-encodes subject and body, and only builds Gmail or Outlook web compose links or a mailto: with subject and body (mail.rs:60-153). Nothing sends or reads mail; the confirm card shows address, subject and body.
- MCP: registry only. No Command::new, spawn or socket use touches MCP data; entries default to enabled false; commands are validated for metacharacters and spaces (registry.rs:299-331).

### 3. Clean My OS
- Delete takes ids from the stored scan plus the scan id, never paths; unknown ids and stale scan ids are refused (delete.rs:300-332, tests :556-598; clean.rs:221-285). The scan is dropped after every delete.
- Per item, right before the move: no wildcards, not a reparse point, kind unchanged, canonical path equals the scanned path (catches a junction anywhere along the path, and ..), depth at least 2, inside the guard root with the right shape (Tree, Whole, Files), no link, no recent change or new file when re-walked, size cap, path-length cap (delete.rs:181-247). The shell receives the canonical path. Browser caches are cache folders only; cookies, history and logins are never named (categories.rs:254-298).
- Moves use SHFileOperationW with FOF_ALLOWUNDO and are gated by Recycle Bin facts (fixed drive, NoRecycleFiles policy, NukeOnDelete, MaxCapacity), so items that would be deleted for good are left alone (native.rs:97-124, delete.rs:75-97). The Recycle Bin is only measured, never emptied; no elevation; paths are never logged (clean.rs:13-17, :173, :271).
- Gaps are listed as L-1 to L-3.

### 4. Circle to Ask and screen awareness
- There is no stream or subscribe API; one BitBlt per call; nothing runs at startup, on a timer or in the background (capture.rs:1-18, circle.rs:844-865). Circle: one capture per arming (circle_capture consumes the armed overlay, circle.rs:596-598), the overlay cancels itself after 90 seconds, and Esc, right-click or the shortcut cancel without capturing. Rectangle input is finite-checked, clipped and area-capped at 20 million pixels (capture.rs:28, :106-172).
- Frames: no fs::, File, temp-file, logging or Debug-of-pixels in capture.rs, ocr.rs or circle.rs; Image has a size-only Debug, is wiped on drop, and the held slot expires after 120 seconds, on discard, on panel close and when a session ends (krovi-sys/src/session.rs:74-150, circle.rs:423-431, :929-947). Krovi own windows are excluded from the frame (SetWindowDisplayAffinity). OCR is Windows.Media.Ocr, local only. Fullscreen detection reads geometry, style bits and class name only (fullscreen.rs:25).
- Session: starts only from aware_start; length clamped to 1-30 minutes in both clamp_minutes and Session::start; ends on the monotonic clock via a timer thread plus an is_over check in aware_ask and aware_status; a failed timer spawn aborts the session; ending clears the held image, resets the tray tooltip and emits aware-state (circle.rs:929-1025). Vision needs three things together: the session was started with the picture option, the question is asked with the picture option, and the provider is still usable (circle.rs:1062-1074); the provider is fixed at session start. Circle Ask AI additionally refuses without confirm: true (circle.rs:674). The system prompt tells the model that text inside the picture is content, not instructions (krovi-net/src/ai.rs:52-54). The indicator gap is M-1.
- circle_open_keys_page, ai_open_keys_page and circle_search can open only fixed key pages or a Google search whose query is percent-encoded; open_url re-checks the exact shape (krovi-sys/src/web.rs:94-117, tests :211-232).

### 5. Tauri configuration
- One capability (default.json) for windows main and circle, permission core:default only. No shell, fs, http, opener or dialog plugin permission is granted. Plugins initialised in Rust: global-shortcut, single-instance, autostart, notification (Cargo.toml:43-46); none is exposed to the webview through a capability.
- The CSP blocks remote scripts, remote images and any connect-src except IPC (tauri.conf.json:37). @html is used twice, both with non-user content. Gaps: M-3, L-7.

### 6. Secrets in the repository
- Searched all tracked and untracked text outside node_modules, target, dist and .git (and the hidden .opencode and .playwright-mcp directories separately) for sk-ant-, sk-proj-, sk-, AIza, gsk_, ghp_, github_pat_, AKIA, xox*, JWT-shaped strings, PEM headers, long Bearer values, api_key assignments and SNOWFLAKE_PAT. The only hits are obviously fake test fixtures (krovi-net/src/ai.rs:2059-2062, :2303; vision.rs:120; ai.test.ts; ai.tauri.test.ts).
- No .env, *.pem, *.key, *.pfx or credentials file exists in the tree. opencode.jsonc and .opencode contain no keys. web/ contains no key-shaped strings.
- snowflake.env was not opened. cortex-answers.mjs reads it from the .krovi-demo folder in the user home, outside the repo, and writes only prompts, model id and answers; answers.json is not present.

## Open items for the owners

| ID | Owner | Action |
|----|-------|--------|
| M-4 | security-reviewer | Add the ElevenLabs row to docs/SECURITY-network-audit.md and fix the "no other outbound sockets" sentence |
| M-1 | panel-shell / circle | Keep the awareness indicator visible, or end the session when the window hides |
| M-2 | circle / live-data | Route Circle and AI copies around the clipboard history, or change the wording |
| M-3 | panel-shell | App manifest with per-window command permissions; withGlobalTauri off |
| L-1 to L-3 | clean | Temp-root hardening, per-batch re-check, bin-capacity cap |
| L-4 to L-6 | ai / explain | Consent flag on ai_complete, redacting Debug, normalised model ban |
| L-7 to L-14, I-1 to I-3 | various | See each item |
