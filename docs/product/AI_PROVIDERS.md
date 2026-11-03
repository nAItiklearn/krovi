# Krovi BYOK AI providers: terms, eligibility, data, endpoints, errors

Researched 2026-11-03. Scope: optional AI features (text summarise/Q&A; image questions about a user-selected screen region). The user's key lives in Windows Credential Manager; calls go straight from the user's PC to the provider; there is no Krovi server or proxy.

Method: official pages read with WebFetch (an AI summariser sits between us and the page, so re-read the cited URL before quoting it publicly). WebSearch was unavailable. Items I could not confirm from an official page are marked **(unverified)**. Model ids and prices rotate; they are as shown on the pages on this date. **No provider is "supported" until its integration works end to end and the owner signs off on the terms items below. Website copy must not claim support before that.**

## 1. Summary

| Provider | BYOK in a 3rd-party app | Audience / age | Trains on API inputs by default | Regions | v1 verdict |
|---|---|---|---|---|---|
| Anthropic | No clause against it; help centre says each person should use their own key | Business terms, "not for consumer use"; age (unverified) | No (commercial). Auto-delete in 30 days | ~180 countries incl. EEA/UK/India | **Ship v1** (lowest risk) |
| OpenAI | Terms pages returned HTTP 403, so (unverified) | Age (unverified) | No since 2023-03-01 unless opted in. Abuse logs up to 30 days | ~200 countries incl. EEA/UK/India | **Ship v1 after the owner reads OpenAI's terms** |
| Gemini (AI Studio key) | No clause against it; keys are per Google Cloud project | **18+**, "not for consumer use"; no apps likely used by under-18s | **Free tier: YES, plus human review. Paid: no** | ~240 listed incl. EEA/UK/CH/India; EEA/UK/CH: the terms say "paid services only" when "making API Clients available to users" there, but also apply paid data terms to free quota there, and Google's billing FAQ says the free tier is available there *[corrected 2026-11-03, see 9.1.3: the earlier blanket "paid services only" was too strong]* | **Ship v1 gated** (acknowledgements), or defer |
| OpenRouter | OK in principle; terms apply; keys non-transferable | 18+ | Off unless opted in; downstream providers vary (unverified) | Provider-specific blocks; no VPN workarounds | Later |
| Groq | Services Agreement not read (unverified) | (unverified) | Inference not retained by default; ZDR opt-in | US data storage | Later |
| Mistral | Terms not read (unverified) | (unverified) | **Free mode: YES (opt-out possible)** | (unverified) | Later |

## 2. Cross-provider rules for Krovi

