<script lang="ts">
  // The separate search window opened by the global shortcut (F07-AC1): the search view alone,
  // in the language and theme of the settings; Esc hides it, opening a result hides it too.
  import { onMount } from 'svelte';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { setLanguage, type LangPreference } from '../lib/i18n.svelte';
  import { setTheme, type ThemePreference } from '../lib/theme';
  import SearchView from './SearchView.svelte';

  let volumes = $state<VolumeView[]>([]);
  let error = $state<string | null>(null);
  let view: SearchView | undefined = $state();

  const hide = () => void backend.hideSearch();

  onMount(() => {
    const offs: (() => void)[] = [];
    let alive = true;
    void (async () => {
      try {
        const s = await backend.getSettings();
        setLanguage(s.language as LangPreference);
        setTheme(s.theme as ThemePreference);
        volumes = await backend.volumes();
      } catch (e) {
        error = errorMessage(e);
      }
      const a = await backend.onSearchFocus(() => {
        view?.focus();
        void backend.volumes().then((v) => (volumes = v));
      });
      const b = await backend.onVolumesChanged(() => void backend.volumes().then((v) => (volumes = v)));
      if (alive) offs.push(a, b);
      else [a, b].forEach((f) => f());
      view?.focus();
    })();
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') hide();
    };
    window.addEventListener('keydown', esc);
    return () => {
      alive = false;
      offs.forEach((f) => f());
      window.removeEventListener('keydown', esc);
    };
  });
</script>

<main class="window">
  <SearchView bind:this={view} {volumes} compact onerror={(m) => (error = m)} onopened={hide} />
  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
</main>

<style>
  .window {
    height: 100%;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
</style>
