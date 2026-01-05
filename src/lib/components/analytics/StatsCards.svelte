<script lang="ts">
  import { formatBytes, formatNumber } from '$lib/utils/format';

  export let totalFiles: number;
  export let totalSize: number;
  export let driveCount: number;
</script>

<div class="grid grid-cols-2 lg:grid-cols-4 gap-4">
  <div class="bg-gray-800/50 rounded-lg p-4">
    <div class="text-gray-400 text-sm">Total Files</div>
    <div class="text-2xl font-bold text-white">{formatNumber(totalFiles)}</div>
  </div>
  <div class="bg-gray-800/50 rounded-lg p-4">
    <div class="text-gray-400 text-sm">Total Size</div>
    <div class="text-2xl font-bold text-prism-400">{formatBytes(totalSize)}</div>
  </div>
  <div class="bg-gray-800/50 rounded-lg p-4">
    <div class="text-gray-400 text-sm">Drives Scanned</div>
    <div class="text-2xl font-bold text-white">{driveCount}</div>
  </div>
  <div class="bg-gray-800/50 rounded-lg p-4">
    <div class="text-gray-400 text-sm">Avg File Size</div>
    <div class="text-2xl font-bold text-white">{totalFiles > 0 ? formatBytes(Math.round(totalSize / totalFiles)) : '0 B'}</div>
  </div>
</div>