1. **"Test connection" must be two steps.** (a) List models proves only that the key exists. (b) A 1-token inference probe reveals billing, quota, free-tier eligibility and region problems. Gemini's own docs say `models.list` "can return 200 OK even if inference calls are blocked". Show the results as separate statuses: Authentication, Billing, Quota/rate, Eligibility (tier/region/policy), Model, Safety.
2. Send the key only in a header, never in a URL (Gemini's reference still shows `?key=`; do not use it). Make the HTTP calls from the Rust side, never from the webview. Never log request or response bodies.
3. Do not add a Krovi proxy or accept consumer-subscription tokens (e.g. claude.ai sessions). Anthropic's AUP bans services that "resell, proxy, or otherwise provide access to Claude through unauthorized means", including "route requests through consumer subscriptions".
4. Image questions send screen content off-device. Show a preview of the exact region and ask for confirmation before each send; nothing is captured passively.
5. Do not hard-code model ids as the only option. Read the list endpoint, keep one default constant per provider (updated per release), and map 404 to "Model not found, pick another".
6. Retry only transient errors (5xx, 408, rate-limit 429). Never auto-retry billing, spend-cap or daily-quota errors.
7. Show an "AI-generated, may be wrong" label on every answer. Anthropic's AUP requires consumer-facing chat products to disclose AI; its Commercial Terms say users must be told outputs "should not be relied upon".

## 3. Google Gemini API (AI Studio keys)

Sources: https://aistudio.google.com/docs/terms (Additional Terms effective 2026-03-23), /docs/available-regions, /docs/billing, /docs/rate-limits, /docs/troubleshooting, /docs/api-errors, /docs/api-key, /docs/pricing, /docs/models, /docs/image-understanding, /api-reference/models/list-models, /api-reference/generate-content. (The old ai.google.dev URLs 301-redirect here.)

**Terms**
- "You must be 18 years of age or older to use the APIs." Also no use in apps "directed towards or is likely to be accessed by individuals under the age of 18". Services are for professional/business use: "not for consumer use". Krovi is a general consumer app, so require an "I am 18+" confirmation before enabling Gemini; owner to decide whether that is enough.
- "You may use only Paid Services when making API Clients available to users in the European Economic Area, Switzerland, or the United Kingdom." The same page also says users located there get paid-tier data terms on all Services. These two statements are in tension; whether Krovi counts as "making API Clients available" under a BYOK model is a legal question (unverified). Conservative handling: in those regions require the user to confirm their project has billing enabled.
- No rule on key sharing in the Additional Terms. The key docs say "Treat your Gemini API key like a password" and "Never expose keys client-side in production" (aimed at web/mobile apps shipping a developer's key). Krovi ships no key; each user supplies their own.
- Prohibited: building competing models, bypassing safety filters, clinical medical advice, auto-bypassing human-confirmation requests. Keep Search/Maps grounding off (it carries display and 30-day retention rules).
- No explicit "this is AI" display requirement for plain generation.

**Eligibility**
- Available in ~240 countries/territories, including the EEA, UK, Switzerland and India. Not listed (so unsupported, inferred from absence): e.g. China, Russia, Belarus, Iran, Cuba, North Korea, Syria.
- The billing page says the free and paid tiers are available in "many regions" and answers "Yes" for EEA/UK/CH, but the per-country free-tier list was not on any page I read (unverified).
- **A valid key does not mean free-tier access.** Eligibility is checked per Google Cloud project: "a new key in the same project won't fix it". Statuses in AI Studio: Restricted, Free tier unavailable, Payment needed, Unavailable. Linking billing moves the project to the paid tier and "doesn't unlock free-tier quota".
- Rate limits and tiers apply per project, not per key. Daily quota resets at midnight Pacific. Tier 1 needs a linked billing account.
- New paid users default to Prepay ($5-$5,000). At $0 balance every key under that billing account stops (HTTP 402). The $300 welcome credit cannot be used for the Gemini API on accounts opened after 2026-03-02.

**Data**
- Unpaid (AI Studio, free API quota): Google uses content and outputs to "provide, improve, and develop Google products and services and machine learning technologies"; "human reviewers may read, annotate, and process your API input and output" (de-linked from account/key/project). "Do not submit sensitive, confidential, or personal information to the Unpaid Services."
- Paid (billed Cloud project): prompts/responses are not used to improve products; logged for a limited time for abuse detection and legal disclosures. The pricing page row "Used to improve our products" shows Yes (free) / No (paid).
- Krovi cannot tell which tier a key is on. Treat every Gemini key as possibly free-tier until the user attests otherwise.

**Endpoints** (base `https://generativelanguage.googleapis.com/v1beta`, header `x-goog-api-key: <key>`)
- List / test: `GET /models?pageSize=1`, response `{ "models": [...], "nextPageToken" }`. Proves only that the key exists.
- Text: `POST /models/gemini-3.5-flash-lite:generateContent` with `{"contents":[{"role":"user","parts":[{"text":"..."}]}],"generationConfig":{"maxOutputTokens":512}}`. Text is at `candidates[].content.parts[].text`. Do not send `temperature`/`topP`/`topK`: ignored on 3.6+ and flagged to become 400s later.
- Image: add a part `{"inlineData":{"mimeType":"image/png","data":"<base64>"}}` (the `inlineData` part type is in the reference; the `mimeType`/`data` sub-field names are (unverified) here, because the image page now shows the Interactions API). Types png, jpeg, webp, heic, heif. Whole inline request capped at 20 MB.
- Note: Google now says generateContent "remains fully supported" but recommends the Interactions API (`POST /interactions`, `{"model","input"}`, output in `steps[]` where `type=="model_output"`) "for all new development". No sunset date is given. One page shows an `Api-Revision: 2026-05-20` header and another omits it (unverified). Put the call behind a provider adapter and start with generateContent.
- Cheap models: `gemini-3.5-flash-lite` ($0.30/$2.50 per 1M tokens, free tier yes), `gemini-3.1-flash-lite` ($0.25/$1.50, free), `gemini-3.8-flash` ($0.75/$3.75 through 2026-12-31). `gemini-2.5-flash` is deprecated and reroutes to `3.5-flash-lite` from 2026-12-01; `2.0-flash`/`2.0-flash-lite` are shut down.

**Errors** (body shape `{"error":{"code","message","status"}}` for generateContent)

| Category | Signal |
|---|---|
| Authentication | 400 INVALID_ARGUMENT "API key not valid. Please pass a valid API key." (historical, unverified). Interactions API: 401 `authentication`. Leaked key: 403 PERMISSION_DENIED "Your API key was reported as leaked. Please use another API key." (verified). Fix: new key. |
| Billing | 402 when Prepay credit is depleted (verified, do not retry). 403 PERMISSION_DENIED with status "Payment needed" for an outstanding balance (verified); fix in AI Studio Billing, access returns on its own. |
| Quota / rate | 429 RESOURCE_EXHAUSTED (verified). Interactions API splits it: `rate_limit_exceeded`, `too_many_requests` (back off), `quota_exceeded` (daily, wait for the reset). Free tier has the tightest limits. 503 UNAVAILABLE: retry. |
| **Free tier unavailable / project not eligible / region / policy** | 403 PERMISSION_DENIED "Your project has been denied access" with project status Restricted / Free tier unavailable / Payment needed / Unavailable (verified). Policy suspension also 403. Legacy: 400 FAILED_PRECONDITION "Gemini API free tier is not available in your country. Please enable billing on your project in Google AI Studio" (unverified, old table). "User location is not supported for the API use" FAILED_PRECONDITION is widely reported but is not on the current official pages (unverified). Interactions API: 400 `failed_precondition` (example: disabled billing). Message to user: "This key is valid, but its Google project cannot use this tier or region." Do not suggest creating more keys; suggest enabling billing or using another provider. |
| Model not found | 404 NOT_FOUND (message wording unverified); Interactions API `model_not_found` (verified). |
| Safety block | Usually HTTP 200, not an error: no candidates and `promptFeedback.blockReason` in SAFETY, OTHER, BLOCKLIST, PROHIBITED_CONTENT, IMAGE_SAFETY; or `candidates[].finishReason` of SAFETY, RECITATION, SPII, PROHIBITED_CONTENT, BLOCKLIST, LANGUAGE. Interactions API reports these as error codes (`safety`, `recitation`, ...). Show "The provider blocked this content", not "error". |

**Recommendation: v1 gated, or defer to v1.1 if the owner wants legal certainty.** Highest data risk of the three majors, because most hobbyist keys are free tier and image questions send screen content. Required UI (shown when the user adds a Gemini key, with confirmation):
- "Free-tier Gemini keys: Google may use your prompts, images and answers to improve its products, and human reviewers may read them. Do not send sensitive, confidential or personal content. Keys on a billing-enabled (paid) project are not used for training."
- "Gemini API terms require you to be 18 or older. In the EEA, UK and Switzerland only paid-tier use is permitted: confirm your project has billing enabled." (checkbox) *[Overstated 2026-11-03: the terms do not plainly say this for a person using their own key. Replacement copy is in 9.4.]*
- "A valid key does not guarantee access. Free-tier eligibility is decided per Google Cloud project and region."
- Per-send image preview and confirm; default off.

## 4. OpenAI API

Sources read: https://developers.openai.com/api/docs/guides/error-codes, /guides/your-data, /guides/images-vision, /guides/text, /guides/safety-best-practices, /models, /supported-countries, /api/reference/resources/models/methods/list. **Not readable:** openai.com/policies (terms-of-use, row-terms-of-use, service-terms, business-terms, usage-policies) all returned HTTP 403, and web.archive.org is blocked. So every terms-level claim below is **(unverified)** and the owner must read them before shipping.

**Terms (unverified):** whether end users may enter their own key into a third-party desktop app, minimum age, key-sharing wording, any mandatory disclosure. Verified: the safety-best-practices page has "no explicit requirement to tell users they are interacting with AI"; `safety_identifier` (hashed user id) is "recommended, not required". Never send a raw username or email.

**Eligibility:** supported-countries page lists ~200 places including the EEA, UK and India. It warns that "accessing or offering access to our services outside of the countries and territories listed" may get an account blocked or suspended. Absent from the list (inferred): China, Hong Kong, Russia, Belarus, Iran, North Korea, Cuba, Syria, Venezuela. Age and consumer-vs-business: (unverified).

**Data:** "data sent to the OpenAI API is not used to train or improve OpenAI models" since 2023-03-01 unless the customer opts in. Abuse-monitoring logs of prompts/responses are kept "up to 30 days" (longer if law requires). Zero Data Retention needs OpenAI approval, so it is not realistic for BYOK users. The Responses API keeps "Application State" for 30 days by default or when `store` is true, so send `"store": false`.

**Endpoints** (base `https://api.openai.com/v1`, header `Authorization: Bearer <key>`)
- List / test: `GET /models` returns `{ "object":"list","data":[{id,created,owned_by,shutdown_date}] }`. Proves the key only; billing and quota errors appear only on inference.
- Text: `POST /responses` with `{"model":"gpt-6-luna","input":"...","store":false,"max_output_tokens":512}`. Text is the `output_text` item inside the `message` item of the `output` array. Do not assume `output[0].content[0].text` (docs warn against it). Chat Completions is not announced as deprecated, but Responses is recommended.
- Image: `"input":[{"role":"user","content":[{"type":"input_text","text":"..."},{"type":"input_image","image_url":"data:image/png;base64,<b64>","detail":"auto"}]}]`. The docs example uses a public URL; the base64 data-URL form is documented only in SDK examples. PNG, JPEG, WEBP, non-animated GIF; up to 512 MB per request.
- Models (from the models page): `gpt-6-luna` cheapest ($0.10/$0.50 per 1M tokens, image input yes), `gpt-6.1-terra` ($1/$5), `gpt-6.1-sol` ($2/$10). Other pages use `gpt-6-astra` in examples. Always populate the picker from `GET /models`.

**Errors**

| Category | Signal |
|---|---|
| Authentication | 401: "Incorrect API key provided" (typo, deleted, other org), "Must be a member of an organization", "IP not authorized" (project allowlist). Code `invalid_api_key` (unverified). |
| Billing / no credit | 429 where `error.code` is `credit_balance_exhausted`, `organization_spend_limit_exceeded`, `project_spend_limit_exceeded` or `organization_usage_limit_exceeded`; `error.type` may still read `insufficient_quota` (the older code). "Retrying billing, spend, or quota errors won't restore API access." |
| Quota / rate limit | 429 "Rate limit reached for requests" or `rate_limit_error` / `slow_down`. Follow `Retry-After`; retry with backoff. Distinguish from the row above by `error.code`, not by status. |
| Eligibility / region | 403 "unsupported country, region, or territory" (code `unsupported_country_region_territory` is unverified). |
| Model not found | 404 `model_not_found`, or 403 when the project lacks access to the model (both unverified; not on the fetched error page). |
| Safety | Not on the fetched pages (unverified): refusal content parts, `content_filter` finish reason, or a 400 for a flagged prompt. Map any of these to "The provider declined this content". |
| Transient | 500; 503 `service_unavailable_error` / `server_is_overloaded` (honour `Retry-After`). |

**Recommendation: ship v1 once the owner has read OpenAI's Terms of Use, Service Terms and Usage Policies** (the only provider where the terms check is blocked on me). Disclosures: "Your text and images are sent to OpenAI. OpenAI does not train on API data by default but may keep it up to 30 days for abuse monitoring." Plus the common AI-output label.

## 5. Anthropic (Claude API)

Sources: https://platform.claude.com/docs/en/api/errors, /api/rate-limits, /api/models/list, /models/overview, /build-with-claude/vision; https://www.anthropic.com/legal/commercial-terms (effective 2025-06-17), /legal/aup, /supported-countries; https://support.claude.com/en/articles/9767949-api-key-best-practices-keeping-your-keys-safe-and-secure; https://privacy.claude.com (training and retention articles).

**Terms**
- Customers may use the Services "to power products and services Customer makes available to its own customers and end users". Nothing forbids a user typing their own key into a client app, and the help centre says "If someone needs access to the Claude API, they should obtain their own key". It also warns that giving a tool your key "means you are giving the developer of that tool access to your Claude Console account", so disclose that Krovi keeps the key only in Credential Manager and sends it only to api.anthropic.com.
- "Services under these Terms are not for consumer use" (business terms). The user is the customer, so this affects the user's account, not Krovi, but note it.
- "Customer is responsible for all activity under its account." Customers must tell their users that factual assertions in Outputs should not be relied upon.
- Resale/proxying is barred (see section 2.3). AUP: consumer-facing chatbots "must disclose to users that they are interacting with AI". The disclosure need not name Claude.

**Eligibility:** supported-countries page lists ~180 places including the EEA, UK and India; people physically in, or entities headquartered or majority-owned in, unlisted places are barred. Absent (inferred): China, Russia, Belarus, Iran, Cuba, North Korea, Syria. Minimum age is not stated on the pages read ((unverified); AUP defers to the Terms of Service). Treat as 18+ in the UI.

**Data:** "Anthropic may not train models on Customer Content from Services" (inputs and outputs). Exceptions: explicit feedback (thumbs up/down) or user opt-in. Retention: inputs/outputs deleted from the backend "within 30 days" by default (exceptions: Files API, agreements, usage-policy enforcement, law). Content flagged by safety classifiers can be kept up to 2 years (scores 7 years).

**Endpoints** (base `https://api.anthropic.com`; headers `x-api-key: <key>`, `anthropic-version: 2023-06-01`, `content-type: application/json`)
- List / test: `GET /v1/models?limit=1`, response `{ "data":[{id,display_name,lifecycle,...}],"has_more" }`. The docs' list example uses `Authorization: Bearer` with a newer `anthropic-version` date, while the Messages examples use `x-api-key`; check which your key accepts during integration. Proves the key only.
- Text: `POST /v1/messages` with `{"model":"claude-haiku-5-5","max_tokens":512,"messages":[{"role":"user","content":"..."}]}`. Text is in `content[]` blocks with `type=="text"`. Haiku 5.5 accepts `"thinking":{"type":"disabled"}`; set it for short summaries to cut cost and latency. Sonnet 5.5 rejects "disabled" (use `{"type":"between_tools"}`).
- Image: `{"type":"image","source":{"type":"base64","media_type":"image/png","data":"<b64>"}}` as a content block, before the text block (the docs recommend image first). JPEG, PNG, GIF, WebP; 10 MB per image (base64) on the Claude API; max 8000x8000 px; 128 MB per request.
- Models: `claude-haiku-5-5` (from $0.10/$0.50 per MTok), `claude-sonnet-5-5` ($2/$10), `claude-opus-5-5` ($4/$20). All current models take image input.

**Errors** (body `{"type":"error","error":{"type","message"},"request_id"}`)

| Category | Signal |
|---|---|
| Authentication | 401 `authentication_error` (malformed, revoked or expired key). |
| Billing / no credit | 402 `billing_error` (current docs). Older behaviour was 400 `invalid_request_error` with "Your credit balance is too low to access the Anthropic API" (unverified, not on the current page), so also match that text. A user-set spend limit gives 400 `invalid_request_error` "You have reached your specified API usage limits" (verified). |
| Quota / rate limit | 429 `rate_limit_error` with `retry-after`: back off. **Tier monthly spend cap** (Start $500/month) is also 429 `rate_limit_error` but has no `retry-after` and `error.details.error_code == "enforced_spend_limit_reached"`: do not retry, show "monthly cap reached, resumes <date>". Rare 429s from acceleration limits. 529 `overloaded_error`: transient, retry. |
| Eligibility / region / policy | 403 `permission_error` (key lacks permission; org or workspace settings). Unsupported-region wording is not documented on the pages read (a 403 "Request not allowed" is commonly seen; unverified). New orgs may start in a lower "Evaluation" tier with smaller limits. |
| Model not found | 404 `not_found_error` (message text unverified). Retired models fail; `lifecycle` in the list shows `deprecated`/`retired`. |
| Safety | No HTTP error for normal refusals. A `stop_reason` of `refusal` is expected on a 200 (unverified, not on the pages read). Image problems are 400 `invalid_request_error`; oversize is 413 `request_too_large`. |
| Transient | 500 `api_error`, 504 `timeout_error`. |

**Recommendation: ship v1.** Cleanest terms and data position of the three. Disclosures: "Krovi sends your text and selected screen region directly to Anthropic with your own API key. Anthropic does not train on API data by default and deletes it within 30 days (longer if flagged for policy). Use an API key from console.anthropic.com, not a Claude subscription." Plus the AI-output label. Cap Krovi's `max_tokens` and tell users to set a Console spend limit.

## 6. Candidates (not for v1)

**OpenRouter** (https://openrouter.ai/terms, /docs/api-reference/errors, /docs/api-reference/limits, /docs/guides/privacy/data-collection)
- 18+. Key holders are liable for all usage "whether or not authorized"; no reselling or transferring access; no VPN/proxy workarounds for provider region blocks ("material breach"). Products built on it must pass Model Terms to their users.
- Does not store prompts unless the user opts in; an opt-in "use of inputs/outputs" gives a 1% discount. Underlying model providers have their own policies; free `:free` models are limited to 20 req/min and 50/day under 10 credits purchased. Whether free routes train on data is (unverified).
- Header `Authorization: Bearer`, base `https://openrouter.ai/api/v1`. `GET /key` needs a valid key and returns limit/usage/`is_free_tier`: use it as the test. `GET /models` was not documented as authenticated (unverified), so it is not a valid test.
- Errors `{error:{code,message,metadata}}`: 401 invalid/disabled key; **402 insufficient credits**; 403 moderation/guardrail/budget (metadata has reasons and `limit_source`); 408; 429; 502 model unavailable; 503 no provider matches routing. Once streaming starts the HTTP status is 200 and the error is in the body.
- Verdict: later. It adds a third party in the data path and a catalogue of models with differing data terms.

**Groq** (https://console.groq.com/docs/errors, /docs/your-data, /docs/vision, groq.com/terms-of-use)
- The terms page fetched is the website terms only; the Services Agreement (BYOK, age, regions) was not read: (unverified).
- Data: "By default, Groq does not retain customer data for inference requests"; abuse/reliability logs up to 30 days unless Zero Data Retention is enabled; data stored in the US. Whether Groq trains on customer data is not stated on the page (unverified).
- OpenAI-compatible: base `https://api.groq.com/openai/v1`, Bearer, `image_url` data URL. Vision models listed: `qwen/qwen3.8-27b` (20 MB, 3 images) and `meta-models/muse-spark-open` (limited preview). **Flag: AGENTS.md bans `muse-spark-1.2`/`1.3` on any provider. `muse-spark-open` is a different id in the same family; the owner should confirm whether the ban covers it, and it must not be offered in a picker until then.**
- Errors `{error:{message,type}}`: 401, 403, 404, 413, 422, 424, 429, custom 498 (flex tier at capacity), 500/502/503.
- Verdict: later.

**Mistral** (https://docs.mistral.ai/api/, https://help.mistral.ai, https://legal.mistral.ai/terms)
- The legal index only was readable; consumer/commercial terms, age and regions: (unverified).
- Data: in "Free mode" Mistral says "we may use your data (input and output)" to train models; opt-out is available; the pay-as-you-go default is unclear.
- Base `https://api.mistral.ai`, Bearer, `POST /v1/chat/completions`; `GET /v1/models` inferred (unverified); image input shape and small vision model ids not confirmed.
- Verdict: later, and only with the same free-tier-trains warning as Gemini.

## 7. Open items before any provider is called "supported"

1. Owner reads OpenAI's Terms of Use, Service Terms and Usage Policies (blocked for me) and confirms age and BYOK wording.
2. Owner decides the Gemini gating: 18+ attestation, EEA/UK/CH paid-tier attestation, and whether Krovi counts as "making API Clients available" (legal read).
3. Confirm age requirements for Anthropic Console, OpenAI and the candidates.
4. During integration, capture real error bodies with a revoked key, an empty balance, an exhausted quota and a bad model id for each provider, and replace every "(unverified)" error string above.
5. Verify Gemini `inlineData` field names and the `Api-Revision` question against a live call.
6. Confirm whether the AGENTS.md `muse-spark` ban extends to `muse-spark-open`.

## 9. Follow-up research (2026-11-03)

Scope: Gemini BYOK in depth (the owner asked for a real answer, not deferral), Groq as the "truly free" fallback, and OpenAI's API-governing terms as distinct from the ChatGPT Terms of Use. **Where this section disagrees with the v1 verdicts in sections 1, 3, 4 and 6, this section wins (verdicts in 9.5).**

**Method and evidence grades.** WebSearch was unavailable. WebFetch worked for most pages. `r.jina.ai/<url>` got past the HTTP 403 on openai.com/policies. `r.jina.ai/https://lite.duckduckgo.com/lite/?q=...` worked as a search engine. web.archive.org was blocked (so older OpenAI terms could not be compared). Google's forum is readable as `https://discuss.ai.google.dev/t/<id>.json` and `/search.json?q=`. Grades: **[raw]** = I read the page text itself (Gemini Additional Terms, Google APIs ToS, Hitodama's pages). **[sum]** = returned by an AI summariser that caps quotes near 125 characters; re-read before quoting publicly. **[3p]** = third-party site.

**Corrections made inline to earlier sections** (each marked in place): (1) §1 table, Gemini "Regions" cell: "EEA/UK/CH: paid services only" was too strong (see 9.1.3). (2) §3 recommendation, second UI bullet: it stated as fact something the terms do not plainly say (replacement copy in 9.4).

### 9.1 Gemini (AI Studio key)

#### 9.1.1 Hitodama, the live product the owner pointed at [raw]

Sources: https://hitodama.in/ (home + FAQ), /requirements/, /legal/terms/ (EULA, updated 2026-10-06), /legal/privacy/ (updated 2026-10-31), /legal/. (WebFetch got HTTP 403; plain `curl` with a browser User-Agent worked.)

What it is: a paid (one-time, ₹499/$14 on the page), voice-first Windows assistant by one developer in India ("Otism"), sold through Dodo Payments. Features overlap Krovi's plans: Circle to Ask (hold Ctrl+Shift, circle a screen region, ask), dictation (Ctrl+Win), routines, to-do. It runs entirely on the user's own AI Studio key, held "in an encrypted local store using Windows DPAPI". It is very new (legal pages dated October 2026), so it has no track record.

What its FAQ and legal pages actually say:
- Home FAQ, "Is the API key really free?" (the page ships two variants of this answer): "Yes. Free to make in Google AI Studio, no card. Free to use: Hitodama is built to keep everyday use inside Google's free tier, so there's no usage bill unless you choose to turn billing on." The other variant: "Not for apps where you live. In the European Economic Area, the UK and Switzerland, Google's terms allow apps like Hitodama to use only its paid service, so you need a billing-enabled key, and Google bills you directly for what you use."
- "It runs on your own Google AI Studio API key, so there's nothing of ours to subscribe to." / "Hitodama talks to Google directly ... anything beyond the free tier is between you and Google."
- EULA, "Your Google AI Studio key": "Hitodama does not include model access. You supply your own Google AI Studio API key, and your use of Google's services is governed by your agreement with Google." And: "Google may also limit, suspend, or close your API key or your Google account under its own policies. That is between you and Google: we cannot prevent or reverse it, and we are not responsible for it."
- Privacy: "Because you use your own key, that relationship is between you and Google, and we never see it." "On Google's free tier, Google may use what you send to improve its products, and human reviewers may read it. With Hitodama that can include your voice, screenshots, memories, and file names. Do not share sensitive or confidential information while using a free key."
- Privacy and Requirements pages both say the opposite of the EEA FAQ variant: "In the European Economic Area, the United Kingdom, and Switzerland, free keys work too, and Google applies its paid-service data terms to them there".
- Age: "You must be at least 18 years old to buy or use Hitodama." Its stated reason is its own ("The application operates real files, applications, and settings on your computer"), not Google's clause.
- Country gate: it refuses sales where "Google doesn't offer the Gemini API in your country".

What it does **not** do (premise check): I found no mention of "consumer use" or Google's 18+ clause on any of the five pages. So the owner's description is right about the free-tier and EEA question, but Hitodama does not address the "not for consumer use" language. Its justification is exactly the party distinction (the contract is you-and-Google, "we never see it") plus disclosure plus a country gate. It is a published position, not evidence that Google reviewed or accepted it. Its own pages disagree with each other on the EEA/UK/CH free tier, which is the same ambiguity found in the terms (9.1.3).

#### 9.1.2 Other apps that take a pasted Gemini key, and what they say about terms

| App | Says about Google's terms/data | Source |
|---|---|---|
| Meetily (local-first meeting notes) | Explicit on data: free tier content is used "to improve products and services", reviewers may annotate it; EEA/CH/UK get paid-tier data terms; "the tier you are on is determined by your API key". Silent on 18+ and consumer use. [sum] | https://meetily.ai/llm-privacy/gemini |
| Google's own Gemini CLI | "Your use of Gemini CLI is governed by the Gemini API Terms of Service"; separate "Unpaid Services" and "Paid Services" rows for API-key users. A Google-authored client where the user's key = the user's agreement. [sum] | https://github.com/google-gemini/gemini-cli/blob/main/docs/resources/tos-privacy.md |
| Raycast (v1 manual) | Google keys supported but "requests are processed through our servers" (a proxy, unlike Krovi). Only says "You'll be responsible for the API costs". Silent on terms. Changelog v2.2 (2026-09-08) lists only Custom Providers, Ollama and OpenRouter as BYOK routes; I could not read why. [sum] | https://manual.raycast.com/ai/bring-your-own-key , https://www.raycast.com/changelog |
| Chatbox | "Direct Connection: Data goes directly to AI providers, not through intermediary servers". Silent on Google terms. [sum] | https://chatboxai.app/en/guide/byok |
| Jan, Continue.dev, Cline, Msty, TypingMind, Obsidian Copilot, Page Assist | Setup steps only. No mention of Google terms, free-tier data use, 18+ or consumer use. [sum] | docs.jan.ai/desktop/remote-models/google , docs.continue.dev/customize/model-providers/top-level/gemini , docs.cline.bot/provider-config/google-gemini.md , docs.msty.app , docs.typingmind.com , github.com/logancyang/obsidian-copilot (docs/getting-started.md) , github.com/n4ze3m/page-assist |

Pattern: the field is silent. Only Hitodama and Meetily warn users about free-tier data use, and nobody cites "not for consumer use". Being in a crowd is not a defence, but it shows Google has not (visibly) acted against BYOK clients.

#### 9.1.3 What the terms actually say

**Gemini API Additional Terms, effective March 23, 2026** [raw] (https://ai.google.dev/gemini-api/terms 301-redirects to https://aistudio.google.com/docs/terms):
- Acceptance: "To use Gemini API, Google AI Studio, and the other Google developer services that reference these terms (collectively, the "APIs" or "Services"), you must accept (1) the Google APIs Terms of Service ... and (2) these Gemini API Additional Terms of Service".
- Age (section "Age Requirements"): "You must be 18 years of age or older to use the APIs. You also will not use the Services as part of a website, application, or other service (collectively, "API Clients") that is directed towards or is likely to be accessed by individuals under the age of 18."
- Consumer use (first sentence of "Use Restrictions"): "Use of Google AI Studio and Gemini API is for developers building with Google AI models for professional or business purposes, not for consumer use."
- Regions: "You may only access the Services (or make API Clients available to users) within an available region. **You may use only Paid Services when making API Clients available to users in the European Economic Area, Switzerland, or the United Kingdom.**"
- Competing models: "You may not use the Services to develop models that compete with the Services (e.g., Gemini API or Google AI Studio)."
- Unpaid data: Google uses submitted content and responses "to provide, improve, and develop Google products and services and machine learning technologies"; "human reviewers may read, annotate, and process your API input and output" (disconnected "from your Google Account, API key, and Cloud project" first); "Do not submit sensitive, confidential, or personal information to the Unpaid Services."
- EEA/CH/UK data: "the terms under "How Google uses Your Data" in "Paid Services" apply to all Services, including Google AI Studio and unpaid quota in the Gemini API, even though they are offered free of charge."
- What counts as paid: "Your access to Gemini API is a "Paid Service" only when accessing the API through a Cloud Project associated with an active billing account." (AI Studio itself counts as paid if the Google account can reach a billing-enabled project.)
- Agent actions: "When using agentic services, including the Computer Use API, you are solely responsible for the actions and tasks performed by the service ... You will not automatically bypass any requests for human confirmation." (Relevant to Agent Mode: always confirm before acting.)
- "you" is never defined in the Additional Terms. "End user" appears only in the Grounding and Maps sections ("An end user is an individual you permit to use your application."). The words "third-party client", "bring your own key" and "key sharing" do not appear.
- History [sum, archive pages terms_09_25_25 ... terms_12_18_25]: the 18+, API Clients and EEA/CH/UK sentences are present in the 2025-09-25 version. "not for consumer use" is absent through the 2025-11-20 version and present from 2025-12-18. So it is about 11 months old and looks like a deliberate tightening.

**Google APIs Terms of Service** [raw] (https://developers.google.com/terms, last modified 2021-11-09):
- 1(a): "You may not use the APIs and may not accept the Terms if (a) you are not of legal age to form a binding contract with Google, or (b) you are a person barred from using or receiving the APIs under the applicable laws of the United States or other countries including the country in which you are resident or from which you use the APIs."
- 1(b): "If you are using the APIs on behalf of an entity ... (and all references to "you" in the Terms refer to that entity)." So "you" is whoever accepts: a person, or the entity they bind.
- 3(a): "The APIs are designed to help you enhance your websites and applications ("API Client(s)")." So "API Client" means the account holder's own application.
- 2(a): "You will require your end users to comply with (and not knowingly enable them to violate) applicable law, regulation, and the Terms." 2(c): "You will not misrepresent or mask either your identity or your API Client's identity when using the APIs or developer accounts." 3(d): you will provide "a privacy policy for your API Client". 5(b): "Before you submit content to our APIs through your API Client, you will ensure that you have the necessary rights (including the necessary rights from your end users) to grant us the license."
- 4(a): you will not "Sublicense an API for use by a third party. Consequently, you will not create an API Client that functions substantially the same as the APIs and offer it for use by third parties."
- 4(b): "Developer credentials (such as passwords, keys, and client IDs) are intended to be used by you and identify your API Client. You will keep your credentials confidential ... **Developer credentials may not be embedded in open source projects.**" This is the one clause that bites a MIT repo directly (and it is the section Google's staff link in every BYOK thread, see 9.1.5).

**EEA/UK/CH is genuinely ambiguous.** Three statements: (i) the bold "paid only when making API Clients available to users" sentence; (ii) paid data terms apply to unpaid quota "even though they are offered free of charge"; (iii) Google's billing FAQ: "Can I use the Gemini API for free in EEA (including EU), the UK, and CH?" "Yes, we make the free tier and paid tier available in many regions." (https://aistudio.google.com/docs/billing.md [sum]). Statements (ii) and (iii) only make sense if a person in those regions may use free quota. The reading that reconciles all three: sentence (i) governs someone who offers an API Client to other people there; a person using their own key for themselves is covered by (ii). That is a reading, not a Google statement. The earlier table's blanket "paid services only" ignored (ii) and (iii).

#### 9.1.4 Who is the contracting party? (a) developer on own key vs (b) key-holder using a tool

Where the contract forms, and who Google checks:
- "Google AI Studio automatically creates a default Google Cloud project and API key after you accept the Terms of Service." (https://aistudio.google.com/docs/api-key.md [sum])
- Troubleshooting [sum] (https://aistudio.google.com/docs/troubleshoot-ai-studio.md): "You must be 18 or older to use Google AI Studio." "You must meet the age requirements (18+) on your Google Account." Users failing the check "may be redirected" and must fix their birthday or "Complete age verification on your Google Account". Also: "You must accept the Google Terms of Service and Generative AI Additional Terms of Service." "You must be located in a supported region." "The Google Cloud project must not be flagged for abuse." A 403 "Access Restricted" means "you are using Google AI Studio in a way that does not follow the Terms of Service"; "One common reason is you are not located in a supported region."
- So every check Google documents (age, terms acceptance, region, project health, free-tier eligibility per project) runs against the key-holder's Google account and project, before Krovi is involved.

| Clause | (a) Developer builds on own key, serves consumers | (b) Key-holder uses the API through a tool (Krovi BYOK) |
|---|---|---|
| Who is "you" | The developer | The key-holder |
| 18+ | Developer must be 18+ and the app must not be directed at / likely accessed by under-18s | Key-holder must be 18+, enforced by Google on the account. App limb: Krovi gates the Gemini feature behind an adult attestation |
| "not for consumer use" | Squarely engaged: the API Client is a consumer product | Ambiguous, see below |
| EEA/UK/CH paid only | Engaged ("making API Clients available to users") | Arguably not engaged (the key-holder offers the client to nobody else), but Google has not said so |
| Free-tier data / sensitive data ban | The developer is uploading other people's content | The key-holder uploads their own content (and whatever is on their screen); Krovi must warn |
| Credentials | Developer key must stay server-side | Key stays on the user's PC; Krovi holds no credential |
| Indemnity for end-user misuse (API ToS, [sum] section 9) | On the developer | Nobody to indemnify: Krovi has no contract |

**Does "not for consumer use" restrict (a) but not (b)?** Honest answer: that is one plausible reading, not a safe conclusion.
- Reading 1 (product): the API may not back consumer products. This hits (a) and spares (b). It is the owner's hypothesis and is consistent with the Additional Terms' own examples (every other clause is written for someone who serves users).
- Reading 2 (audience): the API is for "developers building with Google AI models"; a person who builds nothing and just uses AI features through a consumer app is outside the intended audience. This reading spares (a)-with-professional-product and **hits (b)**, because the key-holder is the consumer. A third-party article reads the clause this way for personal use of AI Studio (remio.ai [3p], speculative, cites no Google statement).
- Reading 3 (purpose): "professional or business purposes" describes the purpose of the key-holder's use. A student or hobbyist using Krovi for personal notes is outside it.
- Textual point in favour of leniency: of the four clauses in this part of the terms, "not for consumer use" is the only one phrased as a statement of purpose ("Use ... is for ...") rather than a covenant ("You must", "You will not", "You may use only"). "Consumer" is not defined. No documented enforcement mechanism checks it (the documented 403 causes are region, age, abuse flags, tier eligibility). That makes it the softest clause, not a dead one: the 403 text itself says AI Studio access can be restricted when the terms are not followed.
- What BYOK changes for sure: the exposure sits with the key-holder's account and project (key or project restriction), and Krovi has no contract to breach. What it does not change: the user may be using the API in a way the clause was meant to discourage, and a restricted key will be blamed on Krovi. That reputational risk is real.

**Does Krovi agree to any Google terms in BYOK?** No, provided all of these hold: (1) Krovi owns no Google account, Cloud project, OAuth client or API key for the product; (2) Krovi ships no credential (also required by API ToS 4(b) for open source); (3) Krovi runs no server or proxy and never receives the key; (4) the user, not Krovi, creates the key in AI Studio and accepts Google's terms there. The maintainer's own development key is the maintainer's personal agreement with Google: keep it out of the repo and CI artifacts and do not use it for anyone else's traffic.

**Would registering or identifying make Krovi a "developer" anyway?** No evidence for it:
- Google's integration guide defines "A partner is anyone building an integration between the Gemini API and end-user developers", requires partners to send an `x-goog-api-client` header ("company-product/version"), and says "If you are building an end-user application ... skip this guide". Krovi is an end-user app. There is no registration step. (https://aistudio.google.com/docs/partner-integration.md [sum]. Same page mentions "Bring Your Own Key" implementations as a normal integration path.)
- A truthful `User-Agent: Krovi/<version>` is fine and consistent with 2(c) (no masking). Do not send a fake identity.
- Things that **would** make Krovi a developer/party: shipping or generating its own key; a Krovi-owned OAuth client ("Sign in with Google" for Gemini; it would also hit 4(b)); any proxy or backend; pooling, rotating or relaying users' keys; offering a Krovi-paid "free AI" tier; storing keys off the PC.
- Google staff do call Krovi an "API Client": moderator Sai_Deepika_K (2026-08-25, thread 179107): "The Gemini API Additional Terms of Service and Google APIs Terms of Service define any application that accesses the APIs as an ..." (API Client) [sum, first sentence]. That fits the text (the user's application) and still leaves every obligation on "you".

**Verdict on the argument.** Holds up well for: Krovi is not Google's customer, not bound, not directly liable; 18+ (Google enforces it at the account); key ownership and consent happening at Google's own sign-up. Holds up only partly for: "consumer use" and EEA/UK/CH, which are ambiguous, untested and unresolved by Google. Hitodama is making the same bet. The right response to an unresolved clause is disclosure and a clean architecture, which is cheap here, not abstention.

#### 9.1.5 Has Google said anything about third-party clients or BYOK?

- No official statement, FAQ or policy page addresses BYOK clients. The nearest are the partner guide (above) and the Gemini CLI terms page (above).
- Google's forum has at least 20 threads asking exactly this (BYOK desktop app, BYOK web tool, open-source EEA study app, all-ages app, consumer iOS app, OCR extension). Search: https://discuss.ai.google.dev/search.json?q=%22not%20for%20consumer%20use%22 . None got a substantive answer. Staff (flags `staff:true, moderator:true`) answered with: "We can't provide specific legal guidance on this forum. Please refer to the documents below:" followed by links to the Additional Terms, Available regions, Google APIs Terms and its "Developer credentials" section. Examples: 197851 (free-tier BYOK app, answered 2026-11-03 by Pooja_Kapse); 188326 (BYOK web tool asking how the terms apply to users' own keys, 2026-10-14); 183085 (free open-source BYOK desktop app for EEA students, Sai_Deepika_K, 2026-09-30); 184536 (all-ages iOS app, 2026-10-05); 188423 (asks whether "consumer use" describes the developer's own use, 2026-10-14); 179107 (Windows BYOK app, 2026-08-25). Threads 144246, 172956, 146844, 181039, 173744 have no reply at all. A non-staff "Regular" said third-party extensions should "Allow users to configure their own Gemini API key (BYOK)" (189889): not an official statement.
- Reading the silence: Google has been asked the question publicly for six months, declined every time to say "no", and keeps pointing to the "Developer credentials" clause. That is weak evidence of tolerance, not permission. It also means no one can get a written answer from Google; the owner should not wait for one.

#### 9.1.6 What Krovi should do (Gemini)

1. The user creates the key themselves in their own AI Studio account (link to https://aistudio.google.com/api-keys). Krovi never creates projects or keys, never uses OAuth, never opens Google sign-in.
2. Key in Windows Credential Manager only; sent only to `generativelanguage.googleapis.com` from the Rust side; header `x-goog-api-key`. No proxy, no relay, no cloud sync of the key, no key in logs, config, crash reports or exports.
3. Ship no key, ever. Add a secret scan to CI (patterns for `AIza...` and the newer `AQ.` keys) so a fork or contributor cannot embed one (API ToS 4(b)).
4. Consent screen before the key field is enabled (copy in 9.4): 18+ and "I created this key myself"; link to the Additional Terms, with the consumer-use sentence quoted; free-tier data use and human review; avoid sensitive content; region question with billing attestation for EEA/UK/CH. Store the consent version and timestamp locally.
5. Keep the Gemini feature gated and off by default, independent of the rest of the app, so the "likely to be accessed by under-18s" limb is addressed by gating the Gemini feature behind an adult attestation. (Google has not confirmed that feature gating is enough; thread 184536 asked and got no answer.)
6. Test connection in two steps (§2.1) and surface eligibility: list models, then a 1-token inference. Map: 403 "Your project has been denied access" (Restricted / Free tier unavailable / Payment needed / Unavailable) to "This key is valid, but its Google project cannot use this tier or region"; 403 "reported as leaked" to "Create a new key"; 429 to quota. New: AI Studio keys created since 2026-05-28 are "auth keys" and "The Gemini API rejects requests from unrestricted standard keys"; the docs give no status code or message for that rejection (unverified), so a failing old key should show "This key may be an unrestricted standard key or Blocked: in AI Studio choose Restrict to Gemini API only, or create a new key." Do not validate the key prefix: Hitodama's own mock-up shows an `AQ.`-prefixed key (unverified), so do not require `AIza`.
7. Per-send confirmation with image preview for Circle-to-Ask, plus the line "If your key is on the free tier, Google may use and review this image." Screen content often holds other people's data, which the free tier forbids sending.
8. Agent Mode intent fallback: the model proposes, the user confirms. Never auto-execute on a Gemini response ("You will not automatically bypass any requests for human confirmation").
9. Marketing and website: describe it as "bring your own key" and link Google's terms. Do not advertise "free Gemini AI" or imply Google endorses Krovi. Do not claim support before the integration works (document rule at the top).
10. Isolate Gemini behind the provider adapter so it can be disabled in a point release if Google changes the terms or restricts BYOK clients.

#### 9.1.7 Free-tier models and rate limits (as of 2026-11-03)

- Free of charge (pricing page [sum], https://aistudio.google.com/docs/pricing.md): `gemini-3.8-flash`, `gemini-3.6-flash`, `gemini-3.5-flash-lite`, `gemini-3.1-flash-lite`, `gemini-3-flash-preview`, `gemini-2.5-flash` (deprecated; routes to 3.5-flash-lite on 2026-12-01), `gemini-2.5-flash-lite` (existing users). For every one, "Used to improve our products" is **Yes** on the free tier. Not free: `gemini-4-argon` ("not available on the Free tier") and the Flash image-generation models.
- Image input (from the pricing input rows): `gemini-3.5-flash-lite` "(text / image / video / audio)"; `gemini-3.1-flash-lite` "(text / image / video)"; `gemini-3-flash-preview` and `gemini-2.5-flash-lite` list image; `gemini-3.6-flash` is described as multimodal; the page does not state image input for `gemini-3.8-flash`. Best default for Circle-to-Ask on a free key: `gemini-3.5-flash-lite`.
- Rate limits: **Google publishes no RPM/RPD numbers in the docs.** "Rate limits depend on a variety of factors (such as your usage tier) and can be viewed in Google AI Studio." Also: "Rate limits are applied per project, not per API key"; RPD resets at midnight Pacific; free qualification is "Active project or free trial". [sum]
- Third-party figures, unverified: scriptbyai.com (published 2026-10-19, says copied from AI Studio's Free tier view) lists `gemini-3.5-flash-lite` and `gemini-3.1-flash-lite` at 10 RPM / 250,000 TPM / 500 RPD, and `gemini-3.8-flash` and `gemini-3.6-flash` at 5 RPM / 250,000 TPM / 20 RPD (https://www.scriptbyai.com/gemini-api-free-tier-limits/ [3p]). Other sites disagree (tokenmix.ai: 1,500 RPD, apparently older). Treat as order of magnitude only: do not hard-code, show a "Quota" status on 429, and prefer flash-lite for free keys.

### 9.2 Groq, the "truly free" fallback

Sources: https://console.groq.com/docs/rate-limits , /docs/models , /docs/vision , /docs/deprecations , /docs/tool-use , /docs/structured-outputs , /docs/your-data , /docs/billing-faqs , /docs/legal/services-agreement (Last Modified June 22, 2026) , /docs/coding-with-groq , https://groq.com/terms-of-use . All [sum].

**Free plan.** No card: the billing FAQ says "To upgrade from the Free tier to the Developer tier, you'll need to provide a valid payment method", which implies the free tier needs none; several third-party guides say outright "no credit card" (klymentiev.com, perkstack.co [3p]). The official pages never say it in so many words (unverified). Limits "apply at the organization level" and exceeding one returns 429 with `retry-after`.

Free-plan table (the rate-limits page shows one table under two tab labels; it is the Free tab because the models page lists `qwen/qwen3.8-27b` at 250K TPM / 1K RPM for the Developer plan, so the 8K-TPM table cannot be the Developer plan):

| Model | RPM | RPD | TPM | TPD |
|---|---|---|---|---|
| `openai/gpt-oss-120b` | 30 | 1K | 8K | 200K |
| `openai/gpt-oss-20b` | 30 | 1K | 8K | 200K |
| `qwen/qwen3.8-27b` (vision, preview) | 30 | 1K | 8K | 200K |
| `openai/gpt-oss-safeguard-20b` | 3 | 1K | 2K | 200K |
| `whisper-large-v3` / `-turbo` | 20 | 2K | n/a | n/a (7.2K audio sec/hour) |

**Vision.** Only `qwen/qwen3.8-27b`: 20 MB per image URL request, "a maximum of 3 images", "Each image counts as 2048 input tokens", tool use and JSON mode supported with images. It sits under **Preview** ("may be discontinued at short notice"). Base64 `image_url` data URLs are the documented format; a separate base64 size cap is not stated (unverified). `meta-models/muse-spark-open` is the only other vision entry (limited preview, Enterprise): excluded by the AGENTS.md ban family; Krovi's model picker must filter any id starting with `muse-spark` and `meta-models/muse-spark`. Llama 4 is gone: deprecations page lists `llama-4-scout-17b-16e-instruct` shutdown 07/17/26 and `llama-4-maverick-17b-128e-instruct` 03/09/26 (free and developer tiers), so earlier Llama vision advice is obsolete. `qwen/qwen3.6-27b` shut down 09/14/26.

**Text models usable on the free plan today:** `openai/gpt-oss-120b` (131K context, 65K max output; tool use yes but not parallel; JSON mode yes; strict structured outputs yes; reasoning model), `openai/gpt-oss-20b` (same features, faster), `qwen/qwen3.8-27b` (parallel tool calls, JSON mode, strict structured outputs). Not available on the free plan: `llama-3.3-70b-versatile` and `llama-3.1-8b-instant` (shown only with an "Enterprise" badge; the deprecations page schedules both for 08/16/26 on free and developer tiers), `qwen/qwen3-32b` (07/17/26), `moonshotai/kimi-k2-instruct-0905` (04/15/26), `minimaxai/minimax-m2.7` (Enterprise preview). Expect constant churn: read the model list at runtime and map 404 to "pick another model".

**Terms (Services Agreement, 2026-06-22):**
- Age: "You must be 18 years of age or older to access or use the Cloud Services." and "By using the Cloud Services, you represent that you are age 18 or older."
- Consumer: "Cloud Services and the AI Model Services under this Agreement are not for consumer use." (Same wording class as Google's, so 9.1.4 applies: the user is the Customer, Krovi is the user's client.)
- Account: "Customer may not resell or lease access to its Account." "Customer will only make its Account available to Authorized Account Users." Customer is responsible for "the security of its passwords for the Account (including any keys for Groq's APIs)". No clause on third-party clients. Groq's own docs promote third-party tools with "native Groq provider support": Cline, Roo Code, Kilo Code, OpenCode, Factory Droid (/docs/coding-with-groq). Such tools take the user's own Groq key, though that page does not say so in words.
- Training: "Groq is not permitted to use Inputs or Outputs for training or fine-tuning any AI Model Services or other models" (contractual, section 4.2). Retention: "Groq does not access, use, store, or retain Inputs or Outputs except as necessary to provide the Cloud Services"; the data page says "By default, Groq does not retain customer data for inference requests", logs of inputs/outputs only for reliability debugging or abuse, "retained for up to 30 days", stored in US GCP, Zero Data Retention available. I did not confirm that the free plan has different data terms (unverified).

**Is it enough?**
- **Circle-to-Ask on a screenshot crop: yes, narrowly.** One question costs about 2,048 tokens for the image plus prompt plus answer, roughly 3,500 tokens. With 8K TPM that is about 2 questions a minute and about 55 a day on the 200K TPD cap. Fine for a human at a desk; fails if Krovi retries aggressively. Risk: the only vision model is a preview and may vanish; the UI must say so and fall back gracefully.
- **Long-document summary: no, not on the free plan.** The 131K context is irrelevant when TPM is 8K: one request can use at most about 6K tokens of document, so a 50K-token document needs chunking and 8+ minutes, and 200K tokens a day is about 150K words total. Acceptable only for short documents (a few thousand words); anything longer should be offered with Gemini or a paid key. (The exact error for an over-TPM request is not confirmed; assume 413 or 429 and do not retry.)
- **Agent Mode intent fallback (short JSON): yes.** `gpt-oss-20b` with JSON mode or a strict schema, 30 RPM, 1K RPD, a few hundred tokens each. Set low reasoning effort and a small `max_completion_tokens` so reasoning tokens do not eat the 8K TPM (token accounting for reasoning is unverified).

### 9.3 OpenAI API terms (not the ChatGPT terms)

**Which documents govern an API key.** The **Services Agreement** (the "Business Terms") at https://openai.com/policies/services-agreement/ , effective January 1, 2026 per the page; plus the **Service Terms** (https://openai.com/policies/service-terms/ , no date visible) and the **Usage Policies** (https://openai.com/policies/usage-policies/ , the version I could read is "Effective: November 20, 2026", i.e. announced, not yet in force on 2026-11-03; the version in force now was not readable). The **Terms of Use** (https://openai.com/policies/row-terms-of-use/ , EU copy at /eu-terms-of-use/) are for individuals using ChatGPT, DALL-E and OpenAI's other consumer services. All read through `r.jina.ai` [sum]; `/business-terms/` returned HTTP 403 even through the reader (it appears to be an alias).

**Clauses (Services Agreement unless noted):**
- Scope: it "only applies to use of OpenAI's APIs, ChatGPT Enterprise, ChatGPT Business, ChatGPT for Clinicians" plus other developer offerings, and "does not apply to OpenAI services used by consumers or individuals unless specified above" (the API is specified). "Customer" is "the organization agreeing to these terms"; the signer "represents it is lawfully able to enter into this Agreement" and, for an entity, has authority to bind it.
- Minimum age: **the Services Agreement states none**; the only age rule is 3.3, "Customer will not, and will not permit End Users to: ... (c) allow minors to use OpenAI Services without consent from their parent or guardian". Capacity to contract is the practical floor. The separate https://developers.openai.com/api/docs/guides/safety-checks/under-18-api-guidance.md page sets duties for apps that serve minors (zero data retention before processing personal data of under-13s; "OpenAI reserves the right to audit organizations for compliance"). It sets no minimum age for key holders.
- Keys: 3.1 "Customer will not share Account access credentials or individual login credentials between multiple users." "Customer may not resell or lease access to its Account or any End User Account." 3.3(g): no "buy, sell, or transfer API keys from, to, or with a third party". Help centre (https://help.openai.com/en/articles/5112595-best-practices-for-api-key-safety): "Your API key is intended to be used by you." "The sharing of API keys is against the Terms of Use." "OpenAI does not support the sharing of API keys." "Never deploy your key in client-side environments like browsers or mobile apps."
- End users: a "Customer Application" is "Customer's applications, products, or services that integrate with an OpenAI API"; an "End User" is "any party: (a) who accesses the Services under Customer's Account; or (b) who uses Customer Applications". 3.2: the Customer answers for "the activities of End Users ... who access the Services through a Customer Application". 2.2 grants "the right to use OpenAI's API to integrate the Services into Customer Applications."
- Intermediaries (Usage Policies, "Providing access to our models"): operators of "a model-access intermediary platform or other service that enables third parties to directly access OpenAI models" need OpenAI's "prior review and approval"; not triggered "solely because an application integrates OpenAI models".
- Data: 4.2 "OpenAI will not use Customer Content to develop or improve the Services, unless Customer explicitly agrees to such use." API guide (https://developers.openai.com/api/docs/guides/your-data): "As of March 1, 2023, data sent to the OpenAI API is not used to train or improve OpenAI models" unless you opt in; "abuse monitoring logs are generated for all API feature usage and retained for up to 30 days"; "the Responses API has a 30 day Application State retention period by default" (send `store: false`); Zero Data Retention is "subject to prior approval by OpenAI"; image and file inputs are CSAM-scanned and a flagged image is retained for manual review even under ZDR.

**BYOK analysis.** The user opens an API account and accepts the Services Agreement: the user is the "Customer". Krovi is, at most, a "Customer Application" of that user and the user is its only End User. The key is "intended to be used by you"; a local client that the key-holder runs and that sends the key only to api.openai.com is the key-holder using their key, as with curl or the official SDK. Sharing means giving it to someone else; with no Krovi server, Krovi never receives it. (A BYOK thread on Google's forum, 178632, relays that OpenAI support called server-side key storage "effectively 'sharing' the key with a third party" [3p, unverified]; Krovi's local-only design avoids that.) No clause in the Services Agreement bars third-party clients. Two traps: do not turn Krovi into a generic pass-through console that "enables third parties to directly access OpenAI models" (keep calls purpose-built), and never accept a ChatGPT login or token instead of an API key.

**Why third-party summaries confuse the two.** (1) Two separate contracts: the consumer Terms of Use say "You must be at least 13 years old or the minimum age required in your country to consent to use the Services" and "If you are under 18 you must have your parent or legal guardian's permission". They now say API use belongs to the Business Terms ("Our Business Terms govern use of ChatGPT Enterprise, our APIs, and our other services for businesses and developers"). (2) Older Terms of Use named the API among "Services" and a 2024 blog (codenote.net [3p]) tells API developers to copy the 13/parental-consent rule; I could not read the old text (archive blocked), so that history is unverified. (3) The 13 and 18 figures that remain in API documentation (under-18 guidance) describe the end users of apps, not the account holder. (4) The Services Agreement has no age number, only capacity and the 3.3(c) minors rule. For Krovi's UI, require "I am 18 or older" for every provider: it is stated for Google and Groq and satisfies capacity and 3.3(c) for OpenAI.

### 9.4 Draft UI copy

Common: store consent version and time locally; the key field stays disabled until all required boxes are ticked. Link every terms name to its URL.

**Gemini** (dialog title: "Use your own Gemini key")
- Body: "Krovi does not provide Gemini access. You create your own key in Google AI Studio, and your use of the Gemini API is an agreement between you and Google. Krovi never sees your key: it stays in Windows Credential Manager on this PC, and requests go straight from this PC to Google."
- [ ] "I am 18 or older, and I created this key myself with my own Google account."
- [ ] "I have read the Gemini API Additional Terms. Google says the Gemini API is 'for developers building with Google AI models for professional or business purposes, not for consumer use'. I am responsible for using my key in line with Google's terms, and Google may restrict my key or project."
- [ ] "If my key is on the free tier, Google may use what I send, including screenshots and answers, to improve its products, and people at Google may read it. I will not send sensitive, confidential or other people's personal content with a free key. Keys on a billing-enabled project are handled under Google's paid terms."
- Region (default from the Windows region, user can change): "Where I use Krovi: ( ) EEA, UK or Switzerland ( ) Elsewhere". If EEA/UK/CH: [ ] "My key's Google project has billing enabled. (Google's terms say only paid services may be used when making API clients available to users in these regions.)"
- Footnote: "A valid key does not guarantee access. Google decides free-tier access per project and region. If Krovi reports a problem, fix it in AI Studio; creating more keys will not help."
- Per-send (image): "Send this region to Google Gemini with your key? If your key is on the free tier, Google may use and review this image. [Send] [Cancel]"

**Groq** (title: "Use your own Groq key")
- Body: "Krovi sends your text and the image you select straight to Groq using your own key, stored on this PC only. Groq's Services Agreement requires you to be 18 or older and says the service is 'not for consumer use'. Groq states it does not train on your inputs or outputs and does not keep them by default (it may log them for up to 30 days to fix errors or investigate abuse), and customer data is stored in the United States."
- [ ] "I am 18 or older, I created this key myself, and I am responsible for using it in line with Groq's terms."
- Note: "The free plan is small (about 55 image questions a day, and long documents will not fit). The image model is a Groq preview model that can be withdrawn without notice."

**OpenAI** (title: "Use your own OpenAI API key")
- Body: "Use an API key from platform.openai.com, not your ChatGPT login. You are the customer under OpenAI's Services Agreement. Keep the key to yourself: OpenAI says 'Your API key is intended to be used by you.' Krovi stores it on this PC only and sends it only to OpenAI. OpenAI does not train on API data by default; it may keep it for up to 30 days for abuse monitoring. Krovi turns off optional storage (`store: false`)."
- [ ] "I am 18 or older and I created this key myself."
- Note: "API use is billed to your OpenAI account. Set a spend limit in the OpenAI dashboard."

All providers, on every answer: "AI-generated, may be wrong."

### 9.5 Verdicts and residual risk

**Gemini: ship in v1: YES, gated** (default off, consent screen above, adult attestation, EEA/UK/CH billing attestation, per-send preview, no shipped credentials, provider adapter that can be switched off). This reverses "or defer": the party analysis in 9.1.4 holds for the clauses Google can enforce (18+, region, project eligibility, all at the account), and the remaining ambiguity is shared with the live commercial product Hitodama and with dozens of other tools. Residual risk in plain words:
1. Google has never confirmed that third-party BYOK clients are allowed and has refused every public request to say so. It can change the terms or enforcement without notice. If it does, Krovi must pull Gemini in a point release.
2. "Not for consumer use" is undefined and could be read against Krovi's users (a private person using AI as a consumer). The legal exposure lands on their keys and projects, not on Krovi, but the support burden and blame land on Krovi.
3. Free-tier keys feed Google's training and human review. Screenshots often contain other people's data. Disclosure reduces harm, it does not remove it, and it sits awkwardly beside a privacy-first positioning. Recommend the per-send preview and a visible "free key: Google may review" badge, and steer privacy-minded users to Groq or a paid project.
4. The 18+ gate is self-declared; Krovi cannot check it. Google checks age on the Google account, which is the real control.
5. EEA/UK/CH: the conservative billing attestation may exclude users who could lawfully use free quota there. Relax only if Google clarifies.
6. Free limits are not published and can change; a project can lose free-tier eligibility at any time.

**Groq: ship in v1: YES** as the zero-cost option (text via `gpt-oss-120b`/`20b`, vision via `qwen/qwen3.8-27b` labelled preview). Best contractual data terms of the three (no training, no default retention). No long-document summarisation on the free plan; offer Gemini or a paid key for that. Residual risk: same 18+ and "not for consumer use" wording as Google (same analysis, but Groq itself lists third-party coding tools with native Groq support, so I rate it slightly lower risk than Gemini); the vision model is a preview and models churn fast (Llama 4 was removed within months); free limits are tiny; data is stored in the US; sign-up and the free plan terms were summarised, not read in full.

**OpenAI: ship in v1: YES.** The terms are now read: no consumer-use language aimed at API users, no stated minimum age beyond capacity, no clause against third-party clients, contractual no-training. Residual risk: key-sharing language, so Krovi must never touch the key beyond the local call; the Usage Policies "intermediary" paragraph (effective 2026-11-20) means Krovi must stay a purpose-built app, not a raw model console; the user needs prepaid credit; abuse logs are kept up to 30 days and flagged images longer; and I could not read the Usage Policies version currently in force or archived OpenAI terms.

Each provider is "supported" only after the integration works end to end and the owner signs off on the consent copy (rule at the top of this file). Website copy must wait for that.

### 9.6 Still unverified

- Whether Google accepts the party distinction (no statement exists); EEA/UK/CH meaning of "making API Clients available to users" for a self-user.
- Gemini free-tier RPM/RPD (only third-party numbers); the status code and message for an unrestricted standard key; `AQ.` key prefix (seen only in Hitodama's mock-up).
- Gemini `inlineData` field names (from §7 item 5) are still unconfirmed against a live call.
- Groq: official statement that the free plan needs no card; free-plan-specific data terms; base64 image size cap; exact error for an over-TPM request; countries where Groq is unavailable.
- OpenAI: the Usage Policies in force before 2026-11-20; older Terms of Use scope (archive blocked); exact API-signup age check; Service Terms effective date. Anthropic and the remaining candidates were not re-checked this round.
- Raycast's stated reason for dropping native Google/OpenAI/Anthropic BYOK in v2 (page unreadable).
- `muse-spark-open` ban scope (§7 item 6) is still open; the filter above is a safe default.

## 10. Snowflake Cortex (experimental, added 2026-11-03)

Scope: Snowflake Cortex as a fifth BYOK provider (owner request): the user's own Snowflake account URL plus a Programmatic Access Token (PAT) kept in Credential Manager at `Krovi/ai/snowflake`; the account URL (an address, not a secret) is the only Snowflake value in `ai.v1`. Labelled "Your Snowflake account · billed by Snowflake" and **Experimental** in the UI. **Not "supported" until it has been run against a live account.**

**Method and grades.** WebFetch only (an AI summariser sits between us and the page, so every item is **[sum]**: re-read the cited page before quoting it publicly). No Snowflake account was available, so nothing below was observed on a wire. Pages read:
- https://docs.snowflake.com/en/user-guide/snowflake-cortex/cortex-rest-api and .../cortex-llm-rest-api (the Cortex REST API guide, 103,581 characters; the last 3,581 were not read)
- https://docs.snowflake.com/en/developer-guide/snowflake-rest-api/reference/cortex-inference (REST reference)
- https://docs.snowflake.com/en/user-guide/programmatic-access-tokens (PAT)
- https://docs.snowflake.com/en/user-guide/snowflake-cortex/aisql (data handling) and https://docs.snowflake.com/en/sql-reference/functions/ai_complete

**Endpoint: the brief and the docs differ.** The brief named `POST {account}/api/v2/cortex/inference:complete` with `{model, messages, max_tokens}`. The REST reference does list that path ("LLM text completion inference, similar to snowflake.cortex.Complete") and also `GET /api/v2/cortex/models` ("Returns the LLMs available for the current session"), but the pages I could read give **no request or response schema** for either. What the Cortex REST API guide documents in detail is three OpenAI/Anthropic-compatible endpoints under `https://<account_identifier>.snowflakecomputing.com`:

| Endpoint | Spec | Models |
|---|---|---|
| `POST /api/v2/cortex/v1/chat/completions` | OpenAI Chat Completions | all models |
| `POST /api/v2/cortex/v1/messages` | Anthropic Messages | Claude only |
| `POST /api/v2/cortex/v1/responses` | OpenAI Responses (preview) | OpenAI models on AWS only |

Krovi uses **`/api/v2/cortex/v1/chat/completions`**: it is the documented contract, it is the same shape Krovi already builds for OpenAI and Groq, and it has a documented image path. Consequences that differ from the brief: the token limit field is `max_completion_tokens` (default 4096, theoretical max 131,072; `max_tokens` is "deprecated" there and "returns an error"), and `stream` is documented to default to `false` (Krovi still sends `"stream": false`).

**Request** (as built by `krovi_net::ai`):
- Headers: `Authorization: Bearer <PAT>`, `X-Snowflake-Authorization-Token-Type: PROGRAMMATIC_ACCESS_TOKEN` (documented as optional; "if you omit it, Snowflake infers the type"), `Content-Type: application/json`, `Accept: application/json`.
- Body: `{"model", "messages":[{"role","content"}], "max_completion_tokens", "stream": false}`; `content` is a string, or for a picture an array of `{"type":"text"}` and `{"type":"image_url","image_url":{"url":"data:image/png;base64,..."}}` parts.
- Non-streaming reply: text at `choices[0].message.content`, token counts in `usage.prompt_tokens` / `completion_tokens` / `total_tokens`. (The only full JSON example on the page is a tool call, so the plain-text shape is inferred from the compatibility tables: unverified.)
- Test connection is one completion of 16 tokens (there is no model-list step; see Models).

**Errors** (the page lists statuses, no JSON bodies; "error messages are generated by Snowflake, not by the model provider"):

| Signal | Krovi kind |
|---|---|
| 401, 403 "Not Authorized", `PAT_INVALID`, a network-policy message | auth ("token expired, role without Cortex access, or a network policy blocks this PC") |
| 200 OK with error code `390112` (an expired *session* token) | auth |
| 400 "unknown model <name>" | model ("not available in your region") |
| 400 "all requests were throttled by remote service" | rate-limit |
| 400 "invalid options object" / "schema validation failed" / "max tokens of N exceeded" | other |
| 402 "budget exceeded" | billing |
| 429 "too many requests" | rate-limit |
| 503 "inference timed out" | unavailable |

For a PAT the docs give **no HTTP status for an expired token or a network-policy block** (they name `PAT_INVALID` as a catch-all), which is why 401 and 403 are reported as one "this token will not do" message.

**PAT facts that shape the copy** [sum]: sent as `Authorization: Bearer`; by default the user must be subject to a network policy with at least one network rule (a PERSON user can create a token without one but needs one to authenticate with it; the policy must allow the app's IP ranges); default expiry 15 days, maximum 365, not changeable later; the role is fixed at creation and revoking it makes the token unusable; at most 15 tokens per user; the token format is not documented (so Krovi checks only length, never a prefix; up to 2,048 characters are accepted).

**Privileges.** The user's default role needs `SNOWFLAKE.CORTEX_USER` (granted to PUBLIC by default) or `SNOWFLAKE.CORTEX_REST_API_USER` (a finer-grained role that works once CORTEX_USER is revoked from PUBLIC). A 403 can therefore also mean "your admin restricted Cortex".

**Models.** The "Models" part of the guide lists ids with a status (GA, public preview, private preview, legacy). Public preview models "aren't suitable for production workloads", private preview ones work only for accounts Snowflake enabled, and after a legacy date only accounts that already used a model can keep calling it. Availability differs per region and with the cross-region inference setting ("Regional availability" is a separate page that was not read). Krovi ships a **static list of the GA ids** (Claude Opus 5 / 4.8 / 4.7 / 4.6 / 4.5, Sonnet 5 / 4.6 / 4.5, Haiku 4.5; `openai-gpt-6-astra`, `openai-gpt-5.6-luna` / `-sol` / `-terra`, `openai-gpt-5.4` / `5.2` / `5.1` / `5` / `5-mini` / `5-nano`; `llama3.1-8b`), and the user can type any other id (including a fine-tuned `database.schema.model`). Defaults: text `llama3.1-8b` (the least region-dependent GA model), pictures `claude-sonnet-4-5`. **Correction to the brief:** it said there is no public model-list endpoint; the REST reference does list `GET /api/v2/cortex/models`, but its schema is not readable, so Krovi does not call it in v1. Once verified against a live account it could replace the static list (set `supports_model_list` for Snowflake and add a parser).
- `snowflake-arctic` (named in the brief) is not in the current catalog, so it is not listed.
- **DeepSeek is excluded** (owner decision: not in v1): `deepseek-v4-flash` (private preview) and `deepseek-r1` (rate-limit tables only) are not listed, and any id containing `deepseek` is refused in `clean_model` for every provider. The marker `muse-spark` / `muse-wonder` / `muse-glimmer` ban applies here too.

**Pictures.** A documented multimodal path exists, so Snowflake is not text-only: "images are supported for Claude models (`claude-sonnet-4-5` and newer) and OpenAI models (`openai-gpt-4.1` and newer)", base64 only, "limited to 20 per conversation with a 20 MiB max request size"; for other models `image_url` is an error. Krovi sends a picture only with a model that matches that rule (`Provider::model_accepts_images`), otherwise it uses `claude-sonnet-4-5`. The rule is a heuristic on the id: Snowflake answers 400 if it is wrong.

**Terms and data** [sum]. The user is the Snowflake customer and Krovi is only their client, so Snowflake's customer agreement and AI terms apply between them (not read: Snowflake's AI terms and acceptable use policy). On data: the AISQL page says the LLMs are "deployed within the Snowflake Service perimeter", and names the model families' providers (OpenAI, Anthropic, Meta, Mistral AI, DeepSeek, xAI); it does **not** say whether those providers ever receive customer data or whether customer data trains models, and its legal-notice table (inputs as "Usage Data", outputs as "Customer Data") was flattened, so treat that mapping as unverified. Cross-region inference can run a request in another region only when the account enabled it. Krovi's card therefore says only what is verifiable: requests go to the user's own account, billed and governed by their Snowflake agreement, Snowflake runs the models inside its own service, and with cross-region inference a request can be processed in another region. It does **not** claim that data never leaves the account or region, or that it is never used for training; re-check both against Snowflake's AI terms before the owner removes the Experimental label.

**Verdict: ship as Experimental.** Residual risk, in plain words:
1. Nothing here has been run against a live account; the response and error bodies are inferred from the guide's tables.
2. The brief's `inference:complete` endpoint was not used because its schema is unreadable; if Snowflake deprecates the OpenAI-compatible endpoint, the adapter changes in one place (`chat_url` / `chat_body`).
3. PAT failures (expired, network policy, role) are indistinguishable by status code; the message lists all three.
4. Model availability varies by region, so the default text model may be missing in an account's region; the user is told to pick another.
5. Terms and data handling for the third-party models were not read in full (see above).

**Still unverified:** the exact 200 body of a plain completion; error JSON bodies; the PAT string format; whether `max_completion_tokens: 16` is accepted by every GA model (the OpenAI reasoning models may want more); `GET /api/v2/cortex/models`; the status Snowflake returns for a PAT blocked by a network policy.
