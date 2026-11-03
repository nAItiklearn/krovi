# Krovi BYOK AI provider research: DeepSeek (official API) and DeepSeek via OpenRouter

Researched 2026-11-03. Companion to `AI_PROVIDERS.md` (not edited by this file). Scope: optional BYOK AI features (text Q&A; Circle to Ask image questions about a user-selected screen region). The user's key lives in Windows Credential Manager; calls go straight from the user's PC to the provider; there is no Krovi server or proxy.

**Method and honesty notes (read first)**
- Official pages were read with WebFetch. An AI summariser sits between us and each page, so re-read the cited URL before quoting anything publicly. WebSearch failed with an API error on every try. Bash (curl/jq) was unavailable during the session, so no filtering was done deterministically.
- The OpenRouter catalog scan is therefore summariser-assisted: the full `https://openrouter.ai/api/v1/models` JSON (805,040 characters, `"total_count":451`) was read through the `r.jina.ai` mirror in nine 100k-character chunks. A direct WebFetch of `openrouter.ai/api/v1/models` (and of `...models?q=deepseek%20free`) returned a *different* document: 10 entries, `anthropic/...` ids, Anthropic `/v1/models` field names (`display_name`, `created_at`, `max_input_tokens`). That is not OpenRouter's schema, so it was discarded as a tool artifact. A deterministic `jq` pass over the catalog should be run before shipping.
- Items I could not confirm are marked **(unverified)**. Model ids, prices and free lists rotate; everything below is as shown on 2026-11-03.
- **Nothing here is "supported" until the integration works end to end and the owner signs off. Website copy must not claim DeepSeek support before then.**

---

## 1. Bottom line

| Question | Answer |
|---|---|
| Is there a confirmed DeepSeek signup credit ($5, no card)? | **No. Not confirmed on any official page I could reach.** The concept of a "granted balance" exists, but no amount, eligibility, or expiry is documented. The owner's "$5, no card" claim is **(unverified)**. Do not market it. |
| Does "DeepSeek 1027" exist? | **No.** Nothing named 1027 / 10-27 anywhere. The closest real id is **DeepSeek-V4.1-Flash-1017** (released 2026-10-17; API id `deepseek-flash`). 1027 is most likely a transposition of 1017 (inference). |
| Is there a DeepSeek `:free` model on OpenRouter today? | **No.** None in the full catalog scan, none in the Free Models collection page, and the `:free` page URLs 404. The "actually free, ongoing DeepSeek" option does not currently exist. |
| Does the DeepSeek API support images (Circle to Ask)? | **Yes**, on `deepseek-flash` only (`deepseek-v4-pro` does not). |
| Are DeepSeek's terms clean enough for Krovi's local-first/privacy-first bar? | **No, not clean.** Data is processed and stored in the People's Republic of China, governed by PRC law, and DeepSeek publishes no no-training or fixed-retention promise for API inputs. |
| Banned family (muse-spark)? | DeepSeek is **unrelated** to Muse Spark. But the OpenRouter catalog contains a whole `meta/muse-*` family, including one entry described as "distilled from Muse Spark", so any OpenRouter integration needs a denylist or, better, a curated allowlist. |
| Ship in v1? | (a) DeepSeek official API: **No, defer** (or ship gated only if the owner explicitly accepts the PRC-processing risk). (b) DeepSeek via OpenRouter `:free`: **No, it does not exist.** |

---

## 2. DeepSeek official API (platform.deepseek.com / api-docs.deepseek.com)

### 2.1 Signup credit / "free to start"

