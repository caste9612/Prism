<script lang="ts">
  // Statistics by file type (F08-AC3): categories, extensions and size bands, for one disk or
  // the whole catalog. Bars are drawn as HTML with every value written out (no colour-only
  // reading); the category colour is a swatch next to the label.
  import type { StatRowView } from '../lib/bindings/StatRowView';
  import type { TypeStatsView } from '../lib/bindings/TypeStatsView';
  import { formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';

  let { stats }: { stats: TypeStatsView } = $props();
  const lang = $derived(language());

  const share = (bytes: number) => (stats.total.bytes > 0 ? bytes / stats.total.bytes : 0);
  const percent = (bytes: number) => {
    const fmt = new Intl.NumberFormat(lang, { style: 'percent', minimumFractionDigits: 1, maximumFractionDigits: 1 });
    const s = share(bytes);
    return s > 0 && s < 0.001 ? `< ${fmt.format(0.001)}` : fmt.format(s);
  };
  const categories = $derived(
    [...stats.categories].filter((c) => c.totals.files > 0).sort((a, b) => b.totals.bytes - a.totals.bytes),
  );
  const maxBand = $derived(Math.max(1, ...stats.sizeBands.map((b) => b.totals.bytes)));

  function bandLabel(row: StatRowView, i: number): string {
    const from = Number(row.key);
    const next = stats.sizeBands[i + 1];
    if (i === 0 && next) return t('types.below', { size: formatBytes(Number(next.key), lang) });
    if (!next) return t('types.over', { size: formatBytes(from, lang) });
    return t('types.range', { from: formatBytes(from, lang), to: formatBytes(Number(next.key), lang) });
  }
</script>

<div class="stats">
  <p class="muted total">
    {t('types.total', { files: formatCount(stats.total.files, lang), size: formatBytes(stats.total.bytes, lang) })}
  </p>

  <section aria-labelledby="types-cat">
    <h2 id="types-cat">{t('types.categories')}</h2>
    <ul class="bars">
      {#each categories as c (c.key)}
        <li>
          <span class="label"><span class="swatch" style:background="var(--cat-{c.key})"></span>{t(`category.${c.key}`)}</span>
          <span class="bar"><span style:width="{share(c.totals.bytes) * 100}%" style:background="var(--cat-{c.key})"></span></span>
          <span class="num">{formatBytes(c.totals.bytes, lang)}</span>
          <span class="num muted">{percent(c.totals.bytes)}</span>
          <span class="num muted">{t('space.files', { count: formatCount(c.totals.files, lang) })}</span>
        </li>
      {/each}
    </ul>
  </section>

  <div class="two">
    <section aria-labelledby="types-ext">
      <h2 id="types-ext">{t('types.extensions')}</h2>
      <table>
        <thead>
          <tr>
            <th scope="col">{t('types.extension')}</th>
            <th scope="col" class="num">{t('space.size')}</th>
            <th scope="col" class="num">{t('types.files')}</th>
          </tr>
        </thead>
        <tbody>
          {#each stats.extensions as e (e.key)}
            <tr>
              <td>
                <span class="swatch" style:background="var(--cat-{e.category ?? 'other'})"></span>
                {e.key === '' ? t('types.noExtension') : `.${e.key}`}
              </td>
              <td class="num">{formatBytes(e.totals.bytes, lang)}</td>
              <td class="num">{formatCount(e.totals.files, lang)}</td>
            </tr>
          {/each}
          {#if stats.otherExtensions.files > 0}
            <tr class="muted">
              <td>{t('types.otherExtensions')}</td>
              <td class="num">{formatBytes(stats.otherExtensions.bytes, lang)}</td>
              <td class="num">{formatCount(stats.otherExtensions.files, lang)}</td>
            </tr>
          {/if}
        </tbody>
      </table>
    </section>

    <section aria-labelledby="types-bands">
      <h2 id="types-bands">{t('types.bands')}</h2>
      <ul class="bars bands">
        {#each stats.sizeBands as b, i (b.key)}
          <li>
            <span class="label">{bandLabel(b, i)}</span>
            <span class="bar"><span style:width="{(b.totals.bytes / maxBand) * 100}%"></span></span>
            <span class="num">{formatBytes(b.totals.bytes, lang)}</span>
            <span class="num muted">{t('space.files', { count: formatCount(b.totals.files, lang) })}</span>
          </li>
        {/each}
      </ul>
    </section>
  </div>
</div>

<style>
  .stats {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .total {
    margin: 0;
  }
  h2 {
    font-size: 14px;
    margin-bottom: 8px;
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: 24px;
  }
  .bars {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bars li {
    display: grid;
    grid-template-columns: 150px 1fr 80px 56px 110px;
    align-items: center;
    gap: 10px;
  }
  .bands li {
    grid-template-columns: 140px 1fr 80px 110px;
  }
  .label {
    display: flex;
    align-items: center;
    gap: 8px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .swatch {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 3px;
    flex: none;
    margin-right: 6px;
    vertical-align: -1px;
  }
  .label .swatch {
    margin-right: 0;
  }
  .bar {
    height: 10px;
    background: var(--track);
    border-radius: 4px;
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: 0 4px 4px 0;
    background: var(--accent);
    min-width: 2px;
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    padding: 5px 6px;
    border-bottom: 1px solid var(--border);
    text-align: left;
  }
  th {
    font-weight: 600;
    color: var(--text-2);
    font-size: 12.5px;
  }
</style>
