<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import type { TreeNode } from '$lib/types';
  import Skeleton from '$lib/components/common/Skeleton.svelte';
  import TreeNodeItem from './TreeNodeItem.svelte';

  export let maxDepth = 3;
  export let rootPath: string | null = null;

  let tree: TreeNode[] = [];
  let loading = true;
  let error: string | null = null;
  let expandedPaths = new Set<string>();

  async function loadTree() {
    loading = true;
    error = null;
    try {
      tree = await invoke<TreeNode[]>('get_directory_tree', {
        rootPath,
        maxDepth
      });
      // Pre-expand all nodes up to maxDepth
      expandAll(tree, 0);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function expandAll(nodes: TreeNode[], depth: number) {
    if (depth >= maxDepth) return;
    for (const node of nodes) {
      if (node.children.length > 0) {
        expandedPaths.add(node.path);
        expandAll(node.children, depth + 1);
      }
    }
    expandedPaths = expandedPaths; // Trigger reactivity
  }

  function handleToggle(event: CustomEvent<{ node: TreeNode }>) {
    const { node } = event.detail;
    if (expandedPaths.has(node.path)) {
      expandedPaths.delete(node.path);
    } else {
      expandedPaths.add(node.path);
    }
    expandedPaths = expandedPaths; // Trigger reactivity
  }

  function calculateMaxSize(nodes: TreeNode[]): number {
    if (nodes.length === 0) return 1;
    return Math.max(...nodes.map(n => n.size));
  }

  onMount(() => {
    loadTree();
  });
</script>

<div class="tree-explorer h-full flex flex-col">
  <div class="flex items-center justify-between mb-4 px-2">
    <h3 class="text-lg font-semibold text-gray-200">File System Tree</h3>
    <button
      on:click={loadTree}
      disabled={loading}
      class="px-3 py-1 text-sm bg-indigo-600 hover:bg-indigo-500 rounded transition-colors disabled:opacity-50"
    >
      {loading ? 'Loading...' : 'Refresh'}
    </button>
  </div>

  {#if loading}
    <div class="flex-1 overflow-hidden space-y-2 p-2">
      {#each Array(10) as _, i}
        <div class="flex items-center gap-2" style="padding-left: {(i % 3) * 20}px">
          <Skeleton width="16px" height="16px" rounded="sm" />
          <Skeleton width="{200 - (i % 3) * 40}px" height="20px" />
          <Skeleton width="60px" height="16px" />
        </div>
      {/each}
    </div>
  {:else if error}
    <div class="flex-1 flex items-center justify-center">
      <div class="text-center text-red-400 p-4">
        <p class="font-medium">Failed to load tree</p>
        <p class="text-sm opacity-75">{error}</p>
        <button
          on:click={loadTree}
          class="mt-2 px-3 py-1 text-sm bg-red-600/50 hover:bg-red-500/50 rounded"
        >
          Retry
        </button>
      </div>
    </div>
  {:else if tree.length === 0}
    <div class="flex-1 flex items-center justify-center text-gray-500">
      <p>No files indexed. Run a scan first.</p>
    </div>
  {:else}
    <div class="flex-1 overflow-auto font-mono text-sm">
      {#each tree as rootNode}
        <TreeNodeItem
          node={rootNode}
          {expandedPaths}
          maxSize={calculateMaxSize(tree)}
          indentLevel={0}
          on:toggle={handleToggle}
        />
      {/each}
    </div>
  {/if}
</div>

<style>
  .tree-explorer {
    background: rgba(0, 0, 0, 0.2);
    border-radius: 0.5rem;
    padding: 1rem;
  }
</style>
