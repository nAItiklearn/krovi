// Realistic in-memory demo data. In DEMO MODE (plain browser, no Tauri) every
// view is driven from these fixtures so the whole panel is clickable with no
// backend. Shapes mirror krovi-core records (see types.ts); timestamps are real
// epoch-ms offsets from load time so "x ago" labels read naturally.

import type {
  CalendarEvent,
  ClipEntry,
  CurrencyRates,
  MediaState,
  MediaTrack,
  Note,
  ScreenTimeRecord,
  StoredShelf,
  SystemStats,
  Task,
  WeatherData,
  WorldClockZone,
} from './types';

const NOW = Date.now();
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

export const clipboardFixture: ClipEntry[] = [
  { id: 9, kind: 'Text', content: 'Studio hours: Mon–Fri, 10am – 7pm. Sat by appointment.', pinned: true, created_at: NOW - 23 * HOUR },
  { id: 8, kind: 'Link', content: 'https://github.com/nAItiklearn/krovi', pinned: false, created_at: NOW - 4 * MIN },
  { id: 7, kind: 'Text', content: 'Linen tote — natural, 38 x 42 cm, $24', pinned: false, created_at: NOW - 12 * MIN },
  { id: 6, kind: 'Image', content: 'blobs/screenshot-1600x1000.png', pinned: false, created_at: NOW - 18 * MIN },
  { id: 5, kind: 'Text', content: '#0e9e8f', pinned: false, created_at: NOW - 40 * MIN },
  { id: 4, kind: 'Link', content: 'https://krovi.tech/compare', pinned: false, created_at: NOW - 2 * HOUR },
  { id: 3, kind: 'Text', content: 'npm run dev — opens the clickable demo', pinned: false, created_at: NOW - 3 * HOUR },
  { id: 2, kind: 'Link', content: 'https://www.rust-lang.org', pinned: false, created_at: NOW - 5 * HOUR },
  { id: 1, kind: 'Text', content: 'Meeting moved to 3pm Thursday', pinned: false, created_at: NOW - 8 * HOUR },
];

export const pasteStackFixture: string[] = [
  'INV-2026-0042',
  'naitik@example.com',
  'https://krovi.tech',
  'The quick brown fox jumps over the lazy dog.',
];

export const notesFixture: Note[] = [
  { id: 3, body: 'Launch checklist\n- [x] core tests green\n- [x] website live\n- [ ] package installer', created_at: NOW - 2 * DAY, updated_at: NOW - 6 * MIN },
  { id: 2, body: 'Call the framer back about the 30x40 prints.', created_at: NOW - 1 * DAY, updated_at: NOW - 3 * HOUR },
  { id: 1, body: 'Palette: teal #0e9e8f (light) / #22c7b4 (dark). Keep contrast AA.', created_at: NOW - 3 * DAY, updated_at: NOW - 1 * DAY },
];

export const tasksFixture: Task[] = [
  { id: 1, title: 'Ship the browser demo', done: true, created_at: NOW - 2 * DAY, completed_at: NOW - 1 * HOUR },
  { id: 2, title: 'Wire every tool view to fixtures', done: true, created_at: NOW - 2 * DAY, completed_at: NOW - 30 * MIN },
  { id: 3, title: 'Write unit tests (calc, units, clipboard)', done: false, created_at: NOW - 1 * DAY, completed_at: null },
  { id: 4, title: 'Draft the keyboard-shortcuts help', done: false, created_at: NOW - 1 * DAY, completed_at: null },
  { id: 5, title: 'Review reduced-motion + contrast', done: false, created_at: NOW - 3 * HOUR, completed_at: null },
];

export const shelvesFixture: StoredShelf[] = [
  {
    name: 'Desk',
    items: [
      { id: 5, name: 'Moodboard.png', stored_path: 'blobs/moodboard', size_bytes: 127 * 1024, added_at: NOW - 20 * MIN },
      { id: 4, name: 'Q4-roadmap.pdf', stored_path: 'blobs/q4', size_bytes: Math.round(2.3 * 1024 * 1024), added_at: NOW - 1 * HOUR },
      { id: 3, name: 'Invoice-2026-0042.pdf', stored_path: 'blobs/inv', size_bytes: 180 * 1024, added_at: NOW - 3 * HOUR },
      { id: 2, name: 'Interview-notes.docx', stored_path: 'blobs/notes', size_bytes: 48 * 1024, added_at: NOW - 5 * HOUR },
      { id: 1, name: 'Site-photos.zip', stored_path: 'blobs/photos', size_bytes: Math.round(37.1 * 1024 * 1024), added_at: NOW - 1 * DAY },
    ],
  },
  {
    name: 'Receipts',
    items: [
      { id: 7, name: 'Framing-receipt.jpg', stored_path: 'blobs/framing', size_bytes: 96 * 1024, added_at: NOW - 2 * DAY },
      { id: 6, name: 'Hosting-2026.pdf', stored_path: 'blobs/hosting', size_bytes: 41 * 1024, added_at: NOW - 4 * DAY },
    ],
  },
];

/** Day index = days since the Unix epoch (matches krovi-core ScreenTime). */
export const today = Math.floor(NOW / DAY);

