// The one seam between the UI and its data source.
//
// The frontend NEVER hard-depends on Tauri to render. `isTauri()` checks for the
// injected Tauri internals:
//   * DEMO MODE (plain browser): every loader returns a deep clone of the fixtures so
//     the panel is fully interactive with no backend (this is what the website embeds).
//   * TAURI: every loader calls a real Rust command — there is no fixture fallback on
//     this path, ever. Empty/unknown is reported as empty/null, not invented.
//
// The live feeds (clipboard / media / stats) are events, not loaders: see live.ts.

import type {
  CalendarEvent,
  ClipEntry,
  CurrencyInfo,
  CurrencyRates,
  MediaState,
  MediaTrack,
  Note,
  Place,
  ScreenTimeRecord,
  StoredShelf,
  Task,
  WeatherData,
  WeatherIcon,
  WeatherReply,
  WorldClockZone,
} from './types';
import * as fx from './fixtures';

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export function mode(): 'tauri' | 'demo' {
  return isTauri() ? 'tauri' : 'demo';
}

function clone<T>(v: T): T {
  // structuredClone is available in all modern browsers and in the Tauri webview.
  return typeof structuredClone === 'function' ? structuredClone(v) : JSON.parse(JSON.stringify(v));
}

// Lazily resolve Tauri's invoke only when actually running under Tauri, so the
// demo build never needs the dependency at runtime.
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const api = await import('@tauri-apps/api/core');
  return api.invoke<T>(cmd, args);
}

/**
 * Subscribe to a backend event. Resolves to an unsubscribe function. In demo mode
 * (no backend) nothing is ever emitted, so this resolves to a no-op.
 */
export async function listen<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const api = await import('@tauri-apps/api/event');
  return api.listen<T>(event, (e) => cb(e.payload));
}

// ---------------------------------------------------------------------------
// Pure normalizers (unit-tested)
// ---------------------------------------------------------------------------

/**
 * The backend can report a thunderstorm; the UI's icon set has no storm glyph, so it
 * renders as rain until the design adds one. (Also tolerates unknown future codes.)
 */
export function normalizeIcon(icon: string): WeatherIcon {
  switch (icon) {
    case 'sun':
    case 'cloud':
    case 'partly':
    case 'rain':
    case 'snow':
      return icon;
    case 'storm':
      return 'rain';
    default:
      return 'cloud';
  }
}

export function normalizeWeather(w: WeatherData | null): WeatherData | null {
  if (!w) return null;
  return {
    ...w,
    icon: normalizeIcon(w.icon),
    hourly: w.hourly.map((h) => ({ ...h, icon: normalizeIcon(h.icon) })),
    forecast: w.forecast.map((d) => ({ ...d, icon: normalizeIcon(d.icon) })),
  };
}

/** The backend leaves the `Local` zone's `tz` empty: the webview knows the machine's zone. */
export function withLocalZone(zones: WorldClockZone[], localTz: string): WorldClockZone[] {
  return zones.map((z) => (z.label === 'Local' && !z.tz ? { ...z, tz: localTz || 'UTC' } : z));
}

/** Store `Local` with an empty zone so it keeps following the machine's zone. */
export function forStorage(zones: WorldClockZone[]): WorldClockZone[] {
  return zones.map((z) => (z.label === 'Local' ? { ...z, tz: '' } : z));
}

export const SHELVES_KEY = 'krovi.shelves';
export const CLOCKS_KEY = 'krovi.clocks';

function readLocal<T>(key: string): T | null {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(key) : null;
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}
function writeLocal(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* blocked storage: the demo simply forgets */
  }
}
function dropLocal(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    /* ignore */
  }
}

export function localTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

function normalizeReply(r: WeatherReply): WeatherReply {
  return { ...r, weather: normalizeWeather(r.weather) };
}

// ---------------------------------------------------------------------------
// Demo-mode working copies (so add/remove behave in the browser demo too)
// ---------------------------------------------------------------------------

let demoCalendar: CalendarEvent[] | null = null;
function demoCal(): CalendarEvent[] {
  if (!demoCalendar) demoCalendar = clone(fx.calendarFixture);
  return demoCalendar;
}

