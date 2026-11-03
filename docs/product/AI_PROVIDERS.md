# Krovi BYOK AI providers: terms, eligibility, data, endpoints, errors

Researched 2026-11-03. Scope: optional AI features (text summarise/Q&A; image questions about a user-selected screen region). The user's key lives in Windows Credential Manager; calls go straight from the user's PC to the provider; there is no Krovi server or proxy.

Method: official pages read with WebFetch (an AI summariser sits between us and the page, so re-read the cited URL before quoting it publicly). WebSearch was unavailable. Items I could not confirm from an official page are marked **(unverified)**. Model ids and prices rotate; they are as shown on the pages on this date. **No provider is "supported" until its integration works end to end and the owner signs off on the terms items below. Website copy must not claim support before that.**

## 1. Summary

| Provider | BYOK in a 3rd-party app | Audience / age | Trains on API inputs by default | Regions | v1 verdict |
|---|---|---|---|---|---|
| Anthropic | No clause against it; help centre says each person should use their own key | Business terms, "not for consumer use"; age (unverified) | No (commercial). Auto-delete in 30 days | ~180 countries incl. EEA/UK/India | **Ship v1** (lowest risk) |
| OpenAI | Terms pages returned HTTP 403, so (unverified) | Age (unverified) | No since 2023-03-01 unless opted in. Abuse logs up to 30 days | ~200 countries incl. EEA/UK/India | **Ship v1 after the owner reads OpenAI's terms** |
| Gemini (AI Studio key) | No clause against it; keys are per Google Cloud project | **18+**, "not for consumer use"; no apps likely used by under-18s | **Free tier: YES, plus human review. Paid: no** | ~240 listed incl. EEA/UK/CH/India; EEA/UK/CH: paid services only | **Ship v1 gated** (acknowledgements), or defer |
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
- "Gemini API terms require you to be 18 or older. In the EEA, UK and Switzerland only paid-tier use is permitted: confirm your project has billing enabled." (checkbox)
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
