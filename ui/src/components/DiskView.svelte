<script lang="ts">
  // The screen of one disk (ADR-0019): colour, location and numbers at the top, then the tabs of
  // the functions limited to this disk (F01-AC4).
  import type { Settings } from '../lib/bindings/Settings';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { diskQuery, driveLetter, hashPercent, primaryAction, stickerColor, volumeName } from '../lib/catalog';
  import { ago, formatBytes, formatCount, usedFraction } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import { DISK_TABS, nav, type DiskTab } from '../lib/nav.svelte';
  import BackupView from './BackupView.svelte';
  import CleanupView from './CleanupView.svelte';
  import DuplicatesView from './DuplicatesView.svelte';
  import HealthView from './HealthView.svelte';
  import Menu from './Menu.svelte';
  import SimilarView from './SimilarView.svelte';
  import SpaceView from './SpaceView.svelte';
  import TypesView from './TypesView.svelte';

  let {
    volume,
    volumes,
    tab,
    folderId,
    settings,
    onchange,
    onerror,
    onindex,
    onlabels,
    onremove,
  }: {
    volume: VolumeView;
    volumes: VolumeView[];
    tab: DiskTab;
    folderId: number | null;
    settings: Settings;
    onchange: (s: Settings) => void;
    onerror: (message: string) => void;
    onindex: (v: VolumeView) => void;
    onlabels: (v: VolumeView) => void;
    onremove: (v: VolumeView) => void;
  } = $props();

  const lang = $derived(language());
  const name = $derived(volumeName(volume));
  const used = $derived(usedFraction(volume.capacity, volume.freeSpace));
  const usedBytes = $derived(
    volume.capacity !== null && volume.freeSpace !== null ? volume.capacity - volume.freeSpace : null,
  );
  const action = $derived(primaryAction(volume));
  const letter = $derived(driveLetter(volume.mount));
  const when = (ms: number) => {
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };
  const subtitle = $derived(
    [volume.location?.trim() || t(`kind.${volume.kind}`), volume.fsType, volume.online ? null : t(volume.kind === 'network' ? 'card.unreachable' : 'card.inDrawer')]
      .filter(Boolean)
      .join(' · '),
  );

  async function run(f: () => Promise<unknown>) {
    try {
      await f();
    } catch (e) {
      onerror(errorMessage(e));
    }
  }
  const items = $derived([
    ...(volume.lastIndexedAt !== null ? [{ label: t('card.search'), onselect: () => nav.go({ name: 'search', query: diskQuery(volume) }) }] : []),
    { label: t('card.labels'), onselect: () => onlabels(volume) },
    { label: t('card.ignore'), onselect: () => run(() => backend.setIgnored(volume.id, true)).then(() => nav.go({ name: 'catalog' })) },
    { label: t('card.remove'), onselect: () => onremove(volume), danger: true, separated: true },
  ]);
  const go = (t: DiskTab) => nav.disk(volume.id, t);
</script>

