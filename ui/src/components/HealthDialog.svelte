<script lang="ts">
  // Health of the disk of a volume (F17): overall status with the reasons, the SMART values the
  // user cares about and the history of the readings; when SMART is not available, why (F17-AC4).
  import type { DiskHealthView } from '../lib/bindings/DiskHealthView';
  import { formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';

  let { health, onclose }: { health: DiskHealthView | null; onclose: () => void } = $props();

  const lang = $derived(language());
  const date = (ms: number) => new Date(ms).toLocaleString(lang, { dateStyle: 'medium', timeStyle: 'short' });
  const rows = $derived.by(() => {
    if (!health) return [];
    const h = health;
    const n = (v: number | null) => (v === null ? null : formatCount(v, lang));
    return [
      [t('health.temperature'), h.badge.temperature === null ? null : `${h.badge.temperature} °C`],
      [t('health.hours'), n(h.powerOnHours)],
      [t('health.wear'), h.wear === null ? null : `${h.wear}%`],
      [t('health.spare'), h.availableSpare === null ? null : `${h.availableSpare}%`],
      [t('health.reallocated'), n(h.reallocated)],
      [t('health.pending'), n(h.pending)],
      [t('health.mediaErrors'), n(h.mediaErrors)],
    ].filter((r): r is [string, string] => r[1] !== null);
  });
</script>

<Dialog open={health !== null} title={health ? t('health.title', { model: health.model }) : ''} {onclose}>
  {#if health}
    <p class="muted">{[health.bus.toUpperCase(), health.serial].filter(Boolean).join(' · ')}</p>
    <p class="status {health.badge.status}">
      <strong>{t(`health.status.${health.badge.status}`)}</strong>
    </p>
    {#if health.badge.unavailable}
      <p>{t(`health.unavailable.${health.badge.unavailable}`)}</p>
    {/if}
    {#each health.reasons as r (r)}
      <p class="reason">{t(`health.reason.${r}`)}</p>
    {/each}
    {#if rows.length > 0}
      <dl class="values">
        {#each rows as [label, value] (label)}
          <dt>{label}</dt>
          <dd>{value}</dd>
        {/each}
      </dl>
    {/if}
    {#if health.history.length > 0}
      <h3>{t('health.history')}</h3>
      <table>
        <thead>
          <tr>
            <th>{t('health.when')}</th>
            <th>{t('health.state')}</th>
            <th>{t('health.reallocated')}</th>
            <th>{t('health.pending')}</th>
            <th>{t('health.mediaErrors')}</th>
          </tr>
        </thead>
        <tbody>
          {#each health.history as p (p.at)}
            <tr>
              <td>{date(p.at)}</td>
              <td class="status {p.status}">{t(`health.short.${p.status}`)}</td>
              <td>{p.reallocated ?? '—'}</td>
              <td>{p.pending ?? '—'}</td>
              <td>{p.mediaErrors ?? '—'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {/if}
  {#snippet actions()}
    <button onclick={onclose}>{t('actions.close')}</button>
  {/snippet}
</Dialog>

<style>
  .status.good {
    color: var(--ok);
  }
  .status.warning {
    color: var(--warn);
  }
  .status.critical {
    color: var(--danger);
  }
  .reason {
    margin: 2px 0;
  }
  .values {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 16px;
    margin: 12px 0;
  }
  .values dt {
    color: var(--text-2);
  }
  .values dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  h3 {
    font-size: 14px;
    margin: 12px 0 6px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12.5px;
  }
  th,
  td {
    text-align: left;
    padding: 3px 6px;
    border-bottom: 1px solid var(--border);
  }
</style>
