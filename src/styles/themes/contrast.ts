// WCAG contrast tooling for the theme token files. Pure functions: no DOM, no fs, so the same
// code runs in the unit tests (themes.test.ts) and can be pointed at a new theme file by hand.
//
//   parseThemeCss(css)   -> the token map of one theme file (base block + native-window block)
//   auditTheme(tokens)   -> every text/graphic pair we promise to keep readable, with its ratio
//
// Translucent tokens are composited exactly as the browser does (source-over), so a "text on
// glass" ratio is measured against what is really behind the text, not against the raw token.

export interface RGBA {
  r: number;
  g: number;
  b: number;
  a: number;
}

const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));

/** `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()/rgba()` (comma or space syntax) and `transparent`. */
export function parseColor(input: string): RGBA | null {
  const s = input.trim().toLowerCase();
  if (s === 'transparent') return { r: 0, g: 0, b: 0, a: 0 };
  if (s === 'white') return { r: 255, g: 255, b: 255, a: 1 };
  if (s === 'black') return { r: 0, g: 0, b: 0, a: 1 };

  const hex = /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/.exec(s);
  if (hex) {
    let h = hex[1];
    if (h.length <= 4) h = [...h].map((c) => c + c).join('');
    const n = (i: number) => parseInt(h.slice(i, i + 2), 16);
    return { r: n(0), g: n(2), b: n(4), a: h.length === 8 ? n(6) / 255 : 1 };
  }

  const fn = /^rgba?\(\s*([^)]+)\)$/.exec(s);
  if (fn) {
    const parts = fn[1].split(/[\s,/]+/).filter(Boolean);
    if (parts.length < 3) return null;
    const ch = (p: string) => (p.endsWith('%') ? (parseFloat(p) / 100) * 255 : parseFloat(p));
    const [r, g, b] = [ch(parts[0]), ch(parts[1]), ch(parts[2])];
    let a = 1;
    if (parts[3] !== undefined) a = parts[3].endsWith('%') ? parseFloat(parts[3]) / 100 : parseFloat(parts[3]);
    if ([r, g, b, a].some((v) => Number.isNaN(v))) return null;
    return { r: clamp(r, 0, 255), g: clamp(g, 0, 255), b: clamp(b, 0, 255), a: clamp(a, 0, 1) };
  }
  return null;
}

/** `fg` painted over `bg` (source-over). */
export function composite(fg: RGBA, bg: RGBA): RGBA {
  const a = fg.a + bg.a * (1 - fg.a);
  if (a === 0) return { r: 0, g: 0, b: 0, a: 0 };
  const mix = (f: number, b: number) => (f * fg.a + b * bg.a * (1 - fg.a)) / a;
  return { r: mix(fg.r, bg.r), g: mix(fg.g, bg.g), b: mix(fg.b, bg.b), a };
}

/** WCAG relative luminance of an opaque colour. */
export function luminance(c: RGBA): number {
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
}

/** WCAG contrast ratio of `fg` painted over the opaque `bg`. */
export function contrast(fg: RGBA, bg: RGBA): number {
  const top = composite(fg, bg);
  const l1 = luminance(top);
  const l2 = luminance(bg);
  const [hi, lo] = l1 >= l2 ? [l1, l2] : [l2, l1];
  return (hi + 0.05) / (lo + 0.05);
}

export const toHex = (c: RGBA): string =>
  '#' +
  [c.r, c.g, c.b]
    .map((v) => Math.round(clamp(v, 0, 255)).toString(16).padStart(2, '0'))
    .join('');

// ------------------------------------------------------------------------------ css parsing

export interface ThemeTokens {
  id: string;
  /** declarations of `[data-theme='id']` (custom properties + color-scheme) */
  base: Record<string, string>;
  /** declarations that only apply in the native window (`[data-env='tauri']`), merged over `base` */
  tauri: Record<string, string>;
}

function stripComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, '');
}

function declarations(body: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const part of body.split(';')) {
    const i = part.indexOf(':');
    if (i < 0) continue;
    const name = part.slice(0, i).trim();
    const value = part.slice(i + 1).trim();
    if (name && value) out[name] = value;
  }
  return out;
}

/** Top-level `selector { body }` rules. At-rules are skipped (theme files must not need them). */
function topLevelRules(css: string): { selector: string; body: string }[] {
  const rules: { selector: string; body: string }[] = [];
  let depth = 0;
  let start = 0;
  let selector = '';
  for (let i = 0; i < css.length; i++) {
    const ch = css[i];
    if (ch === '{') {
      if (depth === 0) {
        selector = css.slice(start, i).trim();
        start = i + 1;
      }
      depth++;
    } else if (ch === '}') {
      depth--;
      if (depth === 0) {
        if (!selector.startsWith('@')) rules.push({ selector, body: css.slice(start, i) });
        start = i + 1;
      }
    }
  }
  return rules;
}

