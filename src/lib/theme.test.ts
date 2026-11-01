import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import {
  CHOICE_KEY,
  SYSTEM_DARK,
  SYSTEM_LIGHT,
  THEMES,
  applyTheme,
  isThemeId,
  loadChoice,
  migrateLegacy,
  nextChoice,
  parseChoice,
  resolveTheme,
  saveChoice,
  schemeOf,
  type StorageLike,
} from './theme';

const memory = (seed: Record<string, string> = {}): StorageLike & { data: Record<string, string> } => {
  const data = { ...seed };
  return {
    data,
    getItem: (k) => (k in data ? data[k] : null),
    setItem: (k, v) => {
      data[k] = v;
    },
  };
};

const broken: StorageLike = {
  getItem: () => {
    throw new Error('SecurityError');
  },
  setItem: () => {
    throw new Error('QuotaExceededError');
  },
};

describe('theme resolution', () => {
  it('maps "system" to Graphite or Porcelain by the OS preference', () => {
    expect(resolveTheme('system', true)).toBe('graphite');
    expect(resolveTheme('system', false)).toBe('porcelain');
    expect(SYSTEM_DARK).toBe('graphite');
    expect(SYSTEM_LIGHT).toBe('porcelain');
  });

  it('leaves a concrete choice alone whatever the OS says', () => {
    for (const t of THEMES) {
      expect(resolveTheme(t.id, true)).toBe(t.id);
      expect(resolveTheme(t.id, false)).toBe(t.id);
    }
  });

  it('knows the scheme of every theme', () => {
    expect(schemeOf('graphite')).toBe('dark');
    expect(schemeOf('porcelain')).toBe('light');
    expect(schemeOf('mochi')).toBe('light');
    expect(schemeOf('arcade')).toBe('dark');
  });

  it('system always lands on a theme whose scheme matches the OS', () => {
    expect(schemeOf(resolveTheme('system', true))).toBe('dark');
    expect(schemeOf(resolveTheme('system', false))).toBe('light');
  });

  it('cycles system -> every theme -> system', () => {
    let c = parseChoice('system');
    const seen = [c];
    for (let i = 0; i < THEMES.length; i++) seen.push((c = nextChoice(c)));
    expect(seen.slice(1)).toEqual(THEMES.map((t) => t.id));
    expect(nextChoice(c)).toBe('system');
  });
});

describe('choice parsing + persistence fallbacks', () => {
  it('accepts system and known ids, and nothing else', () => {
    expect(parseChoice('system')).toBe('system');
    expect(parseChoice('tide')).toBe('tide');
    for (const bad of ['', 'Tide', 'nope', null, undefined, 3, {}]) expect(parseChoice(bad)).toBe('system');
    expect(isThemeId('arcade')).toBe(true);
    expect(isThemeId('system')).toBe(false);
  });

  it('migrates the pre-themes light/dark/auto key', () => {
    expect(migrateLegacy('light')).toBe('porcelain');
    expect(migrateLegacy('dark')).toBe('graphite');
    expect(migrateLegacy('auto')).toBe('system');
    expect(migrateLegacy('sepia')).toBeNull();
  });

  it('round-trips through storage', () => {
    const s = memory();
    expect(saveChoice(s, 'mochi')).toBe(true);
    expect(s.data[CHOICE_KEY]).toBe('mochi');
    expect(loadChoice(s)).toBe('mochi');
  });

  it('prefers the new key, falls back to the legacy key, then to system', () => {
    expect(loadChoice(memory({ [CHOICE_KEY]: 'tide', 'krovi.theme': 'light' }))).toBe('tide');
    expect(loadChoice(memory({ 'krovi.theme': 'light' }))).toBe('porcelain');
    expect(loadChoice(memory())).toBe('system');
  });

  it('ignores a corrupt stored value', () => {
    expect(loadChoice(memory({ [CHOICE_KEY]: '{"theme":' }))).toBe('system');
    expect(loadChoice(memory({ [CHOICE_KEY]: 'hotdog' }))).toBe('system');
  });

  it('never throws when storage is missing or blocked', () => {
    expect(loadChoice(null)).toBe('system');
    expect(loadChoice(broken)).toBe('system');
    expect(saveChoice(null, 'tide')).toBe(false);
    expect(saveChoice(broken, 'tide')).toBe(false);
  });
});

