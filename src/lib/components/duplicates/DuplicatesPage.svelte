<script lang="ts">
  import { duplicatesStore, type DuplicateTab, type SortField } from '$lib/stores/duplicates';
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import { invoke } from '@tauri-apps/api/core';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import type { DuplicateGroup, DuplicateFile, SimilarImageGroup } from '$lib/types';

  export let totalFiles = 0;

  // Store subscriptions - derived stores need to be accessed from the store object, not the value
  $: state = $duplicatesStore;

  // Get derived stores from the store object
  const { filteredGroups, availableExtensions, folderDuplicates } = duplicatesStore;

  // Local UI state
  let expandedGroups = new Set<number>();
  let previewImage: { path: string; name: string } | null = null;
  let deleteConfirmFiles: { id: number; path: string }[] = [];
  let showDeleteModal = false;
  let deleting = false;

  // Tabs
  const tabs: { id: DuplicateTab; label: string; icon: string }[] = [
    { id: 'exact', label: 'Exact Duplicates', icon: '=' },
    { id: 'similar', label: 'Similar Images', icon: '~' },
    { id: 'byFolder', label: 'By Folder', icon: '/' }
  ];

  // Sort options
  const sortOptions: { value: SortField; label: string }[] = [
    { value: 'wastedSpace', label: 'Wasted Space' },
    { value: 'fileCount', label: 'File Count' },
    { value: 'size', label: 'File Size' },
    { value: 'name', label: 'Name' }
  ];

  // Size filter presets
  const sizePresets = [
    { value: 0, label: 'All Sizes' },
    { value: 1024 * 1024, label: '> 1 MB' },
    { value: 10 * 1024 * 1024, label: '> 10 MB' },
    { value: 100 * 1024 * 1024, label: '> 100 MB' },
    { value: 1024 * 1024 * 1024, label: '> 1 GB' }
  ];

  function toggleGroup(id: number) {
    if (expandedGroups.has(id)) {
      expandedGroups.delete(id);
    } else {
      expandedGroups.add(id);
    }
    expandedGroups = expandedGroups;
  }

  function isFileSelected(file: DuplicateFile): boolean {
    return state.selectedFiles.has(`${file.id}:${file.path}`);
  }

  async function openInExplorer(path: string) {
    try {
      await invoke('open_in_explorer', { path });
    } catch (e) {
      console.error('Failed to open in explorer:', e);
    }
  }

  function showDeleteConfirm() {
    deleteConfirmFiles = duplicatesStore.getSelectedFiles();
    showDeleteModal = true;
  }

  async function confirmDelete() {
    deleting = true;
    try {
      await duplicatesStore.deleteSelected();
      showDeleteModal = false;
      deleteConfirmFiles = [];
    } catch (e) {
      console.error('Delete failed:', e);
    } finally {
      deleting = false;
    }
  }

  function isImageFile(path: string): boolean {
    const ext = path.split('.').pop()?.toLowerCase();
    return ['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp'].includes(ext || '');
  }

  function getImageSrc(path: string): string {
    return convertFileSrc(path);
  }

  // Extension badge colors
  const extColors: Record<string, string> = {
    pdf: 'bg-red-500/20 text-red-400',
    doc: 'bg-blue-500/20 text-blue-400',
    docx: 'bg-blue-500/20 text-blue-400',
    xls: 'bg-green-500/20 text-green-400',
    xlsx: 'bg-green-500/20 text-green-400',
    jpg: 'bg-purple-500/20 text-purple-400',
    jpeg: 'bg-purple-500/20 text-purple-400',
    png: 'bg-purple-500/20 text-purple-400',
    gif: 'bg-purple-500/20 text-purple-400',
    mp4: 'bg-orange-500/20 text-orange-400',
    mp3: 'bg-pink-500/20 text-pink-400',
    zip: 'bg-yellow-500/20 text-yellow-400',
    rar: 'bg-yellow-500/20 text-yellow-400',
  };

  function getExtColor(ext: string | null): string {
    if (!ext) return 'bg-gray-500/20 text-gray-400';
    return extColors[ext.toLowerCase()] || 'bg-gray-500/20 text-gray-400';
  }
