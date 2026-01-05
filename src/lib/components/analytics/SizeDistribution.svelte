<script lang="ts">
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import type { SizeCategory, ExtensionCategory } from '$lib/types';

  export let sizeDistribution: SizeCategory[] = [];
  export let extensionDistribution: ExtensionCategory[] = [];

  function getMaxSize(data: { total_size: number }[]): number {
    return Math.max(...data.map(d => d.total_size), 1);
  }
</script>

{#if sizeDistribution.length > 0}
  <div class="grid lg:grid-cols-2 gap-6">
    <div class="bg-gray-800/50 rounded-lg p-4">
      <h3 class="text-white font-medium mb-4 flex items-center gap-2">
        <svg class="w-5 h-5 text-prism-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z" />
        </svg>
        Size Distribution
      </h3>
      <div class="space-y-3">
        {#each sizeDistribution as cat}
          {@const pct = (cat.total_size / getMaxSize(sizeDistribution)) * 100}
          <div>
            <div class="flex justify-between text-sm mb-1">
              <span class="text-gray-300">{cat.name}</span>
              <span class="text-gray-500">{formatNumber(cat.count)} files</span>
            </div>
            <div class="h-2 bg-gray-700 rounded-full overflow-hidden">
              <div class="h-full bg-prism-500 rounded-full" style="width: {pct}%"></div>
            </div>
          </div>
        {/each}
      </div>
    </div>

    <div class="bg-gray-800/50 rounded-lg p-4">
      <h3 class="text-white font-medium mb-4 flex items-center gap-2">
        <svg class="w-5 h-5 text-prism-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
        </svg>
        Top Extensions
      </h3>
      <div class="space-y-2">
        {#each extensionDistribution.slice(0, 10) as ext}
          {@const pct = (ext.total_size / getMaxSize(extensionDistribution)) * 100}
          <div class="flex items-center gap-3">
            <span class="w-16 text-xs font-mono text-gray-400">.{ext.extension}</span>
            <div class="flex-1 h-2 bg-gray-700 rounded-full overflow-hidden">
              <div class="h-full bg-blue-500 rounded-full" style="width: {pct}%"></div>
            </div>
            <span class="text-xs text-gray-500 w-20 text-right">{formatBytes(ext.total_size)}</span>
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}
