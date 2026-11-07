<script lang="ts">
  // One disk of the catalog (F01): name, kind, mount, space, totals, state, current job.
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { ago, formatBytes, formatCount, usedFraction } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import { primaryAction, volumeName } from '../lib/catalog';
  import Icon from './Icon.svelte';

  let {
    volume,
    onindex,
    oncancel,
    onlabels,
    onremove,
  }: {
    volume: VolumeView;
    onindex: (v: VolumeView) => void;
    oncancel: (jobId: number) => void;
    onlabels: (v: VolumeView) => void;
    onremove: (v: VolumeView) => void;
  } = $props();

  const lang = $derived(language());
  const used = $derived(usedFraction(volume.capacity, volume.freeSpace));
  const action = $derived(primaryAction(volume));
  const whenText = (ms: number) => {
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };
  const jobText = $derived.by(() => {
    const j = volume.job;
    if (!j) return '';
    if (j.state === 'queued') return t('job.queued');
    if (j.phase === 'writing') {
      return t('job.writing', { done: formatCount(j.done, lang), total: formatCount(j.total, lang) });
    }
    return t('job.scanning', { count: formatCount(j.done, lang) });
  });
  const jobFraction = $derived(
    volume.job && volume.job.total > 0 ? Math.min(1, volume.job.done / volume.job.total) : null,
  );
</script>

<article class="card" class:offline={!volume.online} style:--label={volume.color ?? 'transparent'} aria-label={volumeName(volume)}>
  <header>
    <span class="kind" title={t(`kind.${volume.kind}`)}><Icon name={volume.kind} size={20} /></span>
    <div class="title">
      <h3>{volumeName(volume)}</h3>
      <p class="muted">
        {[t(`kind.${volume.kind}`), volume.mount, volume.fsType].filter(Boolean).join(' · ')}
      </p>
    </div>
    <span class="badge" class:on={volume.online}>
      {volume.online ? t('catalog.online') : t('catalog.offline')}
    </span>
  </header>

  {#if volume.location}
    <p class="location"><Icon name="pin" size={14} /> {t('catalog.location', { location: volume.location })}</p>
  {/if}

  {#if used !== null && volume.capacity !== null && volume.freeSpace !== null}
    <div class="space">
      <div
        class="bar"
        role="meter"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(used * 100)}
        aria-label={t('catalog.used', { used: formatBytes(volume.capacity - volume.freeSpace, lang), total: formatBytes(volume.capacity, lang) })}
      >
        <span style:width="{used * 100}%" class:full={used > 0.9}></span>
      </div>
      <div class="space-text">
        <span>{t('catalog.used', { used: formatBytes(volume.capacity - volume.freeSpace, lang), total: formatBytes(volume.capacity, lang) })}</span>
        <span class="muted">{t('catalog.free', { free: formatBytes(volume.freeSpace, lang) })}</span>
      </div>
    </div>
  {/if}

  {#if volume.lastIndexedAt !== null}
    <p class="stats">
      <span>{t('catalog.files', { count: formatCount(volume.fileCount, lang) })}</span>
      <span>{t('catalog.folders', { count: formatCount(volume.folderCount, lang) })}</span>
      <span>{t('catalog.indexedSize', { size: formatBytes(volume.indexedSize, lang) })}</span>
    </p>
  {/if}

  {#if volume.job}
    <div class="job" aria-live="polite">
      <div class="job-head">
        <strong>{t(`job.${volume.job.kind}`)}</strong>
        <span class="muted">{jobText}</span>
        <button class="ghost small" onclick={() => volume.job && oncancel(volume.job.id)}>{t('job.cancel')}</button>
      </div>
      <div class="bar thin" class:indeterminate={jobFraction === null}>
        <span style:width={jobFraction === null ? '35%' : `${jobFraction * 100}%`}></span>
      </div>
    </div>
  {/if}

  <footer>
    <span class="muted when">
      {#if volume.status !== 'indexed' && volume.status !== 'indexing'}
        <span class="status {volume.status}">{t(`status.${volume.status}`)}</span>
      {/if}
      <!-- F01-AC1: for a disconnected disk, when it was last seen matters most. -->
      {#if !volume.online && volume.lastSeenAt !== null}
        {t('catalog.lastSeen', { when: whenText(volume.lastSeenAt) })}
      {:else if volume.lastIndexedAt !== null}
        {t('catalog.indexedOn', { when: whenText(volume.lastIndexedAt) })}
      {:else if volume.status === 'new'}
        {t('catalog.neverIndexed')}
      {/if}
    </span>
    <div class="buttons">
      {#if action}
        <button class:primary={action === 'index'} onclick={() => onindex(volume)}>
          <Icon name="refresh" size={15} /> {t(`actions.${action}`)}
        </button>
      {/if}
      <button class="ghost icon" title={t('actions.labels')} aria-label={t('actions.labels')} onclick={() => onlabels(volume)}>
        <Icon name="tag" size={16} />
      </button>
      <button class="ghost icon danger" title={t('actions.remove')} aria-label={t('actions.remove')} disabled={volume.job !== null} onclick={() => onremove(volume)}>
        <Icon name="trash" size={16} />
      </button>
    </div>
  </footer>
</article>

<style>
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-left: 4px solid var(--label);
    border-radius: var(--radius);
    padding: 16px 16px 12px;
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .card.offline .kind,
  .card.offline h3 {
    opacity: 0.75;
  }
  header {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .kind {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 9px;
    background: var(--accent-soft);
    color: var(--accent);
    flex: none;
  }
  .title {
    flex: 1;
    min-width: 0;
  }
  h3 {
    font-size: 15px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title p {
    margin: 2px 0 0;
    font-size: 12.5px;
  }
  .badge {
    font-size: 12px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-3);
    white-space: nowrap;
  }
  .badge.on {
    color: var(--ok);
  }
  .location {
    margin: -4px 0 0;
    font-size: 13px;
    color: var(--text-2);
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .bar {
    height: 8px;
    background: var(--track);
    border-radius: 999px;
    overflow: hidden;
  }
  .bar.thin {
    height: 5px;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: 999px;
    transition: width 0.3s;
  }
  .bar span.full {
    background: var(--warn);
  }
  .bar.indeterminate span {
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      margin-left: -35%;
    }
    to {
      margin-left: 100%;
    }
  }
  .space-text {
    display: flex;
    justify-content: space-between;
    margin-top: 6px;
    font-size: 12.5px;
  }
  .stats {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    font-size: 13px;
    color: var(--text-2);
  }
  .job {
    background: var(--surface-2);
    border-radius: var(--radius-sm);
    padding: 8px 10px;
  }
  .job-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
    font-size: 13px;
  }
  .job-head .muted {
    flex: 1;
  }
  .small {
    padding: 2px 8px;
    font-size: 12.5px;
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }
  .when {
    font-size: 12.5px;
  }
  .status {
    font-weight: 600;
    margin-right: 6px;
  }
  .status.new {
    color: var(--accent);
  }
  .status.reindex_needed {
    color: var(--warn);
  }
  .buttons {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .buttons button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .icon {
    padding: 6px;
  }
</style>
