<script lang="ts">
  // One disk of the catalog (F01, ADR-0019): its colour and physical location, name and letter,
  // used space split by file category, a few numbers, and what is happening to it now.
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { barParts, categories } from '../lib/categories.svelte';
  import { diskQuery, hashPercent, primaryAction, stickerColor, volumeName } from '../lib/catalog';
  import { nav } from '../lib/nav.svelte';
  import { ago, formatBytes, formatCount, usedFraction } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Menu from './Menu.svelte';

  let {
    volume,
    onopen,
    onindex,
    oncancel,
    onlabels,
    onexplore,
    onignore,
    onremove,
    onhealth,
  }: {
    volume: VolumeView;
    onopen: (v: VolumeView) => void;
    onindex: (v: VolumeView) => void;
    oncancel: (jobId: number) => void;
    onlabels: (v: VolumeView) => void;
    onexplore: (v: VolumeView) => void;
    onignore: (v: VolumeView) => void;
    onremove: (v: VolumeView) => void;
    onhealth: (v: VolumeView) => void;
  } = $props();

  const lang = $derived(language());
  const name = $derived(volumeName(volume));
  const used = $derived(usedFraction(volume.capacity, volume.freeSpace));
  const usedBytes = $derived(
    volume.capacity !== null && volume.freeSpace !== null ? volume.capacity - volume.freeSpace : null,
  );
  const parts = $derived(barParts(used, categories.segments(volume.id) ?? []));
  const letter = $derived(volume.online && volume.mount ? volume.mount.replace(/\\$/, '') : null);
  const action = $derived(primaryAction(volume));
  const indexed = $derived(volume.lastIndexedAt !== null);
  const when = (ms: number) => {
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };
  const jobFraction = $derived(
    volume.job && volume.job.total > 0 ? Math.min(1, volume.job.done / volume.job.total) : null,
  );
  const jobText = $derived.by(() => {
    const j = volume.job;
    if (!j) return '';
    if (j.state === 'queued') return t('job.queued');
    if (j.phase === 'writing') {
      return t('job.writing', { done: formatCount(j.done, lang), total: formatCount(j.total, lang) });
    }
    return t('job.scanning', { count: formatCount(j.done, lang) });
  });
  const state = $derived.by(() => {
    if (!volume.online) {
      return volume.lastSeenAt !== null ? t('card.inDrawerSeen', { when: when(volume.lastSeenAt) }) : t('card.inDrawer');
    }
    if (volume.status === 'reindex_needed') return t('status.reindex_needed');
    if (!indexed) return t('catalog.neverIndexed');
    const parts = [t('catalog.indexedOn', { when: when(volume.lastIndexedAt!) })];
    const h = volume.hashes;
    if (h && h.total > 0) {
      parts.push(h.hashed >= h.total ? t('card.contentDone') : t('card.content', { percent: hashPercent(h) }));
    }
    return parts.join(' · ');
  });
  const items = $derived([
    ...(volume.online && volume.kind !== 'folder' ? [{ label: t('actions.open'), onselect: () => onexplore(volume) }] : []),
    ...(action ? [{ label: t(action === 'update' ? 'card.updateNow' : 'actions.index'), onselect: () => onindex(volume) }] : []),
    ...(volume.lastIndexedAt !== null ? [{ label: t('card.search'), onselect: () => nav.go({ name: 'search', query: diskQuery(volume) }) }] : []),
    { label: t('card.labels'), onselect: () => onlabels(volume) },
    { label: t('card.ignore'), onselect: () => onignore(volume) },
    { label: t('card.remove'), onselect: () => onremove(volume), danger: true, separated: true },
  ]);
</script>

