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
  // Several dialogs live in the page at once: each needs its own title id.
  const uid = $props.id();
  const titleId = `${uid}-title`;

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });
</script>

<dialog bind:this={dialog} onclose={onclose} aria-labelledby={titleId}>
  <h2 id={titleId}>{title}</h2>
  <div class="body">{@render children()}</div>
  {#if actions}
    <div class="actions">{@render actions()}</div>
  {/if}
</dialog>

<style>
  dialog {
    border: 0;
    border-radius: 16px;
    background: var(--surface);
    color: var(--text);
    padding: 22px;
    width: min(460px, 92vw);
    box-shadow: var(--shadow-pop);
  }
  dialog::backdrop {
    background: rgb(10 12 20 / 45%);
  }
  h2 {
    font-size: 20px;
    line-height: 1.25;
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
