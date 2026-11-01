// Production build of the idle-companion preview harness only, for honest CPU/RAM numbers
// (minified, no HMR socket, no dev-server noise from other agents editing files).
//   npx vite build --config qa/personalization/vite.preview.config.ts
//   npx vite preview --config qa/personalization/vite.preview.config.ts --port 5182
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { resolve } from 'node:path';

export default defineConfig({
  root: resolve(__dirname, '../..'),
  plugins: [svelte()],
  base: './',
  build: {
    outDir: resolve(__dirname, 'dist-preview'),
    emptyOutDir: true,
    target: 'es2021',
    rollupOptions: { input: resolve(__dirname, '../../src/components/idle/preview.html') },
  },
  preview: { port: 5182, strictPort: true },
});
