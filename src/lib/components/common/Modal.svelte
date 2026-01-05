<script lang="ts">
  import { createEventDispatcher } from 'svelte';

  export let show = false;
  export let title = '';
  export let description = '';
  export let confirmText = 'Confirm';
  export let cancelText = 'Cancel';
  export let confirmClass = 'bg-red-600 hover:bg-red-700';
  export let loading = false;

  const dispatch = createEventDispatcher<{
    confirm: void;
    cancel: void;
  }>();
</script>

{#if show}
  <div class="fixed inset-0 bg-black/60 flex items-center justify-center z-50" role="dialog">
    <div class="bg-gray-800 rounded-xl p-6 max-w-md w-full mx-4 shadow-2xl">
      <div class="flex items-center gap-4 mb-4">
        <slot name="icon">
          <div class="w-12 h-12 bg-red-500/20 rounded-full flex items-center justify-center">
            <svg class="w-6 h-6 text-red-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
            </svg>
          </div>
        </slot>
        <div>
          <h3 class="text-lg font-semibold text-white">{title}</h3>
          {#if description}
            <p class="text-sm text-gray-400">{description}</p>
          {/if}
        </div>
      </div>

      <slot name="content" />

      <div class="flex gap-3 justify-end mt-4">
        <button
          on:click={() => dispatch('cancel')}
          class="px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white rounded-lg"
          disabled={loading}
        >
          {cancelText}
        </button>
        <button
          on:click={() => dispatch('confirm')}
          disabled={loading}
          class="px-4 py-2 {confirmClass} text-white rounded-lg disabled:opacity-50"
        >
          {loading ? 'Loading...' : confirmText}
        </button>
      </div>
    </div>
  </div>
{/if}
