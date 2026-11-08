<script lang="ts">
  // Home screen = disk catalog (F01, ADR-0019): the "whole catalog" block, the disks as compact
  // cards (or a list) with filters, the disks Prism cannot tell apart (F02-AC3), the jobs in
  // progress and the last actions on files.
  import type { ActionView } from '../lib/bindings/ActionView';
  import type { AmbiguousView } from '../lib/bindings/AmbiguousView';
  import type { JobView } from '../lib/bindings/JobView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { batchActions } from '../lib/actions';
  import { categories } from '../lib/categories.svelte';
  import {
    catalogCards,
    filterCounts,
    hashPercent,
    stickerColor,
    volumeName,
    type DiskFilter,
    type DiskSort,
  } from '../lib/catalog';
  import { ago, formatBytes, formatCount, usedFraction } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import { nav } from '../lib/nav.svelte';
  import DiskCard from './DiskCard.svelte';

  let {
    volumes,
    jobs,
    hashingPaused,
    onerror,
    onindex,
    oncancel,
    onlabels,
    onremove,
  }: {
    volumes: VolumeView[];
    jobs: JobView[];
    hashingPaused: boolean;
    onerror: (message: string) => void;
    onindex: (v: VolumeView) => void;
    oncancel: (jobId: number) => void;
    onlabels: (v: VolumeView) => void;
    onremove: (v: VolumeView) => void;
  } = $props();

  const lang = $derived(language());
  const VIEW_KEY = 'prism.catalog.view';
  let filter = $state<DiskFilter>('all');
  let sort = $state<DiskSort>('location');
  let view = $state<'cards' | 'list'>(readView());
  function readView(): 'cards' | 'list' {
    try {
      return localStorage.getItem(VIEW_KEY) === 'list' ? 'list' : 'cards';
    } catch {
      return 'cards';
    }
  }
  function setView(v: 'cards' | 'list') {
    view = v;
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      // Not remembered: the choice still applies now.
    }
  }

  const counts = $derived(filterCounts(volumes));
  const cards = $derived(catalogCards(volumes, filter, sort));
  const shown = $derived(volumes.filter((v) => v.status !== 'ignored'));
  const totals = $derived({
    files: shown.reduce((s, v) => s + v.fileCount, 0),
    bytes: shown.reduce((s, v) => s + v.indexedSize, 0),
  });

  // The coloured bars: counted in the background, one disk at a time.
  $effect(() => {
    categories.request(volumes);
  });

  // Totals of the functions on the whole catalog, asked once when the screen opens.
  let recoverable = $state<number | null>(null);
  let similarGroups = $state<number | null>(null);
  let recent = $state<ActionView[]>([]);
  $effect(() => {
    void backend
      .duplicateGroups({ minSize: 1_000_000, category: null, volumeId: null, folder: null }, [])
      .then((d) => (recoverable = d.totalRecoverable), () => (recoverable = null));
    void backend.similarImages(6, null).then((s) => (similarGroups = s.totalGroups), () => (similarGroups = null));
  });
  $effect(() => {
    void volumes;
    void backend.recentActions(200).then((r) => (recent = r), () => (recent = []));
  });
  const batches = $derived(batchActions(recent, 4));

  // Disks to identify: asked again whenever the list of volumes changes.
  let ambiguous = $state<AmbiguousView[]>([]);
  $effect(() => {
    void volumes;
    void backend.ambiguousVolumes().then(
      (list) => (ambiguous = list),
      () => (ambiguous = []),
    );
  });
  async function identify(a: AmbiguousView, volumeId: number | null) {
    try {
      await backend.resolveAmbiguous(a.key, volumeId);
      ambiguous = ambiguous.filter((x) => x.key !== a.key);
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  async function run(action: () => Promise<unknown>) {
    try {
      await action();
    } catch (e) {
      onerror(errorMessage(e));
    }
  }
  const explore = (v: VolumeView) => run(() => backend.openVolume(v.id));
  const ignore = (v: VolumeView) => run(() => backend.setIgnored(v.id, true));
  const open = (v: VolumeView) => nav.disk(v.id);
  const health = (v: VolumeView) => nav.disk(v.id, 'health');

  const when = (ms: number) => {
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };
  const nameOf = (id: number | null) => {
    const v = id === null ? undefined : volumes.find((x) => x.id === id);
    return v ? volumeName(v) : null;
  };

  // Jobs in progress: indexing, updates and copies, plus the content analysis of each disk.
  const activity = $derived([
    ...jobs
      .filter((j) => (j.kind === 'index' || j.kind === 'update' || j.kind === 'copy') && (j.state === 'running' || j.state === 'queued'))
      .map((j) => ({
        key: `job-${j.id}`,
        label: t(`activity.${j.kind}`, { name: nameOf(j.volumeId) ?? '' }),
        percent: j.state === 'queued' ? null : j.total > 0 ? Math.floor((j.done / j.total) * 100) : null,
        detail: j.state === 'queued' ? t('job.queued') : null,
      })),
    ...shown
      .filter((v) => v.hashes && v.hashes.running && v.hashes.hashed < v.hashes.total)
      .map((v) => ({
        key: `hash-${v.id}`,
        label: t('activity.hash', { name: volumeName(v) }),
        percent: hashPercent(v.hashes!),
        detail: null,
      })),
  ]);

  const FILTERS: DiskFilter[] = ['all', 'online', 'network', 'offline'];
</script>

<section class="catalog">
  {#each ambiguous as a (a.key)}
    <div class="identify card" role="region" aria-label={t('identify.title', { name: a.label ?? a.mount ?? '' })}>
      <div class="identify-text">
        <h2>{t('identify.title', { name: a.label ?? a.mount ?? '' })}</h2>
        <p class="mono muted">{[a.mount, a.fsType, a.capacity !== null ? formatBytes(a.capacity, lang) : null].filter(Boolean).join(' · ')}</p>
        <p>{t('identify.question')}</p>
      </div>
      <ul class="candidates">
        {#each a.candidates as c (c.volumeId)}
          <li>
            <button class="primary small" onclick={() => identify(a, c.volumeId)}>{t('identify.isThis', { name: c.name })}</button>
            <span class="muted">
              {[c.location ? t('catalog.location', { location: c.location }) : null, c.capacity !== null ? formatBytes(c.capacity, lang) : null, c.lastSeenAt !== null ? t('catalog.lastSeen', { when: when(c.lastSeenAt) }) : null]
                .filter(Boolean)
                .join(' · ')}
            </span>
          </li>
        {/each}
        <li><button class="small" onclick={() => identify(a, null)}>{t('identify.another')}</button></li>
      </ul>
    </div>
  {/each}

  <section class="whole" aria-label={t('whole.title')}>
    <div class="cell">
      <div class="cap">{t('whole.catalog')}</div>
      <div class="big">{formatCount(counts.all, lang)} <small>{t('whole.disks', { count: counts.all })}</small></div>
    </div>
    <div class="cell">
      <div class="cap">{t('whole.files')}</div>
      <div class="big">{formatCount(totals.files, lang)}</div>
    </div>
    <button class="cell link" onclick={() => nav.go({ name: 'all', tab: 'types' })}>
      <span class="cap">{t('whole.catalogued')} →</span>
      <span class="big">{formatBytes(totals.bytes, lang)}</span>
    </button>
    <button class="cell link sep" onclick={() => nav.go({ name: 'all', tab: 'duplicates' })}>
      <span class="cap">{t('whole.duplicates')} →</span>
      <span class="big signal">{recoverable !== null ? formatBytes(recoverable, lang) : '…'}</span>
    </button>
    <button class="cell link sep" onclick={() => nav.go({ name: 'all', tab: 'similar' })}>
      <span class="cap">{t('whole.similar')} →</span>
      <span class="big">{similarGroups !== null ? formatCount(similarGroups, lang) : '…'} <small>{t('whole.groups')}</small></span>
    </button>
    <button class="cell link sep" onclick={() => nav.go({ name: 'all', tab: 'backup' })}>
      <span class="cap">{t('whole.backup')} →</span>
      <span class="big"><small>{t('whole.verify')}</small></span>
    </button>
  </section>

  <div class="toolbar">
    <h1>{t('catalog.title')}</h1>
    <div class="filters" role="group" aria-label={t('catalog.filter')}>
      {#each FILTERS as f (f)}
        <button class="small" aria-pressed={filter === f} onclick={() => (filter = f)}>
          {t(`catalog.filters.${f}`)} <span class="mono">{counts[f]}</span>
        </button>
      {/each}
    </div>
    <label class="sort">
      <span>{t('catalog.sort')}</span>
      <select bind:value={sort}>
        <option value="location">{t('catalog.sorts.location')}</option>
        <option value="name">{t('catalog.sorts.name')}</option>
        <option value="used">{t('catalog.sorts.used')}</option>
      </select>
    </label>
    <div class="pills inset" role="group" aria-label={t('catalog.view')}>
      <button class="small" aria-pressed={view === 'cards'} onclick={() => setView('cards')}>{t('catalog.cards')}</button>
      <button class="small" aria-pressed={view === 'list'} onclick={() => setView('list')}>{t('catalog.list')}</button>
    </div>
  </div>

  {#if shown.length === 0}
    <p class="empty card">{t('catalog.empty')}</p>
  {:else if cards.length === 0}
    <p class="empty card">{t('catalog.noneInFilter')}</p>
  {:else if view === 'cards'}
    <div class="grid">
      {#each cards as v (v.id)}
        <DiskCard
          volume={v}
          onopen={open}
          {onindex}
          {oncancel}
          {onlabels}
          onexplore={explore}
          onignore={ignore}
          {onremove}
          onhealth={health}
        />
      {/each}
    </div>
  {:else}
    <div class="card list">
      <table class="data">
        <thead>
          <tr>
            <th>{t('catalog.columns.disk')}</th>
            <th>{t('catalog.columns.where')}</th>
            <th class="num">{t('catalog.columns.used')}</th>
            <th class="num">{t('catalog.columns.free')}</th>
            <th class="num">{t('catalog.columns.files')}</th>
            <th>{t('catalog.columns.state')}</th>
          </tr>
        </thead>
        <tbody>
          {#each cards as v (v.id)}
            {@const used = usedFraction(v.capacity, v.freeSpace)}
            <tr class:offline={!v.online} aria-label={volumeName(v)}>
              <td>
                <span class="row-name" style:--sticker={stickerColor(v)}>
                  <span class="sticker" class:ring={!v.online}></span>
                  <button class="link-name" onclick={() => open(v)}>{volumeName(v)}</button>
                  {#if v.online && v.mount}<span class="letter">{v.mount.replace(/\\$/, '')}</span>{/if}
                </span>
              </td>
              <td>{v.location ?? t(`kind.${v.kind}`)}</td>
              <td class="num">
                {v.capacity !== null && v.freeSpace !== null ? formatBytes(v.capacity - v.freeSpace, lang) : '—'}
                {#if used !== null}<span class="muted"> · {Math.round(used * 100)}%</span>{/if}
              </td>
              <td class="num">{v.freeSpace !== null ? formatBytes(v.freeSpace, lang) : '—'}</td>
              <td class="num">{v.lastIndexedAt !== null ? formatCount(v.fileCount, lang) : '—'}</td>
              <td>
                {#if v.job}
                  {t(`job.${v.job.kind}`)}…
                {:else if !v.online}
                  {v.lastSeenAt !== null ? t('card.inDrawerSeen', { when: when(v.lastSeenAt) }) : t('card.inDrawer')}
                {:else if v.lastIndexedAt === null}
                  {t('catalog.neverIndexed')}
                {:else}
                  {t('catalog.indexedOn', { when: when(v.lastIndexedAt) })}
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <div class="panels">
    <section class="card panel" aria-label={t('activity.title')}>
      <h2>{t('activity.title')}</h2>
      {#if activity.length === 0}
        <p class="muted">{hashingPaused ? t('activity.paused') : t('activity.none')}</p>
      {:else}
        <ul>
          {#each activity as a (a.key)}
            <li class="job-row">
              <span>{a.label}</span>
              <span class="track"><span style:width={a.percent !== null ? `${a.percent}%` : '0%'}></span></span>
              <span class="mono muted">{a.detail ?? (a.percent !== null ? `${a.percent}%` : '…')}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
    <section class="card panel" aria-label={t('lastActions.title')}>
      <h2>{t('lastActions.title')}</h2>
      {#if batches.length === 0}
        <p class="muted">{t('lastActions.none')}</p>
      {:else}
        <ul>
          {#each batches as b (`${b.at}-${b.action}-${b.volumeId}`)}
            {@const disk = nameOf(b.volumeId)}
            <li class="log-row">
              <span class="mono muted">{when(b.at)}</span>
              <span>
                {t(b.action === 'delete' ? 'lastActions.deleted' : 'lastActions.recycled', { count: formatCount(b.done, lang) })}{#if disk}{' '}{t('lastActions.from', { name: disk })}{/if}{#if b.skipped > 0}{' · '}{t('lastActions.skipped', { count: b.skipped })}{/if}{#if b.failed > 0}{' · '}{t('lastActions.failed', { count: b.failed })}{/if}
              </span>
              <span class="mono">{formatBytes(b.bytes, lang)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  </div>
</section>

<style>
  .catalog {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .whole {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    background: var(--block);
    color: var(--block-text);
    border-radius: 16px;
    overflow: hidden;
  }
  .cell {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 16px 20px;
    min-width: 0;
    text-align: left;
  }
  .cell.link {
    background: transparent;
    color: var(--block-text);
    border-radius: 0;
    min-height: 0;
  }
  .cell.link:hover {
    background: color-mix(in srgb, var(--block-text) 8%, transparent);
  }
  .cell.sep {
    border-left: 1px solid var(--block-line);
  }
  .cap {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--block-text-2);
  }
  .big {
    font-family: var(--font-display);
    font-size: 28px;
    font-weight: 600;
    line-height: 1.15;
    white-space: nowrap;
  }
  .big small {
    font-size: 14px;
    font-weight: 500;
    color: var(--block-text-2);
  }
  .big.signal {
    color: var(--block-signal);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .toolbar h1 {
    font-size: 22px;
    margin-right: 12px;
  }
  .filters {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .filters button {
    background: var(--surface);
  }
  .filters button[aria-pressed='true'] {
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
  }
  .filters .mono {
    font-size: 11.5px;
  }
  .sort {
    margin: 0 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 400;
  }
  .sort select {
    width: auto;
    min-height: 28px;
    background: var(--surface);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(270px, 1fr));
    gap: var(--gap);
  }
  .list {
    padding: 6px 12px;
    overflow-x: auto;
  }
  .list tr.offline {
    color: var(--text-3);
  }
  .row-name {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .link-name {
    background: transparent;
    min-height: 0;
    padding: 0;
    font-weight: 600;
  }
  .link-name:hover:not(:disabled) {
    background: transparent;
    text-decoration: underline;
  }
  .panels {
    display: flex;
    gap: var(--gap);
    flex-wrap: wrap;
  }
  .panel {
    flex: 1 1 400px;
    padding: 14px 18px;
  }
  .panel h2 {
    font-size: 15px;
    margin-bottom: 6px;
  }
  .panel p {
    margin: 6px 0;
  }
  .panel ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .job-row {
    display: grid;
    grid-template-columns: 1fr 140px 48px;
    align-items: center;
    gap: 12px;
    padding: 6px 0;
  }
  .job-row .mono {
    text-align: right;
    font-size: 11.5px;
  }
  .track {
    height: 6px;
    background: var(--track);
    border-radius: 3px;
    overflow: hidden;
  }
  .track span {
    display: block;
    height: 100%;
    background: var(--signal);
  }
  .log-row {
    display: grid;
    grid-template-columns: 110px 1fr auto;
    gap: 12px;
    padding: 6px 0;
  }
  .log-row .mono {
    font-size: 11.5px;
  }
  .identify {
    display: flex;
    flex-wrap: wrap;
    gap: 14px 24px;
    padding: 16px 18px;
    border: 2px solid var(--signal);
  }
  .identify-text {
    flex: 1 1 320px;
  }
  .identify h2 {
    font-size: 17px;
  }
  .identify p {
    margin: 4px 0 0;
  }
  .candidates {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
    align-content: start;
  }
  .candidates li {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
  }
</style>
