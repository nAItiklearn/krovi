import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import {
  HINT_AFTER_MS,
  RECEIVED_CAP,
  __test,
  closeMessage,
  config,
  countdown,
  diagnose,
  emptyState,
  mergeShares,
  msLeft,
  needsHint,
  percent,
  progress,
  progressKey,
  receiveFromPhone,
  received,
  reduce,
  setToaster,
  shareFiles,
  shares,
  stopShare,
  type FirewallReport,
  type Share,
  type TransferState,
} from './transfer';

const share = (over: Partial<Share> = {}): Share => ({
  id: 's1',
  url: 'http://192.168.1.2:50000/s/x',
  qrSvg: '<svg/>',
  files: [{ name: 'a.pdf', size: 100 }],
  mode: 'send',
  expiresAt: 600_000,
  downloads: 0,
  lanIp: '192.168.1.2',
  port: 50000,
  createdAt: 0,
  uploads: 0,
  reached: false,
  pin: null,
  maxUploadBytes: 2 ** 31,
  receivedBytes: 0,
  saveDir: null,
  ...over,
});
const withShare = (...s: Share[]): TransferState => ({ ...emptyState(), shares: s });

describe('pure helpers', () => {
  it('percent clamps and handles unknown totals', () => {
    expect(percent(50, 200)).toBe(25);
    expect(percent(300, 200)).toBe(100);
    expect(percent(-5, 200)).toBe(0);
    expect(percent(5, 0)).toBe(0);
    expect(percent(5, NaN)).toBe(0);
  });

  it('countdown formats m:ss and h:mm:ss, never negative', () => {
    expect(countdown(600_000)).toBe('10:00');
    expect(countdown(9_000)).toBe('0:09');
    expect(countdown(9_001)).toBe('0:10'); // rounds up: never shows 0:00 early
    expect(countdown(3_723_000)).toBe('1:02:03');
    expect(countdown(-5)).toBe('0:00');
  });

  it('msLeft is clamped at zero', () => {
    expect(msLeft({ expiresAt: 1000 }, 400)).toBe(600);
    expect(msLeft({ expiresAt: 1000 }, 5000)).toBe(0);
  });

  it('shows the connection hint only after 20s with no phone', () => {
    const s = share({ createdAt: 1_000 });
    expect(needsHint(s, 1_000 + HINT_AFTER_MS - 1)).toBe(false);
    expect(needsHint(s, 1_000 + HINT_AFTER_MS)).toBe(true);
    expect(needsHint({ ...s, reached: true }, 1_000 + 10 * HINT_AFTER_MS)).toBe(false);
    // no createdAt from an older backend: fall back to expiry minus the default lifetime
    expect(needsHint(share({ createdAt: 0, expiresAt: 600_000 }), HINT_AFTER_MS)).toBe(true);
  });

  it('closeMessage covers every reason', () => {
    for (const r of ['expired', 'stopped', 'quit', 'locked'] as const) expect(closeMessage(r).length).toBeGreaterThan(5);
    expect(closeMessage('locked')).toMatch(/PIN/);
  });

  it('progressKey separates shares and files', () => {
    expect(progressKey('a', 'x.png')).not.toBe(progressKey('b', 'x.png'));
    expect(progressKey('a', 'x.png')).not.toBe(progressKey('a', 'y.png'));
  });
});

describe('diagnose (firewall panel text)', () => {
  const rep = (over: Partial<FirewallReport> = {}): FirewallReport => ({
    lanIp: '192.168.1.35',
    adapter: 'Wi-Fi',
    ruleInstalled: false,
    profile: 'Private',
    exe: 'krovi-app.exe',
    ...over,
  });

  it('explains a Public network (the rule Krovi adds is Private-only)', () => {
    const t = diagnose(rep({ profile: 'Public' })).join(' ');
    expect(t).toMatch(/Public/);
    expect(t).toMatch(/choose Private/);
  });
  it('mentions the address and adapter', () => {
    expect(diagnose(rep()).join(' ')).toMatch(/192\.168\.1\.35 \(Wi-Fi\)/);
  });
  it('reports a missing or installed rule', () => {
    expect(diagnose(rep()).join(' ')).toMatch(/no Krovi firewall rule/i);
    expect(diagnose(rep({ ruleInstalled: true })).join(' ')).toMatch(/rule is installed/i);
  });
  it('handles no network at all and a missing report', () => {
    expect(diagnose(rep({ lanIp: null })).join(' ')).toMatch(/no Wi-Fi or LAN/i);
    expect(diagnose(null)).toEqual([]);
  });
});

