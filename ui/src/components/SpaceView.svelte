<script lang="ts">
  // Space of a disk (F08): treemap with drill-down and breadcrumb, the folder content and the
  // largest files beside it, statistics by type. Everything comes from the catalog, so it works
  // for disks that are not connected (F08-AC4); opening files needs the disk connected.
  import type { LargeFileView } from '../lib/bindings/LargeFileView';
  import type { SpaceFolderView } from '../lib/bindings/SpaceFolderView';
  import type { TypeStatsView } from '../lib/bindings/TypeStatsView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { volumeName } from '../lib/catalog';
  import { ago, formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import { CATEGORIES, type Cell } from '../lib/treemap';
  import Icon from './Icon.svelte';
  import Treemap from './Treemap.svelte';
  import TypeStats from './TypeStats.svelte';

  type Tab = 'map' | 'largest' | 'types';

  let {
    volume,
    onback,
    onerror,
  }: { volume: VolumeView; onback: () => void; onerror: (message: string) => void } = $props();

  const lang = $derived(language());
  let view = $state<SpaceFolderView | null>(null);
  let tab = $state<Tab>('map');
  let largest = $state<LargeFileView[] | null>(null);
  let stats = $state<TypeStatsView | null>(null);
  let statsScope = $state<'volume' | 'catalog'>('volume');
  let selected = $state<number | null>(null);
  let loading = $state(false);

  async function load(folderId: number | null) {
    loading = true;
    try {
      view = await backend.spaceFolder(volume.id, folderId);
      largest = null;
      selected = null;
      if (tab === 'largest') void loadLargest();
    } catch (e) {
      onerror(errorMessage(e));
    } finally {
      loading = false;
    }
  }

  async function loadLargest() {
    if (!view) return;
    try {
      largest = await backend.largestFiles(volume.id, view.folderId, 100);
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  async function loadStats() {
    try {
      stats = await backend.typeStats(statsScope === 'volume' ? volume.id : null);
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  $effect(() => {
    void load(null);
  });

  function show(next: Tab) {
    tab = next;
    if (next === 'largest' && largest === null) void loadLargest();
    if (next === 'types' && stats === null) void loadStats();
  }

  function scope(next: 'volume' | 'catalog') {
    statsScope = next;
    stats = null;
    void loadStats();
  }

  const folderSize = $derived(view?.treemap.size ?? 0);
  const parentId = $derived(view && view.breadcrumb.length > 1 ? view.breadcrumb.at(-2)!.id : null);
  const crumbName = (name: string, i: number) => (i === 0 ? volumeName(volume) : name);
  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (e) {
      onerror(errorMessage(e));
    }
  };
  const open = (id: number) => run(() => backend.openEntry(volume.id, id));
  const reveal = (id: number) => run(() => backend.revealEntry(volume.id, id));

  function describe(cell: Cell): string[] {
    if (!cell.node) return [t('space.other'), formatBytes(cell.size, lang)];
    const n = cell.node;
    const kind = n.kind === 'dir' ? t('category.dir') : t(`category.${n.kind}`);
    return [n.name || volumeName(volume), `${formatBytes(n.size, lang)} · ${kind}`];
  }
  const when = (ms: number | null) => {
    if (ms === null) return '';
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };
</script>

<section class="space">
  <header class="head">
    <button class="ghost back" onclick={onback}><Icon name="back" size={16} /> {t('nav.catalog')}</button>
    <div class="title">
      <h1>{volumeName(volume)}</h1>
      <p class="muted">
        {t('space.summary', { size: formatBytes(folderSize, lang), files: formatCount(view?.items.reduce((a, i) => a + i.files, 0) ?? 0, lang) })}
        {#if !volume.online}
          · {t('space.offline', { when: when(volume.lastIndexedAt) })}
        {/if}
      </p>
    </div>
  </header>

  {#if view}
    <nav class="crumbs" aria-label={t('space.path')}>
      {#each view.breadcrumb as c, i (c.id)}
        {#if i > 0}<span class="sep" aria-hidden="true">›</span>{/if}
        {#if i === view.breadcrumb.length - 1}
          <span class="current" aria-current="location">{crumbName(c.name, i)}</span>
        {:else}
          <button class="link" onclick={() => load(c.id)}>{crumbName(c.name, i)}</button>
        {/if}
      {/each}
      {#if parentId !== null}
        <button class="ghost small up" onclick={() => load(parentId)} title={t('space.up')} aria-label={t('space.up')}>
          <Icon name="up" size={14} />
        </button>
      {/if}
    </nav>
  {/if}

  <div class="tabs" role="tablist" aria-label={t('space.title')}>
    {#each [['map', 'space.map'], ['largest', 'space.largest'], ['types', 'space.types']] as [key, label] (key)}
      <button role="tab" aria-selected={tab === key} class:active={tab === key} onclick={() => show(key as Tab)}>
        {t(label!)}
      </button>
    {/each}
  </div>

  {#if !view}
    <p class="muted">{loading ? t('space.loading') : ''}</p>
  {:else if tab === 'map'}
    <div class="map">
      <div class="map-area">
        <Treemap
          tree={view.treemap}
          label={t('space.mapLabel', { name: crumbName(view.breadcrumb.at(-1)?.name ?? '', view.breadcrumb.length - 1) })}
          {describe}
          onopen={(id) => load(id)}
          onselect={(id) => (selected = id)}
        />
        <ul class="legend" aria-label={t('space.legend')}>
          {#each CATEGORIES as c (c)}
            <li><span class="swatch" style:background="var(--cat-{c})"></span>{t(`category.${c}`)}</li>
          {/each}
        </ul>
      </div>
      <div class="list" aria-label={t('space.contents')}>
        <ul>
          {#each view.items as item (item.id)}
            <li class:selected={selected === item.id}>
              <span class="swatch" style:background={item.isDir ? 'var(--cat-dir-2)' : `var(--cat-${item.kind})`}></span>
              {#if item.isDir}
                <button class="link name" onclick={() => load(item.id)} title={item.name}>{item.name}</button>
              {:else}
                <span class="name" title={item.name}>{item.name}</span>
              {/if}
              <span class="num">{formatBytes(item.size, lang)}</span>
              <span class="bar" aria-hidden="true"><span style:width="{folderSize > 0 ? (item.size / folderSize) * 100 : 0}%"></span></span>
              {#if volume.online}
                <span class="acts">
                  <button class="ghost icon tiny" title={t('space.open')} aria-label={`${t('space.open')}: ${item.name}`} onclick={() => open(item.id)}><Icon name="open" size={13} /></button>
                  <button class="ghost icon tiny" title={t('space.reveal')} aria-label={`${t('space.reveal')}: ${item.name}`} onclick={() => reveal(item.id)}><Icon name="folder" size={13} /></button>
                </span>
              {/if}
            </li>
          {:else}
            <li class="muted">{t('space.empty')}</li>
          {/each}
        </ul>
        {#if view.moreItems > 0}
          <p class="muted more">{t('space.moreItems', { count: formatCount(view.moreItems, lang) })}</p>
        {/if}
      </div>
    </div>
  {:else if tab === 'largest'}
    {#if largest === null}
      <p class="muted">{t('space.loading')}</p>
    {:else}
      <table class="largest">
        <thead>
          <tr>
            <th scope="col" class="num">{t('space.size')}</th>
            <th scope="col">{t('space.name')}</th>
            <th scope="col">{t('space.folder')}</th>
            <th scope="col">{t('space.modified')}</th>
            {#if volume.online}<th scope="col"><span class="sr">{t('space.actions')}</span></th>{/if}
          </tr>
        </thead>
        <tbody>
          {#each largest as f (f.entry.id)}
            <tr>
              <td class="num">{formatBytes(f.entry.size, lang)}</td>
              <td><span class="swatch" style:background="var(--cat-{f.entry.kind})"></span>{f.entry.name}</td>
              <td class="muted path" title={f.folder}>{f.folder || '\\'}</td>
              <td class="muted">{when(f.entry.modified)}</td>
              {#if volume.online}
                <td class="acts">
                  <button class="ghost icon tiny" title={t('space.open')} aria-label={`${t('space.open')}: ${f.entry.name}`} onclick={() => open(f.entry.id)}><Icon name="open" size={13} /></button>
                  <button class="ghost icon tiny" title={t('space.reveal')} aria-label={`${t('space.reveal')}: ${f.entry.name}`} onclick={() => reveal(f.entry.id)}><Icon name="folder" size={13} /></button>
                </td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else}
    <div class="scope" role="radiogroup" aria-label={t('types.scope')}>
      <button role="radio" aria-checked={statsScope === 'volume'} class:active={statsScope === 'volume'} onclick={() => scope('volume')}>{t('types.scopeVolume')}</button>
      <button role="radio" aria-checked={statsScope === 'catalog'} class:active={statsScope === 'catalog'} onclick={() => scope('catalog')}>{t('types.scopeCatalog')}</button>
    </div>
    {#if stats === null}
      <p class="muted">{t('space.loading')}</p>
    {:else}
      <TypeStats {stats} />
    {/if}
  {/if}
</section>

<style>
  .space {
    display: flex;
    flex-direction: column;
    gap: 12px;
    height: 100%;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .head h1 {
    font-size: 22px;
  }
  .head p {
    margin: 2px 0 0;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .crumbs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    font-size: 13px;
  }
  .sep {
    color: var(--text-3);
  }
  .current {
    font-weight: 600;
  }
  button.link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent);
    text-align: left;
  }
  button.link:hover {
    text-decoration: underline;
  }
  .up {
    margin-left: 6px;
  }
  .tabs,
  .scope {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--border);
  }
  .scope {
    border-bottom: 0;
  }
  .tabs button,
  .scope button {
    border: 0;
    background: none;
    padding: 7px 12px;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    color: var(--text-2);
  }
  .scope button {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }
  .tabs button.active {
    color: var(--accent);
    box-shadow: inset 0 -2px 0 var(--accent);
    font-weight: 600;
  }
  .scope button.active {
    background: var(--accent-soft);
    color: var(--accent);
    border-color: var(--accent);
  }
  .map {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 360px;
    gap: 14px;
    flex: 1;
    min-height: 420px;
  }
  .map-area {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    min-height: 0;
  }
  .map-area > :global(.treemap) {
    flex: 1;
  }
  .legend {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    font-size: 12px;
    color: var(--text-2);
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .swatch {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 3px;
    flex: none;
    margin-right: 6px;
    vertical-align: -1px;
  }
  .legend .swatch,
  .list .swatch {
    margin-right: 0;
  }
  .list {
    overflow: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 6px 4px;
  }
  .list ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .list li {
    display: grid;
    grid-template-columns: 12px minmax(0, 1fr) 72px 60px auto;
    align-items: center;
    gap: 8px;
    padding: 4px 6px;
    border-radius: var(--radius-sm);
  }
  .list li.selected {
    background: var(--accent-soft);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .bar {
    height: 6px;
    background: var(--track);
    border-radius: 3px;
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    min-width: 1px;
  }
  .acts {
    display: flex;
    gap: 2px;
  }
  button.tiny {
    padding: 3px;
  }
  .more {
    margin: 6px 8px;
    font-size: 12px;
  }
  .largest {
    width: 100%;
    border-collapse: collapse;
  }
  .largest th,
  .largest td {
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    text-align: left;
  }
  .largest th {
    color: var(--text-2);
    font-weight: 600;
    font-size: 12.5px;
  }
  .path {
    max-width: 420px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