const demoPlaces: Place[] = [
  { name: 'Pune, Maharashtra, India', lat: 18.5, lon: 73.9 },
  { name: 'London, England, United Kingdom', lat: 51.5, lon: -0.1 },
  { name: 'New York, New York, United States', lat: 40.7, lon: -74.0 },
  { name: 'Tokyo, Japan', lat: 35.7, lon: 139.7 },
  { name: 'San Francisco, California, United States', lat: 37.8, lon: -122.4 },
];

function demoWeatherReply(city?: string): WeatherReply {
  const w = clone(fx.weatherFixture);
  if (city) w.city = city;
  return {
    status: 'ok',
    weather: w,
    stale: false,
    fetchedAt: Date.now(),
    place: { city: w.city, lat: 18.5, lon: 73.9 },
    error: null,
  };
}

// ---------------------------------------------------------------------------
// The bridge
// ---------------------------------------------------------------------------

export const bridge = {
  async loadClipboard(): Promise<ClipEntry[]> {
    return isTauri() ? invoke<ClipEntry[]>('load_clipboard') : clone(fx.clipboardFixture);
  },
  async loadNotes(): Promise<Note[]> {
    return isTauri() ? invoke<Note[]>('load_notes') : clone(fx.notesFixture);
  },
  async loadTasks(): Promise<Task[]> {
    return isTauri() ? invoke<Task[]>('load_tasks') : clone(fx.tasksFixture);
  },
  /**
   * Shelves. Natively they live in the Rust store (SQLite); a list left in localStorage by an
   * older build is migrated once and then removed. The browser demo keeps them in localStorage.
   */
  async loadShelves(): Promise<StoredShelf[]> {
    if (!isTauri()) return readLocal<StoredShelf[]>(SHELVES_KEY) ?? clone(fx.shelvesFixture);
    const legacy = readLocal<StoredShelf[]>(SHELVES_KEY);
    if (legacy && legacy.length) {
      const saved = await invoke<StoredShelf[]>('save_shelves', { shelves: legacy });
      dropLocal(SHELVES_KEY);
      return saved;
    }
    return invoke<StoredShelf[]>('load_shelves');
  },
  async saveShelves(shelves: StoredShelf[]): Promise<StoredShelf[]> {
    if (!isTauri()) {
      writeLocal(SHELVES_KEY, shelves);
      return shelves;
    }
    return invoke<StoredShelf[]>('save_shelves', { shelves });
  },
  async loadScreenTime(): Promise<ScreenTimeRecord[]> {
    return isTauri() ? invoke<ScreenTimeRecord[]>('load_screen_time') : clone(fx.screenTimeFixture);
  },

  // --- Weather -------------------------------------------------------------

  /** Current weather, or `null` when no place is set / offline with nothing cached. */
  async loadWeather(): Promise<WeatherData | null> {
    if (!isTauri()) return clone(fx.weatherFixture);
    try {
      return normalizeWeather(await invoke<WeatherData | null>('load_weather'));
    } catch (e) {
      console.error('[krovi] load_weather failed', e);
      return null;
    }
  },
  /** Weather + why: `needs-city` (ask for a city), `offline` (maybe stale), `ok`. `force` skips the cache. */
  async loadWeatherStatus(force = false): Promise<WeatherReply> {
    if (!isTauri()) return demoWeatherReply();
    return normalizeReply(await invoke<WeatherReply>('weather_status', { force }));
  },
  /** Candidate places for a typed city name (the only text that leaves the device). */
  async searchPlaces(query: string): Promise<Place[]> {
    if (!isTauri()) {
      const q = query.trim().toLowerCase();
      return demoPlaces.filter((p) => p.name.toLowerCase().includes(q));
    }
    return invoke<Place[]>('weather_search', { query });
  },
  /** Choose one of the places from `searchPlaces` and load its weather. */
  async setWeatherPlace(place: Place): Promise<WeatherReply> {
    if (!isTauri()) return demoWeatherReply(place.name);
    return normalizeReply(
      await invoke<WeatherReply>('weather_set_place', { name: place.name, lat: place.lat, lon: place.lon }),
    );
  },
  /** Geocode a typed city, take the best match and load its weather. */
  async setWeatherCity(query: string): Promise<WeatherReply> {
    if (!isTauri()) {
      const hit = demoPlaces.find((p) => p.name.toLowerCase().includes(query.trim().toLowerCase()));
      if (!hit) throw new Error(`No place found for "${query.trim()}"`);
      return demoWeatherReply(hit.name);
    }
    return normalizeReply(await invoke<WeatherReply>('weather_set_city', { query }));
  },

  // --- Media (live updates come from live.ts; these are one-shot reads) ------

  /** Whole-track view of what is playing, or `null` when nothing is. */
  async loadMedia(): Promise<MediaTrack | null> {
    return isTauri() ? invoke<MediaState | null>('load_media') : clone(fx.mediaFixture);
  },
  /** Full state (artwork, source app, sample time) or `null`. */
  async loadMediaState(): Promise<MediaState | null> {
    if (isTauri()) return invoke<MediaState | null>('load_media');
    return { ...clone(fx.mediaFixture), sampledAt: Date.now(), sourceApp: 'Spotify.exe', artwork: null };
  },

  // --- Clocks / currency ----------------------------------------------------

  /** World clocks (native: Rust store, one-time migration from localStorage; demo: localStorage). */
  async loadWorldClocks(): Promise<WorldClockZone[]> {
    if (!isTauri()) return readLocal<WorldClockZone[]>(CLOCKS_KEY) ?? clone(fx.worldClocksFixture);
    const legacy = readLocal<WorldClockZone[]>(CLOCKS_KEY);
    let zones: WorldClockZone[];
    if (legacy && legacy.length) {
      zones = await invoke<WorldClockZone[]>('save_world_clocks', { clocks: forStorage(legacy) });
      dropLocal(CLOCKS_KEY);
    } else {
      zones = await invoke<WorldClockZone[]>('load_world_clocks');
    }
    return withLocalZone(zones, localTimeZone());
  },
  async saveWorldClocks(zones: WorldClockZone[]): Promise<void> {
    if (!isTauri()) {
      writeLocal(CLOCKS_KEY, zones);
      return;
    }
    await invoke('save_world_clocks', { clocks: forStorage(zones) });
  },
  /** Rates per 1 USD. Empty when offline with nothing cached (never invented). */
  async loadCurrencyRates(): Promise<CurrencyRates> {
    if (!isTauri()) return clone(fx.currencyRatesFixture);
    try {
      return await invoke<CurrencyRates>('load_currency_rates');
    } catch (e) {
      console.error('[krovi] load_currency_rates failed', e);
      return {};
    }
  },
  /** Rates + when they were fetched / the publisher's date / staleness. `force` re-fetches. */
  async loadCurrencyInfo(force = false): Promise<CurrencyInfo> {
    if (!isTauri()) {
      return {
        rates: clone(fx.currencyRatesFixture),
        base: 'USD',
        date: null,
        updatedAt: fx.currencyUpdatedAt,
        stale: false,
        error: null,
      };
    }
    return invoke<CurrencyInfo>('load_currency_info', { force });
  },

  // --- Calendar (persisted) ---------------------------------------------------

  async loadCalendar(): Promise<CalendarEvent[]> {
    return isTauri() ? invoke<CalendarEvent[]>('load_calendar') : clone(demoCal());
  },
  /** Add a reminder; resolves to the whole, chronologically ordered list. Rejects on a bad date/time/title. */
  async addCalendarEvent(ev: { date: string; title: string; time?: string | null }): Promise<CalendarEvent[]> {
    if (isTauri()) {
      return invoke<CalendarEvent[]>('add_calendar_event', { date: ev.date, title: ev.title, time: ev.time ?? null });
    }
    const title = ev.title.trim();
    if (!title) throw new Error('title is empty');
    const list = demoCal();
    const id = list.reduce((m, e) => Math.max(m, e.id ?? 0), 0) + 1;
    list.push({ id, date: ev.date, title, ...(ev.time ? { time: ev.time } : {}) });
    list.sort((a, b) => a.date.localeCompare(b.date) || (a.time ?? '').localeCompare(b.time ?? '') || (a.id ?? 0) - (b.id ?? 0));
    return clone(list);
  },
  async removeCalendarEvent(id: number): Promise<CalendarEvent[]> {
    if (isTauri()) return invoke<CalendarEvent[]>('remove_calendar_event', { id });
    demoCalendar = demoCal().filter((e) => e.id !== id);
    return clone(demoCalendar);
  },

  // --- Events -------------------------------------------------------------------

  /** Fires when the backend creates a note (e.g. dictation): payload is the full note list. */
  onNotesChanged(cb: (notes: Note[]) => void): Promise<() => void> {
    return listen<Note[]>('notes-changed', cb);
  },
};