describe('reduce', () => {
  it('records progress only for known shares', () => {
    const p = { id: 's1', file: 'a.pdf', bytes: 10, total: 100, direction: 'send' as const };
    const r = reduce(withShare(share()), { type: 'progress', payload: p });
    expect(r.state.progress[progressKey('s1', 'a.pdf')]).toEqual(p);
    const ghost = reduce(withShare(share()), { type: 'progress', payload: { ...p, id: 'nope' } });
    expect(ghost.state.progress).toEqual({});
  });

  it('marks a share reached', () => {
    const r = reduce(withShare(share()), { type: 'reached', payload: { id: 's1' } });
    expect(r.state.shares[0].reached).toBe(true);
  });

  it('counts a finished download, clears its progress, and toasts', () => {
    const p = { id: 's1', file: 'a.pdf', bytes: 99, total: 100, direction: 'send' as const };
    let st = reduce(withShare(share()), { type: 'progress', payload: p }).state;
    const r = reduce(st, { type: 'done', payload: { id: 's1', file: 'a.pdf', direction: 'send', size: 100 } });
    expect(r.state.shares[0].downloads).toBe(1);
    expect(r.state.shares[0].reached).toBe(true);
    expect(r.state.shares[0].sent).toEqual(['a.pdf']);
    expect(r.state.progress).toEqual({});
    expect(r.toast).toBe('a.pdf sent to your phone');
    // downloading the same file again counts, but the "sent" mark is not duplicated
    const again = reduce(r.state, { type: 'done', payload: { id: 's1', file: 'a.pdf', direction: 'send', size: 100 } });
    expect(again.state.shares[0].downloads).toBe(2);
    expect(again.state.shares[0].sent).toEqual(['a.pdf']);
    // sending never touches the "received" history
    expect(r.state.received).toEqual([]);
  });

  it('records a received file with its saved path', () => {
    const rs = share({ id: 'r1', mode: 'receive', files: [] });
    const r = reduce(withShare(rs), {
      type: 'done',
      payload: { id: 'r1', file: 'IMG (1).jpg', direction: 'receive', size: 3_400_000, savedPath: 'C:\\Users\\x\\Downloads\\Krovi\\IMG (1).jpg' },
    }, 1234);
    expect(r.state.shares[0].uploads).toBe(1);
    expect(r.state.shares[0].receivedBytes).toBe(3_400_000);
    expect(r.state.shares[0].files).toEqual([{ name: 'IMG (1).jpg', size: 3_400_000 }]);
    expect(r.state.received[0]).toMatchObject({ name: 'IMG (1).jpg', size: 3_400_000, at: 1234, savedPath: 'C:\\Users\\x\\Downloads\\Krovi\\IMG (1).jpg' });
    expect(r.toast).toBe('Received IMG (1).jpg');
  });

  it('caps the received history, newest first', () => {
    let st = withShare(share({ id: 'r1', mode: 'receive', files: [] }));
    for (let i = 0; i < RECEIVED_CAP + 5; i++) {
      st = reduce(st, { type: 'done', payload: { id: 'r1', file: `f${i}.bin`, direction: 'receive', size: 1 } }, i).state;
    }
    expect(st.received).toHaveLength(RECEIVED_CAP);
    expect(st.received[0].name).toBe(`f${RECEIVED_CAP + 4}.bin`);
  });

  it('failure clears the progress row and surfaces the message', () => {
    const p = { id: 's1', file: 'a.pdf', bytes: 5, total: 100, direction: 'receive' as const };
    const st = reduce(withShare(share()), { type: 'progress', payload: p }).state;
    const r = reduce(st, { type: 'failed', payload: { id: 's1', file: 'a.pdf', direction: 'receive', message: 'Larger than the upload limit' } });
    expect(r.state.error).toBe('Larger than the upload limit');
    expect(r.state.progress).toEqual({});
    expect(r.toast).toBe('Larger than the upload limit');
  });

  it('closing removes the share and its progress only', () => {
    let st = withShare(share(), share({ id: 's2' }));
    st = reduce(st, { type: 'progress', payload: { id: 's1', file: 'a', bytes: 1, total: 2, direction: 'send' } }).state;
    st = reduce(st, { type: 'progress', payload: { id: 's2', file: 'b', bytes: 1, total: 2, direction: 'send' } }).state;
    const r = reduce(st, { type: 'closed', payload: { id: 's1', reason: 'expired' } });
    expect(r.state.shares.map((s) => s.id)).toEqual(['s2']);
    expect(Object.keys(r.state.progress)).toEqual([progressKey('s2', 'b')]);
    expect(r.toast).toBe('Share expired and was closed');
  });

  it('closing an unknown share is silent', () => {
    const r = reduce(emptyState(), { type: 'closed', payload: { id: 'zzz', reason: 'stopped' } });
    expect(r.toast).toBeUndefined();
    expect(r.state.shares).toEqual([]);
  });

  it('never mutates the previous state', () => {
    const before = withShare(share());
    const snapshot = JSON.stringify(before);
    reduce(before, { type: 'done', payload: { id: 's1', file: 'a.pdf', direction: 'send', size: 1 } });
    reduce(before, { type: 'closed', payload: { id: 's1', reason: 'stopped' } });
    expect(JSON.stringify(before)).toBe(snapshot);
  });
});

