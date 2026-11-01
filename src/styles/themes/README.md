# Krovi themes

A theme is **one CSS file of token overrides plus one registry line**. Nothing else.

```
src/styles/themes/
  _base.css        shared plumbing (colour-token registration + the cross-fade). Never edit per theme.
  graphite.css     default dark
  porcelain.css    default light
  mochi.css  arcade.css  terracotta.css  tide.css
  contrast.ts      WCAG maths + the audit the tests run on every theme
  themes.test.ts   registry <-> css integrity, token coverage, WCAG AA
src/lib/theme.ts   THEMES registry, themeChoice / resolvedTheme stores, startTheme()
```

## Add a theme in two steps

1. **Copy** `graphite.css` (dark) or `porcelain.css` (light) to `src/styles/themes/<id>.css`,
   rename the two selectors, change the values. Keep both selector lines:

   ```css
   :root[data-theme='<id>'],
   [data-theme-scope='<id>'] { … }          /* second selector lets the preview show all themes at once */
   :root[data-env='tauri'][data-theme='<id>'] { --panel-bg: …; --pill-bg: … }
   ```

2. **Register** it in `THEMES` in `src/lib/theme.ts`:

   ```ts
   { id: '<id>', name: 'Display name', scheme: 'dark', swatch: ['#bg', '#fg', '#accent'], blurb: 'one line' },
   ```

   `swatch[2]` must equal the file's `--accent` (a test checks). The CSS file is picked up
   automatically by the glob in `theme.ts`; the Settings picker is built from `THEMES`.

Run `npx vitest run src/styles/themes`. It fails with the exact token or pair that is wrong.

## The vocabulary (a theme defines ALL of these)

| group | tokens |
|---|---|
| scheme | `color-scheme: dark \| light` (must match the registry `scheme`) |
| surfaces | `--page` `--panel-bg` `--pop-bg` `--pill-bg` |
| pill type | `--pill-fg` `--pill-fg-2` |
| layers | `--surface-1` `--surface-2` `--surface-3` (translucent overlays on the panel) |
| hairlines | `--line` `--line-strong` |
| text | `--text` `--text-2` `--text-3` |
| accent | `--accent` `--accent-ink` (text on accent) `--accent-soft` `--accent-glow` `--danger` |
| primary button | `--invert-bg` `--invert-fg` |
| glass | `--rim-a` `--rim-b` `--grain-opacity` `--card-hi` `--spot` |
| shadow | `--shadow-pill` `--shadow-panel` `--shadow-pop` (tint them: black on dark, a hue on light) |
| idle companion | `--eye-fill` (eye body) `--eye-fill-2` (dim track / off cells) `--eye-accent` (glint, pupil) `--eye-glow` (`transparent` = none) |
| optional | `--eye-blush` (cheeks when the eyes squint; falls back to `transparent`) |

Anything outside this list is rejected by the tests, so a theme can never depend on a
variable that other themes do not define. If the UI needs a **new** token, add it to
`tokens.css` (design), `REQUIRED_TOKENS` and `_base.css` here, and to every theme.

## Rules the tests enforce

- **WCAG AA** (4.5:1) for `--text` / `--text-2` / `--text-3` on the panel, on each of the three
  surface layers, and on popovers; `--pill-fg(-2)` and `--eye-fill` on the pill; `--accent`,
  `--danger` and `--accent` on `--accent-soft`; `--accent-ink` on `--accent`; `--invert-fg` on
  `--invert-bg`. Translucent colours are composited like the browser does. Checked twice:
  over the theme's own `--page` (browser demo) and for the more opaque native-window glass over
  a pure black **and** a pure white wallpaper.
- The native-window block overrides **only** `--panel-bg` and `--pill-bg`.
- Colour tokens must be plain colours (hex / rgb() / rgba() / transparent) so they can be
  audited and cross-faded.

## How the switch works (and why it is cheap)

- `startTheme()` sets `data-theme=<id>` and `data-scheme=dark|light` on `<html>`. **Code that
  needs light-vs-dark must key off `data-scheme`, never a theme id.**
- Every colour token is registered with `@property` in `_base.css`, so a theme change is a
  single transition on `:root` and every `var(--token)` follows. No per-element transition
  rules, no layout, and it never collides with a component's own transitions. It is armed
  ~150 ms after launch so the app does not fade in from the default palette.
- The OS dark/light switch is followed live while the choice is `system` (Graphite / Porcelain).
- Persisted in `localStorage['krovi.themeChoice']` (try/catch; falls back to `system`). The old
  `krovi.theme` (`light|dark|auto`) is migrated once.

## Optional: no flash on launch

`tokens.css` already follows the OS via `prefers-color-scheme` until `startTheme()` runs, which
is enough for `system`. To also avoid a one-frame flash for an explicit choice, call
`startTheme()` first thing in `main.ts` (before `mount`).
