// Shared types for the Krovi panel UI.
//
// The record types here deliberately mirror the public shapes of `krovi-core`
// (see src-tauri/crates/krovi-core/src/*.rs) so the same objects flow whether
// they come from in-memory demo fixtures or, inside the Tauri shell, from the
// Rust core via `invoke`. Timestamps are epoch milliseconds, matching core.

export type ClipKind = 'Text' | 'Link' | 'Image';

export interface ClipEntry {
  id: number;
  kind: ClipKind;
  /** Text, a URL, or a blob reference for images. */
  content: string;
  pinned: boolean;
  created_at: number;
}

export interface Note {
  id: number;
  body: string;
  created_at: number;
  updated_at: number;
}

export interface Task {
  id: number;
  title: string;
  done: boolean;
  created_at: number;
  completed_at: number | null;
}

export interface ShelfItem {
  id: number;
  name: string;
  stored_path: string;
  size_bytes: number;
  added_at: number;
}

export interface StoredShelf {
  name: string;
  items: ShelfItem[];
}

export interface ScreenTimeRecord {
  day: number;
  app: string;
  ms: number;
}

// --- UI-only fixture shapes (tools without a core module yet) ---------------

export interface WeatherForecastDay {
  label: string;
  icon: WeatherIcon;
  low: number;
  high: number;
}

export type WeatherIcon = 'sun' | 'cloud' | 'partly' | 'rain' | 'snow';

export interface WeatherData {
  city: string;
  current: number;
  condition: string;
  icon: WeatherIcon;
  high: number;
  low: number;
  feelsLike: number;
  humidity: number;
  windKph: number;
  uvIndex: number;
  hourly: { time: string; temp: number; icon: WeatherIcon }[];
  forecast: WeatherForecastDay[];
}

export interface MediaTrack {
  title: string;
  artist: string;
  album: string;
  playing: boolean;
  positionSec: number;
  durationSec: number;
}

export interface WorldClockZone {
  label: string;
  /** IANA time zone, e.g. "Asia/Kolkata". */
  tz: string;
}

/** Amount of each currency per 1 USD. */
export type CurrencyRates = Record<string, number>;

export interface CalendarEvent {
  /** ISO date yyyy-mm-dd. */
  date: string;
  title: string;
  time?: string;
}

export type ToolId =
  | 'home'
  | 'clipboard'
  | 'paste'
  | 'timers'
  | 'pomodoro'
  | 'shelf'
  | 'media'
  | 'calendar'
  | 'weather'
  | 'notes'
  | 'clocks'
  | 'convert'
  | 'calculator'
  | 'screentime'
  | 'settings';
