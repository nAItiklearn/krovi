import { describe, expect, it } from 'vitest';
import { BLOCKY_THEMES, THEMES, isBlockyTheme, nextChoice, parseChoice } from '../../lib/theme';
import { auditTheme, composite, contrast, parseColor, parseThemeCss, type RGBA, type ThemeTokens } from './contrast';

// The "companion polish" themes: Obsidian (premium dark), Linen (elegant light), and the two
// blocky 8-bit ones, Voxel (dark) and Pixel Meadow (light inventory grey). themes.test.ts already
// audits every theme file; this adds the explicit, named pairs these themes promise (several above
// AA) plus the pieces only they have.
const nodeFs = 'node:fs';
const fs = await import(/* @vite-ignore */ nodeFs);
const dir = new URL('./', import.meta.url);
const read = (name: string): string => fs.readFileSync(new URL(name, dir), 'utf8');

const BLACK: RGBA = { r: 0, g: 0, b: 0, a: 1 };
const col = (t: ThemeTokens, name: string): RGBA => {
  const c = parseColor(t.base[name] ?? '');
  if (!c) throw new Error(`${name} missing or not a colour`);
  return c;
};
/** the panel as the demo shows it: the glass over the theme's own page colour */
const panelOf = (t: ThemeTokens): RGBA => composite(col(t, '--panel-bg'), composite(col(t, '--page'), BLACK));
const pillOf = (t: ThemeTokens): RGBA => composite(col(t, '--pill-bg'), composite(col(t, '--page'), BLACK));

const NEW = ['obsidian', 'linen', 'voxel', 'pixel-meadow'] as const;
const tokens = Object.fromEntries(NEW.map((id) => [id, parseThemeCss(read(`${id}.css`))])) as Record<
  (typeof NEW)[number],
  ThemeTokens
>;

describe.each(NEW)('%s', (id) => {
  const t = tokens[id];

  it('is registered with the scheme its css declares', () => {
    const def = THEMES.find((x) => x.id === id);
    expect(def).toBeDefined();
    expect(def?.scheme).toBe(t.base['color-scheme']);
    expect(parseChoice(id)).toBe(id);
  });

  it('passes the whole WCAG AA audit', () => {
    const checks = auditTheme(t);
    expect(checks.length).toBeGreaterThan(60);
    expect(checks.filter((c) => !c.pass).map((c) => c.name)).toEqual([]);
  });

  it('body text on the panel is AAA (7:1), secondary text AA, the accent readable', () => {
    const panel = panelOf(t);
    expect(contrast(col(t, '--text'), panel)).toBeGreaterThanOrEqual(7);
    expect(contrast(col(t, '--text-2'), panel)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(col(t, '--text-3'), panel)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(col(t, '--accent'), panel)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(col(t, '--accent-ink'), col(t, '--accent'))).toBeGreaterThanOrEqual(4.5);
  });

  it('eyes stand out on the pill', () => {
    const pill = pillOf(t);
    expect(contrast(col(t, '--eye-fill'), pill)).toBeGreaterThanOrEqual(7);
    expect(contrast(col(t, '--eye-accent'), pill)).toBeGreaterThanOrEqual(3);
  });
});

describe('voxel and pixel-meadow palettes', () => {
  it('Voxel is dark with a grass-green accent; Pixel Meadow is the light grey inventory with a dark-green one', () => {
    expect(tokens.voxel.base['color-scheme']).toBe('dark');
    expect(tokens['pixel-meadow'].base['color-scheme']).toBe('light');
    for (const id of ['voxel', 'pixel-meadow'] as const) {
      const accent = col(tokens[id], '--accent');
      expect(accent.g, id).toBeGreaterThan(accent.r);
      expect(accent.g, id).toBeGreaterThan(accent.b);
    }
  });

  it('Pixel Meadow panels are the classic mid-grey (a neutral grey, not tinted)', () => {
    const p = col(tokens['pixel-meadow'], '--panel-bg');
    expect(p.a).toBe(1);
    expect(Math.max(p.r, p.g, p.b) - Math.min(p.r, p.g, p.b)).toBeLessThanOrEqual(2);
    expect(p.r).toBeGreaterThan(150);
    expect(p.r).toBeLessThan(220);
  });
});

