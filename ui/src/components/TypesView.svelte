<script lang="ts">
  // "File types" tab (F08-AC3): categories, extensions and size bands of one disk, or of the
  // whole catalog when `volumeId` is null.
  import type { TypeStatsView } from '../lib/bindings/TypeStatsView';
  import { backend, errorMessage } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import TypeStats from './TypeStats.svelte';

  let { volumeId, onerror }: { volumeId: number | null; onerror: (message: string) => void } = $props();

  let stats = $state<TypeStatsView | null>(null);
  $effect(() => {
    const id = volumeId;
    stats = null;
    void backend.typeStats(id).then(
      (s) => (stats = s),
      (e) => onerror(errorMessage(e)),
    );
  });
</script>

{#if stats}
  <div class="card types"><TypeStats {stats} /></div>
{:else}
  <p class="empty card">{t('types.loading')}</p>
{/if}

<style>
  .types {
    padding: 16px 18px;
  }
</style>
