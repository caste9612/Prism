<script lang="ts">
  export let width: string = '100%';
  export let height: string = '1rem';
  export let rounded: 'sm' | 'md' | 'lg' | 'full' = 'md';
</script>

<div
  class="animate-pulse bg-gray-700/50"
  class:rounded-sm={rounded === 'sm'}
  class:rounded-md={rounded === 'md'}
  class:rounded-lg={rounded === 'lg'}
  class:rounded-full={rounded === 'full'}
  style="width: {width}; height: {height};"
></div>
