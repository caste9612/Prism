<script lang="ts">
  // Main window (ADR-0019): the top bar, the current screen and the status bar. Everything starts
  // from the catalog or from a disk; there is no side menu. State lives in the backend; this
  // component only reads it and reacts to its events (ADR-0014).
  import { onMount } from 'svelte';
  import type { AppInfo } from './lib/bindings/AppInfo';
  import type { JobView } from './lib/bindings/JobView';
  import type { LabelsInput } from './lib/bindings/LabelsInput';
  import type { Settings } from './lib/bindings/Settings';
  import type { VolumeView } from './lib/bindings/VolumeView';
  import { backend, errorMessage, type Unlisten } from './lib/api';
  import { setLanguage, t, type LangPreference } from './lib/i18n.svelte';
  import { cardJob, newVolumePrompts, volumeName } from './lib/catalog';
  import { CATALOG_TABS, nav } from './lib/nav.svelte';
  import { setTheme, type ThemePreference } from './lib/theme';
  import CatalogView from './components/CatalogView.svelte';
  import BackupView from './components/BackupView.svelte';
  import Dialog from './components/Dialog.svelte';
  import DiskView from './components/DiskView.svelte';
  import DuplicatesView, { presetDuplicates } from './components/DuplicatesView.svelte';
  import Icon from './components/Icon.svelte';
  import LabelsDialog from './components/LabelsDialog.svelte';
  import SearchView from './components/SearchView.svelte';
  import SettingsView from './components/SettingsView.svelte';
  import SimilarView from './components/SimilarView.svelte';
  import TopBar from './components/TopBar.svelte';
  import TypesView from './components/TypesView.svelte';

  let volumes = $state<VolumeView[]>([]);
  let jobs = $state<JobView[]>([]);
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
  // New disks put aside with "Not now": until Prism closes (F02-AC1).
  let dismissed = $state(new Set<number>());
  let editing = $state<VolumeView | null>(null);
  let removing = $state<VolumeView | null>(null);

  const route = $derived(nav.route);
  const prompts = $derived(newVolumePrompts(volumes, dismissed));
  const online = $derived(volumes.filter((v) => v.online && v.status !== 'ignored').length);

  function showToast(message: string) {
    toast = message;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), 6000);
  }

  async function run(action: () => Promise<unknown>) {
    try {
      await action();
    } catch (e) {
      showToast(errorMessage(e));
    }
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
      healthTitle: t('notify.healthTitle'),
      healthBody: t('notify.healthBody'),
      nasTitle: t('notify.nasTitle'),
      nasBody: t('notify.nasBody'),
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

  const index = (v: VolumeView) => run(() => backend.indexVolume(v.id));
  const cancel = (jobId: number) => run(() => backend.cancelJob(jobId));
  const ignore = (v: VolumeView) => run(() => backend.setIgnored(v.id, true));
  const notNow = (v: VolumeView) => (dismissed = new Set([...dismissed, v.id]));
  // Jobs on different physical disks run in parallel; the engine queues the rest (ADR-0009).
  const indexAll = () => prompts.forEach((v) => void index(v));
  const notNowAll = () => (dismissed = new Set([...dismissed, ...prompts.map((v) => v.id)]));
  async function saveLabels(id: number, labels: LabelsInput) {
    await run(() => backend.setLabels(id, labels));
    editing = null;
  }
  async function confirmRemove() {
    if (!removing) return;
    const v = removing;
    removing = null;
    await run(() => backend.removeVolume(v.id));
    if (route.name === 'disk' && route.volumeId === v.id) nav.go({ name: 'catalog' });
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
      jobs = await backend.jobs().catch(() => []);
      const offVolumes = await backend.onVolumesChanged(() => void refresh());
      // F18: "Analyse with Prism" and "Find duplicates here" from Explorer.
      const offShell = await backend.onShellRequest(async (r) => {
        try {
          const at = await backend.locatePath(r.path);
          await refresh();
          if (r.action === 'analyze') {
            nav.disk(at.volumeId, 'space', at.folderId);
          } else {
            presetDuplicates(at.volumeId, at.folder);
            nav.disk(at.volumeId, 'duplicates');
          }
        } catch (e) {
          showToast(errorMessage(e));
        }
      });
      // The tray pauses or resumes the content analysis (F06-AC5).
      const offSettings = await backend.onSettingsChanged(async () => {
        settings = await backend.getSettings().catch(() => settings);
      });
      // Job progress: update the matching cards without reloading the whole list.
      const offJobs = await backend.onJobsChanged((list) => {
        jobs = list;
        volumes = volumes.map((v) => ({ ...v, job: cardJob(list, v.id) }));
      });
      if (alive) unlisten.push(offVolumes, offShell, offSettings, offJobs);
      else [offVolumes, offShell, offSettings, offJobs].forEach((f) => f());
    })();
    return () => {
      alive = false;
      unlisten.forEach((f) => f());
    };
  });