describe.each(['voxel', 'pixel-meadow'] as const)('blocky theme: %s', (id) => {
  const t = tokens[id];
  const blocky = read('_blocky.css');

  it('is listed as blocky, and _blocky.css targets it', () => {
    expect(BLOCKY_THEMES).toContain(id);
    expect(isBlockyTheme(id)).toBe(true);
    expect(blocky).toContain(`:root[data-theme='${id}']`);
  });

  it('draws the pupil in the pill colour, which must read on the eye body', () => {
    expect(contrast(col(t, '--eye-fill'), pillOf(t))).toBeGreaterThanOrEqual(7);
  });

  it('frames are hard-edged steps: no blur radius in any shadow', () => {
    for (const name of ['--shadow-pill', '--shadow-panel', '--shadow-pop']) {
      const layers = t.base[name].split(/,(?![^(]*\))/).map((s) => s.trim());
      expect(layers.length).toBeGreaterThan(3);
      for (const layer of layers) {
        // offset-x offset-y blur spread colour: blur is the third length
        const lengths = layer.replace(/rgba?\([^)]*\)|#[0-9a-f]+/gi, '').trim().split(/\s+/);
        expect(lengths[2], `${name}: ${layer}`).toBe('0');
      }
    }
  });
});

describe('blocky structural css (_blocky.css)', () => {
  const css = read('_blocky.css').replace(/\/\*[\s\S]*?\*\//g, '');

  it('ships no font files and no remote requests', () => {
    expect(css).not.toMatch(/@font-face|@import/);
    const urls = [...css.matchAll(/url\(\s*["']?([^"')]+)/g)].map((m) => m[1]);
    expect(urls.length).toBeGreaterThan(0);
    for (const u of urls) expect(u.startsWith('data:image/svg+xml')).toBe(true);
    // the generated dither must never be smoothed, and the type stays on system monospace
    expect(css).toMatch(/image-rendering:\s*pixelated/);
    expect(css).toMatch(/--font:\s*ui-monospace/);
  });

  it('squares every step of the radius scale', () => {
    for (const r of ['--r-1', '--r-2', '--r-3', '--r-4', '--r-panel']) expect(css).toMatch(new RegExp(`${r}:\\s*0px`));
  });

  it('replaced the Voxel-only _voxel.css (a stale copy would double-apply)', () => {
    expect(() => read('_voxel.css')).toThrow();
  });

  it('only the registered blocky themes are named', () => {
    const named = [...css.matchAll(/data-theme='([\w-]+)'/g)].map((m) => m[1]);
    expect([...new Set(named)].sort()).toEqual([...BLOCKY_THEMES].sort());
  });
});

describe('picker order', () => {
  it('cycles through the new themes after porcelain and ends on voxel, pixel-meadow', () => {
    const ids = THEMES.map((x) => x.id);
    expect(ids.slice(0, 4)).toEqual(['graphite', 'porcelain', 'obsidian', 'linen']);
    expect(ids.slice(-2)).toEqual(['voxel', 'pixel-meadow']);
    expect(nextChoice('porcelain')).toBe('obsidian');
    expect(nextChoice('voxel')).toBe('pixel-meadow');
    expect(nextChoice('pixel-meadow')).toBe('system');
  });

  it('only registered themes are blocky', () => {
    for (const id of BLOCKY_THEMES) expect(THEMES.some((t) => t.id === id)).toBe(true);
    expect(isBlockyTheme('graphite')).toBe(false);
    expect(isBlockyTheme('system')).toBe(false);
  });
});
