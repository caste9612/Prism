<script lang="ts">
  // Health tab of a disk (F17, ADR-0019): the status in large type with the reasons, the values
  // the user cares about, the temperature of the last readings and the history of the changes;
  // when health data is not available, why (F17-AC4).
  import type { DiskHealthView } from '../lib/bindings/DiskHealthView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { mediaLabel } from '../lib/catalog';
  import { formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';

  let { volume, onerror }: { volume: VolumeView; onerror: (message: string) => void } = $props();

  const lang = $derived(language());
  let health = $state<DiskHealthView | null>(null);
  let loaded = $state(false);
  // Read again only when the disk or its connection changes, not at every refresh of the catalog.
  const volumeId = $derived(volume.id);
  const volumeOnline = $derived(volume.online);
  $effect(() => {
    const id = volumeId;
    const online = volumeOnline;
    loaded = false;
    health = null;
    if (!online) {
      loaded = true;
      return;
    }
    void backend.diskHealth(id).then(
      (h) => {
        health = h;
        loaded = true;
      },
      (e) => {
        loaded = true;
        onerror(errorMessage(e));
      },
    );
  });

  const date = (ms: number) => new Date(ms).toLocaleDateString(lang, { day: 'numeric', month: 'short', year: 'numeric' });
  const n = (v: number | null) => (v === null ? null : formatCount(v, lang));
  const values = $derived.by(() => {
    if (!health) return [];
    const h = health;
    return [
      [t('health.temperature'), h.badge.temperature === null ? null : `${h.badge.temperature} °C`],
      [t('health.wear'), h.wear === null ? null : `${h.wear}%`],
      [t('health.hours'), n(h.powerOnHours)],
      [t('health.spare'), h.availableSpare === null ? null : `${h.availableSpare}%`],
      [t('health.reallocated'), n(h.reallocated)],
      [t('health.pending'), n(h.pending)],
      [t('health.mediaErrors'), n(h.mediaErrors)],
    ].filter((r): r is [string, string] => r[1] !== null);
  });
  // Temperatures of the readings, oldest first (the history keeps one per change or per day).
  const temps = $derived(
    health ? health.history.filter((p) => p.temperature !== null).slice(0, 30).reverse() : [],
  );
  const maxTemp = $derived(Math.max(50, ...temps.map((p) => p.temperature ?? 0)));
</script>

<div class="health-tab">
  {#if !loaded}
    <p class="empty card">{t('healthTab.loading')}</p>
  {:else if !volume.online}
    <p class="empty card">{t('healthTab.offline')}</p>
  {:else if !health}
    <div class="card status">
      <div class="state unavailable">{t('health.word.unavailable')}</div>
      <p>{volume.health?.unavailable ? t(`health.unavailable.${volume.health.unavailable}`) : t('health.unavailable.not_supported')}</p>
    </div>
  {:else}
    <section class="card status" aria-label={t('health.title', { model: health.model })}>
      <div class="badge {health.badge.status}" aria-hidden="true">
        {#if health.badge.status === 'good'}
          <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg>
        {:else}
          <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 7v6M12 17h.01" /></svg>
        {/if}
      </div>
      <div class="what">
        <div class="state {health.badge.status}">{t(`health.word.${health.badge.status}`)}</div>
        <div class="muted">{[health.model, mediaLabel({ media: health.media, rpm: health.rpm, kind: volume.kind }), health.bus.toUpperCase(), health.serial].filter(Boolean).join(' · ')}</div>
        {#if health.badge.unavailable}<p>{t(`health.unavailable.${health.badge.unavailable}`)}</p>{/if}
        {#each health.reasons as r (r)}
          <p class="reason">{t(`health.reason.${r}`)}</p>
        {/each}
        {#if health.reasons.length === 0 && health.badge.status === 'good'}
          <p>{t('healthTab.noSigns')}</p>
        {/if}
      </div>
      <div class="note mono">{t('healthTab.readOnly')}</div>
    </section>

    {#if values.length > 0}
      <dl class="stats values">
        {#each values as [label, value] (label)}
          <div>
            <dt class="caption">{label}</dt>
            <dd class="value">{value}</dd>
          </div>
        {/each}
      </dl>
    {/if}

    <div class="lower">
      {#if temps.length >= 2}
        <section class="card chart-card">
          <h2>{t('healthTab.temperatures')}</h2>
          <div class="chart" role="img" aria-label={t('healthTab.chartLabel', { min: Math.min(...temps.map((p) => p.temperature ?? 0)), max: Math.max(...temps.map((p) => p.temperature ?? 0)) })}>
            {#each temps as p (p.at)}
              <span class:hot={(p.temperature ?? 0) >= 45} style:height="{((p.temperature ?? 0) / maxTemp) * 100}%" title="{date(p.at)} · {p.temperature} °C"></span>
            {/each}
          </div>
          <div class="axis mono"><span>{date(temps[0]!.at)}</span><span>{date(temps[temps.length - 1]!.at)}</span></div>
        </section>
      {/if}
      {#if health.history.length > 0}
        <section class="card history">
          <h2>{t('health.history')}</h2>
          <table class="data">
            <thead>
              <tr>
                <th>{t('health.when')}</th>
                <th>{t('health.state')}</th>
                <th class="num">{t('health.reallocated')}</th>
                <th class="num">{t('health.pending')}</th>
                <th class="num">{t('health.mediaErrors')}</th>
              </tr>
            </thead>
            <tbody>
              {#each health.history as p (p.at)}
                <tr>
                  <td class="mono">{date(p.at)}</td>
                  <td class="state-cell {p.status}">{t(`health.short.${p.status}`)}</td>
                  <td class="num">{p.reallocated ?? '—'}</td>
                  <td class="num">{p.pending ?? '—'}</td>
                  <td class="num">{p.mediaErrors ?? '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          <p class="muted">{t('healthTab.notify')}</p>
        </section>
      {/if}
    </div>
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
    max-width: 220px;
    text-align: right;
  }
  .values {
    margin: 0;
  }
  .values dd {
    margin: 0;
  }
  .lower {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    align-items: flex-start;
  }
  .chart-card,
  .history {
    flex: 1 1 380px;
    padding: 16px 18px;
  }
  .lower h2 {
    font-size: 16px;
    margin-bottom: 10px;
  }
  .chart {
    height: 140px;
    display: flex;
    align-items: flex-end;
    gap: 4px;
    border-bottom: 1px solid var(--border);
  }
  .chart span {
    flex: 1;
    background: var(--text-3);
    border-radius: 3px 3px 0 0;
    min-height: 2px;
  }
  .chart span.hot {
    background: var(--signal);
  }
  .axis {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--text-3);
    margin-top: 4px;
  }
  .history p {
    margin: 10px 0 0;
    font-size: 12px;
  }
</style>