What the official pages say:
- Pricing page (https://api-docs.deepseek.com/quick_start/pricing): "Granted Balance and Free Credit" is only a deduction rule. Fees come out of the topped-up or granted balance, "with a preference for using the granted balance first when both balances are available." No amount, no way to obtain one, no expiry is stated.
- Balance API (https://api-docs.deepseek.com/api/get-user-balance): `GET /user/balance` returns `is_available`, and per currency (`CNY` or `USD`) `total_balance`, `granted_balance` ("The total not expired granted balance") and `topped_up_balance`. So granted balances can expire, but the page does not say when or how.
- Open Platform Terms §6.2 (https://cdn.deepseek.com/policies/en-US/deepseek-open-platform-terms-of-service.html): "free quota for paid services are subject to the information displayed on our product pages". The product pages display none.
- Not mentioned anywhere on: pricing page, quick start (https://api-docs.deepseek.com/), token usage page (https://api-docs.deepseek.com/quick_start/token_usage), the marketing page (https://www.deepseek.com/en/platform), the platform sign-in page (https://platform.deepseek.com/), the 2026-10-17 news post, or the change log (https://api-docs.deepseek.com/updates).
- The Help Center FAQ (https://static.deepseek.com/faq/index.html?lang=en) has a question titled "Does the balance expire?", but its answers did not load (titles only). The "New user bonus" answer, if it exists, is there. **(unverified)**

Conclusions:
- A $5 one-time signup credit with no card is **(unverified)**. My memory says DeepSeek granted new accounts a small balance in 2025; I could not confirm that it applies today. Treat it as not offered.
- If a grant exists it is by definition a one-time "granted balance" with an expiry (the API field says "not expired granted balance"); it is not an ongoing free tier. There is no ongoing free tier on the official API: the API is prepaid (Terms §6.1 "you may need to prepay"), and refunds are all-or-nothing on unspent balance (§6.3: "partial refunds are not supported").
- Card requirement for top-up: **(unverified)** (payment methods not on any page I could read).
- **Cheapest honest verification:** the owner signs up in a browser (about 30 seconds) and looks at the Usage/Balance page. Krovi can also surface this at runtime: `GET /user/balance` shows `granted_balance` without spending anything (see 7a).

### 2.2 Models, mappings and prices

Source: https://api-docs.deepseek.com/quick_start/pricing, https://api-docs.deepseek.com/updates, https://api-docs.deepseek.com/api/list-models, https://api-docs.deepseek.com/api/create-chat-completion

| API model id | Maps to | Notes |
|---|---|---|
| `deepseek-flash` | DeepSeek-V4.1-Flash-1017 ("It is now served by DeepSeek-V4.1-Flash-1017") | Default pick for Krovi. Vision: yes. |
| `deepseek-v4-pro` | DeepSeek-V4-Pro-0813 | Vision: "Not supported". |
| `deepseek-v4-flash`, `deepseek-v4-flash-vision-exp` | Legacy names "still accepted"; models retired; "served by the DeepSeek-V4.1-Flash-1017 model and billed at the Flash price" | Do not use. |
| `deepseek-chat`, `deepseek-reasoner` | Change log 2026-04-24: "will be discontinued in three months (2026-07-24)". Not on the pricing page, not in the chat-completions model enum | Treat as retired. Whether they still resolve is **(unverified)**. |

The chat-completions `model` enum accepts exactly: `deepseek-flash`, `deepseek-v4-pro`. The documented `GET /models` example returns those two ids with `"owned_by":"deepseek"`.

- Context: **1M tokens**. Max output: "MAXIMUM: 384K"; request `max_tokens` range is 1 to 393216, defaults 8K (non-thinking) and 64K (thinking; 128K with `reasoning_effort: max`).
- Thinking mode: "Supports both non-thinking and thinking (default) modes" (https://api-docs.deepseek.com/guides/thinking_mode). Default is **thinking on, effort high**. For plain Q&A, send `thinking: {"type":"disabled"}` to avoid reasoning tokens and latency. Reasoning tokens being billed as output is **(unverified)** (the thinking guide says nothing about billing).
- Price per 1M tokens (off-peak / peak). "Peak hours are 01:00 - 04:00 and 06:00 - 10:00 UTC, Monday through Friday, excluding Chinese public holidays"; everything else is off-peak at half price:

| | Flash (`deepseek-flash`) | Pro (`deepseek-v4-pro`) |
|---|---|---|
| Input, cache hit | $0.003 / $0.006 | $0.022 / $0.044 |
| Input, cache miss | $0.15 / $0.30 | $0.66 / $1.32 |
| Output | $0.60 / $1.20 | $1.98 / $3.96 |
| Concurrency limit (per account) | 2500 | 500 |

- Rough cost of one Circle to Ask question on Flash (my estimate): an image is capped at 1024 tokens, so about 1024 x $0.15/M + about 300 output x $0.60/M, roughly $0.0003 off-peak (about $0.0007 at peak). About 15,000 questions per $5.
- Discrepancy: the marketing page https://www.deepseek.com/en/platform shows older/different prices (Flash $0.14 miss / $0.28 out; Pro $0.435 / $0.87). The API docs page is the authoritative one (and OpenRouter's DeepSeek endpoint line, $0.15/$0.60 with weekday doubling in the same UTC windows, matches it). The marketing page is probably stale **(unverified)**.
- Search/Fetch server-side tools are a separate $0.75 per 1,000 requests. Krovi should not use them.

### 2.3 API shape

- Base URL (OpenAI format): `https://api.deepseek.com`. Anthropic-format: `https://api.deepseek.com/anthropic`. Also a Responses API is listed as supported. Use Chat Completions.
- Auth: `Authorization: Bearer <key>`. (`GET /models` returned **401** unauthenticated when fetched; the docs page omits the auth description, so send the Bearer header.)
- Endpoints: `POST /chat/completions`, `GET /models` (fields: `id`, `object`, `owned_by`; optional `context_window`, `max_output_tokens`, `input_modalities`, `output_modalities`, `api_capabilities`), `GET /user/balance`.
- Streaming: `stream: true`, SSE ending `data: [DONE]`; `stream_options.include_usage` (only valid with `stream: true`, else 400). Keep-alive: blank lines for non-streaming, `: keep-alive` comments for streaming; the server closes the connection if inference has not started within 10 minutes.
- JSON mode (https://api-docs.deepseek.com/guides/json_mode): `response_format: {"type":"json_object"}`; the prompt must contain the word "json" and an example; set `max_tokens` high enough; non-JSON output is replaced with spaces, so `content` can be blank even when `finish_reason` is `stop` (handle and retry). Works in thinking mode: not stated **(unverified)**.
- Tool calls (https://api-docs.deepseek.com/guides/tool_calls): `tools` array of `{"type":"function","function":{name,description,parameters}}`; supported in thinking mode since V3.2; `tool_choice` `required` or a named function "are not supported in thinking mode; the API returns a 400 error"; strict mode is Beta on `https://api.deepseek.com/beta`. With tools present, thinking `reasoning_content` must be passed back or the API returns 400. Krovi's v1 features do not need tool calls.
- Thinking mode ignores `temperature`, `presence_penalty`, `frequency_penalty` (no error, no effect).
- Optional `user_id` (<= 512 UTF-8 bytes). Do not put personal data in it; Krovi should omit it.
- Rate limits are **concurrency only, per account** ("regardless of which API Key is used"): https://api-docs.deepseek.com/quick_start/rate_limit. No tokens-per-minute or requests-per-day numbers are published.

### 2.4 Vision (decides Circle to Ask)

Source: https://api-docs.deepseek.com/guides/vision (not labelled beta/experimental on the page; the retired `-exp` model name was the experimental one).
- "The `deepseek-flash` model accepts images alongside text." `deepseek-v4-pro`: "Not supported" (pricing page feature table).
- Format (Chat Completions): `content` is an array: `{"type":"text","text":"..."}` and `{"type":"image_url","image_url":{"url":"data:image/png;base64,<...>"}}`. Images are allowed only in `user` messages (system/assistant images return 400). Formats JPEG, PNG, GIF, WebP, detected from file content. Optional `detail`: `low` shrinks to 512x512; `high`, `original`, `auto` keep the original.
- Limits: request body 48 MiB; single image 32 MiB (base64 or URL); up to 600 images; max 8192 px per side (4096 px per side with 15+ images); external image URLs must download within 60 s. Image cost is capped at about 1024 tokens per image, billed with text tokens.
- Output is text only. The Hugging Face card for the model lists pipeline `image-text-to-text` and claims visual benchmark gains (https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash-1017). I did not test OCR/screen-reading quality hands-on.
- **Implication: Circle to Ask is technically feasible on `deepseek-flash`, using base64 data URLs (Krovi never needs to host an image URL).** It is also the most privacy-sensitive path: screen pixels leave the device for PRC-processed servers (see section 5).

### 2.5 Error codes

Source: https://api-docs.deepseek.com/quick_start/error_codes

| HTTP | DeepSeek name | Meaning | Krovi handling |
|---|---|---|---|
| 400 | Invalid Format | Invalid request body format | Bug in Krovi's request; show generic error |
| 401 | Authentication Fails | Wrong API key | "Key not accepted. Check it on platform.deepseek.com." No retry |
| **402** | **Insufficient Balance** | You have run out of balance; top up | "No balance on this DeepSeek account. Add balance on DeepSeek's site." Never retry. This is the most likely first error for a new key |
| 422 | Invalid Parameters | Request contains invalid parameters | Bug or unsupported option; no retry |
| 429 | Rate Limit Reached | Sending too quickly, or exceeded a concurrency or QPS limit | Back off and retry (bounded) |
| 500 | Server Error | Server issue; retry after a brief wait | Bounded retry |
| 503 | Server Overloaded | High traffic; retry after a brief wait | Bounded retry |

Other: image in a non-user message returns 400 (vision guide); `stream_options` without `stream: true` returns 400; bad `reasoning_content` pass-back with tools returns 400.

---

## 3. "DeepSeek 1027": what actually exists

**Result: no checkpoint, API id, model repo or OpenRouter entry contains "1027" or "10-27". Nothing was released by DeepSeek on 2026-10-27 in any source I read.**

Searched:
- API docs: pricing page, change log (https://api-docs.deepseek.com/updates), news post https://api-docs.deepseek.com/news/news261017 (the site's News nav link points at `/news/news261017`, i.e. the newest entry is the 17 October post), models-list doc, chat-completions enum. The only dated suffix near that range is **1017**.
- Hugging Face: `https://huggingface.co/api/models?author=deepseek-ai&sort=createdAt&direction=-1&limit=40` (newest first); `...?author=deepseek-ai&search=1027` returned `[]`; `...?search=deepseek%201027&sort=createdAt&direction=-1` returned `[]`.
- OpenRouter: full catalog scan (section 4). Only `deepseek/deepseek-v4.1-flash-1017` carries a date-like suffix near it.

What does exist (real ids and dates):

| Checkpoint | Release / HF createdAt | Notes |
|---|---|---|
| **DeepSeek-V4.1-Flash-1017** | **2026-10-17** (HF createdAt 2026-10-17T02:40:38Z; news "Wish-Driven Programming Made Real") | API id `deepseek-flash`; HF `deepseek-ai/DeepSeek-V4.1-Flash-1017`; OpenRouter `deepseek/deepseek-v4.1-flash-1017`. MIT licence. HF card: "a minor update to DeepSeek-V4.1-Flash". 1M context. Vision-capable. This is almost certainly what "1027" means. |
| DeepSeek-V4.1-Flash | 2026-09-10 | previous `deepseek-flash` |
| DeepSeek-V4-Flash-Vision-Exp | change log 2026-08-21; HF createdAt 2026-08-31 | retired, routed to V4.1-Flash-1017 |
| DeepSeek-V4-Pro-0813 | 2026-08-13 | `deepseek-v4-pro` |
| DeepSeek-V4-Flash-0731 | 2026-07-31 | retired |
| DeepSeek-V4 (Pro/Flash preview) | 2026-04-24 (HF 2026-04-22) | |
| DeepSeek-V3.2 | 2025-12-01 | last of the old `deepseek-chat`/`deepseek-reasoner` line |

Note the suffix convention: DeepSeek suffixes are MMDD of release (0731, 0813, 1017), so "1027" would mean 27 October. It does not exist as of this check.

Action for the owner: if you meant a different vendor's model (or something announced after 2026-11-03), say which. Do not hard-code any "1027" id.

---

## 4. OpenRouter: is there a free DeepSeek (`:free`)?

### 4.1 Catalog check (live, 2026-11-03)

Sources: https://openrouter.ai/api/v1/models (via `https://r.jina.ai/https://openrouter.ai/api/v1/models`, 451 models), https://openrouter.ai/collections/free-models, and direct page probes.

**DeepSeek model ids in the catalog** (all paid; none ends in `:free`; none has 0/0 pricing):
- `deepseek/deepseek-v4.1-flash-1017` ("DeepSeek V4.1 Flash 1017"), text+image, 1,048,576 context, $0.15 in / $0.60 out per M on the base endpoint, with weekday UTC-window doubling like DeepSeek's own
- `~deepseek/deepseek-flash-latest` (alias to the above), `~deepseek/deepseek-pro-latest` (alias to V4 Pro 0813), `~deepseek/deepseek-v4-flash-latest` (alias to V4 Flash 0731)
- `deepseek/deepseek-v4.1-flash` (+ a `:batch` variant, not free), `deepseek/deepseek-v4-flash-vision-exp`, `deepseek/deepseek-v4-pro-0813`, `deepseek/deepseek-v4-flash-0731`, `deepseek/deepseek-v4-pro` (0423), `deepseek/deepseek-v4-flash` (0423)
- `deepseek/deepseek-v3.2`, `deepseek/deepseek-v3.2-exp`, `deepseek/deepseek-v3.1-terminus`, `deepseek/deepseek-chat-v3.1`, `deepseek/deepseek-r1-0528`, `deepseek/deepseek-chat-v3-0324`, `deepseek/deepseek-chat`

**Every `:free` id seen** (none is DeepSeek or DeepSeek-derived): `nvidia/nemotron-3-ultra-550b-a55b:free`, `nvidia/nemotron-3.5-super-vl:free`, `nvidia/nemotron-3.5-lightning:free`, `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free`, `nvidia/nemotron-3.5-content-safety:free`, `nvidia/llama-nemotron-rerank-vl-1b-v2:free`, `google/gemma-4-26b-a4b-it:free`, `google/gemma-4-31b-it:free`, `dots-studio/dots-3-note-preview:free`, `thinkingmachines/inkling:free`, `thinkingmachines/inkling-small:free`, `cohere/north-mini-code:free`, `xingchen-agi/xing4.0-29b-a4b:free`, `recursive/flint-v1.0:free`, `respan/span-01-lite:free`, `liquid/lfm-2.5-2.6b:free`. Plus the router `openrouter/free` (id ends in `/free`, not `:free`; "selects free models at random"). The list may be incomplete (summariser pass).

Direct probes, all **HTTP 404**: `openrouter.ai/deepseek/deepseek-v4.1-flash-1017:free`, `.../deepseek-v4.1-flash:free`, `.../deepseek-v4-flash:free`, `.../deepseek-v4-pro:free`, `.../deepseek-r1:free`. (The R1 `:free` ids that existed in 2025 from my memory are gone.)

**Conclusion: the owner's hypothesis is wrong today. There is no ongoing-free DeepSeek on OpenRouter.** The nearest thing is cheap paid hosting: `deepseek-v4.1-flash-1017` has 45 endpoints from $0.025 in / $0.25 out per M (Particle AI, US, flex) to $0.45 / $1.80 (https://openrouter.ai/api/v1/models/deepseek/deepseek-v4.1-flash-1017/endpoints). DeepSeek's own endpoint on OpenRouter showed only about 23% uptime in the prior 30 minutes at fetch time (snapshot, may be transient).

### 4.2 Free-model limits and requirements

Source: https://openrouter.ai/docs/api-reference/limits and https://openrouter.ai/docs/faq (both read via fetch and the `r.jina.ai` mirror).

| All-time credits purchased | Free-model requests per minute | per day |
|---|---|---|
| less than 10 | 20 | **50** |
| 10 or more | 20 | **1000** |

- The higher daily ceiling starts one credit below the threshold (the page says "currently 9 credits"). Extra accounts or keys do not help ("will not affect your rate limits").
- Check usage with `GET /api/v1/key` (`free_model_daily_requests`; the per-minute limit is not reported there).
- A user with **zero purchased credit can use `:free` models**, capped at 50 per day. Negative balance blocks even free models (402). Docs: "All new users receive a small free allowance to test out OpenRouter" (amount not stated).
- **Card needed for `:free`?** The docs never say a card is required, and describe buying credits only as a way to raise the daily limit. Whether sign-up itself needs a card is **(unverified)**. Credits are bought by card, AliPay or USDC; 5.5% fee, $0.80 minimum.
- Free models "are usually not suitable for production use."

### 4.3 Privacy and logging

Sources: https://openrouter.ai/docs/guides/privacy/logging, https://openrouter.ai/docs/guides/routing/provider-selection, https://openrouter.ai/privacy (updated Aug 31, 2026), https://openrouter.ai/docs/faq
- OpenRouter's own logging: "Prompt and completion are not logged by default." Optional prompt logging in exchange for a discount (1%, from the FAQ summary; the logging page itself did not mention the discount **(unverified on that page)**). If you opt in, Terms §6.2 grants OpenRouter a perpetual licence including selling anonymised content.
- OpenRouter's own training use: "OpenRouter does not use your Inputs or Outputs for model training." But "Some Model Providers may use your Inputs and Outputs for model training or improvement."
- Per request: `provider.data_collection` is `allow` (default: "allow providers which store user data non-transiently and may train on it") or `deny` ("use only providers which do not collect user data"). `provider.zdr: true` routes only to zero-data-retention endpoints. Both can also be set account-wide in privacy settings.
- **Free vs paid:** the logging page says "There are separate settings for paid and free models" for allowing routing to training providers. It does not explain further, so whether a user must switch a toggle on to use `:free` models is **(unverified)**. Practically, free endpoints are the ones most likely to train: an example is the Liquid `:free` card, which warns prompts "may be retained and used to train Liquid models."
- The provider retention table on the logging page lists **DeepSeek (as a provider) under "May train, unknown retention period"**, next to Liquid, NVIDIA and Thinking Machines. So routing to DeepSeek's own endpoint via OpenRouter does not avoid the training question. Use non-PRC hosts and `data_collection: "deny"` (but whether any given third-party DeepSeek host qualifies is per-host and **(unverified)**).
- Data location: OpenRouter transfers personal data to the US and elsewhere (EU in-region routing is a Business offering; `deepseek-v4.1-flash-1017` is "not currently available through EU in-region routing").

Does a `:free` endpoint require enabling logging or training? No DeepSeek `:free` exists, so for DeepSeek this is moot. In general the answer is **(unverified)** beyond the "separate settings" sentence above.

---

## 5. Terms, privacy, jurisdiction, restrictions

### 5.1 DeepSeek

Sources: Open Platform Terms (release 2026-04-22, effective 2026-04-29) https://cdn.deepseek.com/policies/en-US/deepseek-open-platform-terms-of-service.html; Terms of Use (last update 2026-03-27) https://cdn.deepseek.com/policies/en-US/deepseek-terms-of-use.html; Privacy Policy (last update 2026-02-10) https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html

- **Age:** no hard minimum. Open Platform preamble: "minors under the age of 18 shall read in the presence of a legal guardian". Terms of Use §2.1: "The Services are primarily intended for adults"; under 18 (or the local minimum age) use only with guardian consent. Privacy Policy: services "not aimed at children"; no general minimum stated. Conclusion: not as strict as Gemini or OpenRouter (both 18+), but Krovi should still require an "I am 18+" confirmation for any provider (owner decision).
- **Building apps for end users / consumer use:** allowed. §1.1 lets developers integrate the models "providing services to both internal and external end users"; §3.2: you are responsible for your application and must agree terms with and manage your end users; §3.3: tell end users your personal-information rules and get consent or another legal basis. **BYOK / user-supplied keys: not addressed (unverified legal position).** In a BYOK model the Krovi user is the account holder; Krovi is client software.
- **Key rules:** §2.2 "do not share or publicly disclose your API key with others, and do not expose it in browser or other client-side code"; the account holder is "solely responsible for any fees incurred and losses caused by sharing or leaking the API key." Krovi stores the user's own key in Credential Manager and calls from Rust; it never shares keys, which fits. Resale: not expressly mentioned.
- **Brand:** §5.2-5.3 restrict implying official cooperation ("strategic partner") and prominent use of DeepSeek's name/logo. Use the plain model name in settings; do not use the logo or say "partnered with DeepSeek". Exact reach of §5.3(2) is **(unverified)**.
- **AI disclosure:** §8.1 "You shall clearly disclose to your end users that the Output content is generated by AI". Krovi already plans an "AI-generated" label.
- **Data retention of inputs/outputs:** Open Platform Terms state no retention period. §7.2 only says records are retained for suspected illegal conduct and reported to authorities. Privacy Policy: personal data kept "as long as necessary", including input data "as long as you have an account"; no fixed periods.
- **Training use of API inputs:** **Unresolved, and the unresolved part is the risk.** The Open Platform Terms say nothing about DeepSeek training on customer inputs (§4.2 only lets *you* use inputs/outputs to train other models). The Privacy Policy lists "User Input" (text, prompts, uploaded files, photos, chat history) as collected and used "to train and improve our technology, such as our machine learning models and algorithms", with a right to "opt-out of using your Personal Data for training our models". Its Introduction carves out end-user data in downstream applications built by developers (the developer is the controller) and contains no explicit statement about API inputs. There is no zero-retention or no-training commitment. Conservative reading: assume API prompts and images may be kept and used for training. Terms of Use §4.3 (consumer chat) says opt-out is the "Improve the model for everyone" toggle, which has no API equivalent that I found.
- **Jurisdiction and data location:** Terms §10.1 "governed by the laws of the People's Republic of China in the mainland"; §10.2 courts at the registered office of Hangzhou DeepSeek Artificial Intelligence Co., Ltd. Privacy Policy: "we directly collect, process and store your Personal Data in People's Republic of China." Controller: Hangzhou DeepSeek Artificial Intelligence Co., Ltd.; privacy@deepseek.com; EU/UK representative via Prighter (rep_deepseek@prighter.com). EEA/UK rights are listed (access, erasure, objection, complaints to a local DPA) but cross-border transfer mechanisms are not named ("appropriate safeguards" where required).
- **Region/sanctions:** §1.4 no guarantee the service is available in any jurisdiction; Export Control and Sanctions section bars use in or for comprehensively sanctioned countries and restricted parties; "Inputs shall not include material or information that requires a license for release or export."
- **Refund/balance:** §6.3 refund of remaining unspent balance only, less handling fees; no partial refunds. Balance expiry not addressed in the Terms (FAQ question exists, answer unread).

### 5.2 OpenRouter

Source: https://openrouter.ai/terms (last updated Aug 31, 2026), https://openrouter.ai/privacy
- Minimum age: "You must be at least 18 years of age to use the Service."
- Apps built on OpenRouter: §5.1-5.2 extend access to "your customers (to the extent you incorporate the Service into your own products and services)", and you must flow terms down and answer for your Authorized Users. §7.4 bans reselling API access. For Krovi this does not bite as designed: each user supplies *their own* OpenRouter account key and pays OpenRouter directly; Krovi is a client, with no pooled key and no resale. (Optional attribution headers `HTTP-Referer` and `X-OpenRouter-Title` exist; Krovi should omit them to avoid sending an app identifier **(owner choice)**.)
- OpenRouter's "BYOK" feature (store your own *provider* keys in OpenRouter, 5% fee) is a different thing from a user pasting an OpenRouter key into Krovi. Terms do not discuss third-party client apps directly **(unverified position)**.
- Training/retention: Terms §6.1 "Some Models may store or train on your Inputs"; "Where possible, OpenRouter has opted out of model training with the Models it uses." No free-model-specific provisions in the Terms. Restricted-model access via VPN/proxy is a material breach (§5.7). Governing law: New York; binding AAA arbitration, class-action waiver.
- Errors (https://openrouter.ai/docs/api-reference/errors): 400 bad request, 401 invalid credentials, **402 insufficient credits** (also for free models if balance is negative), 403 forbidden/guardrail/moderation, 408 timeout, **429 rate limited**, 502 model down or invalid response, 503 no provider meets routing requirements. Once a model starts processing, errors can come back as HTTP 200 with the error in the body or an SSE event.

### 5.3 Government and regulatory restrictions (factual; scope noted)

Sources: https://en.wikipedia.org/wiki/DeepSeek, https://en.wikipedia.org/wiki/DeepSeek_(chatbot) (Wikipedia pages as of late October 2026; they cite Reuters/NYT; primary sources not individually opened)
- **United States:** National Defense Authorization Act for FY2026 directs removal of DeepSeek AI from Department of Defense and Intelligence Community systems and contractors (**official devices only**). A DHS order banning DeepSeek from civilian federal agencies' IT systems was published **2026-10-20** (official use only; China objected). Earlier: Commerce Department bureaus (March 2025), US Navy (January 2025), Texas state devices (about 31 January to 2 February 2025). No US consumer or API ban found.
- **EU:** Italy's data protection authority ordered DeepSeek's **chatbot blocked in Italy on 30 January 2025** (a consumer-facing block); Italy's competition authority opened a probe in June 2025. The Dutch DPA opened an investigation (January 2025). Czech Republic banned DeepSeek products in state administration (July 2025). The Wikipedia pages I read did not mention Germany. From memory, Germany's Berlin data protection commissioner asked Apple and Google to delist the app in June 2025 **(unverified; do not publish)**. No EU-wide ban found. GDPR exposure for a PRC-hosted processor (Privacy Policy names no transfer mechanism) is a legal question for the owner.
- **India:** nothing on the pages I could read. From memory, a Finance Ministry advisory told staff not to use external AI tools such as ChatGPT and DeepSeek on office devices (early 2025) **(unverified; official-device scope if true)**. No consumer or API restriction known.
- **Others (official devices unless noted):** South Korea (new app downloads suspended February 2025; data found sent to ByteDance; also government devices; whether the suspension was lifted is **(unverified)**), Taiwan, Australia (4 February 2025), Canada.
- **Scope summary:** nearly all actions target government devices. The consumer-level actions are Italy's block and South Korea's download suspension, both about the **DeepSeek chat app**, not API calls with a user's own key. No law found forbids an individual from calling the API. Still, an employee using Krovi on a work PC may violate their employer's or government's rules.
- Context only: Wikipedia also reports a February 2026 Anthropic accusation that DeepSeek used fraudulent accounts to gather Claude conversations. It is an allegation and has no bearing on the Muse Spark check.

---

## 6. Banned-family check (muse-spark)

`AGENTS.md` bans `muse-spark-1.2` and `muse-spark-1.3`; the owner confirmed the whole muse-spark family is banned.

- **DeepSeek is unrelated to Muse Spark.** DeepSeek models come from Hangzhou DeepSeek Artificial Intelligence Co. (parent High-Flyer) and are an independent family: V4 Base and instruct models published 2026-04-22, then V4-Flash-0731 ("re-post-trained revision" per OpenRouter), V4-Pro-0813, V4.1-Flash (09-10), V4.1-Flash-1017 ("a minor update to DeepSeek-V4.1-Flash", base model link: https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash). MIT-licensed weights. Nothing in the API docs, the Hugging Face card or any OpenRouter DeepSeek entry mentions Muse Spark or Meta. The HF org's recent `eagle3_*`, `dflash_*`, `dspark_*` items target Gemma/Qwen drafters, not Muse. Absence of evidence is all a docs review can give; the HF card does not describe training provenance in detail.
- **Third-party OpenRouter entries built on DeepSeek** (do not list them in Krovi; none is built on Muse): `particle-ai/particle-1` ("post-trained on DeepSeek V4.1 Flash"), `aion-labs/aion-3.0-mini` ("built on the DeepSeek family"), `aion-labs/aion-2.0` ("a variant of DeepSeek V3.2"), `perplexity/sonar-reasoning-pro` ("powered by DeepSeek R1"). Lineage text is from the catalog descriptions; the model pages themselves did not repeat it.
- **Banned-family entries present in the OpenRouter catalog (must never be reachable):** `meta/muse-spark-1.1`, `meta/muse-spark-1.2`, `meta/muse-spark-1.3`, `meta/muse-spark-1.2-contributor`, `meta/muse-spark-1.3-contributor`, `meta/muse-spark-open`, `meta/muse-wonder`, and `meta/muse-glimmer-30b` (catalog description: "distilled from Muse Spark", i.e. a derivative of the banned base). `Meta` is also listed in OpenRouter's retention table as "30 days, no training", but that does not change the AGENTS.md ban.
- **Router hazard:** `openrouter/auto`, `openrouter/pareto-code`, `openrouter/bodybuilder` (pricing -1, "depends on the model the router picks") and `openrouter/free` choose models for you. Krovi must not offer or send any of those ids: it cannot guarantee a banned-family model is never selected. Whether Auto Router can pick a Muse model is **(unverified)**.
- **Rule for any OpenRouter integration:** use a curated allowlist of exact ids (for example `deepseek/deepseek-v4.1-flash-1017`), never populate a picker from the live catalog without a denylist matching `/muse-/i` and `meta/muse*`.

---

## 7. Verdicts

### (a) DeepSeek official API

- **Ship in v1: No. Defer to v1.1 or later.** It does not meet the "terms actually confirmed clean" bar the owner set: PRC processing and PRC law, no published no-training or retention commitment for API inputs, and the headline trial-credit claim is unconfirmed. Technically it is easy (OpenAI-compatible, cheap, supports images). If the owner overrides and accepts the PRC-processing risk, ship it **gated** (mandatory acknowledgement, below), text first, image questions behind a second per-feature acknowledgement.
- **Honest UI label:** "Pay as you go. You add balance on DeepSeek's site. Krovi receives nothing." Do **not** write "free to start, paid after trial credit" or "$5 free credit" unless the owner confirms on a real account; if `GET /user/balance` shows `granted_balance > 0`, Krovi may show "Trial balance on your DeepSeek account: $X" as a fact from the API, never as a promise. Never call it "free".
- **Required disclosures (draft copy, shown when adding a DeepSeek key; user must tick to continue):**
  - "DeepSeek is run by Hangzhou DeepSeek Artificial Intelligence Co., Ltd. in China. Everything you send (your text, and for Circle to Ask the selected screen area as an image) goes from this PC straight to DeepSeek's servers. DeepSeek's privacy policy says it stores data in the People's Republic of China, and its terms are governed by Chinese law."
  - "DeepSeek does not promise to avoid keeping or training on what you send through its API. Assume it may keep it and use it to improve its models. Do not send sensitive, confidential, work or personal content."
  - "Krovi does not see, store or proxy your requests. Your key stays in Windows Credential Manager. DeepSeek bills you directly."
  - "Some governments and employers restrict DeepSeek on official or work devices. Check your rules."
  - "I am 18 or older and accept DeepSeek's terms."
  - On every answer: "AI-generated, may be wrong" (also required by DeepSeek Open Platform Terms §8.1).
  - For Circle to Ask: show the exact region preview and confirm before each send (already a cross-provider rule in `AI_PROVIDERS.md`).
- **"Test connection" approach (two steps, separate statuses):**
  1. Authentication and balance: `GET https://api.deepseek.com/user/balance` with the Bearer header: 401 = bad key; 200 shows `is_available`, `granted_balance`, `topped_up_balance` at no cost. (Auth requirement for this endpoint is not stated on its docs page, so send the Bearer header; **(unverified)**.) Optionally `GET /models` to confirm `deepseek-flash` is listed.
  2. Inference probe: `POST /chat/completions` with `{"model":"deepseek-flash","messages":[{"role":"user","content":"hi"}],"max_tokens":1,"stream":false,"thinking":{"type":"disabled"}}`. A 402 here means "key valid, no balance". A 429 is rate/concurrency (retry once, bounded). Never retry 401 or 402.
  3. Vision probe only on first Circle to Ask use (tiny PNG data URL), so text-only users are not charged for it.
- **Residual risk:** (1) PRC jurisdiction and unknown retention/training (cannot be eliminated by Krovi); (2) screen content sent off-device to the PRC; (3) regulatory/political drift (US DHS order published 2026-10-20; further actions plausible); (4) model ids have churned fast (`deepseek-chat`/`deepseek-reasoner` retired within about three months of announcement; `deepseek-flash` re-points to a new checkpoint without notice), so read `/models` and keep one default constant; (5) thinking is on by default, which affects cost and latency; (6) the free-credit story could be wrong in either direction; (7) payment-method requirements for top-up are unverified.

### (b) DeepSeek via OpenRouter `:free`

- **Ship in v1: No. The thing does not exist** (no DeepSeek `:free` or zero-priced DeepSeek model in the live catalog). Do not add a "free DeepSeek" option or claim one.
- What does exist: OpenRouter's free pool of non-DeepSeek models (NVIDIA Nemotron, Gemma 4, others), rate-limited to 20 requests/min and 50/day (1000/day after $10 of purchased credits), usable with zero purchased credit. That is a separate provider decision (OpenRouter is "Later" in `AI_PROVIDERS.md`).
- **Honest UI label if OpenRouter is ever added:** "Free models: rate-limited (50 per day; 1,000 per day if you've bought $10 of credit). Some free models may log or train on your prompts. Paid models, including DeepSeek, use your OpenRouter credits." For paid DeepSeek via OpenRouter: "Paid, via OpenRouter credits."
- **Required disclosures (draft):**
  - "Requests go from this PC to OpenRouter, which forwards them to a hosting provider. Which provider, and whether it keeps or trains on your data, varies by provider and model."
  - "Free models are the most likely to keep or train on prompts. Don't send anything sensitive."
  - "DeepSeek models on OpenRouter may be served by DeepSeek itself, whose servers and law are in China, or by other hosts. Krovi asks OpenRouter for providers that do not collect data (`data_collection: deny`), which can limit which hosts are available."
  - "You must be 18+ to use OpenRouter and accept its terms."
- **"Test connection":** `GET https://openrouter.ai/api/v1/key` (documented in the limits page; reports `is_free_tier` and `free_model_daily_requests`) for key validity and limits; then a 1-token probe on an allowlisted model id with `provider: {"data_collection": "deny"}`. Map: 401 bad key, 402 no credits (also hits free models if balance is negative), 429 rate or daily cap (do not retry daily-cap), 502/503 provider down or no provider satisfies the privacy filter (retry-bounded, suggest relaxing nothing automatically). Never use `openrouter/auto`, `openrouter/free`, `openrouter/pareto-code`, `openrouter/bodybuilder`.
- **Residual risk:** model lists and `:free` entries rotate weekly (all 16 current free ids are non-DeepSeek and some are previews with expiry dates, for example `dots-3-note-preview:free` has an expiration of 2026-12-31); downstream provider retention/training is per-host; OpenRouter's catalog contains Muse Spark and a Muse-derived model that must be filtered; the DeepSeek-as-provider route is marked "May train, unknown retention period"; account-level free/paid training toggles are not documented in enough detail to say what a user must enable **(unverified)**.

### Checklist before anything is added to Settings -> AI

- [ ] Owner checks a real DeepSeek account: signup credit amount, expiry, card requirement for top-up. Update section 2.1.
- [ ] Owner accepts or declines PRC processing (decision to log in `DECISIONS.md`).
- [ ] If accepted: acknowledgement UI, 18+ checkbox, AI-generated label, region preview for images.
- [ ] Run a deterministic `jq` over `https://openrouter.ai/api/v1/models` for `deepseek` ids and `:free` suffix before relying on section 4.1.
- [ ] Website copy updated only after the integration ships (project rule: every Krovi feature change also updates the website; the maintainer runs the production deploy).

---

## 8. Source list

DeepSeek
- https://api-docs.deepseek.com/ (quick start, base URLs, auth)
- https://api-docs.deepseek.com/quick_start/pricing (models, mappings, pricing, peak hours, granted balance rule)
- https://api-docs.deepseek.com/quick_start/error_codes
- https://api-docs.deepseek.com/quick_start/rate_limit
- https://api-docs.deepseek.com/quick_start/token_usage
- https://api-docs.deepseek.com/updates (change log)
- https://api-docs.deepseek.com/news/news261017 (V4.1-Flash-1017 release, 2026-10-17)
- https://api-docs.deepseek.com/api/list-models, https://api-docs.deepseek.com/api/create-chat-completion, https://api-docs.deepseek.com/api/get-user-balance
- https://api-docs.deepseek.com/guides/vision, /guides/json_mode, /guides/tool_calls, /guides/thinking_mode
- https://api-docs.deepseek.com/faq -> https://static.deepseek.com/faq/index.html?lang=en (titles only)
- https://www.deepseek.com/en/platform (marketing page; stale prices)
- https://platform.deepseek.com/ (sign-in page)
- https://cdn.deepseek.com/policies/en-US/deepseek-open-platform-terms-of-service.html
- https://cdn.deepseek.com/policies/en-US/deepseek-terms-of-use.html
- https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html
- https://huggingface.co/api/models?author=deepseek-ai&sort=createdAt&direction=-1&limit=40
- https://huggingface.co/api/models?author=deepseek-ai&search=1027 (empty), https://huggingface.co/api/models?search=deepseek%201027&sort=createdAt&direction=-1&limit=20 (empty)
- https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash-1017, https://huggingface.co/api/models/deepseek-ai/DeepSeek-V4.1-Flash-1017

OpenRouter
- https://r.jina.ai/https://openrouter.ai/api/v1/models (451-model catalog, chunked read)
- https://openrouter.ai/collections/free-models
- https://openrouter.ai/api/v1/models/deepseek/deepseek-v4.1-flash-1017/endpoints
- https://openrouter.ai/deepseek/deepseek-v4.1-flash-1017 (and the five `:free` 404 probes in 4.1), https://openrouter.ai/particle-ai/particle-1, https://openrouter.ai/meta/muse-glimmer-30b
- https://openrouter.ai/docs/api-reference/limits, /docs/faq, /docs/api-reference/errors, /docs/api-reference/overview
- https://openrouter.ai/docs/guides/privacy/logging, /docs/guides/routing/provider-selection, /docs/guides/overview/auth/byok
- https://openrouter.ai/terms, https://openrouter.ai/privacy

Government actions (secondary)
- https://en.wikipedia.org/wiki/DeepSeek, https://en.wikipedia.org/wiki/DeepSeek_(chatbot)
