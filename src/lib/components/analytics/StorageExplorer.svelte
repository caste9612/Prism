<script lang="ts">
  import { onMount, afterUpdate, onDestroy } from 'svelte';
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import type { FolderItem, DriveStats } from '$lib/types';
  import { createEventDispatcher } from 'svelte';
  import * as d3 from 'd3';
  import type { HierarchyRectangularNode } from 'd3';

  export let folderContents: FolderItem[] = [];
  export let driveStats: DriveStats[] = [];
  export let treemapPath: string[] = [];
  export let loading = false;
  export let isScanning = false;

  const dispatch = createEventDispatcher<{
    navigate: { index: number };
    drillDown: { path: string; name: string };
  }>();

  // Debounce timer for rendering
  let renderTimeout: ReturnType<typeof setTimeout> | null = null;

  // Type for treemap data
  interface TreemapItem {
    name: string;
    size: number;
    path: string;
    is_folder: boolean;
    file_count: number;
    colorIndex?: number;
    children?: TreemapItem[];
  }

  // Color palette for treemap
  const colorScale = d3.scaleOrdinal([
    '#6366f1', '#8b5cf6', '#a855f7', '#d946ef', '#ec4899',
    '#f43f5e', '#ef4444', '#f97316', '#eab308', '#22c55e',
    '#14b8a6', '#06b6d4', '#0ea5e9', '#3b82f6'
  ]);

  let container: HTMLDivElement;
  let width = 800;
  let height = 400;

  // Get the data to display
  $: displayData = getDisplayData(folderContents, driveStats, treemapPath);
  $: totalSize = displayData.reduce((sum, d) => sum + d.size, 0);

  function getDisplayData(contents: FolderItem[], stats: DriveStats[], path: string[]): Array<{ name: string; size: number; path: string; is_folder: boolean; file_count: number }> {
    // If we have folder contents, use those
    if (contents.length > 0) {
      return contents.map(item => ({
        name: item.name,
        size: item.size,
        path: item.path,
        is_folder: item.is_folder,
        file_count: item.file_count
      }));
    }

    // At root level with no folder contents, use drive stats
    if (path.length === 0 && stats.length > 0) {
      return stats.map(drive => ({
        name: drive.name,
        size: drive.total_size,
        path: drive.path,
        is_folder: true,
        file_count: drive.file_count
      }));
    }

    return [];
  }

  function updateDimensions() {
    if (container) {
      const rect = container.getBoundingClientRect();
      width = rect.width || 800;
      height = Math.max(350, Math.min(500, width * 0.5));
    }
  }

  function cleanupD3() {
    if (!container) return;
    const svg = d3.select(container);
    // Remove all event listeners before removing elements
    svg.selectAll('*')
      .on('mouseover', null)
      .on('mouseout', null)
      .on('click', null);
    svg.selectAll('*').remove();
  }

  function renderTreemap() {
    if (!container || displayData.length === 0) return;

    // Clean up previous D3 content and event listeners
    cleanupD3();

    updateDimensions();

    // Create hierarchy data
    const hierarchyData: TreemapItem = {
      name: 'root',
      size: 0,
      path: '',
      is_folder: true,
      file_count: 0,
      children: displayData.map((d, i) => ({
        ...d,
        colorIndex: i
      }))
    };

    // Create treemap layout
    const root = d3.hierarchy<TreemapItem>(hierarchyData)
      .sum(d => d.children ? 0 : d.size)
      .sort((a, b) => (b.value || 0) - (a.value || 0));

    const treemapLayout = d3.treemap<TreemapItem>()
      .size([width, height])
      .paddingOuter(4)
      .paddingInner(2)
      .round(true);

    treemapLayout(root);

    // Type the leaves as rectangular nodes
    type TreemapNode = HierarchyRectangularNode<TreemapItem>;
    const leaves = root.leaves() as TreemapNode[];

    // Create SVG
    const svg = d3.select(container)
      .append('svg')
      .attr('width', width)
      .attr('height', height)
      .attr('viewBox', `0 0 ${width} ${height}`)
      .style('font-family', 'system-ui, sans-serif');

    // Create groups for each leaf
    const groups = svg.selectAll<SVGGElement, TreemapNode>('g')
      .data(leaves)
      .join('g')
      .attr('transform', d => `translate(${d.x0},${d.y0})`);

    // Add rectangles
    groups.append('rect')
      .attr('width', d => Math.max(0, d.x1 - d.x0))
      .attr('height', d => Math.max(0, d.y1 - d.y0))
      .attr('fill', d => colorScale(String(d.data.colorIndex ?? 0)))
      .attr('rx', 4)
      .attr('ry', 4)
      .style('cursor', d => d.data.is_folder ? 'pointer' : 'default')
      .style('transition', 'opacity 0.2s, transform 0.2s')
      .on('mouseover', function() {
        d3.select(this).style('opacity', 0.85);
      })
      .on('mouseout', function() {
        d3.select(this).style('opacity', 1);
      })
      .on('click', (event, d) => {
        if (d.data.is_folder && d.data.path) {
          dispatch('drillDown', { path: d.data.path, name: d.data.name });
        }
      });

    // Add hover overlay
    groups.append('rect')
      .attr('width', d => Math.max(0, d.x1 - d.x0))
      .attr('height', d => Math.max(0, d.y1 - d.y0))
      .attr('fill', 'white')
      .attr('opacity', 0)
      .attr('rx', 4)
      .attr('ry', 4)
      .style('pointer-events', 'none')
      .attr('class', 'hover-overlay');

    // Add clipPath for text
    groups.append('clipPath')
      .attr('id', (d, i) => `clip-${i}`)
      .append('rect')
      .attr('width', d => Math.max(0, d.x1 - d.x0 - 8))
      .attr('height', d => Math.max(0, d.y1 - d.y0 - 8));

    // Add folder/file icon
    groups.filter(d => (d.x1 - d.x0) > 50 && (d.y1 - d.y0) > 40)
      .append('text')
      .attr('x', 8)
      .attr('y', 18)
      .attr('fill', 'rgba(255,255,255,0.9)')
      .attr('font-size', '14px')
      .text(d => d.data.is_folder ? '\u{1F4C1}' : '\u{1F4C4}');

    // Add name label
    groups.filter(d => (d.x1 - d.x0) > 60 && (d.y1 - d.y0) > 50)
      .append('text')
      .attr('clip-path', (d, i) => `url(#clip-${i})`)
      .attr('x', 8)
      .attr('y', 36)
      .attr('fill', 'white')
      .attr('font-size', '13px')
      .attr('font-weight', '600')
      .text(d => d.data.name)
      .each(function(d) {
        // Truncate text if too long
        const textWidth = d.x1 - d.x0 - 16;
        const text = d3.select(this);
        let textContent = text.text();
        while (text.node()!.getComputedTextLength() > textWidth && textContent.length > 3) {
          textContent = textContent.slice(0, -4) + '...';
          text.text(textContent);
        }
      });

    // Add size label
    groups.filter(d => (d.x1 - d.x0) > 70 && (d.y1 - d.y0) > 65)
      .append('text')
      .attr('x', 8)
      .attr('y', 54)
      .attr('fill', 'rgba(255,255,255,0.95)')
      .attr('font-size', '15px')
      .attr('font-weight', '700')
      .text(d => formatBytes(d.data.size));

    // Add file count label
    groups.filter(d => (d.x1 - d.x0) > 80 && (d.y1 - d.y0) > 80)
      .append('text')
      .attr('x', 8)
      .attr('y', 72)
      .attr('fill', 'rgba(255,255,255,0.7)')
      .attr('font-size', '11px')
      .text(d => `${formatNumber(d.data.file_count)} files`);

    // Add percentage for larger blocks
    groups.filter(d => (d.x1 - d.x0) > 100 && (d.y1 - d.y0) > 95)
      .append('text')
      .attr('x', 8)
      .attr('y', 90)
      .attr('fill', 'rgba(255,255,255,0.5)')
      .attr('font-size', '11px')
      .text(d => {
        const pct = totalSize > 0 ? (d.data.size / totalSize * 100).toFixed(1) : 0;
        return `${pct}%`;
      });
  }

  // Debounced render to avoid excessive re-renders
  function scheduleRender() {
    if (renderTimeout) clearTimeout(renderTimeout);
    renderTimeout = setTimeout(() => {
      renderTreemap();
    }, 50);
  }

  onMount(() => {
    updateDimensions();
    window.addEventListener('resize', updateDimensions);
    return () => {
      window.removeEventListener('resize', updateDimensions);
    };
  });

  onDestroy(() => {
    // Clean up D3 and timers on component destroy
    if (renderTimeout) clearTimeout(renderTimeout);
    cleanupD3();
  });

  afterUpdate(() => {
    if (!loading && displayData.length > 0) {
      scheduleRender();
    }
  });

  // Re-render when data changes (debounced)
  $: if (displayData && container && !loading) {
    scheduleRender();
  }
