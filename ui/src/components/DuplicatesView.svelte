<script lang="ts" module>
  import type { RuleInput } from '../lib/bindings/RuleInput';
  import type { DupFilterInput } from '../lib/bindings/DupFilterInput';

  // Filters and rules survive a change of screen (not a restart).
  let savedFilter: DupFilterInput = { minSize: 1_000_000, category: null, volumeId: null, folder: null };
  let savedRules: RuleInput[] = [{ kind: 'keepOldest', volumeId: null, text: null }];

  /**
   * Opens the duplicates of a folder (Explorer menu, F18-AC1): the screen opens on the tab of
   * its disk (ADR-0019), which gives the disk; the folder is remembered here.
   */
  export function presetDuplicates(_volumeId: number, folder: string) {
    savedFilter = { ...savedFilter, folder: folder || null };
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
  import { onMount, untrack } from 'svelte';
  import { backend, errorMessage } from '../lib/api';
  import { stickerColor, volumeName } from '../lib/catalog';
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
    scope = null,
  }: {
    volumes: VolumeView[];
    settings: Settings;
    onchange: (s: Settings) => void;
    onerror: (message: string) => void;
    /** The disk of the screen (ADR-0019): its copies only; the whole catalog when null. */
    scope?: number | null;
  } = $props();

  const lang = $derived(language());
  const CATEGORIES = ['images', 'videos', 'audio', 'documents', 'archives', 'disk_images', 'code', 'programs', 'other'];

  let filter = $state<DupFilterInput>(
    untrack(() => (scope !== null ? { ...savedFilter, volumeId: scope } : { ...savedFilter })),
  );
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
    // The disk of a disk tab is not a choice to remember for the whole catalog.
    savedFilter = { ...filter, volumeId: scope === null ? filter.volumeId : savedFilter.volumeId };
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
  <div class="stats" aria-label={t('duplicates.title')}>
    <div>
      <div class="caption">{t('duplicates.recoverableCaption')}</div>
      <div class="value signal">{view ? formatBytes(view.totalRecoverable, lang) : '…'}</div>
    </div>
    <div>
      <div class="caption">{t('duplicates.groupsCaption')}</div>
      <div class="value">{view ? formatCount(view.totalGroups, lang) : '…'}</div>
      {#if view && view.totalGroups > groups.length}<div class="detail">{t('duplicates.shownOf', { count: formatCount(groups.length, lang) })}</div>{/if}
    </div>
    <div>
      <div class="caption">{t('duplicates.pendingCaption')}</div>
      <div class="value">{view ? formatCount(view.pendingFiles, lang) : '…'}</div>
      <div class="detail">{settings.hashingPaused ? t('duplicates.pausedShort') : t('duplicates.pendingShort')}</div>
    </div>
    <div class="refresh-cell">
      <button onclick={load} disabled={loading || busy}><Icon name="refresh" size={15} /> {t('duplicates.refresh')}</button>
    </div>
  </div>

  {#if view && view.pendingFiles > 0}
    <div class="note-line" role="status">
      <span>
        {settings.hashingPaused
          ? t('duplicates.pendingPaused', { count: formatCount(view.pendingFiles, lang) })
          : t('duplicates.pending', { count: formatCount(view.pendingFiles, lang) })}
      </span>
      {#if settings.hashingPaused}
        <button class="small primary" onclick={() => onchange({ ...settings, hashingPaused: false })}>{t('duplicates.resume')}</button>
      {/if}
    </div>
  {/if}

  {#if result}
    <div class="note-line done" role="status">
      <span>{result}</span>
      <button class="ghost icon" aria-label={t('actions.close')} onclick={() => (result = null)}><Icon name="close" size={14} /></button>
    </div>
  {/if}

  <div class="body">
    <aside class="side">
      <section class="card box" role="group" aria-label={t('duplicates.filters')}>
        <h2>{t('duplicates.filters')}</h2>
        <div class="field">
          <span class="field-label">{t('duplicates.minSize')}</span>
          <div class="chips">
            {#each MIN_SIZES as m (m)}
              <button class="small" aria-pressed={filter.minSize === m} onclick={() => setFilter({ minSize: m })}>
                {m === 0 ? t('duplicates.anySize') : formatBytes(m, lang)}
              </button>
            {/each}
          </div>
        </div>
        <div class="field">
          <span class="field-label">{t('duplicates.type')}</span>
          <div class="chips">
            <button class="small" aria-pressed={filter.category === null} onclick={() => setFilter({ category: null })}>{t('duplicates.anyType')}</button>
            {#each CATEGORIES as c (c)}
              <button class="small" aria-pressed={filter.category === c} onclick={() => setFilter({ category: c })}>{t(`category.${c}`)}</button>
            {/each}
          </div>
        </div>
        {#if scope === null}
          <label class="field">
            <span class="field-label">{t('duplicates.disk')}</span>
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
        {/if}
        <label class="field">
          <span class="field-label">{t('duplicates.folder')}</span>
          <input
            value={filter.folder ?? ''}
            placeholder={t('duplicates.folderPlaceholder')}
            spellcheck="false"
            onchange={(e) => setFilter({ folder: e.currentTarget.value.trim() || null })}
          />
        </label>
      </section>

      <section class="card box rules" aria-label={t('duplicates.rules')}>
        <h2>{t('duplicates.rules')}</h2>
        <ol>
          {#each rules as r, i (i)}
            <li>
              <span class="mono n">{i + 1}</span>
              <div class="rule-fields">
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
              </div>
              <button class="ghost icon" title={t('duplicates.removeRule')} aria-label={t('duplicates.removeRule')} onclick={() => (rules = rules.filter((_, j) => j !== i))}>
                <Icon name="close" size={14} />
              </button>
            </li>
          {/each}
        </ol>
        <div class="rule-actions">
          <button onclick={() => (rules = [...rules, { kind: 'keepShortestPath', volumeId: null, text: null }])}>
            + {t('duplicates.addRule')}
          </button>
          <button class="primary" onclick={load} disabled={loading || busy}>{t('duplicates.apply')}</button>
        </div>
        <p class="hint">{t('duplicates.rulesHint')}</p>
      </section>
    </aside>

    <div class="list">
      {#if view === null}
        <p class="empty card">{loading ? t('duplicates.loading') : ''}</p>
      {:else if groups.length === 0}
        <p class="empty card">{t('duplicates.nothing')}</p>
      {:else}
        <p class="list-head">
          <span>{t('duplicates.biggestFirst')}</span>
          <span class="mono muted">{t('duplicates.summary', { groups: formatCount(view.totalGroups, lang), size: formatBytes(view.totalRecoverable, lang) })}</span>
        </p>
        {#each groups as g (g.key)}
          {@const s = sel(g)}
          <article class="group card" aria-label={fileName(g)}>
            <header>
              <span class="swatch" style:background="var(--cat-{g.kind})" title={t(`category.${g.kind}`)}></span>
              <h2 title={fileName(g)}>{fileName(g)}</h2>
              <span class="mono muted">{formatBytes(g.size, lang)} × {formatCount(g.files.length, lang)}</span>
              <span class="g-total">{t('duplicates.recoverableShort')} <b class="mono">{formatBytes(g.recoverable, lang)}</b></span>
            </header>
            <ul class="copies">
              {#each g.files as f (f.entryId)}
                {@const checked = s.has(f.entryId)}
                {@const allowed = canSelect(g, f, s)}
                {@const vol = volumes.find((v) => v.id === f.volumeId)}
                <li class:offline={!f.online}>
                  <label title={f.online ? f.fullPath : t('duplicates.offlineHint')}>
                    <input type="checkbox" {checked} disabled={!checked && !allowed} onchange={() => (selection = toggle(selection, g, f))} />
                    <span class="disk" style:--sticker={vol ? stickerColor(vol) : undefined}>
                      <span class="sticker" class:ring={!f.online}></span>
                      <span class="disk-name">{diskLabel(f)}</span>
                    </span>
                    <span class="path mono">{folderOf(f) || '\\'}</span>
                  </label>
                  <span class="mono muted when">{date(f.modified)}</span>
                  <span class="chip {checked ? 'drop' : f.online ? 'keep' : 'drawer'}">
                    {checked ? t('duplicates.chipDrop') : f.online ? t('duplicates.chipKeep') : t('duplicates.offline')}
                  </span>
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
              <p class="group-note">{t('duplicates.onlyOfflineLeft')}</p>
            {/if}
          </article>
        {/each}
      {/if}
    </div>
  </div>

  {#if chosen.count > 0}
    <div class="actionbar" role="region" aria-label={t('duplicates.selected', { count: formatCount(chosen.count, lang), size: formatBytes(chosen.bytes, lang) })}>
      <span class="total">{t('duplicates.selected', { count: formatCount(chosen.count, lang), size: formatBytes(chosen.bytes, lang) })}</span>
      <span class="note">{chosen.permanent ? t('duplicates.barPermanent') : t('duplicates.barRecycle')}</span>
      <button class="push" onclick={() => (selection = new Map())}>{t('duplicates.clearSelection')}</button>
      <button class="signal" disabled={busy} onclick={() => (confirming = true)}>
        {t('duplicates.deleteSelected', { size: formatBytes(chosen.bytes, lang) })}
      </button>
    </div>
  {/if}
</section>

<Dialog
  open={confirming}
  title={t('duplicates.confirmTitle', { count: formatCount(chosen.count, lang), size: formatBytes(chosen.bytes, lang) })}
  onclose={() => (confirming = false)}
>
  <ul class="confirm">
    <li>{t('duplicates.keepOne')}</li>
    <li>{t('duplicates.confirmCheck')}</li>
    {#if chosen.permanent}
      <li class="warn"><strong>{t('duplicates.confirmPermanent')}</strong></li>
    {:else}
      <li>{t('duplicates.confirmRecycle')}</li>
    {/if}
  </ul>
  {#snippet actions()}
    <button onclick={() => (confirming = false)}>{t('actions.cancel')}</button>
    <button class={chosen.permanent ? 'primary danger' : 'signal'} onclick={() => remove(false)}>{t('duplicates.deleteNow')}</button>
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
  }
  .value.signal {
    color: var(--signal-strong);
  }
  .refresh-cell {
    display: flex;
    align-items: center;
    justify-content: flex-end;
  }
  .refresh-cell button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .note-line {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    background: var(--surface);
    border-radius: var(--radius-sm);
    font-size: 12.5px;
  }
  .note-line.done {
    background: var(--ok-soft);
  }
  .note-line span {
    flex: 1;
  }
  .body {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    align-items: flex-start;
  }
  .side {
    flex: 1 1 280px;
    max-width: 330px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .box {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .box h2 {
    font-size: 15px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
  }
  .field-label {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-2);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chips button[aria-pressed='true'] {
    background: var(--accent);
    color: var(--accent-text);
  }
  .rules ol {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .rules li {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 8px;
    background: var(--surface-2);
    border-radius: 9px;
  }
  .rules .n {
    font-size: 11px;
    color: var(--text-3);
    padding-top: 8px;
  }
  .rule-fields {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .rule-fields select {
    background: var(--surface);
  }
  .rule-actions {
    display: flex;
    gap: 8px;
  }
  .rule-actions button {
    flex: 1;
  }
  .hint {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-3);
  }
  .list {
    flex: 999 1 560px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .list-head {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin: 0;
    font-size: 12px;
    color: var(--text-2);
  }
  .group {
    padding: 14px 16px;
  }
  .group header {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding-bottom: 8px;
  }
  .group h2 {
    font-size: 15px;
    min-width: 0;
    max-width: 50%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .group header .mono {
    font-size: 12px;
  }
  .g-total {
    margin-left: auto;
    font-size: 12px;
    color: var(--text-2);
  }
  .g-total b {
    color: var(--signal-strong);
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    flex: none;
  }
  .copies {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .copies li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 86px 96px 32px;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    border-top: 1px solid var(--line);
  }
  .copies label {
    display: grid;
    grid-template-columns: 20px 170px minmax(0, 1fr);
    align-items: center;
    gap: 10px;
    margin: 0;
    font-size: 13px;
    font-weight: 400;
    color: var(--text);
  }
  .copies li.offline label {
    color: var(--text-3);
  }
  .disk {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
  }
  .disk .sticker {
    width: 10px;
    height: 10px;
  }
  .disk-name,
  .path {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .path {
    font-size: 11.5px;
  }
  .when {
    font-size: 11.5px;
    text-align: right;
  }
  .chip {
    justify-self: end;
    font-size: 11px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 6px;
    white-space: nowrap;
  }
  .chip.keep {
    border: 1.5px solid var(--text);
  }
  .chip.drop {
    background: var(--signal-soft);
    color: var(--signal-strong);
  }
  .chip.drawer {
    background: var(--chip);
    color: var(--text-2);
  }
  .group-note {
    margin: 8px 0 0;
    font-size: 12.5px;
    color: var(--warn);
  }
  .confirm {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    line-height: 1.4;
  }
  .confirm .warn {
    color: var(--signal-strong);
  }
</style>