/** Extract the token map of one theme file. Throws when the file does not declare a theme. */
export function parseThemeCss(css: string): ThemeTokens {
  const rules = topLevelRules(stripComments(css));
  let id = '';
  const base: Record<string, string> = {};
  const tauri: Record<string, string> = {};
  for (const r of rules) {
    const m = /\[data-theme(?:-scope)?=['"]([a-z0-9-]+)['"]\]/.exec(r.selector);
    if (!m) continue;
    id ||= m[1];
    Object.assign(/data-env=['"]tauri['"]/.test(r.selector) ? tauri : base, declarations(r.body));
  }
  if (!id) throw new Error('no [data-theme=...] rule found');
  return { id, base, tauri };
}

// ------------------------------------------------------------------------------ the audit

export interface Check {
  /** human readable pair, e.g. "text-2 on panel+s1" */
  name: string;
  ratio: number;
  min: number;
  pass: boolean;
}

const BLACK: RGBA = { r: 0, g: 0, b: 0, a: 1 };
const WHITE: RGBA = { r: 255, g: 255, b: 255, a: 1 };

function need(tokens: Record<string, string>, name: string): RGBA {
  const v = tokens[name];
  const c = v === undefined ? null : parseColor(v);
  if (!c) throw new Error(`token ${name} is missing or not a plain colour: ${String(v)}`);
  return c;
}

/**
 * Audit one theme. Two scenarios, because the surfaces are translucent glass:
 *   demo   – the surface over the theme's own `--page`
 *   native – the (more opaque) native-window surface over the worst case: pure black AND pure white
 *            desktop wallpaper behind it
 * Text pairs need AA (4.5:1); graphics (eyes) need 3:1; the eye body gets the stricter 4.5.
 */
export function auditTheme(t: ThemeTokens): Check[] {
  const checks: Check[] = [];
  const add = (name: string, ratio: number, min: number) =>
    checks.push({ name, ratio, min, pass: ratio >= min });

  const scenario = (label: string, tk: Record<string, string>, backdrops: RGBA[]) => {
    const text = [need(tk, '--text'), need(tk, '--text-2'), need(tk, '--text-3')];
    const names = ['text', 'text-2', 'text-3'];
    const s1 = need(tk, '--surface-1');
    const s2 = need(tk, '--surface-2');
    const s3 = need(tk, '--surface-3');

    for (const [bi, back] of backdrops.entries()) {
      const where = backdrops.length > 1 ? `${label}/${bi === 0 ? 'black' : 'white'}` : label;
      const panel = composite(need(tk, '--panel-bg'), back);
      const layers: [string, RGBA][] = [
        ['panel', panel],
        ['panel+s1', composite(s1, panel)],
        ['panel+s2', composite(s2, panel)],
        ['panel+s3', composite(s3, panel)],
      ];
      for (const [i, fg] of text.entries()) {
        for (const [lname, bg] of layers) add(`${names[i]} on ${lname} [${where}]`, contrast(fg, bg), 4.5);
      }

      const pop = composite(need(tk, '--pop-bg'), panel);
      for (const [i, fg] of text.entries()) add(`${names[i]} on popover [${where}]`, contrast(fg, pop), 4.5);

      const pill = composite(need(tk, '--pill-bg'), back);
      add(`pill-fg on pill [${where}]`, contrast(need(tk, '--pill-fg'), pill), 4.5);
      add(`pill-fg-2 on pill [${where}]`, contrast(need(tk, '--pill-fg-2'), pill), 4.5);
      add(`eye-fill on pill [${where}]`, contrast(need(tk, '--eye-fill'), pill), 4.5);
      add(`eye-accent on pill [${where}]`, contrast(need(tk, '--eye-accent'), pill), 3);

      const accent = need(tk, '--accent');
      add(`accent on panel [${where}]`, contrast(accent, panel), 4.5);
      add(`accent on accent-soft [${where}]`, contrast(accent, composite(need(tk, '--accent-soft'), panel)), 4.5);
      add(`danger on panel [${where}]`, contrast(need(tk, '--danger'), panel), 4.5);
    }
    add(`accent-ink on accent [${label}]`, contrast(need(tk, '--accent-ink'), composite(need(tk, '--accent'), BLACK)), 4.5);
    add(
      `invert-fg on invert-bg [${label}]`,
      contrast(need(tk, '--invert-fg'), composite(need(tk, '--invert-bg'), BLACK)),
      4.5,
    );
  };

  scenario('demo', t.base, [composite(need(t.base, '--page'), BLACK)]);
  scenario('native', { ...t.base, ...t.tauri }, [BLACK, WHITE]);
  return checks;
}

/** Fixed-width text table of an audit, worst ratio first, for the console. */
export function formatAudit(id: string, checks: Check[]): string {
  const rows = [...checks].sort((a, b) => a.ratio / a.min - b.ratio / b.min);
  const fails = rows.filter((r) => !r.pass).length;
  const head = `${id}: ${checks.length - fails}/${checks.length} pass`;
  const worst = rows
    .slice(0, 6)
    .map((r) => `  ${r.pass ? 'ok  ' : 'FAIL'} ${r.ratio.toFixed(2).padStart(5)} (>= ${r.min})  ${r.name}`);
  return [head, ...worst].join('\n');
}
