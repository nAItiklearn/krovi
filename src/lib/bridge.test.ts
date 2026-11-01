import { describe, expect, it } from 'vitest';
import { bridge, mode, normalizeIcon, normalizeWeather, withLocalZone } from './bridge';
import { weatherFixture } from './fixtures';

describe('bridge in demo mode (no Tauri)', () => {
  it('reports demo mode in a plain environment', () => {
    expect(mode()).toBe('demo');
  });

  it('serves fixtures only in demo mode', async () => {
    expect((await bridge.loadClipboard()).length).toBeGreaterThan(0);
    expect((await bridge.loadWeather())?.city).toBe(weatherFixture.city);
    expect((await bridge.loadWeatherStatus()).status).toBe('ok');
    expect((await bridge.loadCurrencyInfo()).rates.USD).toBe(1);
  });

  it('calendar add/remove behaves like the backend: validated, ordered, ids assigned', async () => {
    const before = await bridge.loadCalendar();
    const after = await bridge.addCalendarEvent({ date: '2030-01-02', title: '  Dentist  ', time: '09:30' });
    expect(after).toHaveLength(before.length + 1);
    const added = after.find((x) => x.title === 'Dentist')!;
    expect(added.id).toBeGreaterThan(0);
    expect(added.time).toBe('09:30');
    // chronological
    const dates = after.map((x) => x.date);
    expect([...dates].sort()).toEqual(dates);
    await expect(bridge.addCalendarEvent({ date: '2030-01-02', title: '   ' })).rejects.toThrow();
    const removed = await bridge.removeCalendarEvent(added.id!);
    expect(removed.find((x) => x.id === added.id)).toBeUndefined();
    expect(removed).toHaveLength(before.length);
  });

  it('city search/selection works against the demo list and rejects unknown cities', async () => {
    const hits = await bridge.searchPlaces('pune');
    expect(hits[0].name).toContain('Pune');
    const reply = await bridge.setWeatherPlace(hits[0]);
    expect(reply.weather?.city).toContain('Pune');
    await expect(bridge.setWeatherCity('zzzzqq')).rejects.toThrow(/No place found/);
  });

  it('notes-changed has a harmless no-op listener without a backend', async () => {
    const off = await bridge.onNotesChanged(() => {});
    expect(typeof off).toBe('function');
    off();
  });
});

describe('normalizers', () => {
  it('maps storms to a renderable icon and unknown codes to cloud', () => {
    expect(normalizeIcon('storm')).toBe('rain');
    expect(normalizeIcon('sun')).toBe('sun');
    expect(normalizeIcon('tornado')).toBe('cloud');
  });

  it('normalizes every icon in a forecast and passes null through', () => {
    const w = normalizeWeather({
      ...weatherFixture,
      icon: 'storm' as never,
      hourly: [{ time: 'Now', temp: 1, icon: 'storm' as never }],
      forecast: [{ label: 'Today', icon: 'storm' as never, low: 0, high: 1 }],
    })!;
    expect(w.icon).toBe('rain');
    expect(w.hourly[0].icon).toBe('rain');
    expect(w.forecast[0].icon).toBe('rain');
    expect(normalizeWeather(null)).toBeNull();
  });

  it('fills only the Local zone from the machine zone', () => {
    const zones = withLocalZone(
      [
        { label: 'Local', tz: '' },
        { label: 'Tokyo', tz: 'Asia/Tokyo' },
      ],
      'Asia/Kolkata',
    );
    expect(zones[0].tz).toBe('Asia/Kolkata');
    expect(zones[1].tz).toBe('Asia/Tokyo');
    expect(withLocalZone([{ label: 'Local', tz: '' }], '')[0].tz).toBe('UTC');
    // an already-set Local zone is left alone
    expect(withLocalZone([{ label: 'Local', tz: 'Europe/Paris' }], 'Asia/Kolkata')[0].tz).toBe('Europe/Paris');
  });
});
