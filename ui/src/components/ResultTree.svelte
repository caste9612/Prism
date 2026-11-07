<script lang="ts">
  // The result of a backup check or of a comparison as a tree (F13-AC4, F14-AC3): one level at
  // a time from the backend, subfolders with their totals by class, then the files of the
  // level. Search and size filters apply to the whole tree.
  import type { ReportFolderView } from '../lib/bindings/ReportFolderView';
  import type { TallyView } from '../lib/bindings/TallyView';
  import { backend, errorMessage } from '../lib/api';
  import { childFolder, crumbs, parentFolder } from '../lib/backup';
  import { formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Icon from './Icon.svelte';

  let {
    reportId,
    classes,
    rootName,
    statuses = [],
    onerror,
  }: {
    reportId: number;
    /** Labels of the classes, in the order of the tallies (`missing`, or the four statuses). */
    classes: { key: string; label: string }[];
    rootName: string;
    /** Statuses kept (comparison); all when empty. */
    statuses?: string[];
    onerror: (message: string) => void;
  } = $props();

  const lang = $derived(language());
  const MIN_SIZES = [0, 1_000_000, 10_000_000, 100_000_000, 1_000_000_000];
  let folder = $state('');
  let text = $state('');
  let minSize = $state(0);
  let view = $state<ReportFolderView | null>(null);
  let loading = $state(false);

  async function load(id: number, at: string, filterText: string, min: number, keep: string[]) {
    loading = true;
    try {
      view = await backend.backupFolder(id, at, { text: filterText || null, minSize: min, statuses: keep });
    } catch (e) {
      onerror(errorMessage(e));
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void load(reportId, folder, text, minSize, [...statuses]);
  });

  const sum = (ts: TallyView[]) => ts.reduce((a, x) => ({ files: a.files + x.files, bytes: a.bytes + x.bytes }), { files: 0, bytes: 0 });
  const total = $derived(view ? sum(view.tallies) : { files: 0, bytes: 0 });
</script>

<div class="tree">
  <div class="tools">
    <input
      class="search"
      aria-label={t('backup.search')}
      placeholder={t('backup.search')}
      spellcheck="false"
      value={text}
      onchange={(e) => (text = e.currentTarget.value.trim())}
    />
    <select aria-label={t('backup.minSize')} value={String(minSize)} onchange={(e) => (minSize = Number(e.currentTarget.value))}>
      {#each MIN_SIZES as s (s)}
        <option value={String(s)}>{s === 0 ? t('backup.anySize') : `≥ ${formatBytes(s, lang)}`}</option>
      {/each}
    </select>
  </div>

  <nav class="crumbs" aria-label={t('backup.path')}>
    <button class="link" onclick={() => (folder = '')} disabled={folder === ''}>{rootName}</button>
    {#each crumbs(folder) as c (c.key)}
      <span class="sep">\</span>
      <button class="link" onclick={() => (folder = c.key)} disabled={folder === c.key}>{c.name}</button>
    {/each}
    <span class="muted count">{t('backup.files', { count: formatCount(total.files, lang) })} · {formatBytes(total.bytes, lang)}</span>
  </nav>

  {#if view}
    <ul class="rows" aria-busy={loading}>
      {#if folder !== ''}
        <li class="row folder">
          <button class="link name" onclick={() => (folder = parentFolder(folder))}><Icon name="up" size={15} /> ..</button>
        </li>
      {/if}
      {#each view.folders as f (f.key)}
        {@const all = sum(f.tallies)}
        <li class="row folder">
          <button class="link name" onclick={() => (folder = childFolder(folder, f.key))} title={f.name}>
            <Icon name="folder" size={15} /> <span class="text">{f.name}</span>
          </button>
          <span class="chips">
            {#each classes as c, i (c.key)}
              {#if (f.tallies[i]?.files ?? 0) > 0}
                <span class="chip {c.key}" title={c.label}>{c.label}: {formatCount(f.tallies[i]?.files ?? 0, lang)}</span>
              {/if}
            {/each}
          </span>
          <span class="num">{formatBytes(all.bytes, lang)}</span>
        </li>
      {/each}
      {#each view.files as file (file.rel)}
        <li class="row file">
          <span class="name" title={file.rel}>
            <span class="text">{file.name}</span>
            {#if file.other}
              <span class="muted moved">{t('backup.movedTo', { path: file.other })}</span>
            {/if}
          </span>
          <span class="chips">
            {#if classes.length > 1}
              <span class="chip {file.status}">{classes.find((c) => c.key === file.status)?.label ?? file.status}</span>
            {/if}
            {#if file.evidence}
              <span class="muted evidence">{t(`backup.evidence.${file.evidence}`)}</span>
            {/if}
          </span>
          <span class="num">{formatBytes(file.size, lang)}</span>
        </li>
      {/each}
    </ul>
    {#if view.moreFiles > 0}
      <p class="muted">{t('backup.moreFiles', { count: formatCount(view.moreFiles, lang) })}</p>
    {/if}
    {#if view.folders.length === 0 && view.files.length === 0}
      <p class="muted empty">{t('backup.emptyLevel')}</p>
    {/if}
  {/if}
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .tools {
    display: flex;
    gap: 8px;
  }
  .search {
    flex: 1;
  }
  .tools select {
    width: auto;
  }
  .crumbs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
    font-size: 13.5px;
  }
  .sep {
    color: var(--text-3);
  }
  .count {
    margin-left: auto;
    font-size: 12.5px;
  }
  .link {
    border: 0;
    background: none;
    padding: 2px 4px;
    color: var(--accent);
    cursor: pointer;
    font: inherit;
  }
  .link:disabled {
    color: var(--text);
    cursor: default;
    font-weight: 600;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--border);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto 90px;
    align-items: center;
    gap: 10px;
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
    font-size: 13.5px;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    text-align: left;
  }
  .row.folder .name {
    color: var(--text);
  }
  .file .name {
    flex-direction: column;
    align-items: flex-start;
    gap: 0;
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
  .moved {
    font-size: 12px;
  }
  .chips {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .chip {
    font-size: 11.5px;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-2);
    white-space: nowrap;
  }
  .chip.missing,
  .chip.onlyA {
    color: var(--warn);
  }
  .chip.different {
    color: var(--danger);
  }
  .chip.same {
    color: var(--ok);
  }
  .evidence {
    font-size: 11.5px;
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .empty {
    padding: 16px 0;
    text-align: center;
  }
</style>
