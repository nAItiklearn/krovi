import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('Krovi: #app mount point not found');
}

const app = mount(App, { target });

export default app;
