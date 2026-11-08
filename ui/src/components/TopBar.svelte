<script lang="ts">
  // The bar at the top of the main window (ADR-0019): the mark, where we are, the search box
  // (always there), the new disks found (F02-AC1) and the settings.
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { volumeName } from '../lib/catalog';
  import { formatBytes } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import { nav } from '../lib/nav.svelte';
  import Icon from './Icon.svelte';
  import Logo from './Logo.svelte';
  import Menu from './Menu.svelte';

  let {
    volumes,
    prompts,
    shortcut,
    onindex,
    onindexall,
    onnotnow,
    onnotnowall,
    onlabels,
    onignore,
  }: {
    volumes: VolumeView[];
    /** Connected disks never indexed and not dismissed (F02-AC1). */
    prompts: VolumeView[];
    shortcut: string;
    onindex: (v: VolumeView) => void;
    onindexall: () => void;
    onnotnow: (v: VolumeView) => void;
    onnotnowall: () => void;
    onlabels: (v: VolumeView) => void;
    onignore: (v: VolumeView) => void;
  } = $props();

  const lang = $derived(language());
  const route = $derived(nav.route);
  const here = $derived.by(() => {
    if (route.name === 'disk') {
      const v = volumes.find((x) => x.id === route.volumeId);
      return v ? volumeName(v) : '';
    }
    if (route.name === 'all') return t(`tabs.${route.tab}`);
    if (route.name === 'search') return t('search.title');
    if (route.name === 'settings') return t('settings.title');
    return null;
  });

  let popover = $state(false);
  let root: HTMLElement | undefined = $state();
  $effect(() => {
    if (prompts.length === 0) popover = false;
  });
  $effect(() => {
    if (!popover) return;
    const outside = (e: MouseEvent) => {
      if (root && !root.contains(e.target as Node)) popover = false;
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') popover = false;
    };
    window.addEventListener('mousedown', outside);
    window.addEventListener('keydown', esc);
    return () => {
      window.removeEventListener('mousedown', outside);
      window.removeEventListener('keydown', esc);
    };
  });

  // Typing in the box opens the search screen, which takes over with its own big box.
  function startSearch(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const query = input.value;
    input.value = '';
    nav.go({ name: 'search', query });
  }
</script>