</script>

<div class="shell">
  <TopBar
    {volumes}
    {prompts}
    shortcut={settings.searchShortcut}
    onindex={index}
    onindexall={indexAll}
    onnotnow={notNow}
    onnotnowall={notNowAll}
    onlabels={(v) => (editing = v)}
    onignore={ignore}
  />

  <main>
    {#if route.name === 'catalog'}
      <CatalogView
        {volumes}
        {jobs}
        hashingPaused={settings.hashingPaused}
        onerror={showToast}
        onindex={index}
        oncancel={cancel}
        onlabels={(v) => (editing = v)}
        onremove={(v) => (removing = v)}
      />
    {:else if route.name === 'disk'}
      {@const volume = volumes.find((v) => v.id === route.volumeId)}
      {#if volume}
        <DiskView
          {volume}
          {volumes}
          tab={route.tab}
          folderId={route.folderId}
          {settings}
          onchange={changeSettings}
          onerror={showToast}
          onindex={index}
          onlabels={(v) => (editing = v)}
          onremove={(v) => (removing = v)}
        />
      {:else if volumes.length > 0}
        <p class="empty card">{t('disk.gone')}</p>
      {/if}
    {:else if route.name === 'all'}
      <section class="all">
        <nav class="pills" aria-label={t('whole.title')}>
          {#each CATALOG_TABS as k (k)}
            <button aria-current={route.tab === k ? 'page' : undefined} onclick={() => nav.go({ name: 'all', tab: k })}>{t(`tabs.${k}`)}</button>
          {/each}
        </nav>
        {#if route.tab === 'duplicates'}
          {#key nav.visit}
            <DuplicatesView {volumes} {settings} onchange={changeSettings} onerror={showToast} scope={null} />
          {/key}
        {:else if route.tab === 'similar'}
          <SimilarView {volumes} onerror={showToast} scope={null} />
        {:else if route.tab === 'backup'}
          <BackupView {volumes} onerror={showToast} scope={null} />
        {:else}
          <TypesView volumeId={null} onerror={showToast} />
        {/if}
      </section>
    {:else if route.name === 'search'}
      <SearchView {volumes} onerror={showToast} initial={route.query} />
    {:else}
      <SettingsView {settings} {volumes} {info} onchange={changeSettings} onerror={showToast} />
    {/if}
  </main>

  <footer class="statusbar mono">
    <span><span class="dot" class:on={online > 0}></span> {t('statusbar.online', { count: online })}</span>
    <span class="optional">{settings.hashingPaused ? t('statusbar.hashingPaused') : t('statusbar.hashing')}</span>
    {#if info}<span class="push">Prism {info.version}{info.dev ? ` · ${t('statusbar.dev')}` : ''}</span>{/if}
  </footer>

  {#if toast}
    <div class="toast" role="alert">
      <span>{toast}</span>
      <button class="ghost icon" aria-label={t('actions.close')} onclick={() => (toast = null)}><Icon name="close" size={16} /></button>
    </div>
  {/if}
</div>

<LabelsDialog volume={editing} {volumes} onsave={saveLabels} onclose={() => (editing = null)} />

<Dialog open={removing !== null} title={t('remove.title', { name: removing ? volumeName(removing) : '' })} onclose={() => (removing = null)}>
  <p>{t('remove.body')}</p>
  {#snippet actions()}
    <button onclick={() => (removing = null)}>{t('actions.cancel')}</button>
    <button class="primary danger" onclick={confirmRemove}>{t('actions.remove')}</button>
  {/snippet}
</Dialog>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  main {
    flex: 1;
    overflow: auto;
    padding: 22px 28px 24px;
  }
  .all {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .all > .pills {
    align-self: flex-start;
  }
  .statusbar {
    display: flex;
    gap: 20px;
    flex-wrap: nowrap;
    white-space: nowrap;
    overflow: hidden;
    padding: 6px 28px;
    background: var(--surface);
    border-top: 1px solid var(--border);
    font-size: 11px;
    color: var(--text-2);
  }
  .statusbar .push {
    margin-left: auto;
  }
  .statusbar > span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  @media (max-width: 1120px) {
    .statusbar .optional {
      display: none;
    }
  }
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-3);
    margin-right: 4px;
  }
  .dot.on {
    background: var(--ok);
  }
  .toast {
    position: fixed;
    bottom: 40px;
    right: 18px;
    z-index: 50;
    background: var(--block);
    color: var(--block-text);
    border-radius: 14px;
    padding: 12px 8px 12px 16px;
    display: flex;
    align-items: center;
    gap: 8px;
    box-shadow: var(--shadow-pop);
    max-width: 440px;
  }
  .toast button {
    color: var(--block-text);
  }
</style>
