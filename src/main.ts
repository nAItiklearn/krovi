import { mount } from 'svelte';
import './styles/tokens.css';
import './styles/base.css';
import './styles/components.css';
import { initEnv } from './lib/stores';
import App from './App.svelte';
// Theme sheets (src/styles/themes/*.css) are pulled in by lib/theme.ts, which App imports after
// the three sheets above, so they come later in the cascade and win at equal specificity.

// Stamp data-env / data-embed before the first paint (the theme is stamped by App.svelte at init).
initEnv();

const target = document.getElementById('app');
if (!target) {
  throw new Error('Krovi: #app mount point not found');
}

const app = mount(App, { target });

export default app;
