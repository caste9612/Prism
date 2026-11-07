<script lang="ts">
  // A folder of a catalogued disk: the disk and a path inside it (empty for the whole disk).
  // Disconnected disks are offered too: checks and comparisons use the catalog (F13-AC2).
  import type { SideInput } from '../lib/bindings/SideInput';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { volumeName } from '../lib/catalog';
  import { t } from '../lib/i18n.svelte';

  let {
    label,
    volumes,
    value,
    onchange,
    onlineOnly = false,
  }: {
    label: string;
    volumes: VolumeView[];
    value: SideInput;
    onchange: (v: SideInput) => void;
    onlineOnly?: boolean;
  } = $props();

  const uid = $props.id();
  const choices = $derived(volumes.filter((v) => v.lastIndexedAt !== null && (!onlineOnly || v.online)));
</script>

<div class="side" role="group" aria-label={label}>
  <label for="{uid}-disk">{label}</label>
  <div class="fields">
    <select
      id="{uid}-disk"
      aria-label={t('backup.disk')}
      value={String(value.volumeId)}
      onchange={(e) => onchange({ ...value, volumeId: Number(e.currentTarget.value) })}
    >
      {#each choices as v (v.id)}
        <option value={String(v.id)}>{volumeName(v)}{v.online ? '' : ` (${t('catalog.offline').toLowerCase()})`}</option>
      {/each}
    </select>
    <input
      aria-label={t('backup.folder')}
      value={value.folder}
      placeholder={t('backup.folderPlaceholder')}
      spellcheck="false"
      oninput={(e) => onchange({ ...value, folder: e.currentTarget.value })}
    />
  </div>
</div>

<style>
  .side {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .side > label {
    margin: 0;
    font-size: 12.5px;
  }
  .fields {
    display: grid;
    grid-template-columns: minmax(140px, 220px) minmax(0, 1fr);
    gap: 8px;
  }
</style>
