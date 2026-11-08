<script lang="ts" module>
  export interface MenuItem {
    label: string;
    onselect: () => void;
    danger?: boolean;
    disabled?: boolean;
    /** A line above this item. */
    separated?: boolean;
  }
</script>

<script lang="ts">
  // A small menu that opens under its "⋯" button (ADR-0019): the actions of a card that are not
  // worth a button of their own. Closes on a choice, a click outside or Esc.
  import { t } from '../lib/i18n.svelte';

  let { label, items, align = 'end' }: { label: string; items: MenuItem[]; align?: 'start' | 'end' } = $props();

  let open = $state(false);
  let root: HTMLElement | undefined = $state();

  function choose(item: MenuItem) {
    open = false;
    item.onselect();
  }

  $effect(() => {
    if (!open) return;
    const outside = (e: MouseEvent) => {
      if (root && !root.contains(e.target as Node)) open = false;
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') open = false;
    };
    window.addEventListener('mousedown', outside);
    window.addEventListener('keydown', esc);
    return () => {
      window.removeEventListener('mousedown', outside);
      window.removeEventListener('keydown', esc);
    };
  });
</script>

<div class="menu-root" bind:this={root}>
  <button
    class="ghost icon"
    aria-label={label}
    title={t('actions.more')}
    aria-haspopup="menu"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" />
    </svg>
  </button>
  {#if open}
    <div class="menu {align}" role="menu" aria-label={label}>
      {#each items as item (item.label)}
        {#if item.separated}<div class="sep" role="separator"></div>{/if}
        <button role="menuitem" class:danger={item.danger} disabled={item.disabled} onclick={() => choose(item)}>
          {item.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .menu-root {
    position: relative;
    display: inline-flex;
  }
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    z-index: 30;
    min-width: 240px;
    background: var(--surface);
    border-radius: 12px;
    box-shadow: var(--shadow-pop);
    padding: 6px;
    display: flex;
    flex-direction: column;
  }
  .menu.end {
    right: 0;
  }
  .menu.start {
    left: 0;
  }
  .menu button {
    text-align: left;
    background: transparent;
    min-height: 34px;
    border-radius: 7px;
    padding: 0 10px;
    font-weight: 400;
  }
  .menu button:hover:not(:disabled) {
    background: var(--chip);
  }
  .menu button.danger {
    color: var(--danger);
    font-weight: 600;
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--line);
  }
</style>
