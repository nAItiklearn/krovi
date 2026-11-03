# Accounts and sync: backend research and recommendation

_Status: research, no code. Written 2026-11-03. Owner instruction: "Accounts and sync: optional and kept separate from
your own API keys; research the backend first."_

Method note: the web-search tool was unavailable during this research, so every fact below comes from primary pages
(vendor docs/pricing, GitHub API, crates.io API, RFCs) fetched on 2026-11-03. Anything not confirmed is marked
**(unverified)**. Nothing here is legal advice.

## 0. Read this first: this conflicts with two existing project statements

- `docs/SPEC.md` section 2 (Non-goals) says "No cloud sync, accounts, or 'pro' tier."
- `docs/SPEC.md` section 1 and the website trust line say "No account · No ads · No tracking", and
  `docs/SECURITY-network-audit.md` says "Krovi runs **no server**".

Any option below that makes Krovi operate an account system changes those promises. The recommendation in section 3 is
chosen partly because it does not break them. The owner must decide (open question 1) before any build.

## 1. Requirements used for scoring

R1 optional (works fully signed-out) · R2 BYOK keys never synced (stay in Windows Credential Manager) · R3 end-to-end
encrypted, backend cannot read content · R4 free for an indie OSS project + self-hostable · R5 desktop-safe auth (loopback
PKCE or device code, no secret in the binary) · R6 export/delete (GDPR), EU + India users · R7 low ops · R8 licence.

Key insight: with R3, the server can only ever hold opaque ciphertext. Row-level security on content, server-side search,
SQL queries and rich realtime become irrelevant. The backend reduces to "authenticated blob store". So the cheapest and
least-risky backend wins; feature-rich backends buy nothing here.

## 2. Backend comparison (2026-11-03)

