<script lang="ts">
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import type { ScanProgress } from '$lib/types';

  export let progress: ScanProgress;

  $: isIndexing = progress.indexing != null;
  $: indexingPercent = progress.indexing?.percent ?? 0;
  $: indexingRate = progress.indexing?.files_per_second ?? 0;
</script>

<div class="mt-4 bg-gray-900/50 rounded-lg p-3">
  <div class="flex items-center justify-between mb-2">
    <div class="flex items-center gap-3">
      <div class="w-4 h-4 border-2 border-prism-400 border-t-transparent rounded-full animate-spin"></div>
      <span class="text-sm text-white font-medium">
        {formatNumber(progress.files_scanned)} files scanned
      </span>
      {#if !isIndexing && progress.files_per_second > 0}
        <span class="text-sm text-gray-400">
          ({Math.round(progress.files_per_second)} files/sec)
        </span>
      {/if}
    </div>
    <span class="text-sm text-prism-400 font-medium">{formatBytes(progress.total_size)}</span>
  </div>

  {#if isIndexing}
    <!-- Indexing progress bar -->
    <div class="mt-3 mb-2">
      <div class="flex items-center justify-between mb-1">
        <span class="text-xs text-gray-400">Building search index...</span>
        <span class="text-xs text-prism-400 font-medium">{indexingPercent.toFixed(0)}%</span>
      </div>
      <div class="w-full bg-gray-700 rounded-full h-2">
        <div
          class="bg-gradient-to-r from-prism-500 to-prism-400 h-2 rounded-full transition-all duration-300"
          style="width: {Math.min(indexingPercent, 100)}%"
        ></div>
      </div>
      <div class="flex items-center justify-between mt-1">
        <span class="text-xs text-gray-500">
          {formatNumber(progress.indexing?.files_indexed ?? 0)} / {formatNumber(progress.indexing?.total_files ?? 0)} files
        </span>
        {#if indexingRate > 0}
          <span class="text-xs text-gray-500">
            {formatNumber(Math.round(indexingRate))} files/sec
          </span>
        {/if}
      </div>
    </div>
  {:else}
    <div class="text-xs text-gray-500 truncate">{progress.current_path}</div>
  {/if}
</div>