<header class="topbar">
  <div class="where">
    <button class="home" aria-label={t('top.home')} onclick={() => nav.go({ name: 'catalog' })}>
      <Logo size={26} />
      {#if here === null}<span class="wordmark">prism</span>{/if}
    </button>
    {#if here !== null}
      <button class="crumb" onclick={() => nav.go({ name: 'catalog' })}>{t('top.catalog')}</button>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true" class="chev"><path d="m9 6 6 6-6 6" /></svg>
      <span class="current">{here}</span>
      {#if route.name === 'all'}<span class="scope">{t('top.wholeCatalog')}</span>{/if}
    {/if}
  </div>

  {#if route.name !== 'search'}
    <label class="search">
      <span class="sr-only">{t('top.search')}</span>
      <Icon name="search" size={17} />
      <input type="search" placeholder={t('top.searchPlaceholder')} oninput={startSearch} />
      <kbd class="mono">{shortcut.replace('+', ' ')}</kbd>
    </label>
  {:else}
    <span class="spacer"></span>
  {/if}

  <div class="side">
    {#if prompts.length > 0}
      <div class="newdisks" bind:this={root}>
        <button class="pill" aria-expanded={popover} aria-haspopup="dialog" onclick={() => (popover = !popover)}>
          {t(prompts.length === 1 ? 'newDisk.pillOne' : 'newDisk.pillMany', { count: prompts.length })}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
        </button>
        {#if popover}
          <div class="popover" role="dialog" aria-label={t('newDisk.popoverTitle')}>
            <h2>{t('newDisk.popoverTitle')}</h2>
            <ul>
              {#each prompts as v (v.id)}
                <li aria-label={volumeName(v)}>
                  <div class="disk">
                    <strong>{volumeName(v)}</strong>
                    <span class="mono muted">
                      {[t(`kind.${v.kind}`), v.mount, v.capacity !== null ? formatBytes(v.capacity, lang) : null].filter(Boolean).join(' · ')}
                    </span>
                  </div>
                  <button class="primary small" onclick={() => onindex(v)}>{t('newDisk.index')}</button>
                  <Menu
                    label={t('newDisk.optionsFor', { name: volumeName(v) })}
                    items={[
                      { label: t('actions.labels'), onselect: () => onlabels(v) },
                      { label: t('newDisk.notNow'), onselect: () => onnotnow(v) },
                      { label: t('newDisk.ignore'), onselect: () => onignore(v) },
                    ]}
                  />
                </li>
              {/each}
            </ul>
            <div class="foot">
              {#if prompts.length > 1}
                <button class="signal" onclick={onindexall}>{t('newDisk.indexAll')}</button>
              {/if}
              <button onclick={onnotnowall}>{t('newDisk.notNowAll')}</button>
            </div>
            <p class="hint">{t('newDisk.hint')}</p>
          </div>
        {/if}
      </div>
    {/if}
    <button
      class="icon settings"
      aria-label={t('settings.title')}
      aria-pressed={route.name === 'settings'}
      onclick={() => nav.go({ name: 'settings' })}
    >
      <Icon name="settings" size={17} />
    </button>
  </div>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 28px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
    position: relative;
    z-index: 20;
  }
  .where {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    font-family: var(--font-display);
    font-size: 16px;
  }
  .home {
    display: flex;
    align-items: center;
    gap: 10px;
    background: transparent;
    padding: 0 4px;
    min-height: 36px;
  }
  .home:hover:not(:disabled) {
    background: var(--chip);
  }
  .wordmark {
    font-family: var(--font-display);
    font-weight: 700;
    font-size: 18px;
  }
  .crumb {
    background: transparent;
    padding: 0 4px;
    font-weight: 600;
    color: var(--text-2);
  }
  .chev {
    color: var(--text-3);
    flex: none;
  }
  .current {
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .scope {
    font-family: var(--font);
    font-size: 12px;
    font-weight: 600;
    padding: 4px 10px;
    border-radius: 8px;
    background: var(--chip);
  }
  .search {
    flex: 1 1 320px;
    max-width: 640px;
    margin: 0 auto;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 40px;
    padding: 0 14px;
    border: 1.5px solid var(--text);
    border-radius: 12px;
    background: var(--surface);
    color: var(--text);
    font-weight: 400;
    cursor: text;
  }
  .search input {
    border: 0;
    padding: 0;
    background: transparent;
    font-size: 14px;
    outline: none;
  }
  .search kbd {
    font-size: 11px;
    color: var(--text-2);
    background: var(--chip);
    border-radius: 6px;
    padding: 3px 7px;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .side {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-left: auto;
  }
  .settings {
    width: 36px;
    height: 36px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  .settings[aria-pressed='true'] {
    background: var(--accent);
    color: var(--accent-text);
    border-color: var(--accent);
  }
  .newdisks {
    position: relative;
  }
  .pill {
    display: flex;
    align-items: center;
    gap: 8px;
    border-radius: 999px;
    background: var(--signal);
    color: var(--signal-text);
    font-weight: 600;
  }
  .pill:hover:not(:disabled) {
    background: var(--signal-hover);
  }
  .popover {
    position: absolute;
    right: 0;
    top: calc(100% + 8px);
    width: 400px;
    max-width: calc(100vw - 32px);
    background: var(--surface);
    border-radius: 14px;
    box-shadow: var(--shadow-pop);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .popover h2 {
    font-size: 16px;
    padding: 2px 4px 8px;
  }
  .popover ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .popover li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 4px;
    border-top: 1px solid var(--line);
  }
  .disk {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .disk span {
    font-size: 11.5px;
  }
  .foot {
    display: flex;
    gap: 8px;
    padding: 10px 4px 2px;
    border-top: 1px solid var(--line);
  }
  .foot button {
    flex: 1;
  }
  .hint {
    margin: 6px 4px 0;
    font-size: 11.5px;
    color: var(--text-3);
  }
</style>
