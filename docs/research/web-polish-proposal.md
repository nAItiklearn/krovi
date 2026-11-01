# Website polish proposal: palette and type, the drifting tool-chain, inline clips

_Owner: ui-polish. 2026-11-01. SPEC §11.11. Pattern study only: nothing of VoiceOS's code, copy, assets or colours is
used. Reference captures are in `qa/polish/reference/` for internal study and are not published._

## 0. What was studied (and what is deliberately not copied)

VoiceOS.com, read in my own browser tab (DOM, computed styles, the network list), not guessed:

| Pattern | What they do | What Krovi takes (the idea only) |
|---|---|---|
| "Not just another dictation app" | A pale-blue squircle card (radius up to 180 px). 24 app icons ride a shared SVG `offset-path` that sweeps diagonally up and right, crossing a bar chart whose last bar is a photo-sky highlight ("10×"). Each icon is a `.aw-slot` with the same `offset-distance` keyframe (24 s, linear, infinite) and a **negative `animation-delay` of i x 24/N seconds**, so the chain is continuous with no wrap point. A linear-gradient `mask-image` fades both ends. `animation-play-state` is toggled by an IntersectionObserver, paused under `prefers-reduced-motion`, hidden at <= 640 px. | A diagonal chain of Krovi's own tool tiles drifting into an honest numbers card. Different technique (transform track, below), different content, different colours. |
| Palette | White page, pale-sky card gradient (#dceafa to #fbfdff), navy ink (#171c33), slate body (#6b7188), blue accent, photographic skies, a night-sky privacy panel. | Warm paper and ink with Krovi's ember accent; theme colours as object colours. No blue, no photography. |
| Type | System SF stack (Segoe UI on Windows), light 400 headlines at 60 px with -0.025em tracking mixed with 600; uppercase 12 px eyebrows at 0.14em; DM Mono in the footer; inline emoji and app glyphs inside headlines. | A variable grotesk for display and a calm UI sans for body, both self-hosted. |
| Shape | Mega-radius cards, fully round pill buttons with inset "liquid glass" highlights, floating glass nav pill. | A named radius language (below). |
| Clips | `<video muted loop playsinline preload="none" poster="x.webp">`, 1920x1080 MP4, 12 s, **1.45-2.5 MB each**, played by script when on screen, framed in rounded 16:9 cards with a soft shadow and a mute glyph. | Same mechanics, smaller files (largest ours: 1.0 MB), WebM first, a visible pause button, honest captions. |

## (a) Palette, type, spacing, radius, shadow

### Palette: "Paper, Ink, Ember, and six object colours"

All tokens live in `web/styles.css`; light and dark use `light-dark()` and the existing `data-theme` override.

| Role | Light | Dark | Notes |
|---|---|---|---|
| `--bg` / `--bg-alt` | #fbfaf8 / #f4f2ee | #060607 / #0d0d0f | Warm paper instead of cool white. Dark unchanged. |
| `--surface`, `-2`, `-3` | #fff / #f6f4f0 / #ece9e4 | #141416 / #1b1b1e / #232327 | |
| `--text`, `-2`, `-3` | #1d1d1f / #424245 / #6e6e73 | #f5f5f7 / #c6c6cb / #8e8e95 | Unchanged, so contrast is the audited one. |
| Accent "ember" | #c8440a (text #b23a07) | #ff8a4c | Unchanged: links, focus, live signals, the callout glow. |
| `--wash-1..3` | #efeae0, #f6f3ec, #fbfaf8 | #1a171b, #111013, #0a0a0c | Soft gradient behind big feature cards. |
| Object colours `--sp-<theme>-a/b/ink` | the six app themes, as tile surfaces: Graphite #2b2d35 to #0b0c0f / ink #f4f5f7 · Porcelain #fff to #e6e8ee / #12141c · Mochi #fcecf7 to #ebd0f0 / #5a2148 · Arcade #2a2270 to #0e0c26 / #ff7be3 · Terracotta #f1dfc2 to #dbbd92 / #4a5314 · Tide #28466a to #152336 / #6ff1cf | same | Objects keep their colour in both modes, like app icons. They tie the page to the six real themes (`src/lib/theme.ts` swatches). |

The identity is monochrome plus **one** accent plus the theme spectrum, used only on tool tiles and the theme chips.
Nothing is blue-first; the nearest cool colours (Tide, Graphite) appear only as 1 of 6 tile colours.

### Type: self-hosted, no CDN, SIL OFL 1.1

| Use | Family | Why | File (latin subset, variable woff2) | Licence |
|---|---|---|---|---|
| Headlines, big numerals | **Bricolage Grotesque** (wght 200-800, **opsz** 12-96 auto) | Confident, slightly warm grotesk; the optical-size axis tightens it at display sizes, which gives the hero and the "15" a distinct voice. | `bricolage-grotesque-latin-opsz.woff2`, 75 KB | SIL OFL 1.1, (c) 2022 The Bricolage Grotesque Project Authors |
| Body and UI | **Geist** (wght 100-900) | Calm, neutral, tuned for UI, pairs without fighting the display face. | `geist-latin-wght.woff2`, 29 KB | SIL OFL 1.1, (c) 2024 The Geist Project Authors |
| kbd, labels, code | **Geist Mono** (wght 100-900) | Same family feel. | `geist-mono-latin-wght.woff2`, 23 KB (not preloaded) | SIL OFL 1.1 |

Files and licence texts are in `web/fonts/` (`OFL-*.txt`). Only the two text faces are preloaded
(104 KB); `font-display: swap` plus metric-adjusted local fallbacks (`size-adjust`, `ascent-override`) limit layout shift.
`font-src 'self'` already covers it: **zero third-party requests**, so the on-page counter stays at 0.

**Scale** (tokens `--t-*`): 12 / 14 / 16 px, lede `clamp(1.08rem, .95rem + .6vw, 1.36rem)`, h3 `clamp(1.28rem, 1.1rem + .8vw, 1.7rem)`,
h2 `clamp(1.9rem, 1.1rem + 3.2vw, 3.3rem)`, h1 `clamp(2.5rem, 1.2rem + 5.6vw, 5rem)`, stat numerals `clamp(4.5rem, 2.2rem + 9vw, 9rem)`.
Headlines: weight 650-700, tracking -0.03 to -0.06em, line height 1.02-1.1. Eyebrows: 12.8 px uppercase, 0.06em.

**Spacing** (`--s-1..10`): 4, 8, 12, 16, 24, 32, 48, 64, 96, 128 px. Section padding `clamp(72px, 10vw, 136px)`.

**Radius language:** `--r-pill` 999 (buttons, chips, nav, the pill) · `--r-tile` 28% (tool tiles, scales with size; 38% with `corner-shape: squircle`) ·
`--r-s/m/l/xl` 10 / 16 / 24 / 32 (controls, cards, clips) · `--r-mega` `clamp(32px, 6vw, 72px)` (the one big feature card per page).
Rule: shapes nest, the inner radius is always smaller than its container's.

**Shadow language:** three elevations (`--shadow-s/m/l`, unchanged) plus object shadows: `--shadow-tile` (hairline + top highlight +
contact shadow + long soft shadow), `--shadow-clip` (1 px hairline + soft lift), `--glow-ember` (the callout only).

## (b) The drifting tool-chain

**Where:** a new section `#shelf` directly under the stats row, before "Live, not stale".
**Content, honest by construction:** fifteen tiles carrying the site's own tool glyphs, in the order of the app's tool rail, coloured from the six
themes. The callout reads **"Fifteen tools. Zero tabs."** and **15 tools behind one hotkey**, with two more verifiable lines (1 hotkey,
0 accounts/ads/telemetry; 0 third-party requests on this page, checked live in the Privacy section). The count is from `src/lib/tools.ts`:
17 rail entries minus Home and Settings = **15** (it was 14 before Explain landed; the original site's "15" counted Home and Settings and
predated Quick Transfer). Re-count from `tools.ts` whenever a tool is added.

**Technique: duplicated track + transform (not `offset-path`).**

1. Three lanes (back, mid, front) on one plane rotated -12 degrees (-7 on phones). Each lane is a flex row `.track { width: max-content }` holding the tile set **twice**
   (JS clones the authored first set, `aria-hidden`; the mid and back lanes are built from the front lane in a rotated order).
2. `@keyframes chain-drift { from { transform: translate3d(-50%,0,0) } to { transform: translate3d(0,0,0) } }`, linear, infinite. The track's
   own width is exactly two sets, so **-50% equals one set to the pixel**: the last frame equals the first, there is no marquee seam. Each tile
   carries its gap as a right margin (including the last), the usual cause of a one-gap hiccup at the wrap.
3. Parallax by depth: tile size 54 / 72 / 94 px, durations 110 / 72 / 50 s, opacity .55 / .85 / 1. Plus a scroll-linked drift of the whole plane
   (`animation-timeline: view()`, +/-28 px) as a progressive enhancement under `@supports`.
4. The lanes run behind the numbers card (z-index) and a `mask-image` fades them out around it, so tiles appear to slip into the callout, which
   carries an ember radial glow at its lower-left "intake".
5. Counter-rotated glyphs (`rotate(12deg)`) keep icons upright on the slope.

**Why not `offset-path`** (what VoiceOS does): `offset-distance` animations run on the main thread in Chromium; `transform` runs on the compositor,
so it stays smooth while the page is busy and costs no layout or paint. A rotated plane gives the same diagonal for free and avoids generating an SVG
path per viewport width in JS.

**Performance and safety**
- 3 x 30 = 90 small elements (15 authored, the rest cloned), `will-change: transform` on 3 tracks only, `contain: layout paint` on the card.
- An IntersectionObserver adds `.is-on`; off-screen or hidden tabs pause via `animation-play-state` (no JS in the loop).
- `prefers-reduced-motion: reduce`: no cloning, no animation, a static diagonal pose of one set.
- No JS: the front lane is still in the HTML as a static row.
- A visually-hidden `<ul>` lists the 15 tools for assistive tech; the lanes are `aria-hidden`.
- Phones keep two smaller lanes (the back lane is dropped) under the callout instead of hiding the whole thing.
- The strict CSP is respected: no inline styles, all motion in `home.css`, `home.js` sets only classes.

## (c) Inline-clip plan

**Format:** WebM (VP9) first, MP4 (H.264) fallback, muted, no audio track, looped, 1280x720, 30 fps; budget **<= 1.5 MB each** (the encoder
steps CRF until it fits). Posters are WebP (3-25 KB). Markup: `<video muted loop playsinline preload="none" poster width height>` with a fixed
`aspect-ratio` frame (no layout shift). Playback by IntersectionObserver (>= 30% visible), paused when scrolled away or the tab is hidden.
**Reduced motion or Data Saver: the poster only, nothing downloads.** A pause button (appears on hover/focus, always on touch) covers WCAG 2.2.2.
CSP gained `media-src 'self'`; `/media/*` is cached for a day with revalidation, `/fonts/*` immutable.

**How they are recorded** (re-runnable, `qa/polish/clips/`): `record-lib.js` is injected into the browser demo (`http://localhost:5173`, or
`npx vite --port 5191`); each clip is **one scripted call** (a visible fake cursor, clicks by aria-label, magenta sync flashes at both ends) wrapped by
chrome-devtools `screencast_start`/`screencast_stop`; `encode-clips.ps1` finds the two flashes, trims between them, and writes MP4, WebM and poster.

| # | Clip | Recorded now | Size (mp4 / webm) | Goes where |
|---|---|---|---|---|
| 1 | Pill springs open, Home, folds back | `01-pill-expand` 8.8 s | 309 / 233 KB | Hero, small screens only (replaces the heavy live iframe) |
| 2 | Live clipboard: a new copy slides in | `02-clipboard-live` 22 s | 545 / 503 KB | "Live, not stale" |
| 3 | Dictated voice note | `03-voice-note` 13.5 s | 458 / 353 KB | Voice |
| 4 | Theme switch through all six | `04-theme-switch` 20.6 s | 1037 / 843 KB | "Make it yours" (new section) |
| 5 | Idle eyes, then wake | `05-idle-eyes` 13.9 s, cropped to the pill strip | 96 / 64 KB | "Make it yours", beside the theme clip |
| 6 | **Quick Transfer** (drop, QR, phone downloads) | **not recorded**: `Transfer.svelte` was still a placeholder when I recorded | - | Tool explorer panel and the docs page |

**All six are first drafts and must be re-recorded once the final UI lands:** the demo's data is simulated, the theme at record time was
whatever the demo had stored (record on Graphite for consistency), clip 2 includes a ~12 s wait for the demo's simulated copy (trim to ~10 s by
seeding one), and the idle clip should be re-shot at 2x device scale. Captions on the site say "recorded from the app's browser demo, with simulated
data", which stays true.

