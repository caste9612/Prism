<script lang="ts">
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import type { DriveInfo, DriveProgress, DriveStats } from '$lib/types';

  export let drive: DriveInfo;
  export let isSelected = false;
  export let isScanning = false;
  export let scanStatus: 'idle' | 'waiting' | 'scanning' | 'done' = 'idle';
  export let progress: DriveProgress | null = null;
  export let stats: DriveStats | null = null;

  $: indexedFiles = stats?.file_count || 0;
  $: indexedSize = stats?.total_size || 0;
</script>

<button
  on:click
  disabled={!drive.is_ready || isScanning}
  class="relative flex flex-col gap-1 px-4 py-3 rounded-xl border-2 transition-all min-w-[140px] overflow-hidden
    {isSelected ? 'bg-prism-500/10 border-prism-500' : 'bg-gray-700/30 border-gray-600 hover:border-gray-500'}
    {!drive.is_ready ? 'opacity-50 cursor-not-allowed' : ''}
    {scanStatus === 'scanning' ? 'border-prism-400 ring-2 ring-prism-400/30' : ''}
    {scanStatus === 'done' ? 'border-green-500/50' : ''}"
>
  <!-- Progress bar fill -->
  {#if (scanStatus === 'scanning' || scanStatus === 'done') && progress}
    <div
      class="absolute bottom-0 left-0 right-0 transition-all duration-300 ease-out
        {scanStatus === 'done' ? 'bg-green-500/20' : 'bg-prism-500/30'}"
      style="height: {progress.progress_percent}%"
    ></div>
    {#if scanStatus === 'scanning'}
      <div class="absolute inset-0 overflow-hidden">
        <div class="absolute inset-0 bg-gradient-to-r from-transparent via-white/5 to-transparent animate-shimmer"></div>
      </div>
    {/if}
  {:else if scanStatus === 'waiting'}
    <div class="absolute inset-0 bg-yellow-500/10"></div>
  {/if}

  <div class="relative flex items-center gap-2">
    <!-- Drive letter -->
    <div class="relative flex items-center justify-center w-8 h-8 rounded-lg bg-gray-600/50 {scanStatus === 'scanning' ? 'bg-prism-500/30' : ''}">
      <span class="text-base font-bold {scanStatus === 'scanning' ? 'text-prism-400' : 'text-white'}">{drive.path.charAt(0)}</span>
      {#if scanStatus === 'scanning'}
        <div class="absolute -top-1 -right-1 w-3 h-3 bg-prism-500 rounded-full animate-pulse"></div>
      {:else if scanStatus === 'done' || indexedFiles > 0}
        <div class="absolute -top-1 -right-1 w-3 h-3 bg-green-500 rounded-full"></div>
      {:else if scanStatus === 'waiting'}
        <div class="absolute -top-1 -right-1 w-3 h-3 bg-yellow-500 rounded-full"></div>
      {/if}
    </div>

    <div class="flex flex-col">
      <span class="text-sm font-semibold text-white">{drive.path.substring(0, 2)}</span>
      <span class="text-xs text-gray-500">
        {drive.drive_type === 'Local Disk' ? 'Local' : drive.drive_type === 'Network Drive' ? 'Network' : drive.drive_type}
      </span>
    </div>

    {#if isSelected && !isScanning}
      <svg class="w-4 h-4 text-prism-400 ml-auto" fill="currentColor" viewBox="0 0 20 20">
        <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
      </svg>
    {/if}
  </div>

  <!-- Progress/Stats display -->
  <div class="relative text-left">
    {#if scanStatus === 'scanning' && progress}
      <div class="text-xs text-prism-400 font-bold">
        Scanning {Math.round(progress.progress_percent)}%
      </div>
      <div class="text-xs text-gray-400">
        {formatNumber(progress.files_scanned)} files
      </div>
      <div class="text-xs text-gray-500">
        {formatBytes(progress.total_size)}
      </div>
    {:else if scanStatus === 'done' && progress}
      <div class="text-xs text-green-400 font-bold">
        Scanned 100%
      </div>
      <div class="text-xs text-gray-400">
        {formatNumber(progress.files_scanned)} files
      </div>
      <div class="text-xs text-gray-500">
        {formatBytes(progress.total_size)}
      </div>
    {:else if indexedFiles > 0}
      <div class="text-xs text-green-400 font-medium">
        {formatNumber(indexedFiles)} files
      </div>
      <div class="text-xs text-gray-500">
        {formatBytes(indexedSize)}
      </div>
    {:else if scanStatus === 'waiting'}
      <div class="text-xs text-yellow-400">Waiting...</div>
    {:else if drive.is_ready && drive.total_space > 0}
      <div class="text-xs text-gray-500">{formatBytes(drive.free_space)} free</div>
      <div class="text-xs text-gray-600">{formatBytes(drive.total_space)} total</div>
    {:else}
      <div class="text-xs text-gray-500">Not scanned</div>
    {/if}
  </div>
</button>

<style>
  @keyframes shimmer {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(100%); }
  }
  .animate-shimmer {
    animation: shimmer 1.5s infinite;
  }
</style>
