<script lang="ts">
  // Guided cleanup (F09): recoverable space by category on the connected disks, each with its
  // explanation and total; every item can be deselected (F09-AC3); deleting goes to the Recycle
  // Bin through the safe deletion path (ADR-0012), permanently only where there is no Recycle
  // Bin and after an explicit confirmation (F16-AC2).
  import type { CleanupGroupView } from '../lib/bindings/CleanupGroupView';
  import type { CleanupView } from '../lib/bindings/CleanupView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { volumeName } from '../lib/catalog';
  import { defaultSelection, groupKey, orderGroups, selectedBytes } from '../lib/cleanup';
  import { ago, formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';
  import Icon from './Icon.svelte';

  let { volumes, onerror }: { volumes: VolumeView[]; onerror: (message: string) => void } = $props();

  const lang = $derived(language());
  let view = $state<CleanupView | null>(null);
  let loading = $state(false);
  let selection = $state(new Map<string, Set<number>>());
  let expanded = $state(new Set<string>());
  let confirming = $state<{ group: CleanupGroupView; ids: number[]; bytes: number } | null>(null);
  let busy = $state(false);
  let result = $state<string | null>(null);

  const VISIBLE = 8;

  async function scan() {
    loading = true;
    try {
      const v = await backend.cleanupScan();
      selection = new Map(v.groups.map((g) => [groupKey(g), defaultSelection(g)]));
      view = v;
    } catch (e) {
      onerror(errorMessage(e));
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void scan();
  });

  const groups = $derived(view ? orderGroups(view.groups) : []);
  const nameOf = (id: number) => {
    const v = volumes.find((x) => x.id === id);
    return v ? volumeName(v) : `#${id}`;
  };
  const sel = (g: CleanupGroupView) => selection.get(groupKey(g)) ?? new Set<number>();

  function toggle(g: CleanupGroupView, id: number) {
    const s = new Set(sel(g));
    if (s.has(id)) s.delete(id);
    else s.add(id);
    selection = new Map(selection).set(groupKey(g), s);
  }

  function setAll(g: CleanupGroupView, on: boolean) {
    selection = new Map(selection).set(groupKey(g), on ? new Set(g.items.map((i) => i.id)) : new Set());
  }

  function toggleExpanded(g: CleanupGroupView) {
    const e = new Set(expanded);
    const k = groupKey(g);
    if (e.has(k)) e.delete(k);
    else e.add(k);
    expanded = e;
  }

  function ask(g: CleanupGroupView) {
    const s = sel(g);
    const ids = g.items.filter((i) => s.has(i.id)).map((i) => i.id);
    if (ids.length > 0) confirming = { group: g, ids, bytes: selectedBytes(g, s) };
  }

  async function confirmDelete() {
    if (!confirming) return;
    const { group, ids } = confirming;
    confirming = null;
    busy = true;
    try {
      const r = await backend.deleteEntries(group.volumeId, ids, group.category === 'empty_folders', group.permanent);
      result = t(group.permanent ? 'cleanup.resultDeleted' : 'cleanup.resultRecycled', {
        count: formatCount(r.done, lang),
        size: formatBytes(r.freed, lang),
      });
      if (r.skipped > 0) result += ' ' + t('cleanup.resultSkipped', { count: formatCount(r.skipped, lang) });
      if (r.failed > 0) result += ' ' + t('cleanup.resultFailed', { count: formatCount(r.failed, lang) });
      await scan();
    } catch (e) {
      onerror(errorMessage(e));
    } finally {
      busy = false;
    }
  }

  const when = (ms: number | null) => {
    if (ms === null) return '';
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };
  const title = (g: CleanupGroupView) =>
    g.category === 'browser_cache' ? t('cleanup.cat.browser_cache', { browser: g.owner ?? '' }) : t(`cleanup.cat.${g.category}`);
</script>

<section class="cleanup">
  <header class="page-head">
    <div>
      <h1>{t('cleanup.title')}</h1>
      <p class="muted">{t('cleanup.subtitle')}</p>
    </div>
    <button onclick={scan} disabled={loading || busy}><Icon name="refresh" size={15} /> {t('cleanup.rescan')}</button>
  </header>

  {#if result}
    <div class="result" role="status">
      <span>{result}</span>
      <button class="ghost" aria-label={t('actions.close')} onclick={() => (result = null)}><Icon name="close" size={14} /></button>
    </div>
  {/if}

  {#if view === null}
    <p class="muted">{loading ? t('cleanup.scanning') : ''}</p>
  {:else if groups.length === 0}
    <p class="empty muted">{t('cleanup.nothing')}</p>
  {:else}
    {#each groups as g (groupKey(g))}
      {@const s = sel(g)}
      {@const open = expanded.has(groupKey(g))}
      <article class="group" class:blocked={g.blockedBy !== null} aria-label={`${title(g)} · ${nameOf(g.volumeId)}`}>
        <header>
          <div class="g-title">
            <h2>{title(g)} <span class="muted vol">· {nameOf(g.volumeId)}</span></h2>
            <p class="muted why">{t(`cleanup.why.${g.category}`)}</p>
          </div>
          <div class="g-total">
            <strong>{formatBytes(g.total, lang)}</strong>
            <span class="muted">{t('cleanup.items', { count: formatCount(g.count, lang) })}</span>
          </div>
        </header>

        {#if g.blockedBy}
          <p class="note">{t('cleanup.closeBrowser', { browser: g.blockedBy })}</p>
        {:else}
          <ul class="items">
            {#each open ? g.items : g.items.slice(0, VISIBLE) as item (item.id)}
              <li>
                <label>
                  <input type="checkbox" checked={s.has(item.id)} onchange={() => toggle(g, item.id)} />
                  <span class="path" title={item.path}>{item.path}</span>
                </label>
                <span class="num">{item.isDir && item.size === 0 ? t('cleanup.emptyFolder') : formatBytes(item.size, lang)}</span>
                <span class="muted when">{when(item.modified)}</span>
              </li>
            {/each}
          </ul>
          <div class="g-actions">
            {#if g.items.length > VISIBLE}
              <button class="ghost small" onclick={() => toggleExpanded(g)}>
                {open ? t('cleanup.showLess') : t('cleanup.showAll', { count: formatCount(g.items.length, lang) })}
              </button>
            {/if}
            {#if g.count > g.items.length}
              <span class="muted small-text">{t('cleanup.moreLater', { count: formatCount(g.count - g.items.length, lang) })}</span>
            {/if}
            <span class="spacer"></span>
            <button class="ghost small" onclick={() => setAll(g, s.size < g.items.length)}>
              {s.size < g.items.length ? t('cleanup.selectAll') : t('cleanup.selectNone')}
            </button>
            <button class="primary" class:danger={g.permanent} disabled={s.size === 0 || busy} onclick={() => ask(g)}>
              <Icon name="trash" size={15} />
              {g.permanent
                ? t('cleanup.deletePermanently', { size: formatBytes(selectedBytes(g, s), lang) })
                : t('cleanup.recycle', { size: formatBytes(selectedBytes(g, s), lang) })}
            </button>
          </div>
        {/if}
      </article>
    {/each}
  {/if}

  {#if view && view.recycleBins.length > 0}
    <aside class="bins" aria-label={t('cleanup.binsTitle')}>
      <h2>{t('cleanup.binsTitle')}</h2>
      {#each view.recycleBins as b (b.volumeId)}
        <p>{t('cleanup.bin', { name: nameOf(b.volumeId), count: formatCount(b.items, lang), size: formatBytes(b.bytes, lang) })}</p>
      {/each}
      <p class="muted">{t('cleanup.binHint')}</p>
    </aside>
  {/if}
</section>

<Dialog
  open={confirming !== null}
  title={confirming
    ? t(confirming.group.permanent ? 'cleanup.confirmDeleteTitle' : 'cleanup.confirmRecycleTitle', {
        count: formatCount(confirming.ids.length, lang),
        size: formatBytes(confirming.bytes, lang),
      })
    : ''}
  onclose={() => (confirming = null)}
>
  <p>{t('cleanup.confirmCheck')}</p>
  {#if confirming?.group.permanent}
    <p class="warn"><strong>{t('cleanup.confirmPermanent')}</strong></p>
  {/if}
  {#snippet actions()}
    <button onclick={() => (confirming = null)}>{t('actions.cancel')}</button>
    <button class="primary" class:danger={confirming?.group.permanent} onclick={confirmDelete}>
      {confirming?.group.permanent ? t('cleanup.deleteNow') : t('cleanup.recycleNow')}
    </button>
  {/snippet}
</Dialog>

<style>
  .cleanup {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-width: 1100px;
  }
  .page-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }
  .page-head h1 {
    font-size: 24px;
  }
  .page-head p {
    margin: 2px 0 0;
  }
  .page-head button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .result {
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--accent-soft);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    padding: 8px 8px 8px 14px;
  }
  .result span {
    flex: 1;
  }
  .group {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-shadow: var(--shadow);
  }
  .group.blocked {
    opacity: 0.85;
  }
  .group header {
    display: flex;
    justify-content: space-between;
    gap: 16px;
  }
  .group h2 {
    font-size: 16px;
  }
  .vol {
    font-weight: 400;
  }
  .why {
    margin: 3px 0 0;
    font-size: 13px;
    max-width: 680px;
  }
  .g-total {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    white-space: nowrap;
  }
  .g-total strong {
    font-size: 18px;
  }
  .note {
    margin: 0;
    color: var(--warn);
  }
  .items {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--border);
  }
  .items li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 90px 110px;
    align-items: center;
    gap: 10px;
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
  }
  .items label {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    margin: 0;
    color: var(--text);
    font-size: 14px;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .when {
    font-size: 12px;
    text-align: right;
  }
  .g-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .spacer {
    flex: 1;
  }
  .small-text {
    font-size: 12px;
  }
  .g-actions .primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .warn {
    color: var(--danger);
  }
  .bins {
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    padding: 12px 16px;
  }
  .bins h2 {
    font-size: 15px;
    margin-bottom: 6px;
  }
  .bins p {
    margin: 2px 0;
  }
  .empty {
    padding: 40px 0;
    text-align: center;
  }
</style>