describe('applyTheme', () => {
  it('stamps data-theme and data-scheme', () => {
    const attrs: Record<string, string> = {};
    const root = { setAttribute: (k: string, v: string) => void (attrs[k] = v) };
    applyTheme(root, 'terracotta');
    expect(attrs).toEqual({ 'data-theme': 'terracotta', 'data-scheme': 'light' });
    applyTheme(root, 'arcade');
    expect(attrs).toEqual({ 'data-theme': 'arcade', 'data-scheme': 'dark' });
  });
});

// ---- the live wiring, against a fake document + OS preference -------------------------------

describe('startTheme', () => {
  let attrs: Record<string, string>;
  let osListeners: Set<() => void>;
  let osDark: boolean;
  let store: ReturnType<typeof memory>;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetModules();
    attrs = {};
    osListeners = new Set();
    osDark = true;
    store = memory();
    vi.stubGlobal('document', {
      documentElement: {
        setAttribute: (k: string, v: string) => void (attrs[k] = v),
        removeAttribute: (k: string) => void delete attrs[k],
      },
    });
    vi.stubGlobal('matchMedia', () => ({
      get matches() {
        return osDark;
      },
      addEventListener: (_: string, cb: () => void) => osListeners.add(cb),
      removeEventListener: (_: string, cb: () => void) => osListeners.delete(cb),
    }));
    vi.stubGlobal('localStorage', store);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  const flipOs = (dark: boolean) => {
    osDark = dark;
    osListeners.forEach((cb) => cb());
  };

  it('follows the OS live while the choice is system', async () => {
    const t = await import('./theme');
    t.startTheme();
    expect(attrs).toMatchObject({ 'data-theme': 'graphite', 'data-scheme': 'dark' });
    flipOs(false);
    expect(attrs).toMatchObject({ 'data-theme': 'porcelain', 'data-scheme': 'light' });
    flipOs(true);
    expect(attrs['data-theme']).toBe('graphite');
  });

  it('stops following the OS once a concrete theme is chosen, and persists it', async () => {
    const t = await import('./theme');
    t.startTheme();
    t.themeChoice.set('mochi');
    expect(attrs).toMatchObject({ 'data-theme': 'mochi', 'data-scheme': 'light' });
    flipOs(false);
    flipOs(true);
    expect(attrs['data-theme']).toBe('mochi');
    expect(store.data[CHOICE_KEY]).toBe('mochi');
    expect(get(t.resolvedTheme)).toBe('mochi');
    expect(get(t.resolvedScheme)).toBe('light');
  });

  it('is idempotent: one subscription, the same stop function', async () => {
    const t = await import('./theme');
    const a = t.startTheme();
    const b = t.startTheme();
    expect(b).toBe(a);
    expect(osListeners.size).toBe(1);
    a();
    expect(osListeners.size).toBe(0);
    expect(attrs['data-theme-ready']).toBeUndefined();
  });

  it('arms the cross-fade only after the first paint', async () => {
    const t = await import('./theme');
    t.startTheme();
    expect(attrs['data-theme-ready']).toBeUndefined();
    vi.advanceTimersByTime(200);
    expect(attrs['data-theme-ready']).toBe('');
  });

  it('restores a persisted choice on load', async () => {
    store.data[CHOICE_KEY] = 'tide';
    const t = await import('./theme');
    t.startTheme();
    expect(attrs['data-theme']).toBe('tide');
  });

  it('survives blocked storage and keeps working in memory', async () => {
    vi.stubGlobal('localStorage', broken);
    const t = await import('./theme');
    expect(get(t.themeChoice)).toBe('system');
    expect(() => t.themeChoice.set('arcade')).not.toThrow();
    expect(get(t.themeChoice)).toBe('arcade');
  });

  it('rejects an unknown id instead of storing it', async () => {
    const t = await import('./theme');
    t.themeChoice.set('hotdog' as never);
    expect(get(t.themeChoice)).toBe('system');
  });
});
