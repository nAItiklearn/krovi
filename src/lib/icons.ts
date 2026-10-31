// Original line-icon set (24x24, stroke=currentColor). Hand-drawn generic
// glyphs — no third-party icon assets (clean-room, D-002). Each value is the
// inner markup of an <svg>; rendered by Icon.svelte.

export const icons: Record<string, string> = {
  home: '<path d="M4 11.5 12 4l8 7.5"/><path d="M6 10v9h12v-9"/><path d="M10 19v-5h4v5"/>',
  clipboard:
    '<rect x="6" y="4" width="12" height="17" rx="2"/><path d="M9 4.5A1.5 1.5 0 0 1 10.5 3h3A1.5 1.5 0 0 1 15 4.5V6H9z"/><path d="M9 11h6M9 14.5h6M9 18h4"/>',
  paste:
    '<rect x="4" y="7" width="12" height="14" rx="2"/><path d="M8 7V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-2"/><path d="M7 12h6M7 15.5h4"/>',
  timers:
    '<circle cx="12" cy="13" r="8"/><path d="M12 13V8.5"/><path d="M9.5 2.5h5"/><path d="M18.5 6.5l1.5-1.5"/>',
  pomodoro:
    '<path d="M12 6c4.4 0 8 3.3 8 7.5S16.4 21 12 21s-8-3.3-8-7.5S7.6 6 12 6Z"/><path d="M12 6c-1-1.8-.4-3.2 1.2-4 .2 1.6-.2 3-1.2 4Z"/><path d="M12 10v3.5l2.5 1.5"/>',
  shelf:
    '<path d="M4 7a2 2 0 0 1 2-2h3l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z"/>',
  media:
    '<circle cx="8" cy="17" r="2.5"/><path d="M10.5 17V7l9-2v9"/><circle cx="17" cy="14" r="2.5"/>',
  calendar:
    '<rect x="4" y="5" width="16" height="16" rx="2"/><path d="M4 9h16M8 3v4M16 3v4"/><path d="M8 13h2v2H8zM14 13h2v2h-2z"/>',
  weather:
    '<circle cx="8" cy="9" r="3"/><path d="M8 2.5V4M3.5 9H5M8 15.5V14M12.5 9H11M4.6 5.6l1 1M11.4 5.6l-1 1"/><path d="M9 18a4 4 0 0 1 4-4 4 4 0 0 1 3.9 3.1A3 3 0 0 1 16.5 23H9a2.5 2.5 0 0 1 0-5Z"/>',
  notes:
    '<path d="M6 4h9l5 5v11a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z"/><path d="M15 4v5h5"/><path d="M8 13h8M8 16.5h5"/>',
  clocks:
    '<circle cx="12" cy="12" r="8.5"/><path d="M12 7v5l3.5 2"/><path d="M3.5 12h17"/>',
  convert:
    '<path d="M4 8h13l-3-3M20 16H7l3 3"/>',
  calculator:
    '<rect x="5" y="3" width="14" height="18" rx="2"/><path d="M8 7h8"/><path d="M8.5 12h.01M12 12h.01M15.5 12h.01M8.5 15.5h.01M12 15.5h.01M15.5 15.5v3"/>',
  screentime:
    '<path d="M4 20h16"/><rect x="6" y="11" width="3" height="7" rx="1"/><rect x="11" y="6" width="3" height="12" rx="1"/><rect x="16" y="14" width="3" height="4" rx="1"/>',
  settings:
    '<circle cx="12" cy="12" r="3.2"/><path d="M19 12a7 7 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7 7 0 0 0-2-1.2L16 2h-4l-.5 2.6a7 7 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6A7 7 0 0 0 5 12a7 7 0 0 0 .1 1.2l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 2 1.2L12 22h4l-.5-2.6a7 7 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6A7 7 0 0 0 19 12Z"/>',
  // utility glyphs
  close: '<path d="M6 6l12 12M18 6 6 18"/>',
  pin: '<path d="M9 3h6l-1 6 3 3v2h-5v5l-1 2-1-2v-5H4v-2l3-3z"/>',
  search: '<circle cx="11" cy="11" r="6.5"/><path d="m20 20-3.5-3.5"/>',
  play: '<path d="M8 5l12 7-12 7z"/>',
  pause: '<path d="M8 5h3v14H8zM15 5h3v14h-3z"/>',
  next: '<path d="M7 5l9 7-9 7zM18 5v14"/>',
  prev: '<path d="M17 5l-9 7 9 7zM6 5v14"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  minus: '<path d="M5 12h14"/>',
  chevronLeft: '<path d="M15 5l-7 7 7 7"/>',
  chevronRight: '<path d="M9 5l7 7-7 7"/>',
  copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/>',
  trash: '<path d="M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13"/>',
  check: '<path d="M5 13l4 4 10-11"/>',
  sun: '<circle cx="12" cy="12" r="4.5"/><path d="M12 2v2.5M12 19.5V22M2 12h2.5M19.5 12H22M4.9 4.9l1.8 1.8M17.3 17.3l1.8 1.8M19.1 4.9l-1.8 1.8M6.7 17.3l-1.8 1.8"/>',
  cloud: '<path d="M7 18a4 4 0 0 1 .5-8 5 5 0 0 1 9.6 1.4A3.5 3.5 0 0 1 16.5 18Z"/>',
  partly:
    '<circle cx="8" cy="8" r="3"/><path d="M8 2.5V4M2.5 8H4M4.6 4.6l1 1"/><path d="M9 18a4 4 0 0 1 4-4 4 4 0 0 1 3.9 3.1A3 3 0 0 1 16.5 21H9a2 2 0 0 1 0-3Z"/>',
  rain: '<path d="M7 14a4 4 0 0 1 .5-8 5 5 0 0 1 9.6 1.4A3.5 3.5 0 0 1 16.5 14Z"/><path d="M8 18l-1 2M12 18l-1 2M16 18l-1 2"/>',
  snow: '<path d="M7 13a4 4 0 0 1 .5-8 5 5 0 0 1 9.6 1.4A3.5 3.5 0 0 1 16.5 13Z"/><path d="M9 18h.01M12 20h.01M15 18h.01"/>',
  drop: '<path d="M12 3s6 6.5 6 11a6 6 0 0 1-12 0c0-4.5 6-11 6-11Z"/>',
  wind: '<path d="M4 9h11a2.5 2.5 0 1 0-2.5-2.5M4 14h15a3 3 0 1 1-3 3"/>',
  moon: '<path d="M20 14.5A8 8 0 0 1 9.5 4 7 7 0 1 0 20 14.5Z"/>',
  flag: '<path d="M6 21V4M6 4h11l-2 3 2 3H6"/>',
  grip: '<path d="M9 6h.01M9 12h.01M9 18h.01M15 6h.01M15 12h.01M15 18h.01"/>',
  lap: '<circle cx="12" cy="12" r="8"/><path d="M12 12l4-2"/>',
  reset: '<path d="M5 12a7 7 0 1 1 2 5"/><path d="M4 9v4h4"/>',
};

export type IconName = keyof typeof icons;
