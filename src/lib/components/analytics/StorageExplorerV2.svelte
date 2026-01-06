<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { formatBytes } from '$lib/utils/format';
  import type { DriveStats } from '$lib/types';

  // Types
  interface TreemapNode {
    path: string;
    name: string;
    size: number;
    file_count: number;
    children?: TreemapNode[];
  }

  interface DriveInfo {
    path: string;
    name: string;
    drive_type: string;
    total_space: number;
    free_space: number;
    is_ready: boolean;
  }

  // Props
  export let driveStats: DriveStats[] = [];
  export let isScanning: boolean = false;

  // State
  let selectedDrive: string | null = null;
  let treemapData: TreemapNode[] = [];
  let driveInfo: DriveInfo[] = [];
  let loading = false;
  let error: string | null = null;
  let breadcrumb: { path: string; name: string }[] = [];
  let currentPath: string | null = null;

  // Cache
  let cache = new Map<string, TreemapNode[]>();

  // Container
  let container: HTMLDivElement;
  let containerWidth = 0;
  let containerHeight = 0;

  // Cleanup
  let unlistenTreemapReady: (() => void) | null = null;
  let resizeObserver: ResizeObserver | null = null;

  onMount(() => {
    listen('treemap-ready', () => {
      cache.clear();
      loadTreemapData();
    }).then(unlisten => {
      unlistenTreemapReady = unlisten;
    });

    // Load drive info for All Drives view
    loadDriveInfo();
    loadTreemapData();

    resizeObserver = new ResizeObserver(entries => {
      for (const entry of entries) {
        containerWidth = entry.contentRect.width;
        containerHeight = entry.contentRect.height;
      }
    });

    if (container) {
      resizeObserver.observe(container);
    }
  });

  onDestroy(() => {
    if (unlistenTreemapReady) unlistenTreemapReady();
    if (resizeObserver) resizeObserver.disconnect();
  });

  async function loadDriveInfo() {
    try {
      driveInfo = await invoke<DriveInfo[]>('get_available_drives');
    } catch (e) {
      console.error('Failed to load drive info:', e);
    }
  }

  async function loadTreemapData(path?: string) {
    const cacheKey = `${selectedDrive || 'all'}:${path || 'root'}`;

    // Show cached data immediately if available (even while loading fresh data)
    if (cache.has(cacheKey)) {
      treemapData = cache.get(cacheKey)!;
      // Don't return - still fetch fresh data in background if scanning
      if (!isScanning) return;
    }

    loading = !cache.has(cacheKey); // Only show loading if no cached data
    error = null;

    try {
      const data = await invoke<TreemapNode[]>('get_treemap_data', {
        request: {
          drive: selectedDrive,
          path: path || null,
          max_depth: 4,
          min_size: 10 * 1024 * 1024, // 10MB minimum
        }
      });
      treemapData = data;
      cache.set(cacheKey, data);
    } catch (e) {
      if (!cache.has(cacheKey)) {
        error = `Failed to load: ${e}`;
        treemapData = [];
      }
    } finally {
      loading = false;
    }
  }

  function selectDrive(drivePath: string | null) {
    selectedDrive = drivePath;
    breadcrumb = [];
    currentPath = null;
    loadTreemapData();
  }

  function drillDown(node: TreemapNode) {
    breadcrumb = [...breadcrumb, { path: node.path, name: node.name }];
    currentPath = node.path;
    if (node.children && node.children.length > 0) {
      treemapData = node.children;
    } else {
      loadTreemapData(node.path);
    }
  }

  function navigateTo(index: number) {
    if (index < 0) {
      breadcrumb = [];
      currentPath = null;
      loadTreemapData();
    } else {
      breadcrumb = breadcrumb.slice(0, index + 1);
      currentPath = breadcrumb[index].path;
      loadTreemapData(currentPath);
    }
  }

  // Open folder in Windows Explorer
  async function openInExplorer(path: string) {
    try {
      await invoke('open_in_explorer', { path });
    } catch (e) {
      console.error('Failed to open folder:', e);
    }
  }

  // Color palette for treemap
  const colors = [
    '#3b82f6', '#8b5cf6', '#06b6d4', '#10b981', '#f59e0b', '#ef4444',
    '#ec4899', '#6366f1', '#14b8a6', '#84cc16', '#f97316', '#e11d48'
  ];

  function getColor(index: number, depth: number): string {
    const baseColor = colors[index % colors.length];
    // Lighten for deeper levels
    const lighten = depth * 15;
    return adjustBrightness(baseColor, lighten);
  }

  function adjustBrightness(hex: string, percent: number): string {
    const num = parseInt(hex.replace('#', ''), 16);
    const r = Math.min(255, ((num >> 16) & 255) + percent);
    const g = Math.min(255, ((num >> 8) & 255) + percent);
    const b = Math.min(255, (num & 255) + percent);
    return `rgb(${r}, ${g}, ${b})`;
  }

  // Recursive treemap layout with squarified algorithm
  interface LayoutRect {
    node: TreemapNode;
    x: number;
    y: number;
    width: number;
    height: number;
    depth: number;
    colorIndex: number;
  }

  function layoutTreemap(
    nodes: TreemapNode[],
    x: number,
    y: number,
    width: number,
    height: number,
    depth: number = 0,
    colorIndex: number = 0
  ): LayoutRect[] {
    if (!nodes || nodes.length === 0 || width < 10 || height < 10) return [];

    const total = nodes.reduce((sum, n) => sum + n.size, 0);
    if (total === 0) return [];

    const result: LayoutRect[] = [];
    const sorted = [...nodes].sort((a, b) => b.size - a.size);

    // Squarified treemap algorithm
    let currentX = x;
    let currentY = y;
    let remainingWidth = width;
    let remainingHeight = height;

    for (let i = 0; i < sorted.length; i++) {
      const node = sorted[i];
      const ratio = node.size / total;
      const isHorizontal = remainingWidth >= remainingHeight;

      let rectWidth: number, rectHeight: number;
      if (isHorizontal) {
        rectWidth = remainingWidth * ratio;
        rectHeight = remainingHeight;
        // Pack multiple small items in a row
        if (i < sorted.length - 1 && rectWidth < 60) {
          // Group remaining small items
          let groupSize = node.size;
          let groupCount = 1;
          for (let j = i + 1; j < sorted.length; j++) {
            groupSize += sorted[j].size;
            groupCount++;
          }
          rectWidth = remainingWidth * (groupSize / total) / groupCount * 1.5;
        }
      } else {
        rectWidth = remainingWidth;
        rectHeight = remainingHeight * ratio;
      }

      // Ensure minimum size
      rectWidth = Math.max(10, Math.min(rectWidth, remainingWidth));
      rectHeight = Math.max(10, Math.min(rectHeight, remainingHeight));

      const rect: LayoutRect = {
        node,
        x: currentX,
        y: currentY,
        width: rectWidth,
        height: rectHeight,
        depth,
        colorIndex: colorIndex + i,
      };
      result.push(rect);

      // Layout children recursively inside this rect (with padding)
      if (node.children && node.children.length > 0 && rectWidth > 50 && rectHeight > 40) {
        const padding = 2;
        const headerHeight = 18;
        const childRects = layoutTreemap(
          node.children,
          currentX + padding,
          currentY + headerHeight,
          rectWidth - padding * 2,
          rectHeight - headerHeight - padding,
          depth + 1,
          colorIndex + i
        );
        result.push(...childRects);
      }

      // Move to next position
      if (isHorizontal) {
        currentX += rectWidth;
        remainingWidth -= rectWidth;
      } else {
        currentY += rectHeight;
        remainingHeight -= rectHeight;
      }
    }

    return result;
  }

  // Compute layout reactively
  $: layout = selectedDrive !== null ? layoutTreemap(treemapData, 0, 0, containerWidth, containerHeight) : [];
  $: sortedDrives = [...driveStats].sort((a, b) => b.total_size - a.total_size);

  // Get drive info by path
  function getDriveInfoByPath(path: string): DriveInfo | undefined {
    return driveInfo.find(d => d.path.toUpperCase() === path.toUpperCase());
  }
