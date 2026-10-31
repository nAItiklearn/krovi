/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri expects a fixed dev port and ignores src-tauri when watching.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte()],
  // Krovi uses a relative base so the built assets load both from the Tauri
  // webview (tauri://) and from a static file server.
  base: './',
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: false,
    host: host || false,
    watch: {
      // Never watch the Rust side from the frontend dev server.
      ignored: ['**/src-tauri/**'],
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2021',
    sourcemap: false,
  },
  test: {
    // Pure-logic unit tests (calculator, units, clipboard filter, time fmt).
    environment: 'node',
    include: ['src/**/*.{test,spec}.ts'],
  },
});
