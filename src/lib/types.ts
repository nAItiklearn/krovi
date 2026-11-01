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

// --- Live data (docs/CONTRACTS.md §2) ---------------------------------------

/** Payload of the `media-changed` event (null = nothing is playing). */
export interface MediaState extends MediaTrack {
  /** epoch ms when positionSec was sampled — the UI interpolates while playing */
  sampledAt: number;
  /** e.g. "Spotify.exe", "chrome.exe", "MSEdge" */
  sourceApp: string;
  /** data:image/...;base64 thumbnail, or null */
  artwork: string | null;
}

/** Payload of the `stats-tick` event. Byte counts; cpu is 0..100. */
export interface SystemStats {
  memUsed: number;
  memTotal: number;
  diskUsed: number;
  diskTotal: number;
  /** system drive, e.g. "C:" */
  diskLabel: string;
  cpu: number;
}

/** Payload of the `clipboard-changed` event. */
export interface ClipboardChanged {
  entries: ClipEntry[];
  /** id of a NEW entry (drives the pill glow), or null for pure mutations */
  newId: number | null;
}

/** Return value of the `live_snapshot` command. */
export interface LiveSnapshot {
  clipboard: ClipEntry[];
  media: MediaState | null;
  stats: SystemStats | null;
}

/** A city candidate from the keyless geocoder. */
export interface Place {
  /** "Pune, Maharashtra, India" */
  name: string;
  lat: number;
  lon: number;
}

/**
 * Reply of `weather_status` & friends.
 * - `ok`: fresh data.
 * - `offline`: the refresh failed; `weather` may be a stale cache (see `stale`/`error`).
 * - `needs-city`: no place is known (Windows Location unavailable) — ask the user for a city.
 */
export interface WeatherReply {
  status: 'ok' | 'offline' | 'needs-city';
  weather: WeatherData | null;
  stale: boolean;
  /** epoch ms of the forecast shown */
  fetchedAt: number | null;
  place: { city: string; lat: number; lon: number } | null;
  error: string | null;
}

/** FX table + provenance (`load_currency_info`). Rates are per 1 USD (USD itself = 1). */
export interface CurrencyInfo {
  rates: CurrencyRates;
  base: string;
  /** publisher's date, yyyy-mm-dd */
  date: string | null;
  /** epoch ms when fetched */
  updatedAt: number | null;
  stale: boolean;
  error: string | null;
}

export interface WorldClockZone {
  label: string;
  /** IANA time zone, e.g. "Asia/Kolkata". */
  tz: string;
}

/** Amount of each currency per 1 USD. */
export type CurrencyRates = Record<string, number>;

export interface CalendarEvent {
  /** Assigned by the backend (absent on a not-yet-saved draft). Needed to remove. */
  id?: number;
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
  | 'transfer'
  | 'explain'
  | 'settings';