</script>

<div class="h-full flex flex-col bg-gray-900/50 rounded-lg overflow-hidden">
  <!-- Header with drive selector -->
  <div class="flex items-center gap-2 px-4 py-3 bg-gray-800/50 border-b border-gray-700 flex-wrap">
    <span class="text-gray-400 text-sm mr-2">Drive:</span>

    <button
      on:click={() => selectDrive(null)}
      class="px-3 py-1.5 rounded text-sm font-medium transition-colors
        {selectedDrive === null ? 'bg-prism-500 text-white' : 'bg-gray-700 text-gray-300 hover:bg-gray-600'}"
    >
      All Drives
    </button>

    {#each sortedDrives as drive}
      <button
        on:click={() => selectDrive(drive.path)}
        class="px-3 py-1.5 rounded text-sm font-medium transition-colors flex items-center gap-2
          {selectedDrive === drive.path ? 'bg-prism-500 text-white' : 'bg-gray-700 text-gray-300 hover:bg-gray-600'}"
        title="{drive.name} - {formatBytes(drive.total_size)} indexed"
      >
        <span>{drive.path.replace('\\', '')}</span>
        <span class="text-xs opacity-75">({formatBytes(drive.total_size)})</span>
      </button>
    {/each}

    <button
      on:click={() => { cache.clear(); loadTreemapData(currentPath || undefined); }}
      disabled={loading || isScanning}
      class="ml-auto p-1.5 rounded text-gray-400 hover:text-white hover:bg-gray-700 disabled:opacity-50"
      title="Refresh"
    >
      <svg class="w-4 h-4 {loading ? 'animate-spin' : ''}" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
      </svg>
    </button>
  </div>

  <!-- Breadcrumb -->
  {#if breadcrumb.length > 0 && selectedDrive !== null}
    <div class="flex items-center gap-1 px-4 py-2 bg-gray-800/30 border-b border-gray-700/50 text-sm overflow-x-auto">
      <button on:click={() => navigateTo(-1)} class="text-prism-400 hover:text-prism-300 whitespace-nowrap">
        {selectedDrive ? selectedDrive.replace('\\', '') : 'Root'}
      </button>
      {#each breadcrumb as crumb, i}
        <span class="text-gray-600">/</span>
        <button on:click={() => navigateTo(i)} class="text-prism-400 hover:text-prism-300 whitespace-nowrap truncate max-w-[200px]" title={crumb.path}>
          {crumb.name}
        </button>
      {/each}
    </div>
  {/if}

  <!-- Main content area -->
  <div bind:this={container} class="flex-1 relative min-h-[300px] overflow-hidden">
    {#if loading && treemapData.length === 0}
      <div class="absolute inset-0 flex items-center justify-center bg-gray-900/50 z-10">
        <div class="flex flex-col items-center gap-2">
          <svg class="animate-spin w-8 h-8 text-prism-500" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
          </svg>
          <span class="text-gray-400 text-sm">Loading treemap...</span>
        </div>
      </div>
    {/if}

    {#if error}
      <div class="absolute inset-0 flex items-center justify-center">
        <div class="text-center">
          <p class="text-red-400 mb-2">{error}</p>
          <button on:click={() => loadTreemapData(currentPath || undefined)} class="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded text-white text-sm">
            Retry
          </button>
        </div>
      </div>
    {:else if selectedDrive === null}
      <!-- ALL DRIVES OVERVIEW - Vertical Cards -->
      <div class="p-4 overflow-y-auto h-full">
        <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-3">
          {#each sortedDrives as drive}
            {@const info = getDriveInfoByPath(drive.path)}
            {@const totalSpace = info?.total_space || 0}
            {@const freeSpace = info?.free_space || 0}
            {@const usedSpace = totalSpace - freeSpace}
            {@const usedPercent = totalSpace > 0 ? (usedSpace / totalSpace) * 100 : 0}
            <button
              on:click={() => selectDrive(drive.path)}
              on:contextmenu|preventDefault={() => openInExplorer(drive.path)}
              class="bg-gray-800/60 rounded-xl p-3 hover:bg-gray-700/80 transition-all hover:scale-[1.02] text-left flex flex-col group border border-gray-700/50 hover:border-prism-500/50"
            >
              <!-- Drive Icon & Letter -->
              <div class="flex items-center gap-2 mb-2">
                <div class="w-8 h-8 rounded-lg flex items-center justify-center {
                  info?.drive_type === 'Network Drive' ? 'bg-blue-500/20' :
                  info?.drive_type === 'Removable Disk' ? 'bg-orange-500/20' : 'bg-prism-500/20'
                }">
                  {#if info?.drive_type === 'Network Drive'}
                    <svg class="w-4 h-4 text-blue-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9m-9 9a9 9 0 019-9" />
                    </svg>
                  {:else}
                    <svg class="w-4 h-4 text-prism-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4" />
                    </svg>
                  {/if}
                </div>
                <div class="flex-1 min-w-0">
                  <div class="text-white font-semibold text-sm">{drive.path.replace('\\', '')}</div>
                  <div class="text-gray-500 text-[10px] truncate">{drive.name || info?.drive_type || 'Local Disk'}</div>
                </div>
              </div>

              <!-- Vertical Usage Bar -->
              <div class="flex-1 flex items-end gap-2 min-h-[60px]">
                <div class="w-4 h-full bg-gray-700/50 rounded-full overflow-hidden flex flex-col-reverse">
                  <div
                    class="w-full transition-all duration-500 rounded-full"
                    style="height: {usedPercent}%; background: {usedPercent > 90 ? '#ef4444' : usedPercent > 75 ? '#f59e0b' : '#10b981'}"
                  ></div>
                </div>
                <div class="flex-1 flex flex-col justify-end">
                  <div class="text-[11px] text-gray-400 leading-tight">
                    <span class="text-white font-medium">{formatBytes(usedSpace)}</span> used
                  </div>
                  <div class="text-[10px] text-gray-500 leading-tight mt-0.5">
                    {formatBytes(freeSpace)} free
                  </div>
                  <div class="text-[10px] text-gray-600 leading-tight">
                    {formatBytes(totalSpace)} total
                  </div>
                </div>
              </div>

              <!-- Usage Percentage -->
              <div class="mt-2 pt-2 border-t border-gray-700/50 flex items-center justify-between">
                <span class="text-[10px] {usedPercent > 90 ? 'text-red-400' : usedPercent > 75 ? 'text-yellow-400' : 'text-green-400'} font-medium">
                  {usedPercent.toFixed(0)}% used
                </span>
                <span class="text-[9px] text-gray-500">{drive.file_count.toLocaleString()} files</span>
              </div>
            </button>
          {/each}
        </div>

        {#if sortedDrives.length === 0}
          <div class="text-center text-gray-500 py-12">
            <svg class="w-12 h-12 mx-auto mb-3 opacity-30" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4" />
            </svg>
            <p class="text-sm">No drives scanned yet</p>
            <p class="text-xs mt-1 text-gray-600">Run a scan to see disk usage</p>
          </div>
        {/if}
      </div>
    {:else if layout.length === 0 && !loading}
      <div class="absolute inset-0 flex items-center justify-center">
        <div class="text-center text-gray-500">
          <svg class="w-16 h-16 mx-auto mb-4 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
          </svg>
          <p>No folder data available</p>
          <p class="text-sm mt-1">Run a scan to analyze disk usage</p>
        </div>
      </div>
    {:else}
      <!-- TREEMAP VIEW -->
      <svg width={containerWidth} height={containerHeight} class="absolute inset-0">
        {#each layout as rect (rect.node.path + rect.depth)}
          <g
            class="cursor-pointer"
            on:click|stopPropagation={() => drillDown(rect.node)}
            on:contextmenu|preventDefault|stopPropagation={() => openInExplorer(rect.node.path)}
            on:keypress={(e) => e.key === 'Enter' && drillDown(rect.node)}
            role="button"
            tabindex="0"
          >
            <rect
              x={rect.x + 1}
              y={rect.y + 1}
              width={Math.max(0, rect.width - 2)}
              height={Math.max(0, rect.height - 2)}
              fill={getColor(rect.colorIndex, rect.depth)}
              rx="2"
              stroke={rect.depth === 0 ? '#1f2937' : 'none'}
              stroke-width={rect.depth === 0 ? 2 : 0}
              class="transition-opacity hover:opacity-90"
            />

            {#if rect.width > 50 && rect.height > 20}
              <text
                x={rect.x + 4}
                y={rect.y + 14}
                class="fill-white text-xs font-medium pointer-events-none"
                style="text-shadow: 0 1px 2px rgba(0,0,0,0.8)"
              >
                {rect.node.name.length > Math.floor(rect.width / 7) - 2
                  ? rect.node.name.slice(0, Math.floor(rect.width / 7) - 2) + '...'
                  : rect.node.name}
              </text>
            {/if}

            {#if rect.width > 60 && rect.height > 35 && rect.depth === 0}
              <text
                x={rect.x + 4}
                y={rect.y + 28}
                class="fill-white/70 text-[10px] pointer-events-none"
              >
                {formatBytes(rect.node.size)}
              </text>
            {/if}
          </g>
        {/each}
      </svg>
    {/if}
  </div>

  <!-- Footer -->
  <div class="px-4 py-2 bg-gray-800/30 border-t border-gray-700/50 text-xs text-gray-500 flex items-center justify-between">
    <span>
      {#if selectedDrive === null}
        {sortedDrives.length} drives
      {:else}
        {treemapData.length} folders
      {/if}
    </span>
    <span class="flex items-center gap-3">
      <span class="flex items-center gap-1">
        <kbd class="px-1 py-0.5 bg-gray-700 rounded text-[10px]">Click</kbd>
        <span>{selectedDrive !== null ? 'drill down' : 'explore'}</span>
      </span>
      <span class="flex items-center gap-1">
        <kbd class="px-1 py-0.5 bg-gray-700 rounded text-[10px]">Right-click</kbd>
        <span>open in Explorer</span>
      </span>
    </span>
  </div>
</div>
