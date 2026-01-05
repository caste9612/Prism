<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import type { TreeNode } from '$lib/types';

  export let node: TreeNode;
  export let expandedPaths: Set<string>;
  export let maxSize: number;
  export let indentLevel: number = 0;

  const dispatch = createEventDispatcher<{
    toggle: { node: TreeNode };
  }>();

  // Icon mappings based on node type
  const icons = {
    drive: '💾',
    folder: '📁',
    folderOpen: '📂',
    file: '📄'
  };

  // Size bar colors based on relative size
  function getSizeColor(percent: number): string {
    if (percent > 50) return 'bg-red-500';
    if (percent > 25) return 'bg-orange-500';
    if (percent > 10) return 'bg-yellow-500';
    return 'bg-blue-500';
  }

  $: isExpanded = expandedPaths.has(node.path);
  $: hasChildren = node.children.length > 0 || node.nodeType !== 'file';
  $: sizePercent = (node.size / maxSize) * 100;
  $: indent = indentLevel * 20;

  function handleClick() {
    if (node.nodeType !== 'file') {
      dispatch('toggle', { node });
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleClick();
    }
  }

  function calculateChildMaxSize(children: TreeNode[]): number {
    if (children.length === 0) return 1;
    return Math.max(...children.map(n => n.size));
  }

  function forwardToggle(event: CustomEvent<{ node: TreeNode }>) {
    dispatch('toggle', event.detail);
  }
</script>

<div class="tree-node">
  <!-- Node row -->
  <div
    class="flex items-center py-1 px-2 hover:bg-gray-800/50 cursor-pointer rounded"
    style="padding-left: {indent + 8}px"
    on:click={handleClick}
    on:keydown={handleKeydown}
    role="button"
    tabindex="0"
  >
    <!-- Expand/collapse toggle -->
    <span class="w-4 mr-1 text-gray-500 flex-shrink-0">
      {#if hasChildren}
        <span class="inline-block transition-transform" class:rotate-90={isExpanded}>
          ▶
        </span>
      {/if}
    </span>

    <!-- Icon -->
    <span class="mr-2 flex-shrink-0">
      {#if node.nodeType === 'drive'}
        {icons.drive}
      {:else if node.nodeType === 'folder'}
        {isExpanded ? icons.folderOpen : icons.folder}
      {:else}
        {icons.file}
      {/if}
    </span>

    <!-- Name -->
    <span class="flex-1 truncate text-gray-200" title={node.path}>
      {node.name}
    </span>

    <!-- File count badge -->
    <span class="text-xs text-gray-500 mr-3 flex-shrink-0">
      {formatNumber(node.fileCount)} files
    </span>

    <!-- Size -->
    <span class="w-24 text-right text-gray-400 text-xs flex-shrink-0">
      {formatBytes(node.size)}
    </span>

    <!-- Size bar -->
    <div class="w-20 h-2 bg-gray-700 rounded ml-2 overflow-hidden flex-shrink-0">
      <div
        class="h-full {getSizeColor(sizePercent)} transition-all"
        style="width: {Math.max(1, sizePercent)}%"
      ></div>
    </div>
  </div>

  <!-- Children (recursive) -->
  {#if isExpanded && node.children.length > 0}
    {#each node.children as child (child.path)}
      <svelte:self
        node={child}
        {expandedPaths}
        maxSize={calculateChildMaxSize(node.children)}
        indentLevel={indentLevel + 1}
        on:toggle={forwardToggle}
      />
    {/each}
  {/if}
</div>

<style>
  .tree-node {
    user-select: none;
  }

  .rotate-90 {
    transform: rotate(90deg);
  }
</style>
