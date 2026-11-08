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

  let {
    volumes,
    onerror,
    scope = null,
  }: {
    volumes: VolumeView[];
    onerror: (message: string) => void;
    /** The disk of the screen (ADR-0019): its proposals only; every disk when null. */
    scope?: number | null;
  } = $props();

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

  const groups = $derived(
    view ? orderGroups(view.groups).filter((g) => scope === null || g.volumeId === scope) : [],
  );
  const bins = $derived(view ? view.recycleBins.filter((b) => scope === null || b.volumeId === scope) : []);
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
  <header class="intro">
    <p>{t('cleanup.never')}</p>
    <button onclick={scan} disabled={loading || busy}><Icon name="refresh" size={15} /> {t('cleanup.rescan')}</button>
  </header>

  {#if result}
    <div class="result" role="status">
      <span>{result}</span>
      <button class="ghost" aria-label={t('actions.close')} onclick={() => (result = null)}><Icon name="close" size={14} /></button>
    </div>
  {/if}

  {#if view === null}
    <p class="empty card">{loading ? t('cleanup.scanning') : ''}</p>
  {:else if groups.length === 0}
    <p class="empty card">{scope === null ? t('cleanup.nothing') : t('cleanup.nothingHere')}</p>
  {:else}
    {#each groups as g (groupKey(g))}
      {@const s = sel(g)}
      {@const open = expanded.has(groupKey(g))}
      <article class="group" class:blocked={g.blockedBy !== null} aria-label={`${title(g)} · ${nameOf(g.volumeId)}`}>
        <header>
          <div class="g-title">
            <h2>{title(g)}{#if scope === null}{' '}<span class="muted vol">· {nameOf(g.volumeId)}</span>{/if}</h2>
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
            <button class={g.permanent ? 'primary danger' : 'signal'} disabled={s.size === 0 || busy} onclick={() => ask(g)}>
              {g.permanent
                ? t('cleanup.deletePermanently', { size: formatBytes(selectedBytes(g, s), lang) })
                : t('cleanup.recycle', { size: formatBytes(selectedBytes(g, s), lang) })}
            </button>
          </div>
        {/if}
      </article>
    {/each}
  {/if}

  {#if bins.length > 0}
    <aside class="bins" aria-label={t('cleanup.binsTitle')}>
      <h2>{t('cleanup.binsTitle')}</h2>
      {#each bins as b (b.volumeId)}
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
    <button class={confirming?.group.permanent ? 'primary danger' : 'signal'} onclick={confirmDelete}>
      {confirming?.group.permanent ? t('cleanup.deleteNow') : t('cleanup.recycleNow')}
    </button>
  {/snippet}
</Dialog>

<style>
  .cleanup {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .intro {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .intro p {
    flex: 1;
    margin: 0;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .intro button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--surface);
  }
  .result {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    background: var(--ok-soft);
    border-radius: var(--radius-sm);
  }
  .result span {
    flex: 1;
  }
  .group {
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .group.blocked {
    opacity: 0.85;
  }
  .group header {
    display: flex;
    align-items: flex-start;
    gap: 16px;
  }
  .g-title {
    flex: 1;
    min-width: 0;
  }
  .group h2 {
    font-size: 16px;
  }
  .vol {
    font-family: var(--font);
    font-size: 13px;
    font-weight: 400;
  }
  .why {
    margin: 2px 0 0;
    font-size: 12.5px;
  }
  .g-total {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    white-space: nowrap;
  }
  .g-total strong {
    font-family: var(--font-display);
    font-size: 20px;
  }
  .g-total span {
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  .note {
    margin: 0;
    color: var(--signal-strong);
    font-size: 12.5px;
  }
  .items {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .items li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto 110px;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    border-top: 1px solid var(--line);
  }
  .items label {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    min-width: 0;
    color: var(--text);
    font-weight: 400;
  }
  .path {
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .num {
    font-family: var(--font-mono);
    font-size: 12px;
    text-align: right;
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
  .warn {
    color: var(--signal-strong);
  }
  .bins {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px 18px;
    background: var(--surface);
    border-radius: var(--radius);
  }
  .bins h2 {
    font-size: 15px;
  }
  .bins p {
    margin: 0;
    font-size: 12.5px;
  }
</style>
