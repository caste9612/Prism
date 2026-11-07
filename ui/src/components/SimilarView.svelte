<script lang="ts">
  // Similar images (F11): groups of visually similar images on every disk, with thumbnails side
  // by side (connected disks), resolution, size and where each copy is (F11-AC3); the
  // similarity threshold is adjustable (F11-AC1).
  import type { SimilarFileView } from '../lib/bindings/SimilarFileView';
  import type { SimilarView } from '../lib/bindings/SimilarView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { onMount } from 'svelte';
  import { backend, errorMessage } from '../lib/api';
  import { volumeName } from '../lib/catalog';
  import { formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Icon from './Icon.svelte';

  let { volumes, onerror }: { volumes: VolumeView[]; onerror: (message: string) => void } = $props();

  const lang = $derived(language());
  const PAGE = 30;
  let distance = $state(6);
  let volumeId = $state<number | null>(null);
  let view = $state<SimilarView | null>(null);
  let loading = $state(false);
  let shown = $state(PAGE);
  let thumbs = $state<Record<number, string | null>>({});

  async function load() {
    loading = true;
    try {
      view = await backend.similarImages(distance, volumeId);
      shown = PAGE;
    } catch (e) {
      onerror(errorMessage(e));
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load();
  });

  /** Loads the thumbnail of an image when its tile comes into view. */
  function lazyThumb(node: HTMLElement, file: SimilarFileView) {
    if (!file.online || file.entryId in thumbs) return;
    const io = new IntersectionObserver((entries) => {
      if (!entries.some((e) => e.isIntersecting)) return;
      io.disconnect();
      void backend.imageThumbnail(file.volumeId, file.entryId).then((url) => {
        thumbs = { ...thumbs, [file.entryId]: url };
      });
    });
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }

  async function reveal(f: SimilarFileView) {
    try {
      await backend.revealEntry(f.volumeId, f.entryId);
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  const details = (f: SimilarFileView) =>
    [
      f.width && f.height ? `${f.width}×${f.height}` : null,
      formatBytes(f.size, lang),
      f.distance > 0 ? t('similar.difference', { value: f.distance }) : null,
    ]
      .filter(Boolean)
      .join(' · ');
  const name = (f: SimilarFileView) => f.path.split('\\').pop() ?? f.path;
  const folder = (f: SimilarFileView) => f.path.split('\\').slice(0, -1).join('\\') || '\\';
  const catalogued = $derived(volumes.filter((v) => v.lastIndexedAt !== null));
</script>

<section class="similar">
  <header class="page-head">
    <div>
      <h1>{t('similar.title')}</h1>
      <p class="muted">{t('similar.subtitle')}</p>
    </div>
    <button onclick={load} disabled={loading}><Icon name="refresh" size={15} /> {t('similar.refresh')}</button>
  </header>

  <div class="controls">
    <label class="threshold">
      <span>{t('similar.threshold', { value: distance })}</span>
      <input
        type="range"
        min="0"
        max="16"
        step="1"
        value={distance}
        aria-valuetext={t('similar.thresholdText', { value: distance })}
        onchange={(e) => {
          distance = Number(e.currentTarget.value);
          void load();
        }}
      />
      <span class="ends muted"><span>{t('similar.strict')}</span><span>{t('similar.loose')}</span></span>
    </label>
    <label class="disk">
      <span>{t('similar.disk')}</span>
      <select
        value={volumeId === null ? '' : String(volumeId)}
        onchange={(e) => {
          volumeId = e.currentTarget.value ? Number(e.currentTarget.value) : null;
          void load();
        }}
      >
        <option value="">{t('similar.anyDisk')}</option>
        {#each catalogued as v (v.id)}
          <option value={String(v.id)}>{volumeName(v)}</option>
        {/each}
      </select>
    </label>
  </div>

  {#if view && view.pendingImages > 0}
    <p class="pending" role="status">{t('similar.pending', { count: formatCount(view.pendingImages, lang) })}</p>
  {/if}

  {#if view === null}
    <p class="muted">{loading ? t('similar.loading') : ''}</p>
  {:else if view.groups.length === 0}
    <p class="empty muted">{t('similar.nothing')}</p>
  {:else}
    <p class="summary"><strong>{t('similar.summary', { count: formatCount(view.totalGroups, lang) })}</strong></p>
    {#each view.groups.slice(0, shown) as g (g.key)}
      <article class="group" aria-label={name(g.files[0]!)}>
        <h2>{t('similar.images', { count: formatCount(g.files.length, lang) })}</h2>
        <ul class="tiles">
          {#each g.files as f (f.entryId)}
            <li class="tile" class:offline={!f.online}>
              <div class="thumb" use:lazyThumb={f}>
                {#if thumbs[f.entryId]}
                  <img src={thumbs[f.entryId]} alt={name(f)} />
                {:else}
                  <span class="placeholder muted">{f.online ? '' : t('similar.noPreview')}</span>
                {/if}
              </div>
              <div class="info">
                <strong class="name" title={f.fullPath}>{name(f)}</strong>
                <span class="muted">{details(f)}</span>
                <span class="where">
                  {f.location ? `${f.disk} · ${f.location}` : f.disk}
                  {#if !f.online}<span class="badge">{t('similar.offline')}</span>{/if}
                </span>
                <span class="muted folder" title={f.path}>{folder(f)}</span>
              </div>
              {#if f.online}
                <button class="ghost icon" title={t('similar.open')} aria-label={t('similar.open')} onclick={() => reveal(f)}>
                  <Icon name="open" size={15} />
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      </article>
    {/each}
    {#if view.groups.length > shown}
      <button class="more" onclick={() => (shown += PAGE)}>{t('similar.more')}</button>
    {/if}
    {#if view.totalGroups > view.groups.length}
      <p class="muted">{t('similar.capped', { count: formatCount(view.groups.length, lang) })}</p>
    {/if}
  {/if}
</section>

<style>
  .similar {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-width: 1100px;
  }
  .page-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }
  .page-head h1 {
    font-size: 24px;
  }
  .page-head p {
    margin: 2px 0 0;
  }
  .page-head button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 14px 24px;
    align-items: flex-end;
  }
  .controls label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    font-size: 12.5px;
  }
  .threshold {
    min-width: 260px;
  }
  .threshold input {
    padding: 0;
  }
  .ends {
    display: flex;
    justify-content: space-between;
    font-size: 11.5px;
  }
  .disk {
    min-width: 200px;
  }
  .pending {
    margin: 0;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 14px;
  }
  .summary {
    margin: 0;
  }
  .group {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 16px;
    box-shadow: var(--shadow);
  }
  .group h2 {
    font-size: 14px;
    margin-bottom: 10px;
  }
  .tiles {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 12px;
  }
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .tile .icon {
    position: absolute;
    top: 6px;
    right: 6px;
    background: var(--surface);
  }
  .thumb {
    height: 150px;
    border-radius: 8px;
    background: var(--surface-2);
    display: grid;
    place-items: center;
    overflow: hidden;
  }
  .thumb img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }
  .tile.offline .thumb {
    opacity: 0.7;
  }
  .placeholder {
    font-size: 12px;
    padding: 0 8px;
    text-align: center;
  }
  .info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    font-size: 12.5px;
    min-width: 0;
  }
  .name,
  .folder {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    font-size: 13.5px;
  }
  .badge {
    margin-left: 6px;
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-3);
  }
  .more {
    align-self: center;
  }
  .empty {
    padding: 40px 0;
    text-align: center;
  }
</style>
