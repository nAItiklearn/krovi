import { describe, it, expect } from 'vitest';
import { OPTIONAL_TOKENS, REQUIRED_TOKENS, THEMES } from '../../lib/theme';
import { auditTheme, composite, contrast, formatAudit, parseColor, parseThemeCss, toHex } from './contrast';

// Every css file in this folder, as raw text. Vitest blanks css imports (even `?raw`), so read the
// files from disk; the specifier is a variable so svelte-check does not need node typings.
const nodeFs = 'node:fs';
const fs = await import(/* @vite-ignore */ nodeFs);
const dir = new URL('./', import.meta.url);
const files: Record<string, string> = {};
for (const name of fs.readdirSync(dir) as string[]) {
  if (name.endsWith('.css')) files[name.slice(0, -4)] = fs.readFileSync(new URL(name, dir), 'utf8');
}
const base = files['_base'];
const themeFiles = Object.entries(files)
  .filter(([name]) => !name.startsWith('_'))
  .map(([file, css]) => ({ file, css }));

/** tokens that are not a single plain colour */
const NON_COLOR = new Set(['--grain-opacity', '--shadow-pill', '--shadow-panel', '--shadow-pop']);
const COLOR_TOKENS = [...REQUIRED_TOKENS, ...OPTIONAL_TOKENS].filter((t) => !NON_COLOR.has(t));

describe('contrast maths', () => {
  it('parses the colour syntaxes the theme files use', () => {
    expect(parseColor('#fff')).toEqual({ r: 255, g: 255, b: 255, a: 1 });
    expect(parseColor('#00000080')?.a).toBeCloseTo(0.5, 2);
    expect(parseColor('rgba(10, 20, 30, 0.5)')).toEqual({ r: 10, g: 20, b: 30, a: 0.5 });
    expect(parseColor('rgb(10 20 30 / 25%)')).toEqual({ r: 10, g: 20, b: 30, a: 0.25 });
    expect(parseColor('transparent')?.a).toBe(0);
    expect(parseColor('not a colour')).toBeNull();
  });

  it('matches the WCAG reference ratios', () => {
    const black = parseColor('#000')!;
    const white = parseColor('#fff')!;
    expect(contrast(black, white)).toBeCloseTo(21, 5);
    expect(contrast(white, white)).toBeCloseTo(1, 5);
    // #767676 on white is the canonical "just passes AA" grey
    expect(contrast(parseColor('#767676')!, white)).toBeGreaterThan(4.5);
    expect(contrast(parseColor('#777777')!, white)).toBeLessThan(4.6);
  });

  it('composites translucent colours the way the browser does', () => {
    const out = composite(parseColor('rgba(255,255,255,0.5)')!, parseColor('#000')!);
    expect(toHex(out)).toBe('#808080');
  });
});

describe('registry <-> css files', () => {
  it('has a css file for every registered theme and a registry entry for every file', () => {
    const ids = THEMES.map((t) => t.id).sort();
    expect(themeFiles.map((f) => f.file).sort()).toEqual(ids);
  });

  it('has unique ids and well-formed swatches', () => {
    const ids = THEMES.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const t of THEMES) {
      expect(t.name.length).toBeGreaterThan(0);
      expect(['dark', 'light']).toContain(t.scheme);
      expect(t.swatch).toHaveLength(3);
      for (const hex of t.swatch) expect(hex).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  it('ships both a dark and a light default', () => {
    expect(THEMES.find((t) => t.id === 'graphite')?.scheme).toBe('dark');
    expect(THEMES.find((t) => t.id === 'porcelain')?.scheme).toBe('light');
  });
});

describe.each(themeFiles)('theme file: $file', ({ file, css }) => {
  const tokens = parseThemeCss(css);
  const def = THEMES.find((t) => t.id === file);

  it('declares the theme it is named after', () => {
    expect(tokens.id).toBe(file);
    expect(def).toBeDefined();
  });

  it('defines every required token', () => {
    const missing = REQUIRED_TOKENS.filter((t) => !tokens.base[t]);
    expect(missing).toEqual([]);
  });

  it('uses only the shared vocabulary (no stray variables)', () => {
    const known = new Set<string>([...REQUIRED_TOKENS, ...OPTIONAL_TOKENS, 'color-scheme']);
    const extra = Object.keys(tokens.base).filter((k) => !known.has(k));
    expect(extra).toEqual([]);
  });

  it('declares the colour-scheme the registry promises', () => {
    expect(tokens.base['color-scheme']).toBe(def?.scheme);
  });

  it('gives every colour token a value the audit can read', () => {
    for (const name of COLOR_TOKENS) {
      if (tokens.base[name] === undefined) continue; // optional ones
      expect(parseColor(tokens.base[name]), `${name}: ${tokens.base[name]}`).not.toBeNull();
    }
  });

  it('keeps the swatch in step with the css', () => {
    expect(def?.swatch[2].toLowerCase()).toBe(tokens.base['--accent'].toLowerCase());
  });

  it('overrides the glass for the native window, where the desktop cannot be blurred', () => {
    expect(Object.keys(tokens.tauri).sort()).toEqual(['--panel-bg', '--pill-bg']);
  });

  it('passes WCAG AA for every text pair and the eye contrast bar', () => {
    const checks = auditTheme(tokens);
    const failed = checks.filter((c) => !c.pass);
    expect(failed.map((c) => `${c.name}: ${c.ratio.toFixed(2)} < ${c.min}`)).toEqual([]);
    // a receipt in the test log: pass count and the six tightest pairs
    console.info(formatAudit(file, checks));
  });
});

describe('_base.css', () => {
  it('registers every colour token so the theme switch can cross-fade', () => {
    for (const name of COLOR_TOKENS) {
      expect(base, `@property ${name}`).toMatch(new RegExp(`@property ${name} \\{`));
    }
    expect(base).toMatch(/@property --grain-opacity \{ syntax: '<number>'/);
  });

  it('transitions every registered token', () => {
    const transition = base.slice(base.indexOf(':root[data-theme-ready]'));
    for (const name of [...COLOR_TOKENS, '--grain-opacity']) {
      expect(transition, `transition for ${name}`).toMatch(new RegExp(`${name} \\d+ms`));
    }
  });

  it('arms the cross-fade only after first paint (never on launch)', () => {
    expect(base).toContain(':root[data-theme-ready]');
  });
});
