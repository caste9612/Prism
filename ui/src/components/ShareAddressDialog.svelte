<script lang="ts">
  // REVIEW D59: a network share of the catalog given its new path by hand (a NAS whose address
  // changed). Prism checks that the path answers and how much of its top level matches the
  // catalog; the user confirms.
  import type { SharePathView } from '../lib/bindings/SharePathView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { volumeName } from '../lib/catalog';
  import { t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';

  let {
    volume,
    onclose,
    ondone,
  }: {
    volume: VolumeView | null;
    onclose: () => void;
    ondone: () => void;
  } = $props();

  let path = $state('');
  let check = $state<SharePathView | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  /** Answers, but less than half of the top level of the catalog is there. */
  const unlike = $derived(check !== null && check.reachable && check.total > 0 && check.matching * 2 < check.total);

  $effect(() => {
    if (volume) {
      path = volume.path ?? '';
      check = null;
      error = null;
    }
  });

  async function verify(e: SubmitEvent) {
    e.preventDefault();
    if (!volume) return;
    busy = true;
    error = null;
    check = null;
    try {
      check = await backend.checkSharePath(volume.id, path);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  async function move() {
    if (!volume || !check?.reachable) return;
    busy = true;
    error = null;
    try {
      await backend.moveShare(volume.id, check.path);
      ondone();
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog open={volume !== null} title={t('share.title')} {onclose}>
  {#if volume}
    <p class="intro">
      <span class="who">{volumeName(volume)}</span>
      <span class="muted">{t('share.intro', { path: volume.path ?? '' })}</span>
    </p>
  {/if}
  <form id="share-form" onsubmit={verify} class="form">
    <label for="share-path">{t('share.path')}</label>
    <div class="row">
      <input
        id="share-path"
        class="mono"
        bind:value={path}
        oninput={() => (check = null)}
        placeholder="\\192.168.1.10\Foto"
        spellcheck="false"
        autocomplete="off"
      />
      <button type="submit" disabled={busy || !path.trim()}>{t('share.check')}</button>
    </div>
    <p class="hint">{t('share.hint')}</p>
  </form>
  {#if check}
    <p class="result" class:bad={!check.reachable || unlike} role="status">
      {#if !check.reachable}
        {t('share.unreachable')}
      {:else if check.total === 0}
        {t('share.answersEmpty')}
      {:else}
        {t('share.answers', { matching: check.matching, total: check.total })}{#if unlike}{' '}{t('share.unlike')}{/if}
      {/if}
    </p>
  {/if}
  {#if error}<p class="result bad" role="alert">{error}</p>{/if}
  {#snippet actions()}
    <button onclick={onclose}>{t('actions.cancel')}</button>
    <button class="primary" disabled={busy || !check?.reachable} onclick={move}>{t('share.move')}</button>
  {/snippet}
</Dialog>

<style>
  .intro {
    margin: -6px 0 0;
    display: grid;
    gap: 4px;
  }
  .who {
    font-family: var(--font-display);
    font-weight: 600;
    font-size: 15px;
  }
  .intro .muted {
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
  .row {
    display: flex;
    gap: 8px;
  }
  .row input {
    flex: 1 1 auto;
    min-width: 0;
    border: 1.5px solid var(--text);
  }
  .row button {
    flex: none;
  }
  .hint {
    margin: 0;
    font-size: 11.5px;
    color: var(--text-3);
  }
  .result {
    margin: 12px 0 0;
    font-size: 13px;
  }
  .result.bad {
    color: var(--signal-strong);
  }
</style>
