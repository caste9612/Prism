<script lang="ts">
  // Health tab of a NAS share (REVIEW D66): the array the shared folder is on, in large type with
  // what is wrong, and the disks of the array with their temperature and hours; without the login
  // of the NAS, how to give it.
  import type { NasHealthView } from '../lib/bindings/NasHealthView';
  import type { NasDiskView } from '../lib/bindings/NasDiskView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { raidTone } from '../lib/catalog';
  import { ago, formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import { nav } from '../lib/nav.svelte';

  let { volume, onerror }: { volume: VolumeView; onerror: (message: string) => void } = $props();

  const lang = $derived(language());
  let nas = $state<NasHealthView | null>(null);
  let loaded = $state(false);
  // Read again when the disk changes or a new reading of the NAS reaches its card.
  const volumeId = $derived(volume.id);
  const raidKey = $derived(JSON.stringify(volume.raid));
  $effect(() => {
    const id = volumeId;
    void raidKey;
    void backend.nasHealth(id).then(
      (h) => {
        nas = h;
        loaded = true;
      },
      (e) => {
        loaded = true;
        onerror(errorMessage(e));
      },
    );
  });

  const when = (ms: number) => {
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };
  const storage = $derived(nas?.storage ?? null);
  const tone = $derived(storage ? raidTone(storage.state) : 'good');
  const diskState = (d: NasDiskView) => (d.missing ? 'missing' : d.faulty ? 'faulty' : d.healthy ? 'ok' : 'unhealthy');
  const kind = (d: NasDiskView) =>
    [d.kind === 'ssd' ? t('media.ssd') : d.kind === 'hdd' ? t('media.hdd') : null, d.interface?.toUpperCase()].filter(Boolean).join(' · ') || '—';
  const noSmart = $derived(nas?.disks.some((d) => !d.missing && d.temperature === null) ?? false);
  const activity = (a: string) => t(`raid.activity.${['resync', 'recovery', 'reshape', 'check'].includes(a) ? a : 'other'}`);
</script>

<div class="health-tab">
  {#if !loaded}
    <p class="empty card">{t('healthTab.loading')}</p>
  {:else if !nas || !nas.connected}
    <div class="card status">
      <div class="state unavailable">{t('health.word.unavailable')}</div>
      <div class="what">
        <p>{t('nas.healthIntro')}</p>
        <p><button class="small" onclick={() => nav.go({ name: 'settings' })}>{t('nas.openSettings')}</button></p>
      </div>
    </div>
  {:else if !storage}
    <div class="card status">
      <div class="state unavailable">{t('health.word.unavailable')}</div>
      <div class="what">
        <p>{nas.error ? t(`nas.error.${nas.error}`) : nas.readAt === null ? t('nas.notRead') : t('nas.noStorage')}</p>
      </div>
    </div>
  {:else}
    <section class="card status" aria-label={storage.name}>
      <div class="badge {tone}" aria-hidden="true">
        {#if tone === 'good'}
          <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg>
        {:else}
          <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 7v6M12 17h.01" /></svg>
        {/if}
      </div>
      <div class="what">
        <div class="state {tone}">{t(`raid.word.${storage.state}`)}</div>
        <div class="muted">
          {[storage.name, storage.level !== null ? t('raid.level', { level: storage.level }) : t('raid.single'), storage.fs, nas.server].filter(Boolean).join(' · ')}
        </div>
        {#if storage.status && storage.status.toLowerCase() !== 'ok'}
          <p class="reason">{t('nas.status', { status: storage.status })}</p>
        {/if}
        {#if storage.activity}
          <p>{activity(storage.activity)}{storage.progress !== null ? ` ${storage.progress}%` : ''}</p>
        {/if}
        {#each nas.disks.filter((d) => diskState(d) !== 'ok') as d (d.serial)}
          <p class="reason">{d.model || d.serial}: {t(`nas.diskState.${diskState(d)}`)}</p>
        {/each}
        {#if nas.error}
          <p class="reason">{t(`nas.error.${nas.error}`)} {t('raid.stale')}</p>
        {/if}
      </div>
      <div class="note mono">
        {t('nas.readOnly')}{#if nas.readAt !== null}<br />{t('nas.readAt', { when: when(nas.readAt) })}{/if}
      </div>
    </section>

    <dl class="stats values">
      <div>
        <dt class="caption">{t('nas.size')}</dt>
        <dd class="value">{formatBytes(storage.size, lang)}</dd>
      </div>
      {#if storage.used !== null}
        <div>
          <dt class="caption">{t('nas.used')}</dt>
          <dd class="value">{formatBytes(storage.used, lang)}</dd>
        </div>
      {/if}
      <div>
        <dt class="caption">{t('nas.disks')}</dt>
        <dd class="value">{nas.disks.length}</dd>
      </div>
    </dl>

    {#if nas.disks.length > 0}
      <section class="card disks">
        <h2>{t('nas.disksTitle', { name: storage.name })}</h2>
        <table class="data">
          <thead>
            <tr>
              <th>{t('nas.model')}</th>
              <th>{t('nas.kind')}</th>
              <th class="num">{t('nas.size')}</th>
              <th class="num">{t('nas.temperature')}</th>
              <th class="num">{t('nas.hours')}</th>
              <th>{t('nas.state')}</th>
            </tr>
          </thead>
          <tbody>
            {#each nas.disks as d (d.serial)}
              {@const s = diskState(d)}
              <tr>
                <td>
                  <div>{d.model || '—'}</div>
                  <div class="muted mono serial">{d.serial}{#if d.system}{' · '}{t('nas.system')}{/if}</div>
                </td>
                <td>{kind(d)}</td>
                <td class="num">{d.size > 0 ? formatBytes(d.size, lang) : '—'}</td>
                <td class="num" title={d.temperature === null ? t('nas.noSmart') : undefined}>{d.temperature !== null ? `${d.temperature} °C` : '—'}</td>
                <td class="num" title={d.powerOnHours === null ? t('nas.noSmart') : undefined}>{d.powerOnHours !== null ? formatCount(d.powerOnHours, lang) : '—'}</td>
                <td class="state-cell {s === 'ok' ? 'good' : s === 'unhealthy' ? 'warning' : 'critical'}">{t(`nas.diskState.${s}`)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if noSmart}<p class="muted">{t('nas.noSmartNote')}</p>{/if}
        <p class="muted">{t('nas.notify')}</p>
      </section>
    {/if}
  {/if}
</div>

<style>
  .health-tab {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 20px;
    flex-wrap: wrap;
    padding: 20px 22px;
  }
  .badge {
    width: 64px;
    height: 64px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    flex: none;
    background: var(--ok-soft);
    color: var(--ok);
  }
  .badge.warning {
    background: var(--warn-soft);
    color: var(--warn);
  }
  .badge.critical {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .what {
    flex: 1 1 280px;
  }
  .what p {
    margin: 6px 0 0;
  }
  .state {
    font-family: var(--font-display);
    font-size: 30px;
    font-weight: 700;
    line-height: 1.1;
  }
  .state.good,
  .state-cell.good {
    color: var(--ok);
  }
  .state.warning,
  .state-cell.warning {
    color: var(--warn);
  }
  .state.critical,
  .state-cell.critical {
    color: var(--danger);
  }
  .state.unavailable {
    color: var(--text-2);
  }
  .note {
    font-size: 11.5px;
    color: var(--text-3);
    max-width: 240px;
    text-align: right;
  }
  .values {
    margin: 0;
  }
  .values dd {
    margin: 0;
  }
  .disks {
    padding: 16px 18px;
  }
  .disks h2 {
    font-size: 16px;
    margin-bottom: 10px;
  }
  .disks p {
    margin: 10px 0 0;
    font-size: 12px;
  }
  .serial {
    font-size: 11.5px;
  }
</style>
