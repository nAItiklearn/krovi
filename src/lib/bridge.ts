// The one seam between the UI and its data source.
//
// The frontend NEVER hard-depends on Tauri to render. `isTauri()` checks for the
// injected Tauri internals; in a plain browser (DEMO MODE) every loader returns
// a deep clone of the fixtures so the panel is fully interactive with no backend.
// Inside the Tauri shell the same functions would call the Rust `#[tauri::command]`
// bridge via `invoke` (thin wrappers over krovi-core + krovi-store MemStore).

import type {
  CalendarEvent,
  ClipEntry,
  CurrencyRates,
  MediaTrack,
  Note,
  ScreenTimeRecord,
  StoredShelf,
  Task,
  WeatherData,
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
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const api = await import('@tauri-apps/api/core');
  return api.invoke<T>(cmd, args);
}

export const bridge = {
  async loadClipboard(): Promise<ClipEntry[]> {
    return isTauri() ? invoke<ClipEntry[]>('load_clipboard') : clone(fx.clipboardFixture);
  },
  async loadPasteStack(): Promise<string[]> {
    return isTauri() ? invoke<string[]>('load_paste_stack') : clone(fx.pasteStackFixture);
  },
  async loadNotes(): Promise<Note[]> {
    return isTauri() ? invoke<Note[]>('load_notes') : clone(fx.notesFixture);
  },
  async loadTasks(): Promise<Task[]> {
    return isTauri() ? invoke<Task[]>('load_tasks') : clone(fx.tasksFixture);
  },
  async loadShelves(): Promise<StoredShelf[]> {
    return isTauri() ? invoke<StoredShelf[]>('load_shelves') : clone(fx.shelvesFixture);
  },
  async loadScreenTime(): Promise<ScreenTimeRecord[]> {
    return isTauri() ? invoke<ScreenTimeRecord[]>('load_screen_time') : clone(fx.screenTimeFixture);
  },
  async loadWeather(): Promise<WeatherData> {
    return isTauri() ? invoke<WeatherData>('load_weather') : clone(fx.weatherFixture);
  },
  async loadMedia(): Promise<MediaTrack> {
    return isTauri() ? invoke<MediaTrack>('load_media') : clone(fx.mediaFixture);
  },
  async loadWorldClocks(): Promise<WorldClockZone[]> {
    return isTauri() ? invoke<WorldClockZone[]>('load_world_clocks') : clone(fx.worldClocksFixture);
  },
  async loadCurrencyRates(): Promise<CurrencyRates> {
    return isTauri() ? invoke<CurrencyRates>('load_currency_rates') : clone(fx.currencyRatesFixture);
  },
  async loadCalendar(): Promise<CalendarEvent[]> {
    return isTauri() ? invoke<CalendarEvent[]>('load_calendar') : clone(fx.calendarFixture);
  },
};
