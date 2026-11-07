<script lang="ts">
  // Modal dialog on the native <dialog> element (focus trap and Escape handled by the browser).
  import type { Snippet } from 'svelte';

  let {
    open,
    title,
    onclose,
    children,
    actions,
  }: {
    open: boolean;
    title: string;
    onclose: () => void;
    children: Snippet;
    actions?: Snippet;
  } = $props();

  let dialog: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });
</script>

<dialog bind:this={dialog} onclose={onclose} aria-labelledby="dialog-title">
  <h2 id="dialog-title">{title}</h2>
  <div class="body">{@render children()}</div>
  {#if actions}
    <div class="actions">{@render actions()}</div>
  {/if}
</dialog>

<style>
  dialog {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    padding: 22px 24px;
    width: min(460px, 92vw);
    box-shadow: 0 20px 50px rgb(0 0 0 / 30%);
  }
  dialog::backdrop {
    background: rgb(10 12 20 / 45%);
  }
  h2 {
    font-size: 17px;
    margin-bottom: 14px;
  }
  .body {
    display: grid;
    gap: 14px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }
</style>