export const screenTimeFixture: ScreenTimeRecord[] = [
  { day: today, app: 'Code.exe', ms: 102 * MIN },
  { day: today, app: 'firefox.exe', ms: 74 * MIN },
  { day: today, app: 'WindowsTerminal.exe', ms: 38 * MIN },
  { day: today, app: 'Figma.exe', ms: 26 * MIN },
  { day: today, app: 'Spotify.exe', ms: 19 * MIN },
  { day: today, app: 'Explorer.exe', ms: 7 * MIN },
  { day: today - 1, app: 'Code.exe', ms: 141 * MIN },
  { day: today - 1, app: 'firefox.exe', ms: 63 * MIN },
  { day: today - 1, app: 'Slack.exe', ms: 44 * MIN },
];

export const weatherFixture: WeatherData = {
  city: 'Pune',
  current: 27,
  condition: 'Partly cloudy',
  icon: 'partly',
  high: 31,
  low: 22,
  feelsLike: 29,
  humidity: 58,
  windKph: 11,
  uvIndex: 5,
  hourly: [
    { time: 'Now', temp: 27, icon: 'partly' },
    { time: '13:00', temp: 29, icon: 'sun' },
    { time: '14:00', temp: 30, icon: 'sun' },
    { time: '15:00', temp: 30, icon: 'partly' },
    { time: '16:00', temp: 28, icon: 'cloud' },
    { time: '17:00', temp: 26, icon: 'cloud' },
  ],
  forecast: [
    { label: 'Today', icon: 'partly', low: 22, high: 31 },
    { label: 'Thu', icon: 'sun', low: 23, high: 33 },
    { label: 'Fri', icon: 'partly', low: 22, high: 32 },
    { label: 'Sat', icon: 'rain', low: 21, high: 28 },
    { label: 'Sun', icon: 'cloud', low: 21, high: 29 },
  ],
};

export const mediaFixture: MediaTrack = {
  title: 'Midnight Drive',
  artist: 'Neon Harbor',
  album: 'After Hours',
  playing: true,
  positionSec: 72,
  durationSec: 214,
};

// --- Live feed (demo mode only; the Tauri build never reads these) -----------

const GB = 1024 ** 3;

/** Starting point for the simulated stats feed. */
export const statsFixture: SystemStats = {
  memUsed: 9.4 * GB,
  memTotal: 16 * GB,
  diskUsed: 301 * GB,
  diskTotal: 476 * GB,
  diskLabel: 'C:',
  cpu: 14,
};

/** Tracks the demo media feed cycles through (artwork-less on purpose). */
export const demoMediaQueue: Omit<MediaState, 'sampledAt' | 'positionSec' | 'playing'>[] = [
  { title: 'Midnight Drive', artist: 'Neon Harbor', album: 'After Hours', durationSec: 214, sourceApp: 'Spotify.exe', artwork: null },
  { title: 'Paper Planes', artist: 'Lantern', album: 'Field Notes', durationSec: 198, sourceApp: 'Spotify.exe', artwork: null },
  { title: 'Slow Tide', artist: 'Marigold', album: 'Coastlines', durationSec: 243, sourceApp: 'Spotify.exe', artwork: null },
];

/** What the demo "copies" every ~12 s so the pill and clipboard view feel alive. */
export const demoCopySnippets: string[] = [
  'https://krovi.tech/features',
  'Ship the demo before lunch',
  'rgb(14, 158, 143)',
  'cargo build --release',
  'https://github.com/nAItiklearn/krovi/issues',
  'Meeting notes — Friday 10:30, room 4B',
  'INV-2026-0043',
  'hello@krovi.tech',
];

export const worldClocksFixture: WorldClockZone[] = [
  { label: 'Local', tz: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC' },
  { label: 'San Francisco', tz: 'America/Los_Angeles' },
  { label: 'New York', tz: 'America/New_York' },
  { label: 'London', tz: 'Europe/London' },
  { label: 'Mumbai', tz: 'Asia/Kolkata' },
  { label: 'Tokyo', tz: 'Asia/Tokyo' },
];

export const currencyRatesFixture: CurrencyRates = {
  USD: 1,
  EUR: 0.92,
  GBP: 0.79,
  INR: 83.2,
  JPY: 149.5,
  AUD: 1.52,
  CAD: 1.36,
};

/** Cached-rates timestamp, shown in the Convert view. */
export const currencyUpdatedAt = NOW - 6 * HOUR;

function iso(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

const base = new Date(NOW);
export const calendarFixture: CalendarEvent[] = [
  { id: 1, date: iso(base), title: 'Design review', time: '15:00' },
  { id: 2, date: iso(base), title: 'Framer callback', time: '17:30' },
  { id: 3, date: iso(new Date(NOW + 1 * DAY)), title: 'Ship v0.1 demo', time: '11:00' },
  { id: 4, date: iso(new Date(NOW + 3 * DAY)), title: 'Studio open day' },
  { id: 5, date: iso(new Date(NOW + 6 * DAY)), title: 'Invoice due', time: '09:00' },
  { id: 6, date: iso(new Date(NOW - 2 * DAY)), title: 'Website deployed' },
];
