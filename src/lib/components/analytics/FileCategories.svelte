<script lang="ts">
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import type { FileTypeCategory } from '$lib/types';

  export let categories: FileTypeCategory[] = [];
  export let expandedCategory: string | null = null;

  $: totalSize = categories.reduce((sum, c) => sum + c.total_size, 0);
  $: categorySegments = categories.map((cat, i) => {
    const percent = (cat.total_size / totalSize) * 100;
    const prevSum = categories.slice(0, i).reduce((s, c) => s + (c.total_size / totalSize) * 100, 0);
    return { ...cat, percent, offset: prevSum };
  });

  function toggleCategory(category: string) {
    expandedCategory = expandedCategory === category ? null : category;
  }
</script>

{#if categories.length > 0}
  <div class="bg-gray-800/50 rounded-lg p-4">
    <h3 class="text-white font-medium mb-4 flex items-center gap-2">
      <svg class="w-5 h-5 text-prism-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01" />
      </svg>
      File Categories
    </h3>

    <div class="flex flex-col lg:flex-row items-start gap-8">
      <!-- Donut Chart -->
      <div class="relative w-48 h-48 flex-shrink-0 mx-auto lg:mx-0">
        <svg viewBox="0 0 100 100" class="transform -rotate-90 w-full h-full">
          {#each categorySegments as segment}
            <circle
              cx="50" cy="50" r="40"
              fill="transparent"
              stroke={segment.color}
              stroke-width="18"
              stroke-dasharray="{segment.percent * 2.51} 251"
              stroke-dashoffset="{-segment.offset * 2.51}"
              class="transition-all duration-300"
            />
          {/each}
        </svg>
        <div class="absolute inset-0 flex items-center justify-center">
          <div class="text-center">
            <div class="text-xl font-bold text-white">{formatBytes(totalSize)}</div>
            <div class="text-xs text-gray-400">Total</div>
          </div>
        </div>
      </div>

      <!-- Category Breakdown -->
      <div class="flex-1 space-y-1 w-full">
        {#each categories as cat}
          {@const catPercent = (cat.total_size / totalSize) * 100}
          <div>
            <button
              class="w-full flex items-center gap-3 p-2 rounded hover:bg-gray-700/50 transition-colors"
              on:click={() => toggleCategory(cat.category)}
            >
              <div class="w-3 h-3 rounded flex-shrink-0" style="background: {cat.color}"></div>
              <span class="text-white flex-1 text-left text-sm">{cat.category}</span>
              <div class="flex-1 h-2 bg-gray-700 rounded-full overflow-hidden max-w-[120px]">
                <div class="h-full rounded-full transition-all" style="width: {catPercent}%; background: {cat.color}"></div>
              </div>
              <span class="text-gray-400 text-xs w-12 text-right">{catPercent.toFixed(1)}%</span>
              <span class="text-gray-500 text-xs w-16 text-right">{formatBytes(cat.total_size)}</span>
              <svg class="w-4 h-4 text-gray-400 transition-transform {expandedCategory === cat.category ? 'rotate-180' : ''}" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
              </svg>
            </button>
            {#if expandedCategory === cat.category && cat.extensions.length > 0}
              <div class="ml-6 pl-3 border-l border-gray-700 space-y-1 py-2">
                {#each cat.extensions.slice(0, 8) as ext}
                  {@const extPercent = (ext.size / cat.total_size) * 100}
                  <div class="flex items-center gap-2 text-xs text-gray-400">
                    <span class="w-14 font-mono">.{ext.extension}</span>
                    <div class="flex-1 h-1.5 bg-gray-700 rounded-full overflow-hidden">
                      <div class="h-full rounded-full" style="width: {extPercent}%; background: {cat.color}"></div>
                    </div>
                    <span class="w-12 text-right">{formatNumber(ext.count)}</span>
                    <span class="w-16 text-right">{formatBytes(ext.size)}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}
