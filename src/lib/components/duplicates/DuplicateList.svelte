<script lang="ts">
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import type { DuplicateGroup } from '$lib/types';
  import { createEventDispatcher } from 'svelte';

  export let groups: DuplicateGroup[] = [];
  export let loading = false;
  export let wastedSpace = 0;
  export let totalFiles = 0;
  export let expandedGroups = new Set<number>();
  export let selectedFiles = new Set<string>();

  const dispatch = createEventDispatcher<{
    findDuplicates: void;
    selectAll: void;
    deleteSelected: void;
    toggleGroup: { id: number };
    toggleFile: { id: number; path: string };
    openInExplorer: { path: string };
  }>();

  function isFileSelected(fileId: number, path: string): boolean {
    return selectedFiles.has(`${fileId}:${path}`);
  }
</script>

<div class="space-y-4">
  <div class="flex items-center justify-between">
    <div class="flex items-center gap-4">
      <span class="text-sm text-gray-400">
        {#if groups.length > 0}{groups.length} groups - {formatBytes(wastedSpace)} wasted{/if}
      </span>
      {#if selectedFiles.size > 0}
        <span class="text-sm text-prism-400">{selectedFiles.size} selected</span>
      {/if}
    </div>
    <div class="flex items-center gap-2">
      {#if selectedFiles.size > 0}
        <button
          on:click={() => dispatch('deleteSelected')}
          class="px-3 py-1.5 bg-red-600 hover:bg-red-700 text-white text-sm rounded-lg"
        >
          Delete Selected
        </button>
      {/if}
      {#if groups.length > 0}
        <button
          on:click={() => dispatch('selectAll')}
          class="px-3 py-1.5 bg-gray-700 hover:bg-gray-600 text-white text-sm rounded-lg"
        >
          Select All Duplicates
        </button>
      {/if}
      <button
        on:click={() => dispatch('findDuplicates')}
        disabled={loading || totalFiles === 0}
        class="px-4 py-2 bg-prism-500 hover:bg-prism-600 text-white rounded-lg text-sm disabled:opacity-50"
      >
        {#if loading}
          <span class="flex items-center gap-2">
            <div class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
            Analyzing...
          </span>
        {:else}
          Find Duplicates
        {/if}
      </button>
    </div>
  </div>

  {#if groups.length === 0 && !loading}
    <div class="text-center py-16 text-gray-500">
      {#if totalFiles === 0}
        <p>Scan files first to find duplicates.</p>
      {:else}
        <p>Click "Find Duplicates" to analyze.</p>
      {/if}
    </div>
  {:else}
    <div class="space-y-2">
      {#each groups as group}
        <div class="bg-gray-800/50 rounded-lg overflow-hidden">
          <button
            on:click={() => dispatch('toggleGroup', { id: group.id })}
            class="w-full p-3 flex items-center justify-between hover:bg-gray-800 transition-colors"
          >
            <div class="flex items-center gap-3">
              <span class="text-white">{group.files.length} files</span>
              <span class="text-gray-400">|</span>
              <span class="text-gray-400">{formatBytes(group.size)} each</span>
              <span class="text-gray-400">|</span>
              <span class="text-red-400">{formatBytes(group.wasted_space)} wasted</span>
            </div>
            <svg
              class="w-5 h-5 text-gray-400 transition-transform {expandedGroups.has(group.id) ? 'rotate-180' : ''}"
              fill="none" stroke="currentColor" viewBox="0 0 24 24"
            >
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
            </svg>
          </button>

          {#if expandedGroups.has(group.id)}
            <div class="border-t border-gray-700 p-3 space-y-1">
              {#each group.files as file, idx}
                {@const isSelected = isFileSelected(file.id, file.path)}
                <div
                  class="flex items-center gap-3 p-2 rounded group {isSelected ? 'bg-red-500/10' : 'bg-gray-900/50'} hover:bg-gray-900/80"
                >
                  <button
                    on:click={() => dispatch('toggleFile', { id: file.id, path: file.path })}
                    class="w-5 h-5 rounded border flex items-center justify-center flex-shrink-0
                      {idx === 0 ? 'border-gray-600 bg-gray-700 cursor-not-allowed' : isSelected ? 'border-red-500 bg-red-500' : 'border-gray-600 hover:border-gray-500'}"
                    disabled={idx === 0}
                    title={idx === 0 ? 'Original file' : 'Select for deletion'}
                  >
                    {#if isSelected}
                      <svg class="w-3 h-3 text-white" fill="currentColor" viewBox="0 0 20 20">
                        <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
                      </svg>
                    {/if}
                  </button>
                  {#if idx === 0}
                    <span class="text-xs bg-green-600 px-1.5 py-0.5 rounded text-white">Original</span>
                  {/if}
                  <div class="flex-1 truncate text-sm {idx === 0 ? 'text-white' : 'text-gray-300'}">{file.path}</div>
                  <button
                    on:click={() => dispatch('openInExplorer', { path: file.path })}
                    class="p-1 text-gray-500 hover:text-white opacity-0 group-hover:opacity-100"
                  >
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                    </svg>
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
