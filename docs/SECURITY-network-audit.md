# Krovi — Network / Privacy Audit

_Last updated: 2026-11-03 (added Circle to Ask / screen awareness / bring-your-own-key AI: row 5 and its
own section; added the opt-in Whisper model download: row 4 and its own section;
2026-11-02: added the "Planned (not yet implemented)" pre-declaration section, below). This document is the evidence behind Krovi's "verifiable
privacy" claim: a literal account of every place the app can open a network socket, what
triggers it, what is sent, and how to turn it off. It is maintained in-repo so the claim is
checkable against the source — and should be re-run against each released build._

## TL;DR

The Krovi **desktop app** makes **no telemetry, analytics, or account calls**. Only **two**
live network endpoints exist in the code, both **keyless** public APIs, plus a **manual-only**
update check, plus (added 2026-11-03) an **optional, user-initiated** download of a speech model
from Hugging Face (row 4 below; the downloader exists in `krovi-net` but no app code calls it
yet). The **website** (`web/`) makes **zero** network requests (no trackers, no CDN,
no fonts-from-CDN) — it only contains `<a href>` links to the public GitHub repo.

_Added 2026-11-03: **bring-your-own-key AI** (row 5) and **Circle to Ask / screen awareness**. They
make a network request only when the user has saved their own key **and** confirms, for that one
request, a sentence naming the provider; screen capture itself never touches the network. See
[Circle to Ask, screen awareness and AI](#circle-to-ask-screen-awareness-and-bring-your-own-key-ai).
The "Planned" section at the bottom keeps the original pre-declaration of those features for
comparison, with the differences noted. The Whisper model download has moved out of it too: see
[Whisper model download](#whisper-model-download-opt-in-user-initiated)._

## Method

Literal, repository-wide grep for anything that can open a socket or make a request:
`ureq`, `reqwest`, `hyper`, `isahc`, `TcpStream`, `UdpSocket`, `std::net::`, `fetch(`,
`XMLHttpRequest`, `navigator.geolocation`, and the endpoint hostnames. Every hit was traced to
its call site and classified below. (Build caches `target/` and `node_modules/` are excluded.)

## Declared outbound calls (exhaustive)

| # | Call | Trigger | Endpoint (keyless) | Data sent | Opt-out |
|---|------|---------|--------------------|-----------|---------|
| 1 | **Weather** | Weather tool open / manual refresh | `GET https://api.open-meteo.com/v1/forecast` | Coarse latitude/longitude only (coarsened to ~0.1Â° â‰ˆ 11 km before the request). No identifiers. | Don't open the Weather tool / don't grant location; the whole app works offline without it. |
| 2 | **Currency rates** | Currency tool refresh | `GET https://api.frankfurter.dev/latest` (ECB data) | The base currency code only (e.g. `USD`). No identifiers. | Use cached rates (conversion math is offline); disable the Currency tool. |
| 3 | **Update check** | **Manual only** (user clicks "check for updates") | GitHub Releases (public) | Nothing beyond a standard HTTPS GET. Never automatic. | N/A — only runs when the user asks. |
| 4 | **Whisper model download** (optional) | **User action only**: the user picks a model and presses download. Never prefetched, scheduled or retried in the background. | `GET https://huggingface.co/ggerganov/whisper.cpp/resolve/main/<file>`, which redirects to Hugging Face's CDN (`*.hf.co` / `*.huggingface.co`, e.g. `cdn-lfs.hf.co`, `cas-bridge.xethub.hf.co`). See [Whisper model download](#whisper-model-download-opt-in-user-initiated). | A plain GET per hop with a `Krovi/<version>` User-Agent. No request body, no cookies, no account, no identifiers, no user data. | Don't press download (speech then simply has no model installed); delete the file in settings at any time. |
| 5 | **AI provider, bring-your-own-key** (optional) | **User action only**: "Ask AI" on a circled area, or "Ask" with the picture option during a screen-awareness session, after the user presses the button under the disclosure sentence that names the provider; **Test connection** and **Show available models** in Settings → AI; a feature's text question (`ai_complete`, none exists yet). Needs a key the user saved themselves. Never automatic, never in the background, never on opening a page. | One request to the host of the one provider the user chose: `api.anthropic.com/v1/{messages,models}`, `api.openai.com/v1/{chat/completions,models}`, `generativelanguage.googleapis.com/v1beta/models[/<model>:generateContent]`, `api.groq.com/openai/v1/{chat/completions,models}`, and (experimental) `<account>.snowflakecomputing.com/api/v2/cortex/v1/chat/completions` on the user's own Snowflake account. See [Settings → AI](#settings--ai-bring-your-own-key-providers-and-endpoints) and [Circle to Ask, screen awareness and AI](#circle-to-ask-screen-awareness-and-bring-your-own-key-ai). | A question or fixed test text, and for Circle to Ask **one** PNG of the area the user chose (longest side at most 1568 px). A connection test sends the words "Reply with the single word OK." and nothing of the user's. The key is in a request header. No OCR text, no history, no identifiers. | Don't save a key (then no request can be built: the code stops with "Set up an AI provider" first); don't press the button. Gemini additionally stays off until its notice is accepted. |

There are **no other outbound sockets** (a Google search or an API-key page is opened by the user's own
browser when they click a button, and is not a Krovi socket). The one **inbound** listener is Quick Transfer — LAN-only,
token-gated, exists only while a share is open, auto-expires (see its section below). Weather
city search adds `geocoding-api.open-meteo.com` (only the typed city text). No API keys are shipped. Coarse location is used **only** for
weather. Screen-time stores process names + durations locally (never titles/URLs) and is never
transmitted. Clipboard/notes/shelf/to-do data never leaves the machine.

## Code evidence (where the socket actually is)

- The **single** HTTP call site in Krovi's own code is `src-tauri/crates/krovi-net/src/http.rs`
  (`ureq` GET over rustls), gated behind `#[cfg(feature = "net")]`.
- It is called only by:
  - `src-tauri/crates/krovi-net/src/weather.rs` — endpoint constant
    `https://api.open-meteo.com/v1/forecast`.
  - `src-tauri/crates/krovi-net/src/currency.rs` — endpoint constant
    `https://api.frankfurter.dev`.
  - `src-tauri/crates/krovi-net/src/whisper_models.rs` (`download_model`, user-initiated) via
    `http::open_download` — base URL constant `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/`.
    As of 2026-11-03 nothing in `src-tauri/src` or the frontend calls `download_model` yet.
  - `src-tauri/crates/krovi-net/src/ai.rs` (`ask_vision`, `complete_text`, `list_models`,
    `test_connection`; all user-initiated) via `http::request_json`, the only other socket in
    `krovi-net`. (`vision.rs` is now a thin wrapper over it, kept for Circle to Ask.) Called only by
    `src-tauri/src/ai.rs` (`ai_test`, `ai_models`, `ai_complete`, and `ask_vision` for Circle) and,
    through that, `src-tauri/src/circle.rs` (`circle_ask_vision`, `aware_ask`). The URL is built in the
    crate from a closed provider enum (plus, for Snowflake, a validated account URL) and checked
    against the allowed hosts twice (when the request is built, then in `exchange` before the socket);
    the webview cannot pass one.
- With the `net` feature **off** (the default build), `fetch_forecast` / `fetch_rates` /
  `download_model` return `NetError::Disabled` / `ModelError::Disabled` — i.e. no socket at all.
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
the outbound surface is two keyless GETs, a manual update check and an opt-in model download,
isolated in one auditable module, with the source public. Anyone can re-run the grep above or
trace the built binary.

## Whisper model download (opt-in, user-initiated)

_Added 2026-11-03. Code: `src-tauri/crates/krovi-net/src/whisper_models.rs` (catalog, redirect
policy, streaming/verification core) and `src-tauri/crates/krovi-net/src/http.rs`
(`open_download`, the only socket). **Status: implemented and unit-tested at crate level; not
yet wired to the Tauri commands or the settings UI**, so the running app does not contain a
button that reaches it yet._

Speech recognition itself stays on-device: inference makes no network call and no audio ever
leaves the machine. The only network step is fetching the model file the user chose.

| Property | Behaviour (as implemented) |
|---|---|
| Trigger | `download_model()` is called only by explicit user action. No prefetch, no background or scheduled download, no automatic model choice, no retry loop. |
| Destination | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/<file>` (the official whisper.cpp model repository) and the CDN it redirects to. The URL is built in the crate from the catalog file name; callers cannot pass a URL, and `download_model` only accepts an unmodified catalog entry. |
| Redirects | Followed by hand (ureq's own redirect following is off). At most **5**. Every hop, including the first, must be `https`, on port 443, without credentials, with host `huggingface.co`, `*.huggingface.co` or `*.hf.co` (whole-label match: `huggingface.co.evil.com`, `evilhuggingface.co`, `user@host` tricks and `http://` are refused). Relative `Location` values are resolved against the current hop. |
| Data sent | A `GET` per hop with `User-Agent: Krovi/<version>` and `Accept-Encoding: identity`. No request body, no cookies (ureq's cookie support is not compiled in), no `Authorization`, no identifiers. The only information the host sees is what any HTTPS GET reveals (IP address, the requested file name). |
| Timeouts | Connect 15 s, write 15 s, any single read stalls at most 30 s; no overall deadline (the files are 30-190 MB). |
| Integrity | Size and SHA-256 of each file are **constants in the source** (taken from the Git LFS pointers of the repository), never read from the server. The stream is written to `<file>.part` while being hashed; more than the expected number of bytes aborts the download; the digest must match before the file is renamed atomically to its final name. Any failure, mismatch or cancel deletes the `.part`; an existing good file is not touched until the new one is verified. `verify_model()` re-hashes an installed file on demand; `model_status()` only checks size. |
| Storage | Written only into the directory the caller passes in; the file name is validated (`[A-Za-z0-9._-]`, no `..`). `delete_model()` removes the file and any `.part`. Deleting leaves no residual network activity and nothing re-downloads on its own. |
| Cancel | The caller's cancel flag is polled before every read (64 KiB chunks) and between redirect hops. |
| Opt-out | Never call it / never press the button. Without the `net` cargo feature the function returns `ModelError::Disabled` and no socket code is compiled. |

Catalog (all `q5_1` ggml files from `ggerganov/whisper.cpp`; licence string "MIT, OpenAI Whisper
weights via ggerganov/whisper.cpp"):

| id | file | bytes | SHA-256 |
|---|---|---|---|
| `tiny` | `ggml-tiny-q5_1.bin` | 32,152,673 | `818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7` |
| `base` | `ggml-base-q5_1.bin` | 59,707,625 | `422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898` |
| `small` | `ggml-small-q5_1.bin` | 190,085,487 | `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb` |
| `base.en` | `ggml-base.en-q5_1.bin` | 59,721,011 | `4baf70dd0d7c4247ba2b81fafd9c01005ac77c2f9ef064e00dcf195d0e2fdd2f` |

Verification performed (2026-11-03): the pure logic (catalog sanity, redirect allow-list,
size/hash enforcement through an in-memory reader, cancel and failure cleanup, status/delete/
verify) is covered by unit tests that need no network. **Not performed:** an actual download
(deliberately, to avoid a multi-MB transfer during development), a network trace of a real
download, and a test against the live CDN's redirect chain; those remain to be done once the
feature is wired into the app. The catalog sizes and hashes were read from Hugging Face's LFS
pointers and file pages; the SHA-256 is only proven correct for the real file by a successful
end-to-end download, which has not been run yet.


---

## Circle to Ask, screen awareness and bring-your-own-key AI

_Added 2026-11-03 (docs/DECISIONS.md D-021). Code: `krovi-sys/src/capture.rs` (one-shot region capture),
`ocr.rs` (on-device text recognition), `session.rs` (time limits and the 2-minute image slot),
`web.rs` (the only addresses that may be opened); `krovi-net/src/vision.rs` (request builders and the
single POST, socket in `http.rs::post_json`); `src-tauri/src/circle.rs` (glue); `src/circle/**` and
`src/lib/circle.ts` (UI). **Status: implemented, and the pure logic is unit-tested. Not exercised
end to end:** a real capture on a desktop, Windows OCR with a real language pack, a real provider
call with a real key, and a network trace of one. Nothing here has been observed on a wire yet._

### Screen capture (no network involved)

| Property | Behaviour (as implemented) |
|---|---|
| Passive capture | **None.** No code path captures at startup, on a timer or in the background. `circle::setup` reads one settings row and registers the global shortcut; it creates no window, reads no key and captures nothing. There is no capture loop and no frame stream anywhere (`krovi_sys::capture` has a single "grab one still image of one rectangle" entry point). |
| Circle to Ask | The shortcut (default `Ctrl+Alt+C`; **registered by default**, can be turned off, and registering it captures nothing) shows a dimming overlay on the monitor under the cursor. A capture happens **once, only after the user releases a selection**, and only of that rectangle: Rust hides the overlay window, waits ~70 ms for Windows to stop drawing it, then one GDI `BitBlt`. Esc, a right-click, pressing the shortcut again, or 90 s without a selection close it **without capturing**. One arming allows one capture. |
| Screen awareness | Only after an explicit `aware_start`. Default 10 minutes, **hard cap 30**, enforced in `krovi_sys::session` on the monotonic clock (changing the PC clock cannot extend it; the cap is applied again inside `Session::start`). Always announced: an `aware-state` event (for the pill's indicator) and the tray tooltip. One-click stop (`aware_stop`). The screen (the primary monitor, where the pill is) is captured **only inside `aware_ask`, when the user asks a question**; starting a session captures nothing. |
| Krovi's own windows | Excluded from the frame with `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` for the instant of capture and restored afterwards, so the pill or panel does not cover what is being asked about. If Windows refuses (older Windows, or the call fails for a transparent window), a session capture hides the main window for that instant instead; a Circle capture then simply includes the pill. **Unverified on a real desktop.** |
| Storage | **Memory only.** Nothing is written to disk, nothing goes to the clipboard, no type that holds pixels or screen text derives `Debug`, and nothing about the content is logged. Pixel buffers are overwritten with zeros when dropped (best effort). A circled image is kept for **at most 2 minutes** (so an optional "Ask AI" is possible) or until the user presses "Drop the picture", the panel closes, or the next capture replaces it; session frames are dropped as soon as the answer is returned. Ending a session clears the slot. |
| Text recognition | `Windows.Media.Ocr`, on this PC, using the user's profile languages. Needs an OCR language pack; if there is none the UI says so and tells the user what to install. No network. |
| What the webview sees | Recognised text and the line boxes. **Never the picture.** |
| Things Krovi opens | Only two shapes, both on a user click and both opened by the user's browser, not by Krovi: a Google search for the first 300 characters of the text the user selected, and a provider's API-key page. Both are built in Rust and `krovi_sys::web::open_url` refuses anything else. |

### AI provider requests (the only network use)

| Property | Behaviour (as implemented) |
|---|---|
| Zero without a key | There is no bundled, demo or trial key. With no saved key the Ask-AI button is disabled with the text "Set up an AI provider in Settings → AI", and the backend returns `no-provider` before it builds a request. |
| Trigger | A request is built only for `circle_ask_vision` (the circled image) or `aware_ask` with the picture option (a session frame). Each needs `confirm: true` from the UI, which is set only by the button that sits under the disclosure sentence naming the provider ("This sends the selected image to <Provider> using your key."). For a session, the picture option must also have been agreed at its start (the sentence names the provider there too) and is a separate tick box for each question. The command refuses with `consent-required` / `not-allowed` otherwise. |
| Destination | Exactly one of `POST https://api.openai.com/v1/chat/completions`, `POST https://api.anthropic.com/v1/messages`, `POST https://generativelanguage.googleapis.com/v1beta/models/<model>:generateContent`, `POST https://api.groq.com/openai/v1/chat/completions`, or (experimental) `POST https://<account>.snowflakecomputing.com/api/v2/cortex/v1/chat/completions`. The provider is a closed enum, the URL is built in `krovi-net`, the model id is restricted to `[A-Za-z0-9._-]` (1 to 80 characters; a `/` between parts only for providers that send the model in the body, never for Gemini whose model is in the path) so it cannot change the path, and the host is checked against the allowed hosts both when the request is built and again before sending. See [Settings → AI](#settings--ai-bring-your-own-key-providers-and-endpoints). |
| Sent | A fixed instruction (which tells the model that text inside the picture is content, never instructions), the user's question (cut to 2000 characters) and **one PNG** of the chosen area or screen, scaled down so its longest side is at most 1568 px. **Not sent:** the recognised text, history, identifiers, clipboard, files, audio. Prompt-injection note: text visible in the picture reaches the model like any other content; the model has no tools here, it can only answer. |
| Key | Windows Credential Manager, generic credential, target `Krovi/ai/<id>` for `anthropic`, `openai`, `gemini`, `groq`, `snowflake` (`Provider::credential_target`). Read at the moment of the call, put in a request header (`Authorization: Bearer`, `x-api-key`, `x-goog-api-key`; **never** in a URL), never returned to the webview, never logged, scrubbed from any provider error text before it is shown. The Settings → AI page owns key entry (`ai_set_key`); `circle_set_key` is a thin command over the same storage and the same checks. |
| Transport | `ureq` over rustls. **No redirects are followed** (a 3xx is an error, so a header carrying a key can never be replayed to another host), no cookies, no proxy from the environment, `User-Agent: Krovi/<version>`. Connect 10 s, write 60 s, read 120 s, reply body capped at 4 MiB. Transport errors carry only the failure kind, never the URL or a header. |
| No relay, no cost to Krovi | The request goes from the user's PC straight to the provider; Krovi runs no server and sees neither request nor answer. It is the user's key and quota. |
| Models | Defaults (text / pictures) `claude-haiku-5-5`, `gpt-6-luna`, `gemini-3.5-flash-lite`, `openai/gpt-oss-120b` / `qwen/qwen3.8-27b` (Groq), `llama3.1-8b` / `claude-sonnet-4-5` (Snowflake); chosen in Settings → AI from the provider's own list or typed by the user. The `muse-spark`, `muse-wonder` and `muse-glimmer` families are banned and never listed, selectable or sent to; DeepSeek ids are refused too (not in v1). |
| Errors | Mapped to plain sentences with a machine-readable kind (`auth`, `billing`, `quota`, `rate-limit`, `eligibility` such as a free tier or region the provider does not offer, `model`, `blocked`, `unavailable`, `network`, `other`); the provider's own message is shown only as a trimmed "detail". |
| Opt-out | Never save a key; turn the shortcut off in settings; do not start a session. Without the `net` cargo feature `ask` returns an error and no socket code is compiled. |

Differences from the pre-declaration under "Planned" below: the providers are Anthropic, OpenAI, Google
Gemini, Groq and (experimental) Snowflake Cortex (updated 2026-11-03 with Settings → AI; the first
version of this section had OpenAI, Anthropic and Gemini only); the Circle to Ask shortcut is registered
by default rather than disabled until turned on (it captures nothing until pressed, and nothing is sent
without a key and a confirmation); a temporary screen-awareness session (D-021) exists in addition.
Circle to Ask no longer keeps its own provider or model choice: it uses Settings → AI's (`ai.v1`).

### Settings → AI (bring your own key): providers and endpoints

_Added 2026-11-03 (docs/DECISIONS.md D-022; research in docs/product/AI_PROVIDERS.md). Code:
`krovi-net/src/ai.rs` (registry, requests, parsing, error classification, test connection, model lists),
`krovi-net/src/ai_settings.rs` (what `ai.v1` holds and the consent rules), `krovi-net/src/http.rs`
(`request_json`, still the only socket), `src-tauri/src/ai.rs` (commands), `src/lib/ai.ts` and
`src/components/AiSettings.svelte` (UI). **Status: the pure logic is unit-tested (request shapes, error
classification against sample bodies written from the providers' documentation, banned-model
filtering, key redaction). Not exercised against any live provider: no real key has been used, so
no request below has been observed on a wire.**_

Every endpoint Krovi can contact for AI, and nothing else. All are HTTPS, no redirects followed, no
cookies, no environment proxy, `User-Agent: Krovi/<version>`, key in a header.

| Provider (id) | Host | Requests | Auth header | Notes |
|---|---|---|---|---|
| Anthropic (`anthropic`) | `api.anthropic.com` | `GET /v1/models?limit=100`, `POST /v1/messages` | `x-api-key`, `anthropic-version: 2023-06-01` | |
| OpenAI (`openai`) | `api.openai.com` | `GET /v1/models`, `POST /v1/chat/completions` | `Authorization: Bearer` | every request sends `"store": false` |
| Google Gemini (`gemini`) | `generativelanguage.googleapis.com` | `GET /v1beta/models?pageSize=200`, `POST /v1beta/models/<model>:generateContent` | `x-goog-api-key` (never `?key=`) | off until the user accepts the notice (consent version stored in `ai.v1`) |
| Groq (`groq`) | `api.groq.com` | `GET /openai/v1/models`, `POST /openai/v1/chat/completions` | `Authorization: Bearer` | OpenAI-compatible; image model `qwen/qwen3.8-27b` (preview) |
| Snowflake Cortex (`snowflake`, experimental) | the user's own `<account>.snowflakecomputing.com` (also `.privatelink.`), validated: https only, no path/port/user info | `POST /api/v2/cortex/v1/chat/completions` (no model-list request: a static list) | `Authorization: Bearer <PAT>`, `X-Snowflake-Authorization-Token-Type: PROGRAMMATIC_ACCESS_TOKEN` | the account URL is stored in `ai.v1` (an address, not a secret); the PAT in Credential Manager |

| Property | Behaviour (as implemented) |
|---|---|
| Triggers | **Test connection** (two requests: the model list, then a one-token completion with the text "Reply with the single word OK."; Snowflake only the completion), **Show available models** (one `GET`), a feature's own ask (Circle to Ask; later `ai_complete`). Nothing runs when Settings is opened, in the background, or on a timer. No retries: an error is shown once. |
| Why two steps | A model list passes when the key exists even if inference is blocked (billing, quota, free-tier eligibility, region), so only the one-token request proves the key works. The result is shown as separate statuses (authentication, billing, quota and rate, eligibility, model). |
| Where keys live | Windows Credential Manager only (`Krovi/ai/<id>`). Not in SQLite (`ai.v1` has provider, models, consent and the Snowflake address only; a test pins its field names), not in localStorage, not in a URL, not in a log, backup or error message. `Debug` of every request and of the credentials is redacted (a test builds every request kind for every provider and checks it). The webview receives only "a key is saved" and the last four characters of a long key; the status the UI stores is rebuilt field by field. |
| Key checks | Trimmed; no spaces or line breaks; 16 to 400 characters (2,048 for a Snowflake token); refused when it plainly belongs to another provider (`sk-ant-`, `gsk_`, `AIza`, `sk-`), never requiring a prefix. |
| Gemini gate | Off until the user ticks the notice (18+, own key, Google's terms, free-tier human review and training, region and billing for EEA/UK/CH). `ai_set_key`, `ai_select`, `ai_test`, `ai_complete` and Circle's Ask all refuse Gemini without it (`consent-required`); withdrawing it deselects Gemini. The notice text is versioned (`GEMINI_CONSENT_VERSION`); an older version counts as not accepted. |
| Banned and excluded models | `muse-spark`, `muse-wonder` and `muse-glimmer` (any provider, any position in the id) are filtered from every list, refused by `clean_model`, refused on load of `ai.v1`, and never a default; `deepseek` ids are refused too (not in v1). Tests cover each. |
| No proxy, no account | Requests go from this PC to the provider's host. Krovi runs no server, relay, account or sync for them. Local tools never need a key. |
| Opening a key page | `ai_open_keys_page` opens a fixed list (`krovi_sys::web::keys_page`) in the browser; Krovi sends nothing. A test keeps the list equal to the registry's `key_page`. |
| Session usage | In memory only: request counts, token counts and the last error kind per provider since Krovi started. No content, no key, not stored. |

### Not verified yet
* A real `BitBlt` capture (multi-monitor, 125% / 150% scaling, a monitor to the left of the primary), and
  that the display-affinity exclusion really removes Krovi's own windows from the frame.
* Windows OCR on a real screen, and the behaviour when no OCR language is installed.
* A real request to each provider, and the exact error bodies of the real services (the classification is
  tested against sample bodies written from their documentation, not against live responses).
* A network trace showing no traffic from capture, OCR, an idle session, or an "Ask AI" that was not confirmed.

---

## Quick Transfer (desktop <-> phone over the local network)

_Added 2026-11-01 (owner: transfer). **This is the one feature in Krovi that accepts inbound connections.** Everything above this line describes outbound calls; the statement "there are no other sockets" is superseded by this section for the inbound side. Quick Transfer makes **zero** requests to the internet (see "Internet requests" below)._

### What listens

| Property | Value |
|---|---|
| Who | the Krovi app process itself (no helper process, no Windows service, no scheduled task) |
| Protocol | plain **HTTP/1.1 over TCP** (`tiny_http` 0.12, synchronous, one OS thread per request, at most 48 concurrent handlers, then 503). **No TLS** (see limitations) |
| Interface | exactly **one**: the private IPv4 address (10/8, 172.16/12, 192.168/16) of the active LAN adapter. Never `0.0.0.0`, never IPv6, never loopback in the shipped app. If no private LAN address exists the share is refused with "No Wi-Fi or LAN network found"; there is no fallback to a wider bind. A caller-supplied override (tests only) rejects `0.0.0.0`, multicast and broadcast |
| Adapter choice | `GetAdaptersAddresses`: adapter up + private IPv4 + not loopback/tunnel/PPP + not a VPN/virtual adapter (Hyper-V, WSL, VirtualBox, VMware, Docker, TAP, Tailscale, ZeroTier, WireGuard, Bluetooth...). Adapters with a default gateway win, then the one the OS routes outbound traffic through, then lowest metric. The Windows Mobile Hotspot adapter is used only if nothing better exists |
| Port | random in **49152-65534**, drawn from the OS CSPRNG at each server start (up to 24 tries, then OS-assigned). One server and one port serve all open shares |
| Exists | **only while at least one share is open.** When the last share closes the listener is dropped; measured: the port refuses connections 0.81 s after "stop", and immediately after expiry (the example process then exits) |

### Routes (the complete list)

| Request | Answer |
|---|---|
| `GET`/`HEAD /s/<token>` | download page: file names and sizes, a Download button each (HTML with inline CSS/JS) |
| `GET`/`HEAD /s/<token>/<n>` | file *n* of that share, streamed from disk (never loaded into RAM); `Range` supported (resume). `Content-Type: application/octet-stream`, `Content-Disposition: attachment`, `nosniff` |
| `GET`/`HEAD /r/<token>` | upload page |
| `POST /r/<token>/up?n=<name>` | one file per request, body streamed to disk |
| **anything else** | `404 Not found`: identical body and headers for unknown paths, wrong tokens, expired tokens, send tokens on receive routes and vice versa. No listing, no directory index, no favicon, no robots |
| right token, wrong method | `405` (`POST` on a send token, `GET` on `/up`). `POST` is only ever accepted on receive tokens |

The router is a pure function (`route.rs`); URLs must be plain origin-form, at most 512 bytes, with no control characters; tokens must match `[A-Za-z0-9_-]{16,64}`. Every response carries `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `X-Frame-Options: DENY`, `Cross-Origin-Resource-Policy: same-origin` and a CSP of `default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; connect-src 'self'; form-action 'self'` (the pages' inline script is static and contains no user data; file names are HTML-escaped and only placed in text nodes and quoted attributes).

### Secrets: tokens, ids, PIN

* **Token:** 20 random bytes (**160 bits**) from the OS CSPRNG (`getrandom`: BCryptGenRandom/ProcessPrng), base64url, 27 characters, one per share. If the OS cannot supply entropy the share is not created (no fallback RNG). Lookup compares against every open share in constant time.
* **Share id** (used in UI events and `transfer_stop`): 48 random bits, independent of the token; it is not a credential.
* **PIN (optional, off by default):** 4 digits, uniform (rejection sampling), shown only on the desktop. When on, the phone must enter it before it sees file names or can download/upload; a correct PIN sets a 128-bit session cookie (`HttpOnly; SameSite=Strict; Path=/s/<token>` or `/r/<token>`; max-age = remaining lifetime). **Five wrong PINs close the share** (event `transfer-closed` reason `locked`). PIN entry is a `GET` form so that `POST` stays reserved for uploads.
* **Why the PIN is optional and off by default:** the 160-bit token is already unguessable, and the QR is shown on the user's own screen, so a mandatory PIN would add friction to the core flow (scan, tap, done) without a matching gain. The PIN exists for the case where the *link itself* leaks (a screenshot, a screen-share, someone reading it over your shoulder). It is **not** encryption: the traffic is plain HTTP (see limitations).

### When a share (and the server) closes

Stop button, expiry (default **10 min**, choices 10/30/60 min, clamped 1 min-6 h by the app, shown as a live countdown), PIN lockout, or app quit (inline Tauri plugin closes everything on `RunEvent::Exit`; the OS also closes the sockets on process exit). Stop, lockout and quit abort in-flight transfers at the next chunk; **expiry lets a transfer already in progress finish** but refuses every new request. Closed shares answer 404 (or connection-refused once no share is left).

### What goes in and out of the disk

* **Send:** only the exact files the user dropped or chose (deduplicated by canonical path). Folders are refused. Only the *name* and *size* reach the phone; the local path never leaves the app.
* **Receive:** files land in `%USERPROFILE%\Downloads\Krovi\`. The client-supplied name is reduced to its last path component, reserved characters (`<>:"|?*`, NTFS stream `:`), control and bidi-override characters are removed, Windows device names (`CON`, `NUL`, `COM1`...) are prefixed, trailing dots/spaces trimmed, length capped; **no path traversal, no overwriting**: collisions become `name (1).ext` via an atomic `create_new` reservation. Data is streamed to `<name>.part` and renamed only when complete; interrupted or over-limit uploads are deleted.
* **Upload cap:** configurable (500 MB / 2 GB default / 10 GB / 50 GB), enforced **per file and per share in total**, both from the declared `Content-Length` (rejected with 413 before any byte is written) and again while streaming (a lying client is cut off).
* **Mark of the Web:** every received file gets a `Zone.Identifier` stream (`ZoneId=3`, Internet), so SmartScreen and Office Protected View treat it like any download. Verified on a real upload.
* Nothing is ever executed by Krovi.

### What is logged

**Nothing.** No log file, no access log; remote addresses are not stored, emitted or shown. Events to the UI carry share id, file name and byte counts. The webview keeps (in its own `localStorage` on this machine) the last 40 received file names/sizes/paths and the three settings (expiry, PIN default, upload cap).

### Windows Defender Firewall

* The first time the app listens, Windows may show its own "Allow access" prompt. Krovi never changes the firewall by itself.
* If no request reaches a share within **20 s** the UI shows: make sure the phone is on the same Wi-Fi, and allow Krovi on **Private** networks in the firewall prompt. A "Check network" button (read-only: `netsh advfirewall firewall show rule`, `Get-NetConnectionProfile`) reports the address in use, whether Windows classifies the network as Public/Private, and whether our rule exists.
* An **"Allow Krovi in the firewall"** button, only on explicit click and behind a Windows UAC prompt, runs: delete this exe's existing *inbound* rules (the stale allow/block rules Windows creates from its prompt), then add one rule `Krovi Quick Transfer`: `dir=in action=allow program=<this exe> profile=private protocol=TCP remoteip=localsubnet`. Private profile only, this program only, local subnet only. The UI states this before the click. Declining UAC changes nothing.
* A network classified **Public** is not helped by the Private-only rule; the UI says so and tells the user to switch the network to Private in Windows Settings.

### What someone else on the same Wi-Fi can and cannot do

Can:
* Port-scan and find the open port, and fetch the 404.
* **Read everything on the wire** (token, PIN, file contents) if they can sniff the network: it is plain HTTP. On a shared/open Wi-Fi this is the main risk; mitigations are the short lifetime, the stop button, and the PIN only for link leakage (not for sniffing).
* Hold connections open or send oversized headers: `tiny_http` has no per-connection read timeout, header size cap or thread cap (limitation below). This can stall or exhaust the share server, and therefore the app, but only while a share is open.
* If they learn the token (sniffing, shoulder-surfing): download the shared files of that one share, or (receive share) upload files up to the cap into `Downloads\Krovi` (renamed, never overwriting, Internet-zone marked), until it expires or is stopped.

Cannot:
* Guess a token (2^-160 per try; PIN brute force closes the share after 5 attempts).
* List or read any file that was not explicitly shared, traverse paths, or write outside `Downloads\Krovi`.
* Overwrite an existing file, or make Krovi execute anything.
* Download from a receive token, upload to a send token, or use one share's token on another.
* Reach the server from the internet: it listens on a private (RFC 1918) address only; Krovi opens no router port (no UPnP, NAT-PMP or PCP) and uses no relay.

### Internet requests: none

Source audit of `krovi-transfer`, `src-tauri/src/transfer.rs`, `src/lib/transfer.ts`, `src/views/Transfer.svelte`:
* The crate has **no HTTP client**. Its only sockets are (1) the listener above and `tiny_http`'s own connect-to-itself when the listener is dropped (to wake its accept thread), and (2) a UDP "route probe": `UdpSocket::connect(192.0.2.1:9)`, which only asks the OS routing table which local address it would use; 192.0.2.0/24 is the RFC 5737 documentation range and **connecting a UDP socket sends no packet**.
* The QR code is rendered locally in Rust to inline SVG; the phone pages contain no external URL, font, script or image (unit-tested: no `http://`/`https://`, no `<link>`, no `@import`) and the CSP forbids any other origin.
* The frontend uses no `fetch`, `XMLHttpRequest`, `WebSocket` or `EventSource`; it talks to Rust over Tauri IPC. In the browser demo there is no backend at all (simulated phone, placeholder QR marked "Demo").
* Local helper processes, started only on user action: `explorer.exe` (open the folder), and on the two firewall buttons `netsh.exe` / `powershell.exe` (local queries and the rule change above).
* New dependencies: `tiny_http` (+ `ascii`, `chunked_transfer`, `httpdate`), `qrcode`, `getrandom`, `raw-window-handle`, `rfd` (native file dialog, Windows API only), `windows-sys` (adapter list). None opens an outbound connection.

### Known limitations (stated plainly)

1. **No TLS.** Anyone who can sniff the LAN sees the link and the files. A self-signed certificate would give every phone a browser warning, so v1 keeps HTTP and keeps shares short-lived; use a network you trust.
2. **`tiny_http` hardening gaps:** no read timeouts, no header-size cap, no connection cap, and it cannot force `Connection: close`, so idle keep-alive connections from a phone can linger after a share closes (the listener itself closes at once). Worst case is a denial of service by a hostile LAN device while a share is open. Replacing the server loop with a stricter in-house one is the remedy if this threat matters.
3. Folders cannot be shared yet (select the files inside).
4. If the PC's network address changes while shares are open the next share is refused until the open ones are stopped.
5. Windows Firewall behaviour for a *real* second device was not exercised in testing (see below).

### Verification performed (2026-11-01)

* `cargo test -p krovi-transfer`: 62 unit tests (token entropy/uniformity, filename sanitising incl. traversal/device names/bidi, range parsing, router, adapter selection, QR matrix round-trip, PIN lockout, firewall command construction) + 14 end-to-end tests against a real listening server (404 uniformity, 405 rules, Range, upload caps and cleanup, expiry, stop, port closed, PIN gate and lockout, concurrency, stop-vs-expiry behaviour for in-flight downloads).
* Real network: example server bound to the detected Wi-Fi address (192.168.1.35, not loopback); loopback refused, LAN address answered; 64 MB download byte-identical (SHA-256) and wrong/expired tokens 404.
* Android 14 emulator + Chrome as the phone: page rendered, 64 MB download and a 12.58 MB + unicode-named upload through Chrome's real file picker, all SHA-256 verified on both ends; received files carried the Internet-zone stream.
* **Not verified:** a physical phone scanning the QR; the Windows Firewall prompt and inbound filtering for a genuinely separate device (traffic from the emulator originates on this PC and is loopback-exempt); the UAC firewall button (never executed during testing); the app-quit hook (`RunEvent::Exit`) and the native "Choose files" dialog inside the real Tauri window (both compile and are wired, but the Krovi GUI was not launched).

## Voice v3: zero network calls (2026-11-01)

Scope: the voice commands that touch the PC - "open [folder]", "open/launch [app]", "find [name]", "what's in
[folder]" - and the spoken-reminder path. Code: `src-tauri/crates/krovi-voice` (`catalog.rs`, `osintent.rs`,
`actions.rs`, `listing.rs`, `os.rs`), `src-tauri/src/voice.rs`, `src/lib/voice.ts`. Claim: **none of these makes any network
call, and none can be made to**. Evidence, from weakest to strongest:

### 1. Source audit
* `grep -rniE "std::net|TcpStream|TcpListener|UdpSocket|reqwest|hyper|ureq|curl|WinHttp|WinINet|InternetOpen|URLDownload|WSAStartup|getaddrinfo|https?://|tokio|async-std"` over `krovi-voice/`: no network API. The only hits are the word `hyper` in the terminal-app denylist of the clipboard fallback (the "Hyper" terminal's exe name), the strings `http://`/`https://` in `os.rs` that are a **denylist** (web links are removed from the app list) and in unit tests that assert they are refused, and one `https://example.com` test string for selected text. `src-tauri/src/voice.rs` and `src/lib/voice.ts`: no matches (no `fetch`, `XMLHttpRequest`, `WebSocket`, `EventSource`, `sendBeacon`).
* Dependency tree (`cargo tree -p krovi-voice --features native --edges normal`): `serde` (+ proc-macro crates) and `windows` 0.58 (+ its `windows-core`/`windows-targets` plumbing). No HTTP, TLS, DNS, async or socket crate.
* `windows` features enabled: `Foundation, Media_Audio, Media_Speech, Storage_FileSystem, System_Com(+_StructuredStorage), System_DataExchange, System_Memory, System_Ole, System_Registry, System_Search, System_SystemInformation, System_Threading, System_Time, System_Variant, UI_Accessibility, UI_Input_KeyboardAndMouse, UI_Shell(+_Common), UI_WindowsAndMessaging`. **None of `Win32_Networking_*` (WinHTTP, WinINet, WinSock) or `Win32_NetworkManagement_*` is enabled**, so the crate cannot name a network API even by accident.

### 2. What each step actually calls (all local)
| Step | API | Where the data comes from |
|---|---|---|
| Hear | Windows SAPI 5 in-process recognizer (`SpInprocRecognizer`) | microphone / WAV on this PC (D-012) |
| Folder list | `SHGetKnownFolderPath`; Shell namespace `shell:::{679f85cb-0220-4080-b29b-5540cc05aab6}` (Quick Access) via `IShellItem`/`IEnumShellItems` | local profile + Explorer's local pinned/frequent list |
| App list | `SHGetKnownFolderItem(FOLDERID_AppsFolder)` + `*.lnk` in the two Start Menu folders (`std::fs`) | local Start-menu data; names only (no icons, no thumbnails) |
| Open / launch | `ShellExecuteW("open", <existing directory> | "shell:AppsFolder\<id>" | <existing .lnk>)` | the target is taken only from the catalog; web links (`http(s)://`, `ftp://`), documents (`.html .url .txt .pdf ...`) and any path not in the catalog are refused before the call |
| Find | OLE DB `Search.CollatorDSO`, `SELECT ... FROM SystemIndex WHERE scope='file:' AND System.FileName LIKE ...` | the Windows Search service's local index (`scope='file:'` = local files, never a remote search server) |
| List | `std::fs::read_dir` + metadata, one directory, non-recursive | local file system |
| Reminder | `commands::add_calendar_event_from` -> local store | local SQLite |

What the OS does *after* Krovi asks is outside the claim: a launched app may of course use the network (a launched browser,
Steam, Spotify ...) exactly as if you had started it from the Start menu. Krovi never launches a URL.

### 3. Runtime capture (the example binary `osactions`, no GUI, real recognizer, real OS actions)
Run: `osactions flow` over four real WAVs (synthesized speech through SAPI) - "open downloads" (a real Explorer window opened),
"open notepad" (Notepad launched), "what's in downloads" (1026 items summarised, 50 rows), "search for invoice" (4 index hits) - plus
`osactions say "find krovi" --exec` (1 hit). The whole process lifetime was sampled:
* **Sampler:** every ~100 ms (measured average 123 ms), the full IPv4 + IPv6 TCP and UDP owner-PID tables (`GetExtendedTcpTable` /
  `GetExtendedUdpTable`, ~1 ms per call) were scanned for the process id; in parallel a background job ran `Get-NetTCPConnection
  -OwningProcess`, `Get-NetUDPEndpoint -OwningProcess` and `netstat -ano` against the same pid about once a second.
* **Result, `flow` process (5.3 s, 43 table snapshots, 8 cmdlet + 8 netstat passes):** **0 TCP connections, 0 UDP endpoints,
  0 `netstat` lines** for the pid, at every sample. `find krovi` process (2.5 s, 19 snapshots): the same, all zero.
* **Positive control (so that "0" means something):** a PowerShell process that owns a listening socket, both ends of an
  established loopback connection and a UDP endpoint was sampled with the identical code: it showed 3 TCP endpoints and 1 UDP
  endpoint in the table scan, 36 TCP / 8 UDP sightings from the cmdlets and 24 `netstat -ano` lines.
* **Loaded modules** (union over the run): of the network-related DLLs only `urlmon.dll` (pulled in by the shell's open path) and
  `ws2_32.dll` (linked by Rust's standard library into every Rust program) were present - **not** `winhttp.dll`, `wininet.dll`,
  `dnsapi.dll`. Neither opened a socket (see the zero counts above).
* **Not captured:** `pktmon` (packet capture) needs an elevated prompt (`Failed to communicate with the PktMon driver: Access is
  denied`) and was not run. The socket-table result is per-process; it does not by itself see traffic sent by other processes
  (Explorer, Notepad, the Search service) - which is the point: those are not Krovi.
* The same flow with the GUI (Tauri) was not run (the installed Krovi was deliberately left alone); the Tauri layer adds only
  `voice-*` events over local IPC, the catalog cache and the `voice_known_targets` / `voice_open_result` commands; nothing in them
  touches a socket (source audit above).

### 4. Guards that keep it that way
* Closed sets: folders and apps come from the catalog only; a spoken or typed name that is not in it is refused ("No folder or
  installed app is called ..."), never resolved against the disk or the web.
* `is_app_target` removes every Start-menu entry that is a web link or a document; `WinHost::launch_app` re-checks that the
  target is `shell:AppsFolder\...` or an existing `.lnk`; `open_folder` re-checks `is_dir`.
* The search statement interpolates only the search term, with quotes doubled, `%` and control characters removed; the scope is
  fixed to `file:`.
* `voice_open_result` (the click on a result row) accepts only paths shown in the latest result; files are only *revealed* in
  Explorer (`/select,`), never executed.
* Recognition stays on SAPI (no cloud speech API); the "Online speech recognition" Windows setting does not apply (D-012).

### Known limitations
* A launched app's own network use (above). Windows Search may index OneDrive-synced files, which is Windows' own sync, not Krovi.
* The index only knows what Windows has indexed: on the test PC freshly created files in Downloads and Documents had still not
  appeared in the index after 9 minutes (the indexer was idle), so "find" finds files that Windows Search already knows.

---

# Planned (not yet implemented)

**Nothing in this section exists in the code or the shipped app today.** The exhaustive table
above describes the current build and remains the audit of record; this section only declares,
in advance, the intended network surface of planned features so that they can be diffed against
it when they land. When a feature below actually ships, it must be **moved into the exhaustive
table** with verified call sites — being listed here is not evidence and must never be cited as
a claim of implementation. *(Paste stack: this feature was removed (D-015) and never appears in
this document's network record.)*

## 1. Opt-in Whisper model download — downloader implemented, app wiring still planned

The downloader itself (trigger rules, host allow-list, size/SHA-256 verification, deletion) now
exists in `krovi-net` and is described in the main record above: see
[Whisper model download](#whisper-model-download-opt-in-user-initiated) and row 4 of the
exhaustive table. What is **still planned and not implemented**:

| Property | Intended behaviour |
|---|---|
| Trigger | The explicit "download model" button in settings, and the Tauri command behind it. No prefetch, no background download, no automatic selection. (Nothing in `src-tauri/src` or the frontend calls the downloader yet.) |
| Runtime | The inference DLL is **built from pinned whisper.cpp revision b5454 on CI** as part of the release build — it is **not** downloaded ad hoc at runtime, so the app never fetches shared-library code from the network. |

## 2. BYOK AI (Gemini primary, Groq alternative)

> **Status 2026-11-03:** the image-capable part now exists for Anthropic, OpenAI, Google Gemini and
> Groq, plus experimental Snowflake Cortex; it is recorded in the main table (row 5) and in
> [Circle to Ask, screen awareness and AI](#circle-to-ask-screen-awareness-and-bring-your-own-key-ai).
> The text below is the original pre-declaration, kept for comparison.

* **Zero network calls without the user's own API key.** There is no bundled key, no demo key,
  no trial key, and no endpoint that works key-less. With no key configured the feature is
  inert — the code path makes no request and no "anonymous" fallback exists.
* The key is stored in the **Windows Credential Manager** via the OS credential API.
  It is never written to config files, the SQLite store, logs, or the repository, and never
  shipped inside the app.
* **Exactly what leaves the device:** the **prompt text / user-selected text** that the user
  submitted, addressed directly to the chosen provider (Gemini, or Groq as the alternative).
  **Never** microphone audio, recorded voice data, ambient screen content, clipboard history,
  or file contents — **unless** the user explicitly invokes circle-and-ask for that one
  interaction, in which case only the screen contents circled for that single ask are included
  (see §3). Every other request is text-only.
* **User-triggered only** — a request is sent when the user asks a question or invokes the
  command. No pre-emptive calls, no proactive suggestions, no keep-alive or polling of the
  provider.
* **No Krovi relay.** Requests go from the app straight to the provider's public API endpoint.
  Krovi runs **no server** in this path, sees neither requests nor responses, and cannot
  observe, log, or bill them.
* **No cost to Krovi.** It is the user's own key and the user's own account/quota with the
  provider; Krovi never proxies, meters, or pays for provider usage.

## 3. Circle-and-ask (off by default)

> **Status 2026-11-03:** implemented as Circle to Ask (see the section above). One deliberate difference
> from the text below: the shortcut is **registered by default** (owner decision) instead of being
> disabled until turned on. Registering it captures nothing; a capture needs the user to press it and
> release a selection, and an upload needs a saved key plus a confirmation naming the provider.
> The text below is the original pre-declaration, kept for comparison.

* **Explicit per-use hotkey only.** Capture begins when the user presses the circle-and-ask
  shortcut and ends when the annotated region is submitted. There is **no passive or background
  screen capture of any kind — ever**, and no continuously running capture loop that waits for
  a hotkey to harvest frames.
* **Visible indicator** (distinct on-screen affordance, e.g. a border/dimming overlay) is shown
  **for the entire time** the region is being selected/captured, from hotkey press until the
  capture ends. There is never an invisible capture.
* **What is sent:** **only the screen rectangle the user explicitly circled**, for that one
  interaction, to the user's own vision provider (same BYOK path and rules as §2 — direct to
  the provider, no Krovi relay, stored in Credential Manager only). If no key is configured,
  circle-and-ask makes **no network request**.
* **No storage by default.** The captured region is held in memory for the single request and
  then discarded; nothing is written to disk and it is not added to clipboard history. If the
  user later opts into saving annotations, that will be an explicit, separate setting.
* **Everything is off by default**: the hotkey feature is disabled until the user turns it on,
  and it additionally requires the BYOK vision key to exist before it can do anything.
