<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount, onDestroy } from 'svelte';

  interface SearchResult {
    id: number;
    path: string;
    name: string;
    extension: string | null;
    size: number;
    modified_at: number | null;
  }

  let query = '';
  let results: SearchResult[] = [];
  let totalCount = 0;
  let selectedIndex = 0;
  let loading = false;
  let searchInput: HTMLInputElement;
  let searchTimeout: ReturnType<typeof setTimeout> | null = null;

  // Debounced search - 50ms for instant feel
  $: if (query.length >= 1) {
    if (searchTimeout) clearTimeout(searchTimeout);
    searchTimeout = setTimeout(() => performSearch(), 50);
  } else {
    results = [];
    totalCount = 0;
  }

  async function performSearch() {
    if (query.length < 1) return;
    loading = true;
    try {
      const response = await invoke<{ results: SearchResult[]; total: number }>('quick_search', {
        query,
        limit: 100
      });
      results = response.results;
      totalCount = response.total;
      selectedIndex = 0;
    } catch (e) {
      console.error('Search error:', e);
    } finally {
      loading = false;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  }

  function formatDate(timestamp: number | null): string {
    if (!timestamp) return '';
    return new Date(timestamp * 1000).toLocaleDateString();
  }

  function getFileIcon(ext: string | null): string {
    if (!ext) return 'bg-gray-500';
    const e = ext.toLowerCase();
    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp'].includes(e)) return 'bg-pink-500';
    if (['mp4', 'mkv', 'avi', 'mov', 'wmv', 'flv'].includes(e)) return 'bg-purple-500';
    if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'wma'].includes(e)) return 'bg-green-500';
    if (['pdf'].includes(e)) return 'bg-red-500';
    if (['doc', 'docx', 'odt', 'rtf'].includes(e)) return 'bg-blue-500';
    if (['xls', 'xlsx', 'csv'].includes(e)) return 'bg-emerald-500';
    if (['zip', 'rar', '7z', 'tar', 'gz'].includes(e)) return 'bg-yellow-500';
    if (['exe', 'msi', 'bat', 'cmd'].includes(e)) return 'bg-orange-500';
    if (['js', 'ts', 'py', 'rs', 'go', 'java', 'c', 'cpp', 'h'].includes(e)) return 'bg-cyan-500';
    return 'bg-gray-500';
  }

  async function openFile(result: SearchResult) {
    try {
      await invoke('open_in_explorer', { path: result.path });
    } catch (e) {
      console.error('Failed to open:', e);
    }
  }

  async function copyPath(path: string) {
    await navigator.clipboard.writeText(path);
  }

  async function closeWindow() {
    try {
      await invoke('close_search_window');
    } catch (e) {
      console.error('Failed to close window:', e);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        selectedIndex = Math.min(selectedIndex + 1, results.length - 1);
        scrollToSelected();
        break;
      case 'ArrowUp':
        event.preventDefault();
        selectedIndex = Math.max(selectedIndex - 1, 0);
        scrollToSelected();
        break;
      case 'Enter':
        event.preventDefault();
        if (results[selectedIndex]) {
          openFile(results[selectedIndex]);
        }
        break;
      case 'Escape':
        event.preventDefault();
        closeWindow();
        break;
      case 'c':
        if (event.ctrlKey && results[selectedIndex]) {
          event.preventDefault();
          copyPath(results[selectedIndex].path);
        }
        break;
    }
  }

  function scrollToSelected() {
    const element = document.querySelector(`[data-index="${selectedIndex}"]`);
    element?.scrollIntoView({ block: 'nearest' });
  }

  onMount(() => {
    // Focus input on mount
    searchInput?.focus();

    // Listen for window focus to refocus input
    const unlisten = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        searchInput?.focus();
        searchInput?.select();
      }
    });

    return () => {
      unlisten.then(fn => fn());
    };
  });

  onDestroy(() => {
    if (searchTimeout) clearTimeout(searchTimeout);
  });
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="h-screen bg-gray-900 flex flex-col overflow-hidden select-none" data-tauri-drag-region>
  <!-- Search Header -->
  <div class="p-3 bg-gray-800 border-b border-gray-700">
    <div class="relative">
      <svg class="absolute left-3 top-1/2 -translate-y-1/2 w-5 h-5 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
      </svg>
      <input
        bind:this={searchInput}
        bind:value={query}
        type="text"
        placeholder="Search files... (Esc to close)"
        class="w-full pl-10 pr-4 py-3 bg-gray-900 border border-gray-600 rounded-lg text-white text-lg placeholder-gray-500 focus:outline-none focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500"
        autocomplete="off"
        spellcheck="false"
      />
      {#if loading}
        <div class="absolute right-3 top-1/2 -translate-y-1/2">
          <div class="w-5 h-5 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin"></div>
        </div>
      {/if}
    </div>
    {#if query.length >= 1}
      <div class="mt-2 flex items-center justify-between text-xs text-gray-400">
        <span>{totalCount.toLocaleString()} results</span>
        <span class="flex items-center gap-3">
          <span><kbd class="px-1 py-0.5 bg-gray-700 rounded text-[10px]">↑↓</kbd> navigate</span>
          <span><kbd class="px-1 py-0.5 bg-gray-700 rounded text-[10px]">Enter</kbd> open</span>
          <span><kbd class="px-1 py-0.5 bg-gray-700 rounded text-[10px]">Ctrl+C</kbd> copy</span>
          <span><kbd class="px-1 py-0.5 bg-gray-700 rounded text-[10px]">Esc</kbd> close</span>
        </span>
      </div>
    {/if}
  </div>

  <!-- Results List -->
  <div class="flex-1 overflow-y-auto">
    {#if results.length === 0 && query.length >= 1 && !loading}
      <div class="flex flex-col items-center justify-center h-full text-gray-500">
        <svg class="w-16 h-16 mb-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1" d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <p>No files found for "{query}"</p>
      </div>
    {:else if results.length === 0}
      <div class="flex flex-col items-center justify-center h-full text-gray-500">
        <svg class="w-20 h-20 mb-4 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
        </svg>
        <p class="text-lg">Start typing to search</p>
        <p class="text-sm mt-1">Search across all indexed files instantly</p>
      </div>
    {:else}
      {#each results as result, index (result.id)}
        <button
          data-index={index}
          on:click={() => openFile(result)}
          on:dblclick={() => openFile(result)}
          class="w-full flex items-center gap-3 px-4 py-2 hover:bg-gray-800 transition-colors text-left group
            {selectedIndex === index ? 'bg-indigo-600/20 border-l-2 border-indigo-500' : 'border-l-2 border-transparent'}"
        >
          <!-- File Icon -->
          <div class="w-8 h-8 rounded flex items-center justify-center flex-shrink-0 {getFileIcon(result.extension)}">
            <span class="text-white text-xs font-bold uppercase">
              {result.extension ? result.extension.slice(0, 3) : '?'}
            </span>
          </div>

          <!-- File Info -->
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2">
              <span class="text-white font-medium truncate">{result.name}</span>
            </div>
            <div class="text-xs text-gray-500 truncate">{result.path}</div>
          </div>

          <!-- Metadata -->
          <div class="flex items-center gap-4 flex-shrink-0 text-xs text-gray-400">
            <span class="w-16 text-right">{formatBytes(result.size)}</span>
            <span class="w-20 text-right">{formatDate(result.modified_at)}</span>
          </div>

          <!-- Actions (visible on hover/select) -->
          <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 {selectedIndex === index ? 'opacity-100' : ''}">
            <button
              on:click|stopPropagation={() => copyPath(result.path)}
              class="p-1.5 hover:bg-gray-700 rounded text-gray-400 hover:text-white"
              title="Copy path (Ctrl+C)"
            >
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
              </svg>
            </button>
          </div>
        </button>
      {/each}
    {/if}
  </div>

  <!-- Footer -->
  <div class="px-4 py-2 bg-gray-800 border-t border-gray-700 text-xs text-gray-500 flex items-center justify-between">
    <span>Prism Quick Search</span>
    <span>Press Esc to close</span>
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    overflow: hidden;
  }
</style>
