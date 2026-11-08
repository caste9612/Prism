import { mount } from 'svelte';
import './fonts';
import './app.css';
import App from './App.svelte';
import SearchWindow from './components/SearchWindow.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('missing #app element');

// The search window (F07-AC1) loads the same page with `?view=search`.
const searchWindow = new URLSearchParams(window.location.search).get('view') === 'search';

export default searchWindow ? mount(SearchWindow, { target }) : mount(App, { target });