## Status (updated after the content pass)

Built and verified: `powershell -File scripts\build-web.ps1` (full build, demo included) passes `check` (6 pages, 16 external links). Clips are in
`web/media/`. Lighthouse (accessibility, best practices, SEO, agentic browsing) is 100 on `/` and `/docs`, desktop and mobile
(`qa/web/lighthouse-after2/`). Trace on `/`: CLS 0.00; LCP 1.9 s locally, the LCP element is the hero lede, held back by the existing
`.reveal` fade-in, not by fonts or media. After-screenshots: `qa/polish/after2/`. Still open: re-record the six clips when the UI is final
(Quick Transfer was never recorded), and re-check the chain on 900-1100 px windows.

## Phase 2: what changed on the site

`index.html` (new `#shelf` chain section; 14-tool count everywhere; Quick Transfer tab, panel and palette entry; voice section rewritten for what
is real, with "In v0.2" marks; new "Make it yours" section; privacy ledger hosts corrected to `api.frankfurter.dev/v1/latest`, plus the city-search
host and the LAN-only Quick Transfer row; clips in hero (narrow), Live, Voice, Personal), `styles.css` (fonts, type, spacing, radius, shadow and
object-colour tokens), `home.css` (chain, callout, clips, personal), `home.js` (`initChain`, `initClips`), `vercel.json` (`media-src`, cache
headers), `docs.html` (14 tools, Quick Transfer entry, holidays IN/US/GB), `changelog.html` (v0.2 list made accurate), `compare.html` (holiday
rows and caveat), `scripts/build-web.header.html` (`i-transfer` sprite symbol), `web/fonts/` (3 woff2 + 3 OFL texts).
