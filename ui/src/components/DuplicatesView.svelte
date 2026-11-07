<script lang="ts" module>
  import type { RuleInput } from '../lib/bindings/RuleInput';
  import type { DupFilterInput } from '../lib/bindings/DupFilterInput';

  // Filters and rules survive a change of screen (not a restart).
  let savedFilter: DupFilterInput = { minSize: 1_000_000, category: null, volumeId: null, folder: null };
  let savedRules: RuleInput[] = [{ kind: 'keepOldest', volumeId: null, text: null }];

  /** Opens the duplicates of a folder (Explorer menu, F18-AC1). */
  export function presetDuplicates(volumeId: number, folder: string) {
    savedFilter = { ...savedFilter, volumeId, folder: folder || null };
  }
</script>

<script lang="ts">
  // Exact duplicates (F10) on every disk of the catalog, connected or not: groups with their
  // copies, where each disk is kept, the space a single copy would free; selection rules
  // (F10-AC3) and manual changes within the limits of F10-AC4; deletion through the safe path
  // with the last-copy rule and the content checked again (F16-AC3/AC4).
  import type { DupFileView } from '../lib/bindings/DupFileView';
  import type { DupGroupView } from '../lib/bindings/DupGroupView';
  import type { DuplicatesView } from '../lib/bindings/DuplicatesView';
  import type { Settings } from '../lib/bindings/Settings';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import type { ApiError } from '../lib/bindings/ApiError';
  import { onMount } from 'svelte';
  import { backend, errorMessage } from '../lib/api';
  import { volumeName } from '../lib/catalog';
  import { MIN_SIZES, RULE_KINDS, canSelect, onlyOfflineLeft, proposed, ruleComplete, toggle, totals, type Selection } from '../lib/duplicates';
  import { formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';
  import Icon from './Icon.svelte';

  let {
    volumes,
    settings,
    onchange,
    onerror,
  }: {
    volumes: VolumeView[];
    settings: Settings;
    onchange: (s: Settings) => void;
    onerror: (message: string) => void;
  } = $props();

  const lang = $derived(language());
  const CATEGORIES = ['images', 'videos', 'audio', 'documents', 'archives', 'disk_images', 'code', 'programs', 'other'];

  let filter = $state<DupFilterInput>({ ...savedFilter });
  let rules = $state<RuleInput[]>(savedRules.map((r) => ({ ...r })));
  let view = $state<DuplicatesView | null>(null);
  let selection = $state<Selection>(new Map());
  let loading = $state(false);
  let busy = $state(false);
  let result = $state<string | null>(null);
  let confirming = $state(false);
  let confirmingOffline = $state(false);

  const catalogued = $derived(volumes.filter((v) => v.lastIndexedAt !== null));

  async function load() {
    loading = true;
    savedFilter = { ...filter };
    savedRules = rules.map((r) => ({ ...r }));
    try {
      const v = await backend.duplicateGroups(
        { ...filter },
        rules.filter(ruleComplete).map((r) => ({ ...r })),
      );
      selection = proposed(v.groups);
      view = v;
    } catch (e) {
      onerror(errorMessage(e));
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load();
  });

  function setFilter(patch: Partial<DupFilterInput>) {
    filter = { ...filter, ...patch };
    void load();
  }

  function setRule(i: number, patch: Partial<RuleInput>) {
    rules = rules.map((r, j) => (j === i ? { ...r, ...patch } : r));
  }

  const groups = $derived(view?.groups ?? []);
  const chosen = $derived(totals(groups, selection));
  const sel = (g: DupGroupView) => selection.get(g.key) ?? new Set<number>();
  const fileName = (g: DupGroupView) => g.files[0]?.path.split('\\').pop() ?? '';
  const folderOf = (f: DupFileView) => {
    const parts = f.path.split('\\');
    parts.pop();
    return parts.join('\\');
  };
  const date = (ms: number | null) => (ms === null ? '' : new Date(ms).toLocaleDateString(lang));
  const diskLabel = (f: DupFileView) => (f.location ? `${f.disk} · ${f.location}` : f.disk);

  async function remove(allowOfflineOnly: boolean) {
    confirming = false;
    confirmingOffline = false;
    busy = true;
    try {
      const r = await backend.deleteDuplicates(chosen.ids, allowOfflineOnly, chosen.permanent);
      result = t('duplicates.result', { count: formatCount(r.done, lang), size: formatBytes(r.freed, lang) });
      if (r.skipped > 0) result += ' ' + t('duplicates.resultSkipped', { count: formatCount(r.skipped, lang) });
      if (r.failed > 0) result += ' ' + t('duplicates.resultFailed', { count: formatCount(r.failed, lang) });
      await load();
    } catch (e) {
      // F16-AC4: only copies on disconnected disks would stay; ask before going on.
      if ((e as ApiError | null)?.code === 'only_offline_copies' && !allowOfflineOnly) confirmingOffline = true;
      else onerror(errorMessage(e));
    } finally {
      busy = false;
    }
  }

  async function reveal(f: DupFileView) {
    try {
      await backend.revealEntry(f.volumeId, f.entryId);
    } catch (e) {
      onerror(errorMessage(e));
    }
  }
</script>

<section class="dups">
  <header class="page-head">
    <div>
      <h1>{t('duplicates.title')}</h1>
      <p class="muted">{t('duplicates.subtitle')}</p>
    </div>
    <button onclick={load} disabled={loading || busy}><Icon name="refresh" size={15} /> {t('duplicates.refresh')}</button>
  </header>

  {#if view && view.pendingFiles > 0}
    <div class="pending" role="status">
      <span>
        {settings.hashingPaused
          ? t('duplicates.pendingPaused', { count: formatCount(view.pendingFiles, lang) })
          : t('duplicates.pending', { count: formatCount(view.pendingFiles, lang) })}
      </span>
      {#if settings.hashingPaused}
        <button class="small" onclick={() => onchange({ ...settings, hashingPaused: false })}>{t('duplicates.resume')}</button>
      {/if}
    </div>
  {/if}

  {#if result}
    <div class="result" role="status">
      <span>{result}</span>
      <button class="ghost" aria-label={t('actions.close')} onclick={() => (result = null)}><Icon name="close" size={14} /></button>
    </div>
  {/if}

  <div class="filters" role="group" aria-label={t('duplicates.filters')}>
    <label>
      <span>{t('duplicates.minSize')}</span>
      <select value={String(filter.minSize)} onchange={(e) => setFilter({ minSize: Number(e.currentTarget.value) })}>
        {#each MIN_SIZES as s (s)}
          <option value={String(s)}>{s === 0 ? t('duplicates.anySize') : formatBytes(s, lang)}</option>
        {/each}
      </select>
    </label>
    <label>
      <span>{t('duplicates.type')}</span>
      <select value={filter.category ?? ''} onchange={(e) => setFilter({ category: e.currentTarget.value || null })}>
        <option value="">{t('duplicates.anyType')}</option>
        {#each CATEGORIES as c (c)}
          <option value={c}>{t(`category.${c}`)}</option>
        {/each}
      </select>
    </label>
    <label>
      <span>{t('duplicates.disk')}</span>
      <select
        value={filter.volumeId === null ? '' : String(filter.volumeId)}
        onchange={(e) => setFilter({ volumeId: e.currentTarget.value ? Number(e.currentTarget.value) : null })}
      >
        <option value="">{t('duplicates.anyDisk')}</option>
        {#each catalogued as v (v.id)}
          <option value={String(v.id)}>{volumeName(v)}</option>
        {/each}
      </select>
    </label>
    <label class="grow">
      <span>{t('duplicates.folder')}</span>
      <input
        value={filter.folder ?? ''}
        placeholder={t('duplicates.folderPlaceholder')}
        spellcheck="false"
        onchange={(e) => setFilter({ folder: e.currentTarget.value.trim() || null })}
      />
    </label>
  </div>

  <details class="rules" open>
    <summary>{t('duplicates.rules')}</summary>
    <p class="muted hint">{t('duplicates.rulesHint')}</p>
    <ol>
      {#each rules as r, i (i)}
        <li>
          <select aria-label={t('duplicates.rules')} value={r.kind} onchange={(e) => setRule(i, { kind: e.currentTarget.value })}>
            {#each RULE_KINDS as k (k)}
              <option value={k}>{t(`duplicates.rule.${k}`)}</option>
            {/each}
          </select>
          {#if r.kind === 'keepOnVolume'}
            <select
              aria-label={t('duplicates.disk')}
              value={r.volumeId === null ? '' : String(r.volumeId)}
              onchange={(e) => setRule(i, { volumeId: e.currentTarget.value ? Number(e.currentTarget.value) : null })}
            >
              <option value="">—</option>
              {#each catalogued as v (v.id)}
                <option value={String(v.id)}>{volumeName(v)}</option>
              {/each}
            </select>
          {:else if r.kind === 'neverIn'}
            <input aria-label={t('duplicates.ruleText')} value={r.text ?? ''} spellcheck="false" oninput={(e) => setRule(i, { text: e.currentTarget.value })} />
          {/if}
          <button class="ghost icon" title={t('duplicates.removeRule')} aria-label={t('duplicates.removeRule')} onclick={() => (rules = rules.filter((_, j) => j !== i))}>
            <Icon name="close" size={14} />
          </button>
        </li>
      {/each}
    </ol>
    <div class="rule-actions">
      <button class="ghost small" onclick={() => (rules = [...rules, { kind: 'keepShortestPath', volumeId: null, text: null }])}>
        <Icon name="plus" size={14} /> {t('duplicates.addRule')}
      </button>
      <button class="small" onclick={load} disabled={loading || busy}>{t('duplicates.apply')}</button>
    </div>
  </details>

  {#if view === null}
    <p class="muted">{loading ? t('duplicates.loading') : ''}</p>
  {:else if groups.length === 0}
    <p class="empty muted">{t('duplicates.nothing')}</p>
  {:else}
    <p class="summary">
      <strong>{t('duplicates.summary', { groups: formatCount(view.totalGroups, lang), size: formatBytes(view.totalRecoverable, lang) })}</strong>
      {#if view.totalGroups > groups.length}
        <span class="muted"> · {t('duplicates.shownOf', { count: formatCount(groups.length, lang) })}</span>
      {/if}
    </p>
    {#each groups as g (g.key)}
      {@const s = sel(g)}
      <article class="group" aria-label={fileName(g)}>
        <header>
          <div class="g-title">
            <h2 title={fileName(g)}>{fileName(g)}</h2>
            <p class="muted">{t(`category.${g.kind}`)} · {formatBytes(g.size, lang)} · {t('duplicates.copies', { count: formatCount(g.files.length, lang) })}</p>
          </div>
          <strong class="g-total">{t('duplicates.recoverable', { size: formatBytes(g.recoverable, lang) })}</strong>
        </header>
        <ul class="copies">
          {#each g.files as f (f.entryId)}
            {@const checked = s.has(f.entryId)}
            {@const allowed = canSelect(g, f, s)}
            <li class:offline={!f.online}>
              <label title={f.online ? f.fullPath : t('duplicates.offlineHint')}>
                <input type="checkbox" {checked} disabled={!checked && !allowed} onchange={() => (selection = toggle(selection, g, f))} />
                <span class="where">
                  <span class="disk">{diskLabel(f)}{#if !f.online}<span class="badge">{t('duplicates.offline')}</span>{/if}</span>
                  <span class="path">{folderOf(f) || '\\'}</span>
                </span>
              </label>
              <span class="muted when">{date(f.modified)}</span>
              {#if f.online}
                <button class="ghost icon" title={t('duplicates.open')} aria-label={t('duplicates.open')} onclick={() => reveal(f)}>
                  <Icon name="open" size={15} />
                </button>
              {:else}
                <span></span>
              {/if}
            </li>
          {/each}
        </ul>
        {#if onlyOfflineLeft(g, s)}
          <p class="note">{t('duplicates.onlyOfflineLeft')}</p>
        {/if}
      </article>
    {/each}
  {/if}

  {#if chosen.count > 0}
    <div class="bar" role="region" aria-label={t('duplicates.selected', { count: formatCount(chosen.count, lang), size: formatBytes(chosen.bytes, lang) })}>
      <span>{t('duplicates.selected', { count: formatCount(chosen.count, lang), size: formatBytes(chosen.bytes, lang) })}</span>
      <span class="spacer"></span>
      <button class="ghost small" onclick={() => (selection = new Map())}>{t('duplicates.clearSelection')}</button>
      <button class="primary" class:danger={chosen.permanent} disabled={busy} onclick={() => (confirming = true)}>
        <Icon name="trash" size={15} /> {t('duplicates.deleteSelected', { size: formatBytes(chosen.bytes, lang) })}
      </button>
    </div>
  {/if}
</section>

<Dialog
  open={confirming}
  title={t('duplicates.confirmTitle', { count: formatCount(chosen.count, lang), size: formatBytes(chosen.bytes, lang) })}
  onclose={() => (confirming = false)}
>
  <p>{t('duplicates.confirmCheck')}</p>
  {#if chosen.permanent}
    <p class="warn"><strong>{t('duplicates.confirmPermanent')}</strong></p>
  {:else}
    <p>{t('duplicates.confirmRecycle')}</p>
  {/if}
  {#snippet actions()}
    <button onclick={() => (confirming = false)}>{t('actions.cancel')}</button>
    <button class="primary" class:danger={chosen.permanent} onclick={() => remove(false)}>{t('duplicates.deleteNow')}</button>
  {/snippet}
</Dialog>

<Dialog open={confirmingOffline} title={t('duplicates.confirmOfflineTitle')} onclose={() => (confirmingOffline = false)}>
  <p>{t('duplicates.confirmOffline')}</p>
  {#snippet actions()}
    <button onclick={() => (confirmingOffline = false)}>{t('actions.cancel')}</button>
    <button class="primary danger" onclick={() => remove(true)}>{t('duplicates.deleteAnyway')}</button>
  {/snippet}
</Dialog>

<style>
  .dups {
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
  .page-head button,
  .rule-actions button,
  .bar .primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .pending,
  .result {
    display: flex;
    align-items: center;
    gap: 10px;
    border-radius: var(--radius);
    padding: 8px 8px 8px 14px;
  }
  .pending {
    background: var(--surface-2);
    border: 1px solid var(--border);
  }
  .result {
    background: var(--accent-soft);
    border: 1px solid var(--accent);
  }
  .pending span,
  .result span {
    flex: 1;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 10px 14px;
    align-items: flex-end;
  }
  .filters label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    font-size: 12.5px;
    min-width: 150px;
  }
  .filters label.grow {
    flex: 1;
    min-width: 200px;
  }
  .rules {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 16px;
  }
  .rules summary {
    font-weight: 600;
    cursor: pointer;
  }
  .hint {
    margin: 6px 0;
    font-size: 13px;
  }
  .rules ol {
    margin: 0;
    padding-left: 22px;
    display: grid;
    gap: 6px;
  }
  .rules li {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .rules li select:first-child {
    width: auto;
    min-width: 280px;
  }
  .rules li input,
  .rules li select + select {
    width: auto;
    min-width: 180px;
  }
  .rule-actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .summary {
    margin: 0;
  }
  .group {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    box-shadow: var(--shadow);
  }
  .group header {
    display: flex;
    justify-content: space-between;
    gap: 16px;
  }
  .g-title {
    min-width: 0;
  }
  .group h2 {
    font-size: 15px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .g-title p {
    margin: 2px 0 0;
    font-size: 12.5px;
  }
  .g-total {
    white-space: nowrap;
  }
  .copies {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--border);
  }
  .copies li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 100px 34px;
    align-items: center;
    gap: 10px;
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
  }
  .copies li.offline .where {
    opacity: 0.7;
  }
  .copies label {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    margin: 0;
    color: var(--text);
    font-size: 14px;
  }
  .where {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .disk {
    font-weight: 600;
    font-size: 13px;
  }
  .badge {
    margin-left: 8px;
    font-weight: 400;
    font-size: 11.5px;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-3);
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--text-2);
  }
  .when {
    font-size: 12px;
    text-align: right;
  }
  .note {
    margin: 0;
    color: var(--warn);
    font-size: 13px;
  }
  .warn {
    color: var(--danger);
  }
  .bar {
    position: sticky;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 14px;
    box-shadow: var(--shadow);
  }
  .spacer {
    flex: 1;
  }
  .empty {
    padding: 40px 0;
    text-align: center;
  }
</style>
