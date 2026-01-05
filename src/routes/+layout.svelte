<script lang="ts">
  import '../app.css';
  import { stats } from '$lib/stores/stats';
  import { onMount } from 'svelte';

  onMount(() => {
    stats.load();
  });
</script>

<div class="min-h-screen bg-gray-900 text-gray-100">
  <slot />
</div>
