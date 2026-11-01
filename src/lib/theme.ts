// Theme system (owner: personalization) — docs/CONTRACTS.md §7.
//
// A theme is ONE css file in src/styles/themes/ that overrides the shared token vocabulary under
// `[data-theme='<id>']`, plus ONE entry in THEMES below. That is all it takes to add a theme:
//
//   1. drop `src/styles/themes/<id>.css`   (copy graphite.css or porcelain.css, change values)
//   2. add a line to THEMES                (id, name, scheme, swatch)
//
// The css files are pulled in by the glob below, so there is no third place to touch.
// `startTheme()` stamps `data-theme=<id>` and `data-scheme=dark|light` on <html>; components
// that need to know light vs dark must key off `data-scheme`, never off a theme id.

import { derived, get, writable, type Readable, type Writable } from 'svelte/store';

// Every `*.css` here is a theme (or the shared `_base.css`). Imported for the side effect only.
void import.meta.glob('../styles/themes/*.css', { eager: true });

export type Scheme = 'dark' | 'light';

export interface ThemeDef {
  id: string;
  /** shown in the picker */
  name: string;
  scheme: Scheme;
  /** [background, foreground, accent] — three dots for the picker; keep in step with the css */
  swatch: readonly [string, string, string];
  /** one line for the picker's tooltip */
  blurb: string;
}

const defs = [
  { id: 'graphite', name: 'Graphite', scheme: 'dark', swatch: ['#0b0b0d', '#f4f4f6', '#ff8a4c'], blurb: 'Near-black glass, one ember accent' },
  { id: 'porcelain', name: 'Porcelain', scheme: 'light', swatch: ['#fafafc', '#0a0a0c', '#bc3f09'], blurb: 'Near-white glass, ink text, ember accent' },
  { id: 'mochi', name: 'Mochi', scheme: 'light', swatch: ['#faf2fb', '#342148', '#9c3a62'], blurb: 'Soft lilac and peach, plum ink' },
  { id: 'arcade', name: 'Arcade', scheme: 'dark', swatch: ['#0e0c26', '#f6f3ff', '#ff4fd8'], blurb: 'Indigo-black with an electric glow' },
  { id: 'terracotta', name: 'Terracotta', scheme: 'light', swatch: ['#f1e6d4', '#302016', '#4f5a16'], blurb: 'Warm sand and clay, olive accent' },
  { id: 'tide', name: 'Tide', scheme: 'dark', swatch: ['#152336', '#ecf6ff', '#5fe3c4'], blurb: 'Slate-blue glass, seafoam accent' },
] as const satisfies readonly ThemeDef[];

export type ThemeId = (typeof defs)[number]['id'];
export type ThemeChoice = 'system' | ThemeId;

/** The registry. Order is the picker order. */
export const THEMES = defs;

/** What "Follow system" maps to. */
export const SYSTEM_DARK: ThemeId = 'graphite';
export const SYSTEM_LIGHT: ThemeId = 'porcelain';

/**
 * The shared vocabulary every theme file must define (checked by themes.test.ts). The first group
 * comes from src/styles/tokens.css; the `--eye-*` group is read by the idle companion.
 */
export const REQUIRED_TOKENS = [
  '--page', '--panel-bg', '--pop-bg', '--pill-bg', '--pill-fg', '--pill-fg-2',
  '--surface-1', '--surface-2', '--surface-3', '--line', '--line-strong',
  '--text', '--text-2', '--text-3',
  '--accent', '--accent-ink', '--accent-soft', '--accent-glow', '--danger',
  '--invert-bg', '--invert-fg',
  '--rim-a', '--rim-b', '--grain-opacity', '--card-hi', '--spot',
  '--shadow-pill', '--shadow-panel', '--shadow-pop',
  '--eye-fill', '--eye-fill-2', '--eye-accent', '--eye-glow',
] as const;

/**
 * Tokens a theme MAY define. --eye-blush falls back to transparent. The --wall-* set paints the
 * browser-demo wallpaper (Backdrop): a theme that omits it keeps its scheme's default wallpaper.
 */
export const OPTIONAL_TOKENS = [
  '--eye-blush',
  '--wall-top', '--wall-mid', '--wall-bot', '--wall-glow', '--wall-r1', '--wall-r2', '--wall-line', '--wall-vig',
] as const;

// ------------------------------------------------------------------------------ pure helpers

export const isThemeId = (v: unknown): v is ThemeId => typeof v === 'string' && defs.some((t) => t.id === v);

export function themeById(id: ThemeId): ThemeDef {
  return defs.find((t) => t.id === id) ?? defs[0];
}

export const schemeOf = (id: ThemeId): Scheme => themeById(id).scheme;

/** Anything that is not a known choice becomes 'system'. */
export function parseChoice(raw: unknown): ThemeChoice {
  return raw === 'system' || isThemeId(raw) ? raw : 'system';
}