describe('mergeShares', () => {
  it('takes the backend list but keeps the client-only fields (demo flag, sent marks)', () => {
    const merged = mergeShares(
      [share({ demo: true, downloads: 0, sent: ['a.pdf'] })],
      [share({ downloads: 3 }), share({ id: 'new' })],
    );
    expect(merged.map((s) => s.id)).toEqual(['s1', 'new']);
    expect(merged[0].downloads).toBe(3);
    expect(merged[0].demo).toBe(true);
    expect(merged[0].sent).toEqual(['a.pdf']);
    expect(merged[1].demo).toBeUndefined();
    expect(merged[1].sent).toBeUndefined();
  });
});

describe('browser demo simulation', () => {
  const toasts: string[] = [];
  beforeEach(() => {
    vi.useFakeTimers();
    __test.reset();
    toasts.length = 0;
    setToaster((t) => toasts.push(t));
  });
  afterEach(() => {
    __test.reset();
    setToaster(null);
    vi.useRealTimers();
  });

  it('creates a clearly-marked placeholder share with a real QR svg', async () => {
    const s = await shareFiles([{ name: 'Report.pdf', size: 2_000_000 }]);
    expect(s.demo).toBe(true);
    expect(s.url).toContain('DEMO-not-a-real-link');
    expect(s.qrSvg.startsWith('<svg')).toBe(true);
    expect(s.qrSvg).toContain('viewBox');
    expect(s.files).toEqual([{ name: 'Report.pdf', size: 2_000_000 }]);
    expect(get(shares)).toHaveLength(1);
    expect(s.expiresAt - Date.now()).toBe(600_000);
    expect(s.reached).toBe(false);
    expect(s.pin).toBeNull();
  });

  it('a phone connects, downloads every file, with live progress', async () => {
    const s = await shareFiles([
      { name: 'a.pdf', size: 1_000_000 },
      { name: 'b.png', size: 2_000_000 },
    ]);
    await vi.advanceTimersByTimeAsync(3_000);
    expect(get(shares)[0].reached).toBe(true);
    await vi.advanceTimersByTimeAsync(1_200);
    const live = Object.values(get(progress));
    expect(live.length).toBe(1); // sequential: one file at a time
    expect(live[0].direction).toBe('send');
    expect(live[0].bytes).toBeGreaterThan(0);
    expect(live[0].bytes).toBeLessThan(live[0].total);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(get(shares)[0].downloads).toBe(2);
    expect(get(progress)).toEqual({});
    expect(toasts).toContain('a.pdf sent to your phone');
    expect(toasts).toContain('b.png sent to your phone');
    await stopShare(s.id);
  });

  it('stopping cancels the simulated transfer cleanly', async () => {
    const s = await shareFiles([{ name: 'a.pdf', size: 5_000_000 }]);
    await vi.advanceTimersByTimeAsync(4_000);
    expect(Object.keys(get(progress))).toHaveLength(1);
    await stopShare(s.id);
    expect(get(shares)).toEqual([]);
    expect(get(progress)).toEqual({});
    expect(toasts).toContain('Sharing stopped');
    // nothing resurrects it afterwards
    await vi.advanceTimersByTimeAsync(60_000);
    expect(get(shares)).toEqual([]);
    expect(get(received)).toEqual([]);
  });

  it('expires on schedule with a toast', async () => {
    await shareFiles([{ name: 'a.pdf', size: 10 }], { ttlSecs: 60 });
    await vi.advanceTimersByTimeAsync(59_000);
    expect(get(shares)).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(get(shares)).toEqual([]);
    expect(toasts).toContain('Share expired and was closed');
  });

  it('receive mode simulates phone uploads into the received list', async () => {
    const s = await receiveFromPhone();
    expect(s.mode).toBe('receive');
    expect(s.saveDir).toContain('Downloads');
    await vi.advanceTimersByTimeAsync(60_000);
    expect(get(shares)[0].uploads).toBe(3);
    expect(get(received)).toHaveLength(3);
    expect(get(received)[0].savedPath).toContain('Krovi');
    await stopShare();
    expect(get(shares)).toEqual([]);
  });

  it('honours the PIN setting and rejects empty shares', async () => {
    config.update((c) => ({ ...c, requirePin: true }));
    const s = await shareFiles([{ name: 'x', size: 1 }]);
    expect(s.pin).toMatch(/^\d{4}$/);
    await expect(shareFiles([])).rejects.toThrow(/at least one file/i);
    await stopShare();
  });
});
