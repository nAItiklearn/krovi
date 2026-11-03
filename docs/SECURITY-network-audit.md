# Krovi — Network / Privacy Audit

_Last updated: 2026-11-02 (added "Planned (not yet implemented)" pre-declaration section, below;
the current-build record is unchanged). This document is the evidence behind Krovi's "verifiable
privacy" claim: a literal account of every place the app can open a network socket, what
triggers it, what is sent, and how to turn it off. It is maintained in-repo so the claim is
checkable against the source — and should be re-run against each released build._

## TL;DR

The Krovi **desktop app** makes **no telemetry, analytics, or account calls**. Only **two**
live network endpoints exist in the code, both **keyless** public APIs, plus a **manual-only**
update check. The **website** (`web/`) makes **zero** network requests (no trackers, no CDN,
no fonts-from-CDN) — it only contains `<a href>` links to the public GitHub repo.

_Planned features (Whisper model download, BYOK AI, circle-and-ask) are **not implemented and
not shipped**; they appear nowhere in the exhaustive table below. They are pre-described in the
separate [Planned (not yet implemented)](#planned-not-yet-implemented) section at the bottom of
this document, so their future network surface is declared before any code lands._

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

There are **no other outbound sockets**. The one **inbound** listener is Quick Transfer — LAN-only,
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

## 1. Opt-in Whisper model download

The speech-to-text runtime (Whisper via whisper.cpp) runs **entirely on-device**; inference makes
no network call and **no audio ever leaves the machine**. Because models are large, the one
network-adjacent step will be fetching a model file, and it will be strictly opt-in:

| Property | Intended behaviour |
|---|---|
| Trigger | **User action only** (explicit "download model" button in settings). No prefetch, no background download, no automatic selection. |
| Endpoint | **HTTPS from one pinned host** — the model is fetched from a fixed, allow-listed repository host, not from arbitrary URLs. No other endpoint is contacted for this purpose. |
| Integrity | The model file's **SHA-256 digest is pinned at build time**; the downloaded file is hash-checked before it is accepted. A mismatch deletes the partial file and refuses to use it (no silent fallback to the server's copy of the hash). |
| Storage | Written to a local model directory; **user-deletable** at any time from settings. Deleting the model leaves no residual network activity and the feature degrades to "no model installed" (STT unavailable, no attempt to re-download on its own). |
| Runtime | The inference DLL is **built from pinned whisper.cpp revision b5454 on CI** as part of the release build — it is **not** downloaded ad hoc at runtime, so the app never fetches shared-library code from the network. |

## 2. BYOK AI (Gemini primary, Groq alternative)

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