<article class="disk" class:offline={!volume.online} aria-label={name} style:--sticker={stickerColor(volume)}>
  <div class="top">
    <span class="sticker" class:ring={!volume.online}></span>
    <span class="caption where">{volume.location?.trim() || t(`kind.${volume.kind}`)}</span>
    <Menu label={t('card.actionsFor', { name })} {items} />
  </div>

  <div class="title">
    <h3><button class="name" onclick={() => onopen(volume)}>{name}</button></h3>
    {#if letter}<span class="letter">{letter}</span>{/if}
    {#if used !== null}<span class="pct">{Math.round(used * 100)}%</span>{/if}
  </div>

  <div
    class="bar"
    role="meter"
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={used !== null ? Math.round(used * 100) : undefined}
    aria-label={usedBytes !== null && volume.capacity !== null
      ? t('catalog.used', { used: formatBytes(usedBytes, lang), total: formatBytes(volume.capacity, lang) })
      : t('card.usedUnknown')}
  >
    {#if parts.length > 0}
      {#each parts as p (p.category)}
        <span style:width="{p.width * 100}%" style:background="var(--cat-{p.category})" title={t(`category.${p.category}`)}></span>
      {/each}
    {:else if used !== null}
      <span class="plain" style:width="{used * 100}%"></span>
    {/if}
  </div>

  <dl>
    <dt>{t('card.used')}</dt>
    <dd>{usedBytes !== null ? formatBytes(usedBytes, lang) : indexed ? formatBytes(volume.indexedSize, lang) : '—'}</dd>
    <dt>{t('card.free')}</dt>
    <dd>{volume.freeSpace !== null ? formatBytes(volume.freeSpace, lang) : '—'}</dd>
    <dt>{t('card.files')}</dt>
    <dd>{indexed ? formatCount(volume.fileCount, lang) : '—'}</dd>
    <dt>{t('card.health')}</dt>
    <dd>
      {#if volume.health && volume.health.status !== 'unavailable'}
        {@const h = volume.health}
        <button class="health {h.status}" onclick={() => onhealth(volume)} title={t('health.open')}>
          {t(`health.word.${h.status}`)}{#if h.temperature !== null}{' '}{h.temperature} °C{/if}
        </button>
      {:else if volume.health}
        <span title={volume.health.unavailable ? t(`health.unavailable.${volume.health.unavailable}`) : ''}>{t('card.healthNa')}</span>
      {:else}
        —
      {/if}
    </dd>
  </dl>

  <div class="foot">
    {#if volume.job}
      <div class="job" aria-live="polite">
        <div class="job-line">
          <span><strong>{t(`job.${volume.job.kind}`)}</strong> <span class="mono">{jobText}</span></span>
          <button class="small ghost" onclick={() => oncancel(volume.job!.id)}>{t('job.cancel')}</button>
        </div>
        <div class="job-bar" class:indeterminate={jobFraction === null}>
          <span style:width={jobFraction !== null ? `${jobFraction * 100}%` : undefined}></span>
        </div>
      </div>
    {:else}
      <span class="led" class:on={volume.online && indexed} class:warn={volume.online && !indexed}></span>
      <span class="state">{state}</span>
      {#if action === 'index'}
        <button class="primary small" onclick={() => onindex(volume)}>{t('actions.index')}</button>
      {/if}
    {/if}
  </div>
</article>

<style>
  .disk {
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
  }
  .disk.offline {
    background: var(--offline);
    box-shadow: none;
    border: 1px dashed transparent;
  }
  :global([data-theme='dark']) .disk.offline {
    border-color: #3a424c;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
  }
  .where {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  h3 {
    min-width: 0;
    font-size: 20px;
  }
  .name {
    background: transparent;
    padding: 0;
    min-height: 0;
    font: inherit;
    color: var(--text);
    text-align: left;
    max-width: 100%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    display: block;
  }
  .name:hover:not(:disabled) {
    background: transparent;
    text-decoration: underline;
  }
  .offline .name,
  .offline .pct {
    color: var(--text-2);
  }
  .pct {
    margin-left: auto;
    font-family: var(--font-display);
    font-size: 26px;
    font-weight: 600;
    line-height: 1;
  }
  .bar {
    display: flex;
    height: 8px;
    border-radius: 4px;
    background: var(--track);
    overflow: hidden;
  }
  .offline .bar {
    background: var(--offline-track);
  }
  .offline .bar span {
    opacity: 0.45;
  }
  .bar .plain {
    background: var(--accent);
  }
  dl {
    margin: 0;
    display: grid;
    grid-template-columns: auto 1fr auto 1fr;
    gap: 4px 10px;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--text-2);
    align-items: center;
  }
  dt {
    color: var(--text-3);
  }
  dd {
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .health {
    min-height: 0;
    padding: 0;
    background: transparent;
    font: inherit;
    color: var(--ok);
    text-decoration: underline dotted;
  }
  .health.warning {
    color: var(--warn);
  }
  .health.critical {
    color: var(--danger);
    font-weight: 600;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 7px;
    padding-top: 10px;
    border-top: 1px solid var(--line);
    font-size: 12px;
    color: var(--text-2);
    min-height: 38px;
  }
  .offline .foot {
    border-color: var(--offline-track);
  }
  .state {
    flex: 1;
    min-width: 0;
  }
  .led {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: none;
    background: var(--text-3);
  }
  .led.on {
    background: var(--ok);
  }
  .led.warn {
    background: var(--signal);
  }
  .job {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .job-line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .job-bar {
    height: 4px;
    border-radius: 2px;
    background: var(--track);
    overflow: hidden;
  }
  .job-bar span {
    display: block;
    height: 100%;
    background: var(--signal);
  }
  .job-bar.indeterminate span {
    width: 30%;
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
</style>
