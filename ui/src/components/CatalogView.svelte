<script lang="ts">
  // Home screen = disk catalog (F01), with the prompts for new disks (F02).
  import type { LabelsInput } from '../lib/bindings/LabelsInput';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { groupPrompts, newVolumePrompts, visibleVolumes, volumeName } from '../lib/catalog';
  import { formatBytes } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';
  import Icon from './Icon.svelte';
  import LabelsDialog from './LabelsDialog.svelte';
  import VolumeCard from './VolumeCard.svelte';

  let { volumes, onerror }: { volumes: VolumeView[]; onerror: (message: string) => void } = $props();

  let dismissed = $state(new Set<number>());
  let editing = $state<VolumeView | null>(null);
  let removing = $state<VolumeView | null>(null);

  const groups = $derived(visibleVolumes(volumes));
  const prompts = $derived(newVolumePrompts(volumes, dismissed));
  const grouped = $derived(groupPrompts(prompts));
  const lang = $derived(language());

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
        <VolumeCard volume={v} onindex={index} oncancel={cancel} onlabels={(x) => (editing = x)} onopen={open} onremove={(x) => (removing = x)} />
      {/each}
    </div>
  {/if}

  {#if groups.offline.length > 0}
    <h2 class="section">{t('catalog.offlineSection')}</h2>
    <div class="grid">
      {#each groups.offline as v (v.id)}
        <VolumeCard volume={v} onindex={index} oncancel={cancel} onlabels={(x) => (editing = x)} onopen={open} onremove={(x) => (removing = x)} />
      {/each}
    </div>
  {/if}
</section>

<LabelsDialog volume={editing} onsave={saveLabels} onclose={() => (editing = null)} />

<Dialog open={removing !== null} title={t('remove.title', { name: removing ? volumeName(removing) : '' })} onclose={() => (removing = null)}>
  <p>{t('remove.body')}</p>
  {#snippet actions()}
    <button onclick={() => (removing = null)}>{t('actions.cancel')}</button>
    <button class="primary" onclick={confirmRemove}>{t('actions.remove')}</button>
  {/snippet}
</Dialog>

<style>
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
