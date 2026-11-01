// Entry for the idle-companion preview harness (dev only; never imported by the app bundle).
//
//   standalone:  http://localhost:<port>/src/components/idle/preview.html
//   from main.ts (optional, one line — see the report):
//     if (location.search.includes('idle-preview')) void import('./components/idle/preview-main').then((m) => m.mountPreview())

import { mount } from 'svelte';
import '../../styles/tokens.css';
import '../../styles/base.css';
import '../../styles/components.css';
import { startTheme } from '../../lib/theme';
import Preview from './Preview.svelte';

export function mountPreview(target: HTMLElement = document.getElementById('app') as HTMLElement) {
  document.documentElement.dataset.env ||= 'demo';
  startTheme();
  return mount(Preview, { target });
}

// When loaded straight from preview.html, mount right away.
if (document.getElementById('krovi-idle-preview')) mountPreview();
