<script lang="ts">
  // F03: name, physical location, notes and colour of a disk.
  import type { LabelsInput } from '../lib/bindings/LabelsInput';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { LABEL_COLORS } from '../lib/catalog';
  import { t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';

  let {
    volume,
    onsave,
    onclose,
  }: {
    volume: VolumeView | null;
    onsave: (id: number, labels: LabelsInput) => void;
    onclose: () => void;
  } = $props();

  let name = $state('');
  let location = $state('');
  let notes = $state('');
  let color = $state<string | null>(null);

  $effect(() => {
    if (volume) {
      name = volume.userName ?? '';
      location = volume.location ?? '';
      notes = volume.notes ?? '';
      color = volume.color;
    }
  });

  function save(e: SubmitEvent) {
    e.preventDefault();
    if (!volume) return;
    onsave(volume.id, { name, location, notes, color });
  }
</script>

<Dialog open={volume !== null} title={t('labels.title')} {onclose}>
  <form id="labels-form" onsubmit={save} class="form">
    <div>
      <label for="label-name">{t('labels.name')}</label>
      <input id="label-name" bind:value={name} placeholder={volume?.osLabel ?? t('labels.namePlaceholder')} />
    </div>
    <div>
      <label for="label-location">{t('labels.location')}</label>
      <input id="label-location" bind:value={location} placeholder={t('labels.locationPlaceholder')} />
    </div>
    <div>
      <label for="label-notes">{t('labels.notes')}</label>
      <textarea id="label-notes" rows="3" bind:value={notes}></textarea>
    </div>
    <fieldset>
      <legend>{t('labels.color')}</legend>
      <div class="swatches">
        <button
          type="button"
          class="swatch none"
          class:selected={color === null}
          aria-pressed={color === null}
          aria-label={t('labels.noColor')}
          title={t('labels.noColor')}
          onclick={() => (color = null)}
        ></button>
        {#each LABEL_COLORS as c (c)}
          <button
            type="button"
            class="swatch"
            class:selected={color === c}
            aria-pressed={color === c}
            aria-label={c}
            style:background={c}
            onclick={() => (color = c)}
          ></button>
        {/each}
      </div>
    </fieldset>
  </form>
  {#snippet actions()}
    <button onclick={onclose}>{t('actions.cancel')}</button>
    <button class="primary" type="submit" form="labels-form">{t('actions.save')}</button>
  {/snippet}
</Dialog>

<style>
  .form {
    display: grid;
    gap: 14px;
  }
  fieldset {
    border: 0;
    padding: 0;
    margin: 0;
  }
  legend {
    color: var(--text-2);
    font-size: 13px;
    margin-bottom: 6px;
  }
  .swatches {
    display: flex;
    gap: 8px;
  }
  .swatch {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    padding: 0;
    border: 2px solid var(--surface);
    box-shadow: 0 0 0 1px var(--border);
  }
  .swatch.none {
    background: repeating-linear-gradient(45deg, var(--surface-2), var(--surface-2) 4px, var(--surface) 4px, var(--surface) 8px);
  }
  .swatch.selected {
    box-shadow: 0 0 0 2px var(--accent);
  }
</style>