<section class="disk-screen" style:--sticker={stickerColor(volume)}>
  <header class="head">
    <div class="title">
      <div class="where">
        <span class="sticker" class:ring={!volume.online}></span>
        <span class="caption">{subtitle}</span>
      </div>
      <div class="name-row">
        <h1>{name}</h1>
        {#if letter && volume.online}<span class="letter big">{letter}</span>{/if}
      </div>
    </div>
    <div class="actions">
      {#if action}
        <button class="primary" onclick={() => onindex(volume)}>{t(action === 'update' ? 'actions.update' : 'actions.index')}</button>
      {/if}
      {#if volume.online && volume.kind !== 'folder'}
        <button onclick={() => run(() => backend.openVolume(volume.id))}>{t('actions.open')}</button>
      {/if}
      <button onclick={() => onlabels(volume)}>{t('card.labels')}</button>
      <Menu label={t('card.actionsFor', { name })} {items} />
    </div>
  </header>

  <div class="stats">
    <div>
      <div class="caption">{t('disk.used')}</div>
      <div class="value">{usedBytes !== null ? formatBytes(usedBytes, lang) : formatBytes(volume.indexedSize, lang)}</div>
      <div class="detail">
        {used !== null && volume.capacity !== null ? t('disk.ofTotal', { percent: Math.round(used * 100), total: formatBytes(volume.capacity, lang) }) : t('disk.indexed')}
      </div>
    </div>
    <div>
      <div class="caption">{t('disk.free')}</div>
      <div class="value">{volume.freeSpace !== null ? formatBytes(volume.freeSpace, lang) : '—'}</div>
      <div class="detail">{used !== null ? `${100 - Math.round(used * 100)}%` : ''}</div>
    </div>
    <div>
      <div class="caption">{t('disk.files')}</div>
      <div class="value">{volume.lastIndexedAt !== null ? formatCount(volume.fileCount, lang) : '—'}</div>
      <div class="detail">{volume.lastIndexedAt !== null ? t('catalog.folders', { count: formatCount(volume.folderCount, lang) }) : ''}</div>
    </div>
    <div>
      <div class="caption">{t('disk.health')}</div>
      <div class="value health {volume.health?.status ?? ''}">{volume.health ? t(`health.word.${volume.health.status}`) : '—'}</div>
      <div class="detail">{volume.health?.temperature != null ? `${volume.health.temperature} °C` : ''}</div>
    </div>
    <div>
      <div class="caption">{t('disk.content')}</div>
      <div class="value">{volume.hashes && volume.hashes.total > 0 ? `${hashPercent(volume.hashes)}%` : '—'}</div>
      <div class="detail">{volume.hashes?.running ? t('catalog.hashesRunning') : ''}</div>
    </div>
    <div>
      <div class="caption">{volume.online ? t('disk.updated') : t('disk.seen')}</div>
      <div class="value">
        {#if volume.online}
          {volume.lastIndexedAt !== null ? when(volume.lastIndexedAt) : '—'}
        {:else}
          {volume.lastSeenAt !== null ? when(volume.lastSeenAt) : '—'}
        {/if}
      </div>
      <div class="detail">{volume.job ? t(`job.${volume.job.kind}`) + '…' : ''}</div>
    </div>
  </div>

  <nav class="pills tabs" aria-label={t('disk.functions')}>
    {#each DISK_TABS as k (k)}
      <button aria-current={tab === k ? 'page' : undefined} onclick={() => go(k)}>{t(`tabs.${k}`)}</button>
    {/each}
  </nav>

  <div class="content">
    {#if tab === 'space'}
      {#key `${volume.id}-${folderId}-${nav.visit}`}
        <SpaceView {volume} {folderId} onback={() => nav.go({ name: 'catalog' })} {onerror} />
      {/key}
    {:else if tab === 'duplicates'}
      {#key volume.id}
        <DuplicatesView {volumes} {settings} {onchange} {onerror} scope={volume.id} />
      {/key}
    {:else if tab === 'similar'}
      <SimilarView {volumes} {onerror} scope={volume.id} />
    {:else if tab === 'backup'}
      <BackupView {volumes} {onerror} scope={volume.id} />
    {:else if tab === 'cleanup'}
      <CleanupView {volumes} {onerror} scope={volume.id} />
    {:else if tab === 'health'}
      <HealthView {volume} {onerror} />
    {:else}
      <TypesView volumeId={volume.id} {onerror} />
    {/if}
  </div>
</section>

<style>
  .disk-screen {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .head {
    display: flex;
    align-items: flex-end;
    gap: 20px;
    flex-wrap: wrap;
  }
  .title {
    flex: 1 1 280px;
    min-width: 0;
  }
  .where {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .name-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 6px;
    min-width: 0;
  }
  h1 {
    font-size: 36px;
    font-weight: 700;
    letter-spacing: -0.02em;
    line-height: 1.05;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .letter.big {
    font-size: 14px;
    padding: 2px 8px;
    border-width: 2px;
    border-radius: 7px;
  }
  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
  }
  .actions > button:not(.primary) {
    background: var(--surface);
  }
  .value.health.good {
    color: var(--ok);
  }
  .value.health.warning {
    color: var(--warn);
  }
  .value.health.critical {
    color: var(--danger);
  }
  .tabs {
    align-self: flex-start;
  }
  .content {
    min-width: 0;
  }
</style>
