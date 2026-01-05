<script lang="ts">
  import type { Notification } from '$lib/types';

  export let notifications: Notification[] = [];
</script>

<div class="fixed top-4 right-4 z-50 space-y-2">
  {#each notifications as notif (notif.id)}
    <div class="px-4 py-3 rounded-lg shadow-lg text-sm font-medium flex items-center gap-2 animate-slide-in
      {notif.type === 'success' ? 'bg-green-600 text-white' : notif.type === 'error' ? 'bg-red-600 text-white' : 'bg-gray-700 text-white'}">
      {#if notif.type === 'success'}
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" /></svg>
      {:else if notif.type === 'error'}
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" /></svg>
      {:else}
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>
      {/if}
      {notif.message}
    </div>
  {/each}
</div>

<style>
  @keyframes slide-in {
    from { transform: translateX(100%); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }
  .animate-slide-in {
    animation: slide-in 0.3s ease-out;
  }
</style>