### Supabase (hosted or self-hosted)
- **Desktop auth:** Auth code + PKCE; code is valid 5 min and single-use
  (https://supabase.com/docs/guides/auth/sessions/pkce-flow). Redirect allow-list docs only show custom schemes
  (`io.supabase.flutterquickstart://login-callback`) and `localhost:3000` examples; **loopback `127.0.0.1:<random port>`
  is not documented** (https://supabase.com/docs/guides/auth/redirect-urls, .../native-mobile-deep-linking). Needs a
  spike. Docs show no REST endpoints for PKCE; the Rust side must follow the GoTrue source **(unverified)**. Supabase can
  also act as an OAuth 2.1 server with mandatory PKCE (scopes limited to openid/email/profile/phone), not needed here
  (https://supabase.com/docs/guides/auth/oauth-server).
- **Free tier:** 500 MB DB, 1 GB files, 50k MAU, 5 GB egress, 200 realtime connections / 2M messages, 2 active projects.
  Projects **pause** after about a week of low activity; Free has **no downloadable backups**
  (https://supabase.com/pricing, https://supabase.com/docs/guides/platform/going-into-prod).
- **Pricing cliff:** Pro starts at $25/month, plus separate compute per project ($10 credit covers one Micro); overage
  egress $0.09/GB, MAU $0.00325 each; spend cap on by default. Free is not suitable as the sole production home.
- **Self-host:** official Docker Compose; single project only, no branching/PITR/managed backups/Management API;
  community-supported; you own patching, backups, HA. No telemetry from the self-hosted stack
  (https://supabase.com/docs/guides/self-hosting).
- **Licence:** repo Apache-2.0; Auth (GoTrue) MIT; Realtime Apache-2.0 (GitHub API: supabase/supabase, supabase/auth,
  supabase/realtime).
- **Rust client:** none official. `postgrest` 1.6.0 (last release 2023-07), community `supabase-auth` 0.10.14,
  `supabase_rs` 0.8.0 (crates.io). We need about four HTTPS calls; use `ureq` directly (already used in `krovi-net`).
- **Realtime / authz:** WebSocket; Postgres RLS (strong, but only useful for per-user ownership here).
- **Regions:** Frankfurt, Ireland, Paris, Stockholm, Zurich, London and **Mumbai (ap-south-1)**
  (https://supabase.com/docs/guides/platform/regions). Region choice is a location control, not proof of compliance.

### Appwrite (cloud or self-hosted)
- **Desktop auth:** OAuth docs cover mobile custom-scheme callbacks (`appwrite-callback-<PROJECT_ID>`); **no PKCE,
  loopback or desktop guidance** (https://appwrite.io/docs/products/auth/oauth2).
- **Free tier:** 2 projects, 1 DB/project, 5 functions; when bandwidth limit is hit "API access is denied" until upgrade,
  and uploads/realtime/new users are disabled at their limits (https://appwrite.io/docs/advanced/platform/free). MAU,
  storage and Pro price figures could not be retrieved from the pricing page capture **(unverified)**.
- **Pricing cliff:** on Free there is no overage: a spike (or one abusive client) **freezes the project for everyone**.
- **Self-host:** Docker/Kubernetes, multi-service stack; page itself warns self-hosting "often costs more" in time.
- **Licence:** BSD-3-Clause (GitHub API).
- **Rust client:** official `sdk-for-rust` 0.19.0 is a **server SDK only** (API-key based, would mean a secret in the
  binary); no client SDK (https://appwrite.io/docs/sdks).
- **Verdict:** weakest desktop story; reject.

### PocketBase (self-host only)
- **Desktop auth:** OAuth2 "manual code exchange" returns `authURL`, `state`, `codeVerifier` per provider and exchanges
  the code with the verifier (a PKCE-style flow). Provider client secrets live server-side, which is good for R5. Docs
  never say "PKCE" and give no desktop-loopback guidance (https://pocketbase.io/docs/authentication/).
- **Free / hosting:** software is free; the owner must run a server (single Go binary + SQLite, SSE realtime, per-collection
  API rules). No hosted free tier exists.
- **Governance risk (new, material):** the maintainer **archived the GitHub repo on 2026-10-26** and moved development to
  a self-run host, `ganigeorgiev.com/pocketbase/`; new releases will be published **only** there, Go import paths change,
  there is no official mirror, and v0.40.6 (the last GitHub release) said the new host had "nothing there yet"
  (https://github.com/pocketbase/pocketbase/discussions/7841, GitHub API releases/latest). The site is now at v0.41.1.
  The docs also state it is pre-1.0, "NOT recommended for production critical applications yet", and "full backward
  compatibility is not guaranteed before reaching v1.0.0" (https://pocketbase.io/docs/). Single maintainer.
- **Licence:** MIT (GitHub API). **Rust client:** none; plain HTTPS.
- **Verdict:** technically the best fit for a blob store, but a poor sole dependency for a "verifiable privacy" project
  right now. Reasonable as a documented *user self-host* option later.

### Firebase
- **Auth:** Identity Platform, 50k MAU on Spark. **Free:** Firestore 1 GiB, 20k writes/day, 50k reads/day; Cloud Storage and
  Functions are "Not applicable" on Spark, i.e. need Blaze billing (https://firebase.google.com/pricing).
- Proprietary, no self-host, Google processes the data, no official Rust client **(unverified)**. Conflicts with R4/R8 and
  with Krovi's positioning. **Reject.**

### Convex
- **Free (Starter):** 0.5 GB DB, 1M function calls, 1 GB egress, 1 GB files; Pro $25 per developer/month
  (https://www.convex.dev/pricing).
- **Licence:** FSL-1.1-Apache-2.0 (competing-use restriction; each release converts to Apache-2.0 after two years), so not
  OSI open source today (GitHub licence file, get-convex/convex-backend). Self-hosting supported (Docker; Windows "less
  tested"; has an opt-out telemetry beacon). Official Rust crate `convex` 0.10.4, Apache-2.0 (crates.io).
- Realtime is its core feature, which we do not need. It has no built-in end-user auth, so a separate OIDC provider would
  be required **(unverified)**. **Reject** on licence, extra moving part and unneeded realtime.

### "No backend": sync via the user's own cloud-synced folder (E2E-encrypted files)
- Two variants. **(a) Plain folder:** Krovi reads/writes ordinary files in a folder the user picks; the OneDrive / Google
  Drive for desktop / Dropbox / Syncthing client already on the PC replicates it. Krovi opens **no socket, holds no token,
  registers no OAuth app, compiles in no endpoint.** **(b) Provider API:** Krovi talks to the provider itself:
  OneDrive App Folder (`Files.ReadWrite.AppFolder`, `GET /drive/special/approot`;
  https://learn.microsoft.com/en-us/onedrive/developer/rest-api/concepts/special-folders-appfolder) with device-code
  flow (https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code), or Google Drive `drive.appdata`
  (non-sensitive scope; loopback 127.0.0.1 supported; client secret optional, desktop apps "cannot keep secrets
  confidential"; https://developers.google.com/drive/api/guides/api-specific-auth,
  https://developers.google.com/identity/protocols/oauth2/native-app). (b) forces the owner to register and maintain
  Microsoft/Google OAuth apps (and Google verification) and adds a network surface; **(a) avoids all of it.**
- **Limits:** sync latency is whatever the cloud client does (seconds to minutes); no realtime; no web/mobile access;
  no email-based recovery; depends on third-party client behaviour (placeholder files, locked files, "conflicted copy"
  duplicates). No account exists, so there is nothing to sign in to or delete on a Krovi server.

## 3. Recommendation: "Krovi Vault" = E2E-encrypted sync over a user-chosen folder, no Krovi server

**Ship folder sync first and treat "account" as a local vault key, not a server login.** Put every transport behind one
trait (`SyncTransport`: list / get / put / delete opaque blobs) so a server transport can be added later without touching
crypto or merge logic. If the owner insists on real accounts, the later server transport should be **Supabase** (section 6).

Why: it is the only option that keeps all R1-R8 and the existing "no server / no account / verifiable privacy" claims true,
costs $0 forever, has no GDPR processor role for Krovi, no abuse/ops burden, and adds **zero new network endpoints** to
`docs/SECURITY-network-audit.md` (sync only touches the local filesystem). What it gives up: sign-in UX, realtime, web
access, and email recovery (see Limits above and open question 2).

### 3.1 Keys (no secret in the binary, nothing server-side)
- **Vault Key (VK):** 32 random bytes from the OS CSPRNG at first enable. Never derived from the passphrase.
- **Passphrase wrap:** passphrase -> Argon2id (RFC 9106 second recommended option: t=3, p=4, m=64 MiB, 128-bit salt,
  256-bit tag; https://www.rfc-editor.org/rfc/rfc9106.html; tune upward if it runs under ~0.5 s) -> KEK; KEK wraps VK with
  XChaCha20-Poly1305. Changing the passphrase re-wraps VK only; no data re-encryption.
- **Recovery key:** a second random 256-bit secret, shown once as grouped base32/word list, wraps VK independently. Lose
  both passphrase and recovery key and the data is unrecoverable. That is the honest cost of R3; say so in the UI.
- **Record encryption:** HKDF-SHA256 subkey from VK; XChaCha20-Poly1305 with a fresh random 24-byte nonce per record
  (no counter to coordinate across devices); AAD = vault_id | record_id | schema_version so blobs cannot be swapped or
  replayed across records. Crates (RustCrypto, crates.io 2026): `chacha20poly1305` 0.11.0, `argon2` 0.6.0, plus
  `hkdf`, `zeroize`, `rand`.
- **Device cache:** to avoid re-typing, cache the VK in Windows Credential Manager under a **distinct target prefix**
  (e.g. `Krovi/sync/vault-key`), never under the BYOK prefix. Krovi already links the Win32 credential API
  (`krovi-explain` features `Win32_Security_Credentials`).

### 3.2 Folder layout and sync model (conflict-free by file ownership)
```
<chosen folder>/Krovi-Sync/
  vault.json                      public header: format version, vault_id, KDF params+salt, wrapped VK (pass), wrapped VK (recovery)
  devices/<device_id>/log-000001.kvl   append-only encrypted segments, written ONLY by that device
  devices/<device_id>/ack.kvs     encrypted per-device high-water marks (what it has applied from each peer)
```
- Each device writes only its own files, so the cloud client never has two writers on one file; "conflicted copy" files are
  ignored (names not matching the pattern) and AEAD failures are skipped and logged.
- Log entry = `{record_id (ULID), kind, hlc, device_id, base_hlc, deleted, payload}`, encrypted. The local SQLite store stays
  the source of truth; **never place the live SQLite DB in the synced folder** (sync clients corrupt open DB files). Sync is
  a change-outbox export plus an import that applies remote entries.
- **Merge:** per-record last-writer-wins on a hybrid logical clock (`hlc`, tie-break by `device_id`). Deletions are
  **tombstones** (`deleted=true`), kept until every known device's `ack` has passed them (and at least 90 days), then
  compacted into a snapshot. Exception for **notes**: if two devices edited from the same `base_hlc` (concurrent), keep the
  winner and save the loser as a "conflicted copy" note, so typed text is never silently dropped. Tasks, reminders,
  snippets and settings are plain LWW.
- **Pull triggers:** app start, window focus, and a 5-minute timer while enabled (optionally a filesystem watcher). No polling
  of any remote host exists in this transport.

### 3.3 What syncs, what never syncs
- **Syncs (each category an explicit opt-in toggle, default off):** notes, to-dos, reminders/timer presets, snippets, a
  closed allow-list of UI settings (theme, layout, companion personality choices).
- **Never syncs (enforced by an allow-list schema, not a deny-list):** BYOK API keys (live only in Credential Manager,
  prefix `Krovi/byok/*`; the sync crate has no API that can read that prefix), the VK, clipboard history and file-shelf
  contents (sensitive by default), screen-time/foreground-window data, voice audio and Whisper models, local file paths,
  machine-specific hotkeys, Agent Mode action history. Add a CI test that fails if any sync schema field is named like
  `*key*`, `*token*`, `*secret*`, or if the sync crate depends on the credential-store code path.
- Account/sync state and BYOK are separate in code (`krovi-sync` has no dependency on the BYOK module), in UI (separate
  Settings sections) and in the OS store (separate credential targets).

### 3.4 Disabled until configured
- Default build: `krovi-sync` is compiled but inert; Settings shows "Sync: off". No folder chosen = no file touched, no
  timer, no thread. Enabling requires the user to pick a folder and create/enter a passphrase.
- For the later server transport: a separate crate/feature `sync-server`, **off by default**, with **no URL or key in the
  repo**. It activates only if the owner sets `KROVI_SYNC_URL` (+ public client key) at build time via `option_env!`, or the
  user types a self-host URL in Settings; otherwise it returns `NotConfigured` and opens no socket. HTTPS only; the URL is
  shown in Settings; a new row is added to `docs/SECURITY-network-audit.md` before the code lands.

### 3.5 Export and delete (R6)
- **Export:** "Export my data" writes decrypted JSON/Markdown locally (works with or without sync). For the folder transport
  the user already holds every byte.
- **Delete:** "Delete vault" removes `Krovi-Sync/` and the cached VK; deleting all wrapped VK copies is effective
  crypto-shredding of any copies the cloud provider keeps in versions or recycle bins (they remain ciphertext without VK).
- Krovi stores no personal data off-device in this transport, so there is no controller/processor duty for Krovi, no EU/India
  residency question, and no Krovi-side deletion request to honour. The user's cloud provider's own terms apply.

## 4. Phased plan (one developer; effort is a rough +/-50%)
| Phase | Scope | Effort |
|---|---|---|
| P0 | Owner decisions; amend SPEC non-goal + trust-line wording; add DECISIONS entry; pre-declare in audit doc "Planned" | 0.5 d |
| P1 | `krovi-sync` pure crate: vault header, Argon2id/HKDF/XChaCha, record model, HLC, LWW + tombstones + note-conflict rule, fixed test vectors, property tests (convergence under reorder/duplication/drop) | 4-5 d |
| P2 | Folder transport + SQLite outbox/import; Settings UI (enable, folder picker, passphrase, recovery-key confirm, category toggles, device list, export, delete vault); Credential Manager VK cache; website feature update; audit-doc update | 4-5 d |
| P3 | Hardening: VK rotation, compaction/tombstone GC, size limits, parser fuzzing, `security-reviewer` pass, second-machine manual test on OneDrive and Google Drive for desktop | 2-3 d |
| P4 | **Only if the owner approves a server** (section 6): Supabase transport, PKCE loopback, self-host SQL + docs, privacy policy, deletion RPC, runbook | 8-12 d + ops |

P1-P3 total: roughly 10-13 working days. Ongoing cost: $0.

## 5. Risks of the recommendation (honest list)
- No password-reset or "forgot passphrase" path by design; support burden shifts to documentation and a clear recovery-key flow.
- Cloud-folder clients are not databases: latency, placeholder (Files On-Demand) files and occasional duplicate files.
  Mitigated by per-device single-writer files and idempotent import; must be tested on real OneDrive and Google Drive.
- Crypto code is easy to get wrong. Use only audited RustCrypto primitives, no custom constructions, fixed test vectors, and
  an independent review before release. Pre-1.0 `argon2` 0.6 / `chacha20poly1305` 0.11 are current as of 2026-08.
- Users without a cloud-sync client get no sync (the folder can still be a USB drive, NAS or Syncthing share).

## 6. If the owner insists on server accounts (P4 design, not recommended for v1)
Use **Supabase** (hosted, EU Frankfurt or Mumbai, or self-host) as an authenticated opaque-blob store; PocketBase only as a
documented user self-host option after its post-GitHub release channel proves stable.
- **Auth: PKCE loopback (RFC 8252 section 7.3, RFC 7636).** (1) Generate `code_verifier` (CSPRNG, 43-128 chars),
  `code_challenge = BASE64URL(SHA256(verifier))`, `state`. (2) Bind `127.0.0.1` on an ephemeral port (the repo already uses
  `tiny_http`). (3) Open the system browser at the Supabase authorize URL for GitHub/Google/email with
  `redirect_to=http://127.0.0.1:<port>/cb`, S256 challenge, `state`. (4) Accept one request, verify `state`, show "you can
  close this tab", close the listener (5-min timeout). (5) Exchange `code` + `code_verifier` at the token endpoint within 5
  minutes (single-use). (6) Keep the refresh token in Credential Manager (`Krovi/sync/session`, separate from VK and BYOK).
  RFC 8252 says servers MUST allow any loopback port, but Supabase's docs do not say its allow-list does (spike first;
  fallback is a registered `krovi://` deep link via Tauri's deep-link plugin). Only the project URL and the **public/anon** key are ever in
  the client; **never** the service-role key (RFC 8252 section 8.5: embedded secrets are not confidential).
- **Schema (server sees only ciphertext + sizes + timestamps):** `vault_header(user_id pk, header jsonb)` and
  `vault_log(user_id, device_id, seq, blob bytea, created_at)`, append-only, RLS `user_id = auth.uid()`, per-row size CHECK
  (e.g. 64 KB) and a per-user quota trigger to protect the 500 MB free DB. Same envelope/log format as section 3.2; only the
  transport differs. Account deletion needs an RPC or Edge Function that deletes the auth user and rows **(design detail,
  unverified)**.
- **Cost/ops reality:** the free project pauses after a low-activity week and has no downloadable backups, so any real
  public service means Pro ($25/month for one project, the $10 compute credit covers one Micro) or accepting outages; the owner becomes the data
  processor for email addresses (privacy policy, DPA with the provider, deletion handling; India's DPDP Act and GDPR duties
  not researched here) and must handle abuse and spam signups. Appwrite/Firebase/Convex are ruled out in section 2.

## 7. Open questions for the owner
1. SPEC section 2 says "No cloud sync, accounts" and the trust line says "No account". May we amend to "no account required;
   optional E2E sync you control", or is a real Krovi account (profile, sharing, web access) actually wanted?
2. Is "enter a passphrase / recovery key on the second PC" an acceptable replacement for "sign in with Google/GitHub"?
3. If a server is wanted: who pays (budget cap: $0 vs $25+/month), who operates it, and are you willing to be the data
   processor for user emails (privacy policy, deletion SLA, abuse handling)?
4. Lost passphrase and lost recovery key means unrecoverable data. Acceptable (recommended), or do you want key escrow
   (which weakens R3)?
5. Should clipboard history or file-shelf contents ever be syncable (opt-in, off by default), or banned outright?
6. Notes conflict rule: is "keep both, mark the loser as a conflicted copy" the right behaviour?
7. Any plan for mobile or web companions? That is the one thing that would force a server transport.
8. Is it acceptable that sync quality depends on the user's cloud-folder client, with Krovi documenting tested ones
   (OneDrive, Google Drive for desktop) rather than guaranteeing them?
9. Should we add a DECISIONS.md entry and update the website feature list now, so the "no account" claim and the new
   optional sync stay honest (`docs/SPEC.md`, `docs/SECURITY-network-audit.md`, `web/`)?

## Other sources (inline URLs above are the primary ones; all fetched 2026-11-03)
Appwrite pricing https://appwrite.io/pricing and self-host https://appwrite.io/docs/advanced/self-hosting · PocketBase
https://pocketbase.io/docs/api-realtime/ and `https://api.github.com/repos/pocketbase/pocketbase` (`archived: true`) ·
https://github.com/get-convex/convex-backend and https://crates.io/api/v1/crates/convex · licences via GitHub API
(supabase/supabase, supabase/auth, supabase/realtime, appwrite/appwrite, appwrite/sdk-for-rust) · RFCs
https://www.rfc-editor.org/rfc/rfc8252.html, https://www.rfc-editor.org/rfc/rfc9106.html · crates.io: chacha20poly1305 0.11.0,
argon2 0.6.0, supabase-auth 0.10.14, supabase_rs 0.8.0, postgrest 1.6.0.
