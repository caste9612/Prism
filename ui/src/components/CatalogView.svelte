<script lang="ts">
  // Home screen = disk catalog (F01), with the prompts for new disks (F02) and for the disks
  // Prism cannot tell apart (F02-AC3).
  import type { AmbiguousView } from '../lib/bindings/AmbiguousView';
  import type { DiskHealthView } from '../lib/bindings/DiskHealthView';
  import type { LabelsInput } from '../lib/bindings/LabelsInput';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { groupPrompts, newVolumePrompts, visibleVolumes, volumeName } from '../lib/catalog';
  import { ago, formatBytes } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';
  import Icon from './Icon.svelte';
  import LabelsDialog from './LabelsDialog.svelte';
  import HealthDialog from './HealthDialog.svelte';
  import VolumeCard from './VolumeCard.svelte';

  let {
    volumes,
    onerror,
    onspace,
  }: { volumes: VolumeView[]; onerror: (message: string) => void; onspace: (v: VolumeView) => void } = $props();

  let dismissed = $state(new Set<number>());
  let editing = $state<VolumeView | null>(null);
  let removing = $state<VolumeView | null>(null);
  let health = $state<DiskHealthView | null>(null);
  async function showHealth(v: VolumeView) {
    try {
      health = await backend.diskHealth(v.id);
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  const groups = $derived(visibleVolumes(volumes));
  const prompts = $derived(newVolumePrompts(volumes, dismissed));
  const grouped = $derived(groupPrompts(prompts));
  const lang = $derived(language());

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
    await run(() => backend.resolveAmbiguous(a.key, volumeId));
    ambiguous = ambiguous.filter((x) => x.key !== a.key);
  }
  const seen = (ms: number | null) => {
    if (ms === null) return '';
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };

  async function run(action: () => Promise<unknown>) {
    try {
      await action();
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  const index = (v: VolumeView) => run(() => backend.indexVolume(v.id));
  const cancel = (jobId: number) => run(() => backend.cancelJob(jobId));
  const ignore = (v: VolumeView) => run(() => backend.setIgnored(v.id, true));
  const open = (v: VolumeView) => run(() => backend.openVolume(v.id));
  const notNow = (v: VolumeView) => (dismissed = new Set([...dismissed, v.id]));
  // Jobs on different physical disks run in parallel; the engine queues the rest (ADR-0009).
  const indexAll = () => prompts.forEach((v) => void index(v));
  const notNowAll = () => (dismissed = new Set([...dismissed, ...prompts.map((v) => v.id)]));
  async function saveLabels(id: number, labels: LabelsInput) {
    await run(() => backend.setLabels(id, labels));
    editing = null;
  }
  async function confirmRemove() {
    if (removing) {
      const v = removing;
      removing = null;
      await run(() => backend.removeVolume(v.id));
    }
  }
</script>

<section class="catalog">
  <header class="page-head">
    <div>
      <h1>{t('catalog.title')}</h1>
      <p class="muted">
        {t('catalog.subtitle', { online: groups.online.length, total: groups.online.length + groups.offline.length })}
      </p>
    </div>
  </header>

  {#each ambiguous as a (a.key)}
    <div class="prompt identify" role="region" aria-label={t('identify.title', { name: a.label ?? a.mount ?? '' })}>
      <span class="prompt-icon"><Icon name="disks" size={22} /></span>
      <div class="prompt-text">
        <strong>{t('identify.title', { name: a.label ?? a.mount ?? '' })}</strong>
        <span class="muted">{[a.mount, a.fsType, a.capacity !== null ? formatBytes(a.capacity, lang) : null].filter(Boolean).join(' · ')}</span>
        <span>{t('identify.question')}</span>
        <ul class="candidates">
          {#each a.candidates as c (c.volumeId)}
            <li>
              <button onclick={() => identify(a, c.volumeId)}>{t('identify.isThis', { name: c.name })}</button>
              <span class="muted">
                {[c.location ? t('catalog.location', { location: c.location }) : null, c.capacity !== null ? formatBytes(c.capacity, lang) : null, c.lastSeenAt !== null ? t('catalog.lastSeen', { when: seen(c.lastSeenAt) }) : null]
                  .filter(Boolean)
                  .join(' · ')}
              </span>
            </li>
          {/each}
        </ul>
      </div>
      <div class="prompt-actions">
        <button class="ghost" onclick={() => identify(a, null)}>{t('identify.another')}</button>
      </div>
    </div>
  {/each}

  {#if grouped}
    <div class="prompt grouped" role="region" aria-label={t('newDisk.manyTitle', { count: prompts.length })}>
      <div class="group-head">
        <span class="prompt-icon"><Icon name="disks" size={22} /></span>
        <div class="prompt-text">
          <strong>{t('newDisk.manyTitle', { count: prompts.length })}</strong>
          <span>{t('newDisk.manyQuestion')}</span>
        </div>
        <div class="prompt-actions">
          <button class="primary" onclick={indexAll}>{t('newDisk.indexAll')}</button>
          <button class="ghost" onclick={notNowAll}>{t('newDisk.notNowAll')}</button>
        </div>
      </div>
      <ul class="group-list">
        {#each prompts as v (v.id)}
          <li>
            <span class="row-icon"><Icon name={v.kind} size={18} /></span>
            <span class="row-name">{volumeName(v)}</span>
            <span class="muted row-detail">
              {t('newDisk.detail', { kind: t(`kind.${v.kind}`), mount: v.mount ?? '—', capacity: v.capacity !== null ? formatBytes(v.capacity, lang) : '—' })}
            </span>
            <span class="row-actions">
              <button class="small" onclick={() => index(v)}>{t('newDisk.index')}</button>
              <button class="small ghost" onclick={() => (editing = v)}>{t('actions.labels')}</button>
              <button class="small ghost" onclick={() => notNow(v)}>{t('newDisk.notNow')}</button>
              <button class="small ghost" onclick={() => ignore(v)}>{t('newDisk.ignore')}</button>
            </span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#each grouped ? [] : prompts as v (v.id)}
    <div class="prompt" role="region" aria-label={t('newDisk.title', { name: volumeName(v) })}>
      <span class="prompt-icon"><Icon name={v.kind} size={22} /></span>
      <div class="prompt-text">
        <strong>{t('newDisk.title', { name: volumeName(v) })}</strong>
        <span class="muted">
          {t('newDisk.detail', { kind: t(`kind.${v.kind}`), mount: v.mount ?? '—', capacity: v.capacity !== null ? formatBytes(v.capacity, lang) : '—' })}
        </span>
        <span>{t('newDisk.question')}</span>
      </div>
      <div class="prompt-actions">
        <button class="primary" onclick={() => index(v)}>{t('newDisk.index')}</button>
        <button onclick={() => (editing = v)}>{t('actions.labels')}</button>
        <button class="ghost" onclick={() => notNow(v)}>{t('newDisk.notNow')}</button>
        <button class="ghost" onclick={() => ignore(v)}>{t('newDisk.ignore')}</button>
      </div>
    </div>
  {/each}

  {#if groups.online.length + groups.offline.length === 0}
    <p class="empty muted">{t('catalog.empty')}</p>
  {/if}

  {#if groups.online.length > 0}
    <h2 class="section">{t('catalog.onlineSection')}</h2>
    <div class="grid">
      {#each groups.online as v (v.id)}
        <VolumeCard volume={v} onindex={index} oncancel={cancel} onlabels={(x) => (editing = x)} onopen={open} {onspace} onremove={(x) => (removing = x)} onhealth={showHealth} />
      {/each}
    </div>
  {/if}

  {#if groups.offline.length > 0}
    <h2 class="section">{t('catalog.offlineSection')}</h2>
    <div class="grid">
      {#each groups.offline as v (v.id)}
        <VolumeCard volume={v} onindex={index} oncancel={cancel} onlabels={(x) => (editing = x)} onopen={open} {onspace} onremove={(x) => (removing = x)} onhealth={showHealth} />
      {/each}
    </div>
  {/if}
</section>

<HealthDialog {health} onclose={() => (health = null)} />

<LabelsDialog volume={editing} onsave={saveLabels} onclose={() => (editing = null)} />

<Dialog open={removing !== null} title={t('remove.title', { name: removing ? volumeName(removing) : '' })} onclose={() => (removing = null)}>
  <p>{t('remove.body')}</p>
  {#snippet actions()}
    <button onclick={() => (removing = null)}>{t('actions.cancel')}</button>
    <button class="primary" onclick={confirmRemove}>{t('actions.remove')}</button>
  {/snippet}
</Dialog>

<style>
  .candidates {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .candidates li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }
  .identify {
    align-items: flex-start;
  }
  .catalog {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .page-head h1 {
    font-size: 24px;
  }
  .page-head p {
    margin: 2px 0 0;
  }
  .section {
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-3);
    margin-top: 8px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: var(--gap);
  }
  .prompt {
    display: flex;
    align-items: center;
    gap: 14px;
    background: var(--accent-soft);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    padding: 14px 16px;
  }
  .prompt-icon {
    color: var(--accent);
  }
  .prompt-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .prompt-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .prompt.grouped {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }
  .group-head {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .group-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--border);
  }
  .group-list li {
    display: grid;
    grid-template-columns: 22px minmax(90px, 0.6fr) 1fr auto;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
  .group-list li:last-child {
    border-bottom: 0;
  }
  .row-icon {
    color: var(--text-2);
    display: flex;
  }
  .row-name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-detail {
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-actions {
    display: flex;
    gap: 4px;
  }
  button.small {
    padding: 3px 9px;
    font-size: 12px;
  }
  .empty {
    padding: 40px 0;
    text-align: center;
  }
</style>