/** The pre-themes `krovi.theme` key stored 'light' | 'dark' | 'auto'. */
export function migrateLegacy(raw: unknown): ThemeChoice | null {
  if (raw === 'light') return SYSTEM_LIGHT;
  if (raw === 'dark') return SYSTEM_DARK;
  if (raw === 'auto') return 'system';
  return null;
}

/** system → Graphite/Porcelain by the OS preference; a concrete choice is itself. */
export function resolveTheme(choice: ThemeChoice, systemDark: boolean): ThemeId {
  if (choice === 'system') return systemDark ? SYSTEM_DARK : SYSTEM_LIGHT;
  return isThemeId(choice) ? choice : systemDark ? SYSTEM_DARK : SYSTEM_LIGHT;
}

/** The next choice in picker order (system, then every theme), wrapping. */
export function nextChoice(current: ThemeChoice): ThemeChoice {
  const order: ThemeChoice[] = ['system', ...defs.map((t) => t.id)];
  const i = order.indexOf(current);
  return order[(i + 1) % order.length];
}

// ------------------------------------------------------------------------------ persistence

export const CHOICE_KEY = 'krovi.themeChoice';
const LEGACY_KEY = 'krovi.theme';

export type StorageLike = Pick<Storage, 'getItem' | 'setItem'>;

/** localStorage, or null where it is blocked/absent (private mode, node, a hardened webview). */
export function getStorage(): StorageLike | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/** Never throws: a missing, blocked or corrupt store means the default ('system'). */
export function loadChoice(storage: StorageLike | null): ThemeChoice {
  if (!storage) return 'system';
  try {
    const own = storage.getItem(CHOICE_KEY);
    if (own != null) return parseChoice(own);
    return migrateLegacy(storage.getItem(LEGACY_KEY)) ?? 'system';
  } catch {
    return 'system';
  }
}

export function saveChoice(storage: StorageLike | null, choice: ThemeChoice): boolean {
  if (!storage) return false;
  try {
    storage.setItem(CHOICE_KEY, choice);
    return true;
  } catch {
    return false;
  }
}

// ------------------------------------------------------------------------------ stores

function createChoiceStore(): Writable<ThemeChoice> {
  const inner = writable<ThemeChoice>(loadChoice(getStorage()));
  const set = (v: ThemeChoice) => {
    const next = parseChoice(v);
    inner.set(next);
    saveChoice(getStorage(), next);
  };
  return {
    subscribe: inner.subscribe,
    set,
    update: (fn) => set(fn(get(inner))),
  };
}

/** The user's pick: 'system' or a theme id. Persisted; defaults to 'system'. */
export const themeChoice: Writable<ThemeChoice> = createChoiceStore();

const osDark = writable<boolean>(readOsDark());

function readOsDark(): boolean {
  return typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)').matches : true;
}

/** The theme actually on screen (follows the OS live while the choice is 'system'). */
export const resolvedTheme: Readable<ThemeId> = derived([themeChoice, osDark], ([$c, $d]) =>
  resolveTheme($c, $d),
);
export const resolvedScheme: Readable<Scheme> = derived(resolvedTheme, ($t) => schemeOf($t));

// ------------------------------------------------------------------------------ the DOM side

const READY_ATTR = 'data-theme-ready';

/** Stamp the document. Split out so tests can hand it a fake element. */
export function applyTheme(root: Pick<Element, 'setAttribute'>, id: ThemeId): void {
  root.setAttribute('data-theme', id);
  root.setAttribute('data-scheme', schemeOf(id));
}

let stop: (() => void) | null = null;

/**
 * Wire the theme to the document: stamp `data-theme` / `data-scheme`, re-stamp on every change,
 * and follow the OS light/dark switch while the choice is 'system'. Idempotent — call it as early
 * as possible (before mount) and as often as convenient; the same stop function comes back.
 *
 * The colour cross-fade (see styles/themes/_base.css) is armed shortly after the first stamp, so
 * launch never fades in from the default palette, but every later switch does.
 */
export function startTheme(): () => void {
  if (stop) return stop;
  if (typeof document === 'undefined') return () => {};

  const root = document.documentElement;
  const mq = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
  const onOs = () => osDark.set(mq ? mq.matches : true);
  mq?.addEventListener('change', onOs);
  onOs();

  const off = resolvedTheme.subscribe((id) => applyTheme(root, id));
  const armed = setTimeout(() => root.setAttribute(READY_ATTR, ''), 150);

  const mine = () => {
    off();
    mq?.removeEventListener('change', onOs);
    clearTimeout(armed);
    root.removeAttribute(READY_ATTR);
    if (stop === mine) stop = null;
  };
  stop = mine;
  return mine;
}
