<script lang="ts">
  // "Space" tab of a disk (F08, ADR-0019): the treemap with drill-down and the folder path, and
  // beside it a table with the folder content (or the largest files). Everything comes from the
  // catalog, so it works for disks that are not connected (F08-AC4); opening files needs the
  // disk connected. The statistics by type have their own tab ("File types").
  import type { LargeFileView } from '../lib/bindings/LargeFileView';
  import type { SpaceFolderView } from '../lib/bindings/SpaceFolderView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { untrack } from 'svelte';
  import { backend, errorMessage } from '../lib/api';
  import { volumeName } from '../lib/catalog';
  import { ago, formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import { CATEGORIES, type Cell } from '../lib/treemap';
  import Icon from './Icon.svelte';
  import Treemap from './Treemap.svelte';

  let {
    volume,
    onerror,
    folderId = null,
  }: {
    volume: VolumeView;
    onback?: () => void;
    onerror: (message: string) => void;
    /** The folder to open first (Explorer menu, F18); the root when null. */
    folderId?: number | null;
  } = $props();

  const lang = $derived(language());
  let view = $state<SpaceFolderView | null>(null);
  let mode = $state<'items' | 'largest'>('items');
  let largest = $state<LargeFileView[] | null>(null);
  let selected = $state<number | null>(null);
  let loading = $state(false);

  async function load(id: number | null) {
    loading = true;
    try {
      view = await backend.spaceFolder(volume.id, id);
      largest = null;
      selected = null;
      if (mode === 'largest') void loadLargest();
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

  // Load again only when the disk or the folder asked for changes: the volume object is
  // replaced at every refresh of the catalog, and must not send the map back to the root.
  const volumeId = $derived(volume.id);
  $effect(() => {
    void volumeId;
    const start = folderId;
    untrack(() => void load(start));
  });

  // After an update of the disk the folder on screen is read again, staying where it is (the
  // root if the folder is gone).
  const indexedAt = $derived(volume.lastIndexedAt);
  let seenIndexedAt: number | null | undefined;
  $effect(() => {
    const stamp = indexedAt;
    if (seenIndexedAt !== undefined && stamp !== seenIndexedAt) untrack(() => void reload());
    seenIndexedAt = stamp;
  });
  async function reload() {
    const here = view?.folderId ?? folderId;
    try {
      view = await backend.spaceFolder(volume.id, here);
      if (mode === 'largest') void loadLargest();
    } catch {
      await load(null);
    }
  }

  function show(next: 'items' | 'largest') {
    mode = next;
    if (next === 'largest' && largest === null) void loadLargest();
  }

  const folderSize = $derived(view?.treemap.size ?? 0);
  const folderFiles = $derived(view?.items.reduce((a, i) => a + i.files, 0) ?? 0);
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
  const percent = (size: number) =>
    folderSize > 0 ? ((size / folderSize) * 100).toLocaleString(lang, { maximumFractionDigits: 1, minimumFractionDigits: 1 }) : '—';

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
  <div class="card map-card">
    <div class="path-row">
      <button class="icon up" onclick={() => parentId !== null && load(parentId)} disabled={parentId === null} title={t('space.up')} aria-label={t('space.up')}>
        <Icon name="up" size={14} />
      </button>
      {#if view}
        <nav class="crumbs mono" aria-label={t('space.path')}>
          {#each view.breadcrumb as c, i (c.id)}
            {#if i > 0}<span class="sep" aria-hidden="true">\</span>{/if}
            {#if i === view.breadcrumb.length - 1}
              <span class="current" aria-current="location">{crumbName(c.name, i)}</span>
            {:else}
              <button class="link" onclick={() => load(c.id)}>{crumbName(c.name, i)}</button>
            {/if}
          {/each}
        </nav>
      {/if}
      <span class="summary mono">
        {t('space.summary', { size: formatBytes(folderSize, lang), files: formatCount(folderFiles, lang) })}
      </span>
    </div>
    {#if !volume.online}
      <p class="offline">{t('space.offline', { when: when(volume.lastIndexedAt) })}</p>
    {/if}
    <div class="map-area">
      {#if view}
        <Treemap
          tree={view.treemap}
          label={t('space.mapLabel', { name: crumbName(view.breadcrumb.at(-1)?.name ?? '', view.breadcrumb.length - 1) })}
          {describe}
          onopen={(id) => load(id)}
          onselect={(id) => (selected = id)}
        />
      {:else}
        <p class="muted">{loading ? t('space.loading') : ''}</p>
      {/if}
    </div>
    <ul class="legend" aria-label={t('space.legend')}>
      {#each CATEGORIES as c (c)}
        <li><span class="swatch" style:background="var(--cat-{c})"></span>{t(`category.${c}`)}</li>
      {/each}
    </ul>
  </div>

  <aside class="card side">
    <div class="pills inset" role="group" aria-label={t('space.title')}>
      <button class="small" aria-pressed={mode === 'items'} onclick={() => show('items')}>{t('space.folders')}</button>
      <button class="small" aria-pressed={mode === 'largest'} onclick={() => show('largest')}>{t('space.largest')}</button>
    </div>

    {#if mode === 'items'}
      {#if view}
        <table class="data fixed" aria-label={t('space.contents')}>
          <colgroup>
            <col />
            <col class="c-size" />
            <col class="c-pct" />
            <col class="c-files" />
            {#if volume.online}<col class="c-acts" />{/if}
          </colgroup>
          <thead>
            <tr>
              <th>{t('space.name')}</th>
              <th class="num">{t('space.size')}</th>
              <th class="num">%</th>
              <th class="num">{t('space.filesColumn')}</th>
              {#if volume.online}<th><span class="sr-only">{t('space.actions')}</span></th>{/if}
            </tr>
          </thead>
          <tbody>
            {#each view.items as item (item.id)}
              <tr class:selected={selected === item.id}>
                <td class="name-cell">
                  <span class="swatch" style:background={item.isDir ? 'var(--text-3)' : `var(--cat-${item.kind})`}></span>
                  {#if item.isDir}
                    <button class="link name" onclick={() => load(item.id)} title={item.name}>{item.name}</button>
                  {:else}
                    <span class="name" title={item.name}>{item.name}</span>
                  {/if}
                </td>
                <td class="num">{formatBytes(item.size, lang)}</td>
                <td class="num muted">{percent(item.size)}</td>
                <td class="num muted">{formatCount(item.files, lang)}</td>
                {#if volume.online}
                  <td class="acts">
                    <button class="ghost icon tiny" title={t('space.open')} aria-label={`${t('space.open')}: ${item.name}`} onclick={() => open(item.id)}><Icon name="open" size={13} /></button>
                    <button class="ghost icon tiny" title={t('space.reveal')} aria-label={`${t('space.reveal')}: ${item.name}`} onclick={() => reveal(item.id)}><Icon name="folder" size={13} /></button>
                  </td>
                {/if}
              </tr>
            {:else}
              <tr><td colspan="5" class="muted">{t('space.empty')}</td></tr>
            {/each}
          </tbody>
        </table>
        {#if view.moreItems > 0}
          <p class="muted more">{t('space.moreItems', { count: formatCount(view.moreItems, lang) })}</p>
        {/if}
      {/if}
    {:else if largest === null}
      <p class="muted">{t('space.loading')}</p>
    {:else}
      <table class="data fixed" aria-label={t('space.largest')}>
        <colgroup>
          <col />
          <col class="c-size" />
          <col class="c-date" />
          {#if volume.online}<col class="c-acts" />{/if}
        </colgroup>
        <thead>
          <tr>
            <th>{t('space.name')}</th>
            <th class="num">{t('space.size')}</th>
            <th class="num">{t('space.modified')}</th>
            {#if volume.online}<th><span class="sr-only">{t('space.actions')}</span></th>{/if}
          </tr>
        </thead>
        <tbody>
          {#each largest as f (f.entry.id)}
            <tr>
              <td class="name-cell">
                <span class="swatch" style:background="var(--cat-{f.entry.kind})"></span>
                <span class="stack">
                  <span class="name" title={f.entry.name}>{f.entry.name}</span>
                  <span class="mono muted path" title={f.folder}>{f.folder || '\\'}</span>
                </span>
              </td>
              <td class="num">{formatBytes(f.entry.size, lang)}</td>
              <td class="num muted">{when(f.entry.modified)}</td>
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
  </aside>
</section>

<style>
  .space {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    align-items: flex-start;
  }
  .map-card {
    flex: 999 1 520px;
    min-width: 0;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .path-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .up {
    width: 28px;
    height: 28px;
  }
  .crumbs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
    font-size: 12.5px;
    min-width: 0;
  }
  .sep {
    color: var(--text-3);
    padding: 0 2px;
  }
  .current {
    font-weight: 600;
  }
  button.link {
    background: none;
    padding: 0;
    min-height: 0;
    color: var(--text-2);
    text-align: left;
    font: inherit;
  }
  button.link:hover:not(:disabled) {
    background: none;
    color: var(--text);
    text-decoration: underline;
  }
  .summary {
    margin-left: auto;
    font-size: 11.5px;
    color: var(--text-3);
  }
  .offline {
    margin: 0;
    font-size: 12.5px;
    color: var(--signal-strong);
  }
  .map-area {
    height: 520px;
    display: flex;
  }
  .legend {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    font-size: 11.5px;
    color: var(--text-2);
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .name-cell .swatch {
    display: inline-block;
    vertical-align: middle;
    margin-right: 7px;
  }
  .name-cell .name {
    display: inline-block;
    vertical-align: middle;
    max-width: calc(100% - 20px);
  }
  .name-cell .stack {
    display: inline-flex;
    vertical-align: middle;
    max-width: calc(100% - 20px);
  }
  .swatch {
    width: 9px;
    height: 9px;
    border-radius: 3px;
    flex: none;
  }
  .side {
    flex: 1 1 420px;
    max-width: 520px;
    min-width: 0;
    overflow: hidden;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .side .pills {
    align-self: flex-start;
  }
  .name-cell {
    min-width: 0;
  }
  /* Fixed columns: long names are cut with "…" instead of widening the table. */
  table.fixed {
    table-layout: fixed;
  }
  table.fixed td,
  table.fixed th {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  table.fixed td.acts {
    text-overflow: clip;
  }
  .c-size {
    width: 86px;
  }
  .c-pct {
    width: 44px;
  }
  .c-files {
    width: 68px;
  }
  .c-date {
    width: 88px;
  }
  .c-acts {
    width: 64px;
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
  }
  .stack {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .path {
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  tr.selected td {
    background: var(--signal-soft);
  }
  .acts {
    white-space: nowrap;
    text-align: right;
  }
  .acts .tiny {
    width: 24px;
    height: 24px;
  }
  .more {
    font-size: 12px;
    margin: 4px 0 0;
  }
</style>
