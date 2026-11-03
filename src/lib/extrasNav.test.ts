// How the palette sends the user to a section of the Extras tool: a one-time intent, and the tool id.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const openTool = vi.hoisted(() => vi.fn());
vi.mock('./stores', () => ({ openTool }));

import { EXTRAS_TOOL, extrasAvailable, extrasIntent, openExtras, takeExtrasIntent } from './extrasNav';

beforeEach(() => {
  openTool.mockClear();
  extrasIntent.set(null);
});

describe('openExtras', () => {
  it('opens the Extras tool and leaves a one-time intent for the view', () => {
    openExtras('workflows', { runWorkflow: 'wf-focus-time' });
    expect(openTool).toHaveBeenCalledWith(EXTRAS_TOOL);
    expect(EXTRAS_TOOL).toBe('extras');
    expect(get(extrasIntent)).toEqual({ tab: 'workflows', runWorkflow: 'wf-focus-time' });
  });

  it('hands the intent over exactly once', () => {
    openExtras('search', { query: 'invoice' });
    expect(takeExtrasIntent()).toEqual({ tab: 'search', query: 'invoice' });
    expect(takeExtrasIntent()).toBeNull();
    expect(get(extrasIntent)).toBeNull();
  });

  it('has no intent when nobody asked for one', () => {
    expect(takeExtrasIntent()).toBeNull();
  });
});

describe('extrasAvailable', () => {
  it('is a plain yes/no about whether the tool is registered in tools.ts', () => {
    expect(typeof extrasAvailable()).toBe('boolean');
  });
});
