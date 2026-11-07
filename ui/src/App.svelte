<script lang="ts">
  // Main window: navigation rail and the current screen. State lives in the backend; this
  // component only reads it and reacts to its events (ADR-0014).
  import { onMount } from 'svelte';
  import type { AppInfo } from './lib/bindings/AppInfo';
  import type { Settings } from './lib/bindings/Settings';
  import type { VolumeView } from './lib/bindings/VolumeView';
  import { backend, errorMessage, type Unlisten } from './lib/api';
  import { setLanguage, t, type LangPreference } from './lib/i18n.svelte';
  import { cardJob } from './lib/catalog';
  import { setTheme, type ThemePreference } from './lib/theme';
  import CatalogView from './components/CatalogView.svelte';
  import BackupView from './components/BackupView.svelte';
  import CleanupView from './components/CleanupView.svelte';
  import DuplicatesView from './components/DuplicatesView.svelte';
  import SearchView from './components/SearchView.svelte';
  import Icon from './components/Icon.svelte';
  import SettingsView from './components/SettingsView.svelte';
  import SpaceView from './components/SpaceView.svelte';

  type Page = 'catalog' | 'search' | 'duplicates' | 'backup' | 'cleanup' | 'settings' | 'space';

  let page = $state<Page>('catalog');
  let spaceVolume = $state<number | null>(null);
  let volumes = $state<VolumeView[]>([]);
  let settings = $state<Settings>({
    language: 'system',
    theme: 'system',
    autostart: true,
    searchShortcut: 'Ctrl+Space',
    hashingPaused: false,
    pauseHashingOnBattery: false,
    hashingExcluded: [],
  });
  let info = $state<AppInfo | null>(null);
  let toast = $state<string | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  function showToast(message: string) {
    toast = message;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), 5000);
  }

  async function refresh() {
    try {
      volumes = await backend.volumes();
    } catch (e) {
      showToast(errorMessage(e));
    }
  }

  function applySettings(s: Settings) {
    setLanguage(s.language as LangPreference);
    setTheme(s.theme as ThemePreference);
    // Texts Windows shows outside the window: tray menu and notifications ({name}, {count} are
    // filled in by the backend).
    void backend.setNativeTexts({
      open: t('tray.open'),
      search: t('tray.search'),
      quit: t('tray.quit'),
      pauseHashing: t('tray.pauseHashing'),
      resumeHashing: t('tray.resumeHashing'),
      jobsIdle: t('tray.jobsIdle'),
      jobsRunning: t('tray.jobsRunning'),
      newDiskTitle: t('notify.newDiskTitle'),
      newDisksTitle: t('notify.newDisksTitle'),
      newDiskBody: t('notify.newDiskBody'),
    });
  }

  async function changeSettings(s: Settings) {
    settings = s;
    applySettings(s);
    try {
      await backend.setSettings(s);
    } catch (e) {
      showToast(errorMessage(e));
      // A refused shortcut keeps the previous one (F07-AC1): show what is in effect.
      settings = await backend.getSettings().catch(() => settings);
    }
    info = await backend.appInfo().catch(() => info);
  }

  onMount(() => {
    const unlisten: Unlisten[] = [];
    let alive = true;
    void (async () => {
      try {
        settings = await backend.getSettings();
      } catch (e) {
        showToast(errorMessage(e));
      }
      applySettings(settings);
      info = await backend.appInfo().catch(() => null);
      // F07-AC1: say so when the search shortcut could not be registered.
      if (info?.shortcutError) showToast(t('settings.shortcutError', { value: info.shortcutError }));
      await refresh();
      const offVolumes = await backend.onVolumesChanged(() => void refresh());
      // The tray pauses or resumes the content analysis (F06-AC5).
      const offSettings = await backend.onSettingsChanged(async () => {
        settings = await backend.getSettings().catch(() => settings);
      });
      // Job progress: update the matching cards without reloading the whole list.
      const offJobs = await backend.onJobsChanged((jobs) => {
        volumes = volumes.map((v) => ({
          ...v,
          job: cardJob(jobs, v.id),
        }));
      });
      if (alive) unlisten.push(offVolumes, offSettings, offJobs);
      else [offVolumes, offSettings, offJobs].forEach((f) => f());
    })();
    return () => {
      alive = false;
      unlisten.forEach((f) => f());
    };
  });

  const nav: { page: Page; icon: string }[] = [
    { page: 'catalog', icon: 'disks' },
    { page: 'search', icon: 'search' },
    { page: 'duplicates', icon: 'copy' },
    { page: 'backup', icon: 'backup' },
    { page: 'cleanup', icon: 'broom' },
    { page: 'settings', icon: 'settings' },
  ];
</script>

<div class="shell">
  <nav aria-label="Prism">
    <div class="brand" title={t('app.tagline')}>
      <svg width="26" height="26" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 3 3 20h18z" fill="var(--accent)" opacity="0.9" />
        <path d="M12 3 3 20h18z" fill="none" stroke="var(--accent)" stroke-width="1.5" />
      </svg>
    </div>
    {#each nav as item (item.page)}
      <button
        class="nav-item"
        class:active={page === item.page || (item.page === 'catalog' && page === 'space')}
        aria-current={page === item.page ? 'page' : undefined}
        onclick={() => (page = item.page)}
      >
        <Icon name={item.icon} size={20} />
        <span>{t(`nav.${item.page}`)}</span>
      </button>
    {/each}
  </nav>

  <main>
    {#if page === 'space' && spaceVolume !== null && volumes.some((v) => v.id === spaceVolume)}
      {#key spaceVolume}
        <SpaceView
          volume={volumes.find((v) => v.id === spaceVolume)!}
          onback={() => (page = 'catalog')}
          onerror={showToast}
        />
      {/key}
    {:else if page === 'catalog' || page === 'space'}
      <CatalogView
        {volumes}
        onerror={showToast}
        onspace={(v) => {
          spaceVolume = v.id;
          page = 'space';
        }}
      />
    {:else if page === 'search'}
      <SearchView {volumes} onerror={showToast} />
    {:else if page === 'duplicates'}
      <DuplicatesView {volumes} {settings} onchange={changeSettings} onerror={showToast} />
    {:else if page === 'backup'}
      <BackupView {volumes} onerror={showToast} />
    {:else if page === 'cleanup'}
      <CleanupView {volumes} onerror={showToast} />
    {:else}
      <SettingsView {settings} {volumes} {info} onchange={changeSettings} onerror={showToast} />
    {/if}
  </main>

  {#if toast}
    <div class="toast" role="alert">
      <span>{toast}</span>
      <button class="ghost" aria-label={t('actions.close')} onclick={() => (toast = null)}><Icon name="close" size={16} /></button>
    </div>
  {/if}
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 84px 1fr;
    height: 100%;
  }
  nav {
    background: var(--surface);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 14px 0;
  }
  .brand {
    margin-bottom: 14px;
  }
  .nav-item {
    width: 68px;
    border: 0;
    background: transparent;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 9px 4px;
    color: var(--text-2);
    font-size: 11.5px;
  }
  .nav-item.active {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  main {
    overflow: auto;
    padding: 24px 28px;
  }
  .toast {
    position: fixed;
    bottom: 18px;
    right: 18px;
    background: var(--danger-soft);
    color: var(--text);
    border: 1px solid var(--danger);
    border-radius: var(--radius);
    padding: 10px 8px 10px 14px;
    display: flex;
    align-items: center;
    gap: 8px;
    box-shadow: var(--shadow);
    max-width: 420px;
  }
</style>
