<script lang="ts">
  // F02-AC5: a network folder added by hand with its path, for the folders Prism does not see by
  // itself (a mapping that is not persistent, a folder of another PC or NAS). The backend checks
  // that it answers; the folder is then proposed for indexing like a new disk.
  import { backend, errorMessage } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';

  let {
    open,
    onclose,
    ondone,
  }: {
    open: boolean;
    onclose: () => void;
    ondone: (path: string) => void;
  } = $props();

  let path = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);

  $effect(() => {
    if (open) {
      path = '';
      error = null;
    }
  });

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !path.trim()) return;
    busy = true;
    error = null;
    try {
      await backend.addShare(path);
      ondone(path.trim());
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog {open} title={t('share.addTitle')} {onclose}>
  <p class="intro muted">{t('share.addIntro')}</p>
  <form id="add-share-form" onsubmit={add} class="form">
    <label for="add-share-path">{t('share.addPath')}</label>
    <input
      id="add-share-path"
      class="mono"
      bind:value={path}
      oninput={() => (error = null)}
      placeholder="\\192.168.1.10\Foto"
      spellcheck="false"
      autocomplete="off"
    />
    <p class="hint">{t('share.hint')}</p>
  </form>
  {#if error}<p class="result" role="alert">{error}</p>{/if}
  {#snippet actions()}
    <button onclick={onclose}>{t('actions.cancel')}</button>
    <button class="primary" type="submit" form="add-share-form" disabled={busy || !path.trim()}>
      {busy ? t('share.adding') : t('share.addNow')}
    </button>
  {/snippet}
</Dialog>

<style>
  .intro {
    margin: -6px 0 0;
    font-size: 12.5px;
  }
  .form {
    display: grid;
    gap: 6px;
    margin-top: 14px;
  }
  label {
    color: var(--text-2);
    font-size: 12.5px;
    font-weight: 600;
  }
  input {
    border: 1.5px solid var(--text);
  }
  .hint {
    margin: 0;
    font-size: 11.5px;
    color: var(--text-3);
  }
  .result {
    margin: 12px 0 0;
    font-size: 13px;
    color: var(--signal-strong);
  }
</style>
