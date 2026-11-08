<script lang="ts">
  // F03: name, physical location, colour and notes of a disk (ADR-0019). They are what tells the
  // disk apart when it is in a drawer; the locations already used are suggested.
  import type { LabelsInput } from '../lib/bindings/LabelsInput';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { knownLocations, LABEL_COLORS, stickerColor, volumeName } from '../lib/catalog';
  import { t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';

  let {
    volume,
    volumes,
    onsave,
    onclose,
  }: {
    volume: VolumeView | null;
    volumes: VolumeView[];
    onsave: (id: number, labels: LabelsInput) => void;
    onclose: () => void;
  } = $props();

  let name = $state('');
  let location = $state('');
  let notes = $state('');
  let color = $state<string | null>(null);
  const suggestions = $derived(knownLocations(volumes.filter((v) => v.id !== volume?.id)));

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
  {#if volume}
    <p class="intro">
      <span class="who">{volumeName(volume)}</span>
      {#if volume.mount}<span class="letter">{volume.mount.replace(/\\$/, '')}</span>{/if}
      <span class="muted">{t('labels.intro')}</span>
    </p>
  {/if}
  <form id="labels-form" onsubmit={save} class="form">
    <div>
      <label for="label-name">{t('labels.name')}</label>
      <input id="label-name" bind:value={name} placeholder={volume?.osLabel ?? t('labels.namePlaceholder')} />
    </div>
    <div>
      <label for="label-location">{t('labels.location')}</label>
      <input id="label-location" list="label-locations" bind:value={location} placeholder={t('labels.locationPlaceholder')} />
      <datalist id="label-locations">
        {#each suggestions as s (s)}<option value={s}></option>{/each}
      </datalist>
      {#if suggestions.length > 0}
        <p class="hint">
          {t('labels.recent')}
          {#each suggestions.slice(0, 4) as s, i (s)}{#if i > 0}{' · '}{/if}<button type="button" class="suggest" onclick={() => (location = s)}>{s}</button>{/each}
        </p>
      {/if}
    </div>
    <fieldset>
      <legend>{t('labels.color')}</legend>
      <div class="swatches">
        <button
          type="button"
          class="swatch auto"
          class:selected={color === null}
          aria-pressed={color === null}
          aria-label={t('labels.autoColor')}
          title={t('labels.autoColor')}
          style:--auto={volume ? stickerColor({ id: volume.id, color: null }) : 'transparent'}
          onclick={() => (color = null)}
        ></button>
        {#each LABEL_COLORS as c (c.color)}
          <button
            type="button"
            class="swatch"
            class:selected={color?.toLowerCase() === c.color}
            aria-pressed={color?.toLowerCase() === c.color}
            aria-label={t(`labels.colors.${c.key}`)}
            title={t(`labels.colors.${c.key}`)}
            style:background={c.color}
            onclick={() => (color = c.color)}
          ></button>
        {/each}
      </div>
    </fieldset>
    <div>
      <label for="label-notes">{t('labels.notes')}</label>
      <textarea id="label-notes" rows="3" bind:value={notes}></textarea>
    </div>
  </form>
  {#snippet actions()}
    <button onclick={onclose}>{t('actions.cancel')}</button>
    <button class="primary" type="submit" form="labels-form">{t('actions.save')}</button>
  {/snippet}
</Dialog>

<style>
  .intro {
    margin: -6px 0 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
  }
  .who {
    font-family: var(--font-display);
    font-weight: 600;
    font-size: 15px;
  }
  .intro .muted {
    flex-basis: 100%;
    font-size: 12.5px;
  }
  .form {
    display: grid;
    gap: 14px;
  }
  #label-name {
    border: 1.5px solid var(--text);
  }
  .hint {
    margin: 6px 0 0;
    font-size: 11.5px;
    color: var(--text-3);
  }
  .suggest {
    min-height: 0;
    padding: 0;
    background: transparent;
    font-size: 11.5px;
    color: var(--text-2);
    text-decoration: underline dotted;
  }
  .suggest:hover:not(:disabled) {
    background: transparent;
    color: var(--text);
  }
  fieldset {
    border: 0;
    padding: 0;
    margin: 0;
  }
  legend {
    color: var(--text-2);
    font-size: 12.5px;
    font-weight: 600;
    margin-bottom: 8px;
  }
  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .swatch {
    width: 30px;
    height: 30px;
    min-height: 0;
    border-radius: 50%;
    padding: 0;
    border: 3px solid var(--surface);
    box-shadow: 0 0 0 1px var(--border);
  }
  .swatch.auto {
    background: conic-gradient(var(--auto) 0 50%, var(--surface-2) 50% 100%);
  }
  .swatch.selected {
    box-shadow: 0 0 0 2px var(--signal);
  }
</style>
