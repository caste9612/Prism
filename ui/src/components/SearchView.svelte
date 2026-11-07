<script lang="ts">
  // Search (F07): one box for words and filters, controls equivalent to the filters (F07-AC5),
  // mistakes shown where they are (F07-AC6), results from every disk with the offline ones
  // marked with their name and location (F07-AC4), actions from the keyboard (F07-AC7).
  import type { SearchHitView } from '../lib/bindings/SearchHitView';
  import type { SearchResultView } from '../lib/bindings/SearchResultView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { volumeName } from '../lib/catalog';
  import { ago, formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import { getFilter, setFilter, splitAt, utcOffsetMinutes } from '../lib/search';
  import Icon from './Icon.svelte';

  let {
    volumes,
    onerror,
    compact = false,
    onopened,
  }: {
    volumes: VolumeView[];
    onerror: (message: string) => void;
    /** The separate search window (F07-AC1): smaller, hides after opening a result. */
    compact?: boolean;
    onopened?: () => void;
  } = $props();

  const lang = $derived(language());
  let text = $state('');
  let result = $state<SearchResultView | null>(null);
  let selected = $state(0);
  let copied = $state<number | null>(null);
  let input: HTMLInputElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0;

  export function focus() {
    input?.focus();
    input?.select();
  }

  async function run(q: string) {
    const mine = ++seq;
    try {
      const r = await backend.search(q, utcOffsetMinutes());
      if (mine === seq) {
        result = r;
        selected = 0;
      }
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  function changed(next: string) {
    text = next;
    clearTimeout(timer);
    if (next.trim() === '') {
      result = null;
      return;
    }
    timer = setTimeout(() => void run(next), 120);
  }

  function setControl(key: string, value: string) {
    changed(setFilter(text, key, value === '' ? null : value));
    input?.focus();
  }

  const hits = $derived(result?.hits ?? []);
  const current = $derived(hits[selected] ?? null);

  async function open(h: SearchHitView | null, reveal = false) {
    if (!h || !h.online) return;
    try {
      if (reveal) await backend.revealEntry(h.volumeId, h.entryId);
      else await backend.openEntry(h.volumeId, h.entryId);
      onopened?.();
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  async function copy(h: SearchHitView | null) {
    if (!h) return;
    try {
      await navigator.clipboard.writeText(h.fullPath);
      copied = h.entryId;
      setTimeout(() => (copied = null), 1500);
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      selected = Math.min(selected + 1, Math.max(0, hits.length - 1));
      e.preventDefault();
    } else if (e.key === 'ArrowUp') {
      selected = Math.max(0, selected - 1);
      e.preventDefault();
    } else if (e.key === 'Enter') {
      void open(current, e.shiftKey || e.ctrlKey);
      e.preventDefault();
    } else if (e.key === 'c' && e.ctrlKey && input && input.selectionStart === input.selectionEnd) {
      void copy(current);
      e.preventDefault();
    }
  }

  const when = (ms: number | null) => {
    if (ms === null) return '';
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };
  const errorParts = $derived(result?.error ? splitAt(text, result.error.start, result.error.end) : null);
  const disks = $derived(volumes.filter((v) => v.lastIndexedAt !== null && v.status !== 'ignored'));
</script>

<section class="search" class:compact>
  <div class="box">
    <Icon name="search" size={18} />
    <input
      bind:this={input}
      value={text}
      oninput={(e) => changed(e.currentTarget.value)}
      onkeydown={keydown}
      placeholder={t('search.placeholder')}
      aria-label={t('search.label')}
      aria-invalid={result?.error ? 'true' : undefined}
      aria-describedby={result?.error ? 'search-error' : undefined}
      spellcheck="false"
      autocomplete="off"
    />
  </div>

  <div class="controls" aria-label={t('search.filters')}>
    <select aria-label={t('search.type')} value={getFilter(text, 'type') ?? ''} onchange={(e) => setControl('type', e.currentTarget.value)}>
      <option value="">{t('search.anyType')}</option>
      {#each ['image', 'video', 'audio', 'document', 'archive', 'code', 'program'] as v (v)}
        <option value={v}>{t(`search.types.${v}`)}</option>
      {/each}
    </select>
    <select aria-label={t('search.size')} value={getFilter(text, 'size') ?? ''} onchange={(e) => setControl('size', e.currentTarget.value)}>
      <option value="">{t('search.anySize')}</option>
      {#each ['>1MB', '>100MB', '>1GB', '<100KB'] as v (v)}
        <option value={v}>{t(`search.sizes.${v}`)}</option>
      {/each}
    </select>
    <select aria-label={t('search.modified')} value={getFilter(text, 'modified') ?? ''} onchange={(e) => setControl('modified', e.currentTarget.value)}>
      <option value="">{t('search.anyTime')}</option>
      {#each ['<7d', '<30d', '<1y', '>1y'] as v (v)}
        <option value={v}>{t(`search.times.${v}`)}</option>
      {/each}
    </select>
    <select aria-label={t('search.disk')} value={getFilter(text, 'disk') ?? ''} onchange={(e) => setControl('disk', e.currentTarget.value)}>
      <option value="">{t('search.anyDisk')}</option>
      {#each disks as d (d.id)}
        <option value={d.name}>{volumeName(d)}</option>
      {/each}
    </select>
    <select aria-label={t('search.state')} value={getFilter(text, 'online') ?? ''} onchange={(e) => setControl('online', e.currentTarget.value)}>
      <option value="">{t('search.anyState')}</option>
      <option value="yes">{t('search.onlineOnly')}</option>
      <option value="no">{t('search.offlineOnly')}</option>
    </select>
    <select aria-label={t('search.kind')} value={getFilter(text, 'is') ?? ''} onchange={(e) => setControl('is', e.currentTarget.value)}>
      <option value="">{t('search.filesAndFolders')}</option>
      <option value="file">{t('search.filesOnly')}</option>
      <option value="dir">{t('search.foldersOnly')}</option>
    </select>
  </div>

  {#if result?.error && errorParts}
    <div class="error" id="search-error" role="alert">
      <p>{t(`search.err.${result.error.code}`, { value: result.error.value })}</p>
      <code>{errorParts[0]}<mark>{errorParts[1] || ' '}</mark>{errorParts[2]}</code>
    </div>
  {:else if result}
    <p class="status muted" role="status">
      {result.capped
        ? t('search.manyResults', { count: formatCount(result.total, lang) })
        : t('search.results', { count: formatCount(result.total, lang) })}
      {#if result.total > hits.length}· {t('search.showing', { count: formatCount(hits.length, lang) })}{/if}
    </p>
    <ul class="hits" role="listbox" aria-label={t('search.resultsLabel')}>
      {#each hits as h, i (`${h.volumeId}:${h.entryId}`)}
        <li
          role="option"
          aria-selected={i === selected}
          tabindex="-1"
          class:selected={i === selected}
          class:offline={!h.online}
          onclick={() => (selected = i)}
          ondblclick={() => void open(h)}
          onkeydown={() => {}}
        >
          <span class="kind" style:background={h.isDir ? 'var(--cat-dir-2)' : `var(--cat-${h.kind})`}></span>
          <div class="main">
            <span class="name">{h.name}</span>
            <span class="folder muted" title={h.fullPath}>{h.folder || '\\'}</span>
          </div>
          <div class="disk">
            <span>{h.disk}</span>
            {#if !h.online}
              <span class="off">{h.location ? t('search.offlineAt', { location: h.location }) : t('search.offline')}</span>
            {/if}
          </div>
          <span class="num">{h.isDir ? '' : formatBytes(h.size, lang)}</span>
          <span class="muted when">{when(h.modified)}</span>
          <span class="acts">
            <button class="ghost icon tiny" disabled={!h.online} title={t('search.open')} aria-label={`${t('search.open')}: ${h.name}`} onclick={() => void open(h)}><Icon name="open" size={13} /></button>
            <button class="ghost icon tiny" disabled={!h.online} title={t('search.reveal')} aria-label={`${t('search.reveal')}: ${h.name}`} onclick={() => void open(h, true)}><Icon name="folder" size={13} /></button>
            <button class="ghost icon tiny" title={copied === h.entryId ? t('search.copied') : t('search.copy')} aria-label={`${t('search.copy')}: ${h.name}`} onclick={() => void copy(h)}><Icon name={copied === h.entryId ? 'check' : 'copy'} size={13} /></button>
          </span>
        </li>
      {/each}
    </ul>
    {#if hits.length === 0}
      <p class="empty muted">{t('search.nothing')}</p>
    {/if}
  {:else}
    <div class="help muted">
      <p>{t('search.help')}</p>
      <ul>
        <li><code>report</code> · <code>"foto mare"</code></li>
        <li><code>size:&gt;1GB</code> · <code>size:10MB..1GB</code> · <code>ext:pdf,docx</code> · <code>type:image</code></li>
        <li><code>modified:&lt;30d</code> · <code>modified:&gt;2024-01-01</code> · <code>disk:"Backup Foto"</code> · <code>d:</code></li>
        <li><code>path:Documenti</code> · <code>is:dir</code> · <code>online:no</code> · <code>regex:^IMG_\d+</code></li>
      </ul>
      <p>{t('search.keys')}</p>
    </div>
  {/if}
</section>

<style>
  .search {
    display: flex;
    flex-direction: column;
    gap: 10px;
    height: 100%;
    min-height: 0;
  }
  .box {
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px 12px;
    color: var(--text-2);
  }
  .box:focus-within {
    border-color: var(--accent);
  }
  .box input {
    border: 0;
    padding: 9px 0;
    font-size: 16px;
    background: transparent;
  }
  .box input:focus-visible {
    outline: none;
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .controls select {
    width: auto;
    padding: 4px 8px;
    font-size: 12.5px;
  }
  .error {
    border: 1px solid var(--danger);
    background: var(--danger-soft);
    border-radius: var(--radius-sm);
    padding: 8px 12px;
  }
  .error p {
    margin: 0 0 4px;
  }
  .error code {
    white-space: pre;
  }
  .error mark {
    background: var(--danger);
    color: #ffffff;
    border-radius: 3px;
    padding: 0 2px;
  }
  .status {
    margin: 0;
    font-size: 12.5px;
  }
  .hits {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: auto;
    flex: 1;
    min-height: 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .hits li {
    display: grid;
    grid-template-columns: 10px minmax(0, 1fr) minmax(90px, 0.35fr) 72px 90px auto;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    cursor: default;
  }
  .hits li.selected {
    background: var(--accent-soft);
  }
  .hits li.offline .name {
    color: var(--text-2);
  }
  .kind {
    width: 10px;
    height: 10px;
    border-radius: 3px;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name,
  .folder {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    font-weight: 600;
  }
  .folder {
    font-size: 12px;
  }
  .disk {
    display: flex;
    flex-direction: column;
    font-size: 12px;
    min-width: 0;
  }
  .disk span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .off {
    color: var(--warn);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: 12.5px;
  }
  .when {
    font-size: 12px;
    text-align: right;
  }
  .acts {
    display: flex;
    gap: 2px;
  }
  button.tiny {
    padding: 3px;
  }
  .help code {
    background: var(--surface-2);
    border-radius: 4px;
    padding: 1px 5px;
  }
  .help ul {
    padding-left: 18px;
    line-height: 1.9;
  }
  .empty {
    text-align: center;
    padding: 24px 0;
  }
  .compact .hits li {
    grid-template-columns: 10px minmax(0, 1fr) minmax(80px, 0.3fr) 64px auto;
  }
  .compact .when {
    display: none;
  }
</style>