</script>

<div class="h-full flex flex-col">
  <!-- Stats Header -->
  <div class="grid grid-cols-2 md:grid-cols-4 gap-3 mb-4">
    <div class="bg-gray-800/50 rounded-lg p-4">
      <div class="text-2xl font-bold text-white">{formatNumber(state.totalGroups)}</div>
      <div class="text-sm text-gray-400">Duplicate Groups</div>
    </div>
    <div class="bg-gray-800/50 rounded-lg p-4">
      <div class="text-2xl font-bold text-red-400">{formatBytes(state.totalWastedSpace)}</div>
      <div class="text-sm text-gray-400">Wasted Space</div>
    </div>
    <div class="bg-gray-800/50 rounded-lg p-4">
      <div class="text-2xl font-bold text-purple-400">{formatNumber(state.totalSimilarGroups)}</div>
      <div class="text-sm text-gray-400">Similar Image Groups</div>
    </div>
    <div class="bg-gray-800/50 rounded-lg p-4">
      <div class="text-2xl font-bold text-gray-300">
        {state.detectionTimeMs > 0 ? `${(state.detectionTimeMs / 1000).toFixed(1)}s` : '-'}
      </div>
      <div class="text-sm text-gray-400">Detection Time</div>
    </div>
  </div>

  <!-- Action Buttons -->
  <div class="flex flex-wrap items-center gap-3 mb-4">
    <button
      on:click={() => duplicatesStore.findDuplicates()}
      disabled={state.loading || totalFiles === 0}
      class="px-4 py-2 bg-prism-500 hover:bg-prism-600 text-white rounded-lg text-sm font-medium disabled:opacity-50 flex items-center gap-2"
    >
      {#if state.loading}
        <div class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
        Analyzing...
      {:else}
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
        </svg>
        Find Exact Duplicates
      {/if}
    </button>

    <button
      on:click={() => duplicatesStore.findSimilarImages()}
      disabled={state.similarLoading || totalFiles === 0}
      class="px-4 py-2 bg-purple-600 hover:bg-purple-700 text-white rounded-lg text-sm font-medium disabled:opacity-50 flex items-center gap-2"
    >
      {#if state.similarLoading}
        <div class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
        Analyzing...
      {:else}
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z" />
        </svg>
        Find Similar Images
      {/if}
    </button>

    {#if state.selectedFiles.size > 0}
      <div class="flex items-center gap-2 ml-auto">
        <span class="text-sm text-prism-400">{state.selectedFiles.size} selected</span>
        <button
          on:click={() => duplicatesStore.clearSelection()}
          class="px-3 py-1.5 bg-gray-700 hover:bg-gray-600 text-white text-sm rounded-lg"
        >
          Clear
        </button>
        <button
          on:click={showDeleteConfirm}
          class="px-3 py-1.5 bg-red-600 hover:bg-red-700 text-white text-sm rounded-lg flex items-center gap-1"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
          </svg>
          Delete Selected
        </button>
      </div>
    {:else if state.groups.length > 0}
      <button
        on:click={() => duplicatesStore.selectAllDuplicates()}
        class="px-3 py-1.5 bg-gray-700 hover:bg-gray-600 text-white text-sm rounded-lg ml-auto"
      >
        Select All Duplicates
      </button>
    {/if}
  </div>

  <!-- Progress Bar -->
  {#if state.progress}
    <div class="bg-gray-800/50 rounded-lg p-4 mb-4">
      <div class="flex items-center justify-between mb-2">
        <span class="text-white font-medium">{state.progress.message}</span>
        <span class="text-prism-400">{state.progress.percent.toFixed(0)}%</span>
      </div>
      <div class="w-full bg-gray-700 rounded-full h-2">
        <div
          class="bg-prism-500 h-2 rounded-full transition-all duration-300"
          style="width: {state.progress.percent}%"
        ></div>
      </div>
      {#if state.progress.candidateGroups > 0}
        <div class="text-xs text-gray-400 mt-2">
          {formatNumber(state.progress.filesProcessed)} files processed |
          {formatNumber(state.progress.candidateGroups)} candidate groups |
          {formatNumber(state.progress.confirmedGroups)} confirmed
        </div>
      {/if}
    </div>
  {/if}

  <!-- Tabs -->
  <div class="flex gap-1 mb-4 border-b border-gray-700">
    {#each tabs as tab}
      <button
        on:click={() => duplicatesStore.setActiveTab(tab.id)}
        class="px-4 py-2 text-sm font-medium border-b-2 transition-colors {state.activeTab === tab.id
          ? 'border-prism-500 text-white'
          : 'border-transparent text-gray-400 hover:text-white'}"
      >
        <span class="font-mono mr-1">{tab.icon}</span>
        {tab.label}
        {#if tab.id === 'exact' && state.totalGroups > 0}
          <span class="ml-1 text-xs text-gray-500">({state.totalGroups})</span>
        {:else if tab.id === 'similar' && state.totalSimilarGroups > 0}
          <span class="ml-1 text-xs text-gray-500">({state.totalSimilarGroups})</span>
        {:else if tab.id === 'byFolder' && $folderDuplicates.length > 0}
          <span class="ml-1 text-xs text-gray-500">({$folderDuplicates.length})</span>
        {/if}
      </button>
    {/each}
  </div>

  <!-- Filters (only for exact duplicates tab) -->
  {#if state.activeTab === 'exact' && state.groups.length > 0}
    <div class="flex flex-wrap items-center gap-3 mb-4 p-3 bg-gray-800/30 rounded-lg">
      <!-- Sort -->
      <div class="flex items-center gap-2">
        <span class="text-xs text-gray-400">Sort:</span>
        <select
          value={state.sortBy}
          on:change={(e) => {
            const val = e.currentTarget.value;
            if (val === 'wastedSpace' || val === 'fileCount' || val === 'size' || val === 'name') {
              duplicatesStore.setSortBy(val);
            }
          }}
          class="bg-gray-700 border border-gray-600 text-white text-sm rounded px-2 py-1"
        >
          {#each sortOptions as opt}
            <option value={opt.value}>{opt.label}</option>
          {/each}
        </select>
        <button
          on:click={() => duplicatesStore.toggleSortOrder()}
          class="p-1 rounded hover:bg-gray-700"
          title="Toggle order"
        >
          <svg class="w-4 h-4 text-gray-400 {state.sortOrder === 'asc' ? 'rotate-180' : ''}" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
          </svg>
        </button>
      </div>

      <!-- Size filter -->
      <div class="flex items-center gap-2">
        <span class="text-xs text-gray-400">Min Size:</span>
        <select
          value={state.filterMinSize}
          on:change={(e) => duplicatesStore.setFilterMinSize(parseInt(e.currentTarget.value))}
          class="bg-gray-700 border border-gray-600 text-white text-sm rounded px-2 py-1"
        >
          {#each sizePresets as preset}
            <option value={preset.value}>{preset.label}</option>
          {/each}
        </select>
      </div>

      <!-- Search -->
      <div class="flex-1 min-w-[200px]">
        <input
          type="text"
          placeholder="Search files..."
          value={state.searchQuery}
          on:input={(e) => duplicatesStore.setSearchQuery(e.currentTarget.value)}
          class="w-full bg-gray-700 border border-gray-600 text-white text-sm rounded px-3 py-1 placeholder-gray-400"
        />
      </div>

      <!-- Extension filters -->
      {#if $availableExtensions.length > 0}
        <div class="flex items-center gap-1 flex-wrap">
          {#each $availableExtensions.slice(0, 8) as ext}
            <button
              on:click={() => duplicatesStore.toggleExtensionFilter(ext)}
              class="px-2 py-0.5 text-xs rounded {state.filterExtensions.includes(ext)
                ? 'bg-prism-500 text-white'
                : 'bg-gray-700 text-gray-300 hover:bg-gray-600'}"
            >
              .{ext}
            </button>
          {/each}
          {#if $availableExtensions.length > 8}
            <span class="text-xs text-gray-500">+{$availableExtensions.length - 8} more</span>
          {/if}
        </div>
      {/if}

      {#if state.filterExtensions.length > 0 || state.filterMinSize > 0 || state.searchQuery}
        <button
          on:click={() => duplicatesStore.clearFilters()}
          class="text-xs text-gray-400 hover:text-white"
        >
          Clear filters
        </button>
      {/if}
    </div>
  {/if}

  <!-- Content -->
  <div class="flex-1 overflow-auto">
    <!-- EXACT DUPLICATES TAB -->
    {#if state.activeTab === 'exact'}
      {#if $filteredGroups.length === 0 && !state.loading}
        <div class="text-center py-16 text-gray-500">
          {#if totalFiles === 0}
            <svg class="w-16 h-16 mx-auto mb-4 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4" />
            </svg>
            <p>Scan files first to find duplicates</p>
          {:else if state.groups.length === 0}
            <svg class="w-16 h-16 mx-auto mb-4 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
            <p>Click "Find Exact Duplicates" to analyze</p>
          {:else}
            <p>No duplicates match your filters</p>
          {/if}
        </div>
      {:else}
        <div class="space-y-2">
          {#each $filteredGroups as group (group.id)}
            <div class="bg-gray-800/50 rounded-lg overflow-hidden">
              <!-- Group Header -->
              <button
                on:click={() => toggleGroup(group.id)}
                class="w-full p-3 flex items-center justify-between hover:bg-gray-800 transition-colors"
              >
                <div class="flex items-center gap-4">
                  <span class="text-white font-medium">{group.files.length} files</span>
                  <span class="text-gray-400">|</span>
                  <span class="text-gray-400">{formatBytes(group.size)} each</span>
                  <span class="text-gray-400">|</span>
                  <span class="text-red-400 font-medium">{formatBytes(group.wasted_space)} wasted</span>
                  {#if group.files[0]?.extension}
                    <span class="px-2 py-0.5 text-xs rounded {getExtColor(group.files[0].extension)}">
                      .{group.files[0].extension}
                    </span>
                  {/if}
                </div>
                <div class="flex items-center gap-2">
                  <button
                    on:click|stopPropagation={() => duplicatesStore.selectGroupDuplicates(group.id)}
                    class="px-2 py-1 text-xs bg-gray-700 hover:bg-gray-600 rounded text-gray-300"
                  >
                    Select All
                  </button>
                  <svg
                    class="w-5 h-5 text-gray-400 transition-transform {expandedGroups.has(group.id) ? 'rotate-180' : ''}"
                    fill="none" stroke="currentColor" viewBox="0 0 24 24"
                  >
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
                  </svg>
                </div>
              </button>

              <!-- Expanded Files -->
              {#if expandedGroups.has(group.id)}
                <div class="border-t border-gray-700 p-3 space-y-1">
                  {#each group.files as file, idx}
                    {@const selected = isFileSelected(file)}
                    {@const isImage = isImageFile(file.path)}
                    <div
                      class="flex items-center gap-3 p-2 rounded group {selected ? 'bg-red-500/10' : 'bg-gray-900/50'} hover:bg-gray-900/80"
                    >
                      <!-- Checkbox -->
                      <button
                        on:click={() => duplicatesStore.toggleFileSelection(file.id, file.path)}
                        class="w-5 h-5 rounded border flex items-center justify-center flex-shrink-0
                          {file.is_original ? 'border-gray-600 bg-gray-700 cursor-not-allowed' : selected ? 'border-red-500 bg-red-500' : 'border-gray-600 hover:border-gray-500'}"
                        disabled={file.is_original}
                        title={file.is_original ? 'Original file (best match)' : 'Select for deletion'}
                      >
                        {#if selected}
                          <svg class="w-3 h-3 text-white" fill="currentColor" viewBox="0 0 20 20">
                            <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
                          </svg>
                        {/if}
                      </button>

                      <!-- Original Badge -->
                      {#if file.is_original}
                        <span class="text-xs bg-green-600 px-1.5 py-0.5 rounded text-white flex-shrink-0">Original</span>
                      {/if}

                      <!-- Thumbnail for images -->
                      {#if isImage}
                        <button
                          on:click={() => previewImage = { path: file.path, name: file.name }}
                          class="w-10 h-10 rounded overflow-hidden flex-shrink-0 bg-gray-700 hover:ring-2 hover:ring-prism-500"
                        >
                          <img
                            src={getImageSrc(file.path)}
                            alt=""
                            class="w-full h-full object-cover"
                            loading="lazy"
                          />
                        </button>
                      {/if}

                      <!-- File info -->
                      <div class="flex-1 min-w-0">
                        <div class="truncate text-sm {file.is_original ? 'text-white' : 'text-gray-300'}" title={file.path}>
                          {file.path}
                        </div>
                        {#if file.modified_at}
                          <div class="text-xs text-gray-500">
                            Modified: {new Date(file.modified_at * 1000).toLocaleDateString()}
                          </div>
                        {/if}
                      </div>

                      <!-- Actions -->
                      <button
                        on:click={() => openInExplorer(file.path)}
                        class="p-1.5 text-gray-500 hover:text-white opacity-0 group-hover:opacity-100 rounded hover:bg-gray-700"
                        title="Open in Explorer"
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

    <!-- SIMILAR IMAGES TAB -->
    {:else if state.activeTab === 'similar'}
      {#if state.similarGroups.length === 0 && !state.similarLoading}
        <div class="text-center py-16 text-gray-500">
          <svg class="w-16 h-16 mx-auto mb-4 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z" />
          </svg>
          <p>Click "Find Similar Images" to analyze</p>
          <p class="text-sm mt-1 text-gray-600">Uses perceptual hashing to find visually similar images</p>
        </div>
      {:else}
        <div class="space-y-4">
          {#each state.similarGroups as group (group.id)}
            <div class="bg-gray-800/50 rounded-lg p-4">
              <div class="flex items-center justify-between mb-3">
                <span class="text-white font-medium">{group.images.length} similar images</span>
                <span class="text-xs text-gray-400">Threshold: {group.similarity_threshold}</span>
              </div>
              <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-2">
                {#each group.images as img, idx}
                  <button
                    on:click={() => previewImage = { path: img.path, name: img.name }}
                    class="relative aspect-square rounded-lg overflow-hidden bg-gray-700 hover:ring-2 hover:ring-prism-500 group"
                  >
                    <img
                      src={getImageSrc(img.path)}
                      alt={img.name}
                      class="w-full h-full object-cover"
                      loading="lazy"
                    />
                    {#if idx === 0}
                      <span class="absolute top-1 left-1 text-xs bg-green-600 px-1 rounded">Reference</span>
                    {:else if img.distance_from_first > 0}
                      <span class="absolute top-1 right-1 text-xs bg-gray-900/80 px-1 rounded">
                        {img.distance_from_first} bits
                      </span>
                    {/if}
                    <div class="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/80 to-transparent p-2 opacity-0 group-hover:opacity-100 transition-opacity">
                      <div class="text-xs text-white truncate">{img.name}</div>
                      <div class="text-xs text-gray-400">{formatBytes(img.size)}</div>
                    </div>
                  </button>
                {/each}
              </div>
            </div>
          {/each}
        </div>
      {/if}

    <!-- BY FOLDER TAB -->
    {:else if state.activeTab === 'byFolder'}
      {#if $folderDuplicates.length === 0}
        <div class="text-center py-16 text-gray-500">
          <p>No duplicate data available</p>
          <p class="text-sm mt-1 text-gray-600">Find exact duplicates first to see folder analysis</p>
        </div>
      {:else}
        <div class="space-y-2">
          {#each $folderDuplicates as folder}
            <div class="bg-gray-800/50 rounded-lg p-4 hover:bg-gray-800/70 transition-colors">
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-3 min-w-0">
                  <svg class="w-5 h-5 text-yellow-400 flex-shrink-0" fill="currentColor" viewBox="0 0 20 20">
                    <path d="M2 6a2 2 0 012-2h5l2 2h5a2 2 0 012 2v6a2 2 0 01-2 2H4a2 2 0 01-2-2V6z" />
                  </svg>
                  <div class="min-w-0">
                    <div class="text-white font-medium truncate" title={folder.path}>{folder.path}</div>
                    <div class="text-xs text-gray-500">{folder.duplicateCount} duplicates in {folder.groups.length} groups</div>
                  </div>
                </div>
                <div class="flex items-center gap-4">
                  <span class="text-red-400 font-medium">{formatBytes(folder.wastedSpace)} wasted</span>
                  <button
                    on:click={() => openInExplorer(folder.path)}
                    class="p-1.5 text-gray-400 hover:text-white rounded hover:bg-gray-700"
                    title="Open folder"
                  >
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                    </svg>
                  </button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>

<!-- Image Preview Modal -->
{#if previewImage}
  <div
    class="fixed inset-0 bg-black/90 z-50 flex items-center justify-center p-4"
    on:click={() => previewImage = null}
    on:keydown={(e) => e.key === 'Escape' && (previewImage = null)}
    role="dialog"
    tabindex="-1"
  >
    <div class="max-w-4xl max-h-full flex flex-col" on:click|stopPropagation>
      <div class="flex items-center justify-between mb-2">
        <span class="text-white font-medium truncate">{previewImage.name}</span>
        <button
          on:click={() => previewImage = null}
          class="p-2 text-gray-400 hover:text-white"
        >
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>
      <img
        src={getImageSrc(previewImage.path)}
        alt={previewImage.name}
        class="max-w-full max-h-[80vh] object-contain rounded-lg"
      />
      <div class="text-center mt-2 text-gray-400 text-sm">{previewImage.path}</div>
    </div>
  </div>
{/if}

<!-- Delete Confirmation Modal -->
{#if showDeleteModal}
  <div class="fixed inset-0 bg-black/70 z-50 flex items-center justify-center p-4">
    <div class="bg-gray-800 rounded-xl max-w-md w-full p-6">
      <h3 class="text-xl font-bold text-white mb-2">Delete {deleteConfirmFiles.length} files?</h3>
      <p class="text-gray-400 text-sm mb-4">This action cannot be undone. The files will be permanently deleted.</p>

      <div class="max-h-40 overflow-y-auto bg-gray-900/50 rounded-lg p-3 mb-4">
        {#each deleteConfirmFiles.slice(0, 5) as file}
          <div class="text-sm text-gray-300 truncate">{file.path}</div>
        {/each}
        {#if deleteConfirmFiles.length > 5}
          <div class="text-sm text-gray-500 mt-1">...and {deleteConfirmFiles.length - 5} more</div>
        {/if}
      </div>

      <div class="flex gap-3 justify-end">
        <button
          on:click={() => { showDeleteModal = false; deleteConfirmFiles = []; }}
          class="px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white rounded-lg"
          disabled={deleting}
        >
          Cancel
        </button>
        <button
          on:click={confirmDelete}
          disabled={deleting}
          class="px-4 py-2 bg-red-600 hover:bg-red-700 text-white rounded-lg flex items-center gap-2"
        >
          {#if deleting}
            <div class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
            Deleting...
          {:else}
            Delete Files
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}