</script>

<div class="bg-gray-800/50 rounded-lg p-4">
  <h3 class="text-white font-medium mb-4 flex items-center gap-2">
    <svg class="w-5 h-5 text-prism-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2 1 3 3 3h10c2 0 3-1 3-3V7c0-2-1-3-3-3H7C5 4 4 5 4 7z" />
    </svg>
    Storage Explorer
    {#if totalSize > 0}
      <span class="text-sm text-gray-400 font-normal">({formatBytes(totalSize)} total)</span>
    {/if}
  </h3>

  <!-- Breadcrumb Navigation -->
  <div class="flex items-center gap-2 mb-4 text-sm flex-wrap">
    <button
      class="px-2 py-1 rounded hover:bg-gray-700/50 text-prism-400 hover:text-prism-300 transition-colors flex items-center gap-1"
      on:click={() => dispatch('navigate', { index: -1 })}
    >
      <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" />
      </svg>
      All Drives
    </button>
    {#each treemapPath as segment, i}
      <span class="text-gray-500">/</span>
      <button
        class="px-2 py-1 rounded hover:bg-gray-700/50 text-prism-400 hover:text-prism-300 transition-colors"
        on:click={() => dispatch('navigate', { index: i })}
      >
        {segment}
      </button>
    {/each}
    {#if loading}
      <div class="w-4 h-4 border-2 border-prism-400 border-t-transparent rounded-full animate-spin ml-2"></div>
    {/if}
  </div>

  <!-- Treemap Container -->
  {#if loading}
    <div class="text-gray-500 text-center py-16">
      <div class="w-8 h-8 border-4 border-prism-500 border-t-transparent rounded-full animate-spin mx-auto mb-4"></div>
      Loading folder contents...
    </div>
  {:else if displayData.length > 0}
    <div bind:this={container} class="w-full rounded-lg overflow-hidden" style="min-height: 350px;"></div>
  {:else if isScanning}
    <div class="text-gray-500 text-center py-16">
      <div class="w-8 h-8 border-4 border-prism-500 border-t-transparent rounded-full animate-spin mx-auto mb-4"></div>
      Scanning in progress...
    </div>
  {:else if treemapPath.length > 0}
    <div class="text-gray-500 text-center py-16">
      <svg class="w-12 h-12 mx-auto mb-4 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
      </svg>
      <p>No subfolders found in this directory</p>
      <p class="text-sm text-gray-600 mt-1">Current path: {treemapPath.join(' / ')}</p>
    </div>
  {:else}
    <div class="text-gray-500 text-center py-16">
      <svg class="w-16 h-16 mx-auto mb-4 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4" />
      </svg>
      <p>No folder data available</p>
      <p class="text-sm text-gray-600 mt-2">Run a scan to analyze your storage</p>
    </div>
  {/if}

  <!-- Legend hint -->
  {#if displayData.length > 0 && !loading}
    <div class="mt-3 flex items-center justify-between text-xs text-gray-500">
      <span>Click on folders to drill down</span>
      <span>Rectangle size = space used</span>
    </div>
  {/if}
</div>
