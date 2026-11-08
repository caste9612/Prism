<script lang="ts">
  // Backups (F13–F15): check that the files of a folder have a copy on one or more disks, also
  // disconnected (on the catalog), compare two folders, copy the missing files with a preview,
  // a verified copy, cancel and resume.
  import type { CompareSummaryView } from '../lib/bindings/CompareSummaryView';
  import type { CopyPreviewView } from '../lib/bindings/CopyPreviewView';
  import type { CopyResultView } from '../lib/bindings/CopyResultView';
  import type { JobView } from '../lib/bindings/JobView';
  import type { SideInput } from '../lib/bindings/SideInput';
  import type { VerifySummaryView } from '../lib/bindings/VerifySummaryView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { onMount, untrack } from 'svelte';
  import { backend, errorMessage } from '../lib/api';
  import { STATUSES, copyJob, coverage } from '../lib/backup';
  import { volumeName } from '../lib/catalog';
  import { formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';
  import Icon from './Icon.svelte';
  import ResultTree from './ResultTree.svelte';
  import SidePicker from './SidePicker.svelte';

  let {
    volumes,
    onerror,
    scope = null,
  }: {
    volumes: VolumeView[];
    onerror: (message: string) => void;
    /** The disk of the screen (ADR-0019): proposed as the folder to protect. */
    scope?: number | null;
  } = $props();

  const lang = $derived(language());
  const catalogued = $derived(volumes.filter((v) => v.lastIndexedAt !== null));
  const firstId = (skip: number[] = []) => catalogued.find((v) => !skip.includes(v.id))?.id ?? 0;
  const nameOf = (side: SideInput) => {
    const v = volumes.find((x) => x.id === side.volumeId);
    const disk = v ? volumeName(v) : `#${side.volumeId}`;
    return side.folder ? `${disk} \\ ${side.folder}` : disk;
  };

  let tab = $state<'verify' | 'compare'>('verify');

  // F13: check.
  let source = $state<SideInput>({ volumeId: untrack(() => scope ?? 0), folder: '' });
  let destinations = $state<SideInput[]>([]);
  let checked = $state<{ summary: VerifySummaryView; source: SideInput; destinations: SideInput[] } | null>(null);
  // After a copy the check runs again by itself, once the destination catalog is refreshed.
  let recheck = $state(false);
  let busy = $state(false);

  // F14: comparison.
  let sideA = $state<SideInput>({ volumeId: 0, folder: '' });
  let sideB = $state<SideInput>({ volumeId: 0, folder: '' });
  let byContent = $state(true);
  let compared = $state<{ summary: CompareSummaryView; a: SideInput } | null>(null);
  let shownStatuses = $state<string[]>(['onlyA', 'onlyB', 'different']);

  // F15: copy.
  let copyOpen = $state(false);
  let copyDest = $state<SideInput>({ volumeId: 0, folder: '' });
  let renameOnConflict = $state(false);
  let preview = $state<CopyPreviewView | null>(null);
  let previewError = $state<string | null>(null);
  let copyJobId = $state<number | null>(null);
  let jobs = $state<JobView[]>([]);
  let copyOutcome = $state<CopyResultView | null>(null);

  onMount(() => {
    // On the tab of a disk, that disk is the one to protect (ADR-0019).
    const own = scope !== null && catalogued.some((v) => v.id === scope) ? scope : null;
    source = { volumeId: own ?? firstId(), folder: '' };
    destinations = [{ volumeId: firstId([source.volumeId]), folder: '' }];
    sideA = { volumeId: own ?? firstId(), folder: '' };
    sideB = { volumeId: firstId([sideA.volumeId]), folder: '' };
    let off: (() => void) | undefined;
    void backend.onJobsChanged((list) => (jobs = list)).then((f) => (off = f));
    return () => off?.();
  });

  async function runCheck(src: SideInput = source, dests: SideInput[] = destinations, keepOutcome = false) {
    busy = true;
    try {
      const copySrc = { ...src };
      const copyDests = dests.map((d) => ({ ...d }));
      const summary = await backend.backupVerify(copySrc, copyDests);
      checked = { summary, source: copySrc, destinations: copyDests };
      if (!keepOutcome) copyOutcome = null;
    } catch (e) {
      onerror(errorMessage(e));
    } finally {
      busy = false;
    }
  }

  async function runCompare() {
    busy = true;
    try {
      const summary = await backend.backupCompare({ ...sideA }, { ...sideB }, byContent);
      compared = { summary, a: { ...sideA } };
    } catch (e) {
      onerror(errorMessage(e));
    } finally {
      busy = false;
    }
  }

  async function refreshPreview() {
    if (!checked) return;
    preview = null;
    previewError = null;
    try {
      preview = await backend.copyPreview(checked.summary.reportId, { ...copyDest });
    } catch (e) {
      previewError = errorMessage(e);
    }
  }

  function openCopy() {
    // A connected backup disk if there is one, otherwise another connected disk (F15-AC1).
    const isOnline = (id: number) => volumes.find((v) => v.id === id)?.online === true;
    const online = destinations.find((d) => isOnline(d.volumeId));
    const other = catalogued.find((v) => v.online && v.id !== checked?.source.volumeId);
    copyDest = { ...(online ?? { volumeId: other?.id ?? firstId(), folder: '' }) };
    copyOpen = true;
    void refreshPreview();
  }

  async function startCopy() {
    if (!checked) return;
    copyOpen = false;
    copyOutcome = null;
    try {
      copyJobId = await backend.copyStart(checked.summary.reportId, { ...copyDest }, renameOnConflict);
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  const running = $derived(copyJob(jobs, copyJobId));
  $effect(() => {
    const j = running;
    if (j && copyJobId !== null && (j.state === 'done' || j.state === 'cancelled' || j.state === 'failed')) {
      const id = copyJobId;
      void backend.copyResult(id).then((r) => {
        if (copyJobId === id) {
          copyOutcome = r;
          copyJobId = null;
          if (r && (r.copied > 0 || r.renamed > 0)) recheck = true;
        }
      });
    }
  });

  $effect(() => {
    if (!recheck || !checked) return;
    const refreshing = jobs.some((j) => j.kind === 'refresh' && (j.state === 'running' || j.state === 'queued'));
    if (refreshing) return;
    recheck = false;
    void runCheck(checked.source, checked.destinations, true);
  });

  const statusLabels = $derived(STATUSES.map((s) => ({ key: s, label: t(`backup.status.${s}`) })));
  function toggleStatus(s: string) {
    shownStatuses = shownStatuses.includes(s) ? shownStatuses.filter((x) => x !== s) : [...shownStatuses, s];
  }
</script>

<section class="backup">
  <div class="pills tabs" role="tablist" aria-label={t('backup.title')}>
    <button role="tab" aria-selected={tab === 'verify'} onclick={() => (tab = 'verify')}>{t('backup.tabVerify')}</button>
    <button role="tab" aria-selected={tab === 'compare'} onclick={() => (tab = 'compare')}>{t('backup.tabCompare')}</button>
  </div>

  {#if catalogued.length < 2}
    <p class="muted empty">{t('backup.needTwo')}</p>
  {:else if tab === 'verify'}
    <div class="panel">
      <SidePicker label={t('backup.source')} volumes={catalogued} value={source} onchange={(v) => (source = v)} />
      {#each destinations as d, i (i)}
        <div class="dest">
          <SidePicker
            label={t('backup.destination', { n: i + 1 })}
            volumes={catalogued}
            value={d}
            onchange={(v) => (destinations = destinations.map((x, j) => (j === i ? v : x)))}
          />
          {#if destinations.length > 1}
            <button class="ghost icon" aria-label={t('backup.removeDestination')} title={t('backup.removeDestination')} onclick={() => (destinations = destinations.filter((_, j) => j !== i))}>
              <Icon name="close" size={14} />
            </button>
          {/if}
        </div>
      {/each}
      <div class="actions">
        <button class="ghost small" onclick={() => (destinations = [...destinations, { volumeId: firstId(), folder: '' }])}>
          <Icon name="plus" size={14} /> {t('backup.addDestination')}
        </button>
        <span class="spacer"></span>
        <button class="primary" onclick={() => runCheck()} disabled={busy}>{busy ? t('backup.running') : t('backup.runVerify')}</button>
      </div>
    </div>

    {#if checked}
      {@const c = coverage(checked.summary)}
      {@const s = checked.summary}
      {@const covered = s.total.files > 0 ? Math.floor(((s.byContent.files + s.byMetadata.files) / s.total.files) * 1000) / 10 : 100}
      <div class="result-row">
        <div class="summary" role="status" aria-label={t('backup.resultOf', { name: nameOf(checked.source) })}>
          <div class="headline">
            <span class="big">{covered.toLocaleString(lang)}%</span>
            <span class="of">{t('backup.coveredOf', { name: nameOf(checked.source) })}</span>
          </div>
          <div class="cover-bar" aria-hidden="true">
            <span class="content" style:width="{s.total.files > 0 ? (s.byContent.files / s.total.files) * 100 : 0}%"></span>
            <span class="meta" style:width="{s.total.files > 0 ? (s.byMetadata.files / s.total.files) * 100 : 0}%"></span>
            <span class="missing" style:width="{s.total.files > 0 ? (s.missing.files / s.total.files) * 100 : 0}%"></span>
          </div>
          <p class="mono muted">{t('backup.total', { files: formatCount(s.total.files, lang), size: formatBytes(s.total.bytes, lang) })}</p>
          <ul class="facts">
            <li><span class="key content"></span>{t('backup.byContent', { files: formatCount(s.byContent.files, lang), percent: c.content })}</li>
            <li><span class="key meta"></span>{t('backup.byMetadata', { files: formatCount(s.byMetadata.files, lang), percent: c.metadata })}</li>
            <li class:warn={!c.complete} class:good={c.complete}>
              <span class="key missing"></span>{c.complete ? t('backup.allCopied') : t('backup.missing', { files: formatCount(s.missing.files, lang), size: formatBytes(s.missing.bytes, lang) })}
            </li>
          </ul>
          {#if s.byMetadata.files > 0}
            <p class="muted hint">{t('backup.metadataHint')}</p>
          {/if}
        </div>
        {#if !c.complete}
          <aside class="copy-box">
            <h2>{t('backup.copyMissing')}</h2>
            <div class="copy-big">{formatCount(s.missing.files, lang)} <small>{t('backup.filesWord')} · {formatBytes(s.missing.bytes, lang)}</small></div>
            <p>{t('backup.copyBoxHint')}</p>
            <button class="signal" onclick={openCopy} disabled={running !== null}>{t('backup.copyMissing')}…</button>
          </aside>
        {/if}
      </div>

      {#if running}
        <div class="progress" role="status">
          <span>{t('backup.copying', { done: formatBytes(running.done, lang), total: formatBytes(running.total, lang) })}</span>
          <div class="bar"><span style:width={running.total > 0 ? `${Math.min(100, (running.done / running.total) * 100)}%` : '0%'}></span></div>
          <button class="small" onclick={() => backend.cancelJob(running.id)}>{t('backup.cancelCopy')}</button>
        </div>
      {/if}

      {#if copyOutcome}
        <div class="outcome" role="status" aria-label={t('backup.copyOutcome')}>
          <p>
            {t('backup.copyDone', { count: formatCount(copyOutcome.copied + copyOutcome.renamed, lang), size: formatBytes(copyOutcome.bytes, lang) })}
            {#if copyOutcome.renamed > 0}{' '}{t('backup.copyRenamed', { count: formatCount(copyOutcome.renamed, lang) })}{/if}
            {#if copyOutcome.alreadyThere > 0}{' '}{t('backup.copyAlready', { count: formatCount(copyOutcome.alreadyThere, lang) })}{/if}
            {#if copyOutcome.skipped > 0}{' '}{t('backup.copySkipped', { count: formatCount(copyOutcome.skipped, lang) })}{/if}
          </p>
          {#if copyOutcome.interrupted}
            <p class="warn">{t('backup.copyInterrupted')}</p>
          {/if}
          {#if copyOutcome.failed.length > 0}
            <p class="warn">{t('backup.copyFailed', { count: formatCount(copyOutcome.failed.length, lang) })}</p>
            <ul class="failures">
              {#each copyOutcome.failed as f (f.path)}
                <li><span class="path">{f.path}</span> <span class="muted">{f.error}</span></li>
              {/each}
            </ul>
          {/if}
          {#if copyOutcome.interrupted || copyOutcome.failed.length > 0}
            <button class="small" onclick={startCopy}>{t('backup.resume')}</button>
          {/if}
        </div>
      {/if}

      {#if checked.summary.missing.files > 0}
        {#key checked.summary.reportId}
          <ResultTree
            reportId={checked.summary.reportId}
            classes={[{ key: 'missing', label: t('backup.missingLabel') }]}
            rootName={nameOf(checked.source)}
            {onerror}
          />
        {/key}
      {/if}
    {/if}
  {:else}
    <div class="panel">
      <div class="pair">
        <SidePicker label={t('backup.sideA')} volumes={catalogued} value={sideA} onchange={(v) => (sideA = v)} />
        <SidePicker label={t('backup.sideB')} volumes={catalogued} value={sideB} onchange={(v) => (sideB = v)} />
      </div>
      <div class="actions">
        <span class="mode-label">{t('backup.mode')}</span>
        <label class="radio"><input type="radio" name="mode" checked={!byContent} onchange={() => (byContent = false)} /> {t('backup.modePath')}</label>
        <label class="radio"><input type="radio" name="mode" checked={byContent} onchange={() => (byContent = true)} /> {t('backup.modeContent')}</label>
        <span class="spacer"></span>
        <button class="primary" onclick={runCompare} disabled={busy}>{busy ? t('backup.running') : t('backup.runCompare')}</button>
      </div>
    </div>

    {#if compared}
      <div class="statuses" role="group" aria-label={t('backup.show')}>
        {#each STATUSES as s, i (s)}
          {@const tally = compared.summary.tallies[i]}
          <label class="status-filter {s}">
            <input type="checkbox" checked={shownStatuses.includes(s)} onchange={() => toggleStatus(s)} />
            <span>{t(`backup.status.${s}`)}</span>
            <strong>{formatCount(tally?.files ?? 0, lang)}</strong>
            <span class="muted">{formatBytes(tally?.bytes ?? 0, lang)}</span>
          </label>
        {/each}
      </div>
      {#key compared.summary.reportId}
        <ResultTree
          reportId={compared.summary.reportId}
          classes={statusLabels}
          rootName={`${nameOf(compared.a)}`}
          statuses={shownStatuses}
          {onerror}
        />
      {/key}
    {/if}
  {/if}
</section>

<Dialog open={copyOpen} title={t('backup.copyTitle')} onclose={() => (copyOpen = false)}>
  <SidePicker
    label={t('backup.copyDest')}
    volumes={catalogued}
    onlineOnly
    value={copyDest}
    onchange={(v) => {
      copyDest = v;
      void refreshPreview();
    }}
  />
  <p class="muted hint">{t('backup.copyHint')}</p>
  <fieldset class="conflict">
    <legend>{t('backup.conflict')}</legend>
    <label class="radio"><input type="radio" name="conflict" checked={!renameOnConflict} onchange={() => (renameOnConflict = false)} /> {t('backup.conflictSkip')}</label>
    <label class="radio"><input type="radio" name="conflict" checked={renameOnConflict} onchange={() => (renameOnConflict = true)} /> {t('backup.conflictRename')}</label>
  </fieldset>
  {#if preview}
    <p class="preview">{t('backup.preview', { files: formatCount(preview.files, lang), size: formatBytes(preview.bytes, lang), path: preview.destPath })}</p>
    {#if preview.freeSpace !== null}
      <p class:warn={!preview.enoughSpace} class="muted">{t('backup.free', { size: formatBytes(preview.freeSpace, lang) })}</p>
    {/if}
    {#if !preview.enoughSpace}
      <p class="warn" role="alert">{t('backup.notEnough')}</p>
    {/if}
  {:else if previewError}
    <p class="warn" role="alert">{previewError}</p>
  {/if}
  {#snippet actions()}
    <button onclick={() => (copyOpen = false)}>{t('actions.cancel')}</button>
    <button class="primary" disabled={!preview || !preview.enoughSpace} onclick={startCopy}>{t('backup.startCopy')}</button>
  {/snippet}
</Dialog>

<style>
  .backup {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .tabs {
    align-self: flex-start;
  }
  .panel,
  .summary,
  .outcome,
  .progress {
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: 16px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .dest {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .dest > :global(.side) {
    flex: 1;
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .actions button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .spacer {
    flex: 1;
  }
  .mode-label {
    font-size: 12.5px;
    color: var(--text-2);
  }
  .radio {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    color: var(--text);
    font-size: 13px;
    font-weight: 400;
  }
  .result-row {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    align-items: stretch;
  }
  .summary {
    flex: 999 1 520px;
    min-width: 0;
  }
  .summary p {
    margin: 0;
  }
  .headline {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 12px;
  }
  .big {
    font-family: var(--font-display);
    font-size: 46px;
    font-weight: 700;
    line-height: 1;
  }
  .of {
    font-family: var(--font-display);
    font-size: 18px;
    font-weight: 600;
  }
  .cover-bar {
    display: flex;
    height: 12px;
    border-radius: 6px;
    overflow: hidden;
    background: var(--track);
  }
  .cover-bar .content,
  .key.content {
    background: var(--ok);
  }
  .cover-bar .meta,
  .key.meta {
    background: var(--cat-images);
  }
  .cover-bar .missing,
  .key.missing {
    background: var(--signal);
  }
  .facts {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 4px;
  }
  .facts li {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .key {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    flex: none;
  }
  .copy-box {
    flex: 1 1 280px;
    max-width: 380px;
    background: var(--block);
    color: var(--block-text);
    border-radius: var(--radius);
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .copy-box h2 {
    font-size: 17px;
  }
  .copy-big {
    font-family: var(--font-display);
    font-size: 26px;
    font-weight: 600;
  }
  .copy-big small {
    font-size: 14px;
    color: var(--block-text-2);
  }
  .copy-box p {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--block-text-2);
  }
  .copy-box button {
    margin-top: auto;
  }
  .hint {
    font-size: 12.5px;
    margin: 0;
  }
  .warn {
    color: var(--signal-strong);
  }
  .good {
    color: var(--ok);
  }
  .progress {
    flex-direction: row;
    align-items: center;
  }
  .progress .bar {
    flex: 1;
    height: 6px;
    border-radius: 3px;
    background: var(--track);
    overflow: hidden;
  }
  .progress .bar span {
    display: block;
    height: 100%;
    background: var(--signal);
  }
  .outcome p {
    margin: 0;
  }
  .failures {
    margin: 0;
    padding-left: 18px;
    font-size: 12.5px;
  }
  .failures .path {
    font-family: var(--font-mono);
  }
  .statuses {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .status-filter {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    padding: 6px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font-size: 13px;
    font-weight: 400;
  }
  .status-filter strong {
    font-family: var(--font-mono);
  }
  .conflict {
    border: 0;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .conflict legend {
    font-size: 12.5px;
    margin-bottom: 4px;
  }
  .preview {
    margin: 4px 0 0;
  }
  .empty {
    padding: 40px 0;
    text-align: center;
  }
</style>
