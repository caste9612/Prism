<script lang="ts">
  // Similar images (F11) and videos (F12): groups of visually similar files on every disk, with
  // thumbnails side by side (connected disks), resolution, size, length of the videos and where
  // each copy is (F11-AC3); the similarity threshold is adjustable (F11-AC1).
  import type { SimilarFileView } from '../lib/bindings/SimilarFileView';
  import type { SimilarView } from '../lib/bindings/SimilarView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { onMount, untrack } from 'svelte';
  import { backend, errorMessage } from '../lib/api';
  import { stickerColor, volumeName } from '../lib/catalog';
  import { formatBytes, formatCount } from '../lib/format';
  import { language, t } from '../lib/i18n.svelte';
  import Icon from './Icon.svelte';

  let {
    volumes,
    onerror,
    scope = null,
  }: {
    volumes: VolumeView[];
    onerror: (message: string) => void;
    /** The disk of the screen (ADR-0019): groups with a copy on it; every disk when null. */
    scope?: number | null;
  } = $props();

  const lang = $derived(language());
  const PAGE = 30;
  let kind = $state<'images' | 'videos'>('images');
  let distance = $state(6);
  let volumeId = $state<number | null>(untrack(() => scope));
  let view = $state<SimilarView | null>(null);
  let loading = $state(false);
  let shown = $state(PAGE);
  let thumbs = $state<Record<number, string | null>>({});

  async function load() {
    loading = true;
    try {
      view = kind === 'images' ? await backend.similarImages(distance, volumeId) : await backend.similarVideos(distance, volumeId);
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
      const thumb = file.durationMs === null ? backend.imageThumbnail : backend.videoThumbnail;
      void thumb(file.volumeId, file.entryId).then((url) => {
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
      f.durationMs !== null ? length(f.durationMs) : null,
      f.width && f.height ? `${f.width}×${f.height}` : null,
      formatBytes(f.size, lang),
      f.distance > 0 ? t('similar.difference', { value: f.distance }) : null,
    ]
      .filter(Boolean)
      .join(' · ');
  /** 3:07, or 1:02:03 for long videos. */
  const length = (ms: number) => {
    const s = Math.round(ms / 1000);
    const two = (n: number) => String(n).padStart(2, '0');
    return s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${Math.floor(s / 60)}:${two(s % 60)}`;
  };
  function choose(k: 'images' | 'videos') {
    if (kind === k) return;
    kind = k;
    view = null;
    void load();
  }
  const name = (f: SimilarFileView) => f.path.split('\\').pop() ?? f.path;
  const folder = (f: SimilarFileView) => f.path.split('\\').slice(0, -1).join('\\') || '\\';
  const catalogued = $derived(volumes.filter((v) => v.lastIndexedAt !== null));
</script>

<section class="similar">
  <div class="card controls">
    <div class="pills inset" role="tablist" aria-label={t('similar.title')}>
      <button role="tab" aria-selected={kind === 'images'} onclick={() => choose('images')}>{t('similar.tabImages')}</button>
      <button role="tab" aria-selected={kind === 'videos'} onclick={() => choose('videos')}>{t('similar.tabVideos')}</button>
    </div>
    <label class="threshold">
      <span>{t('similar.thresholdLabel')}</span>
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
      <span class="mono">{distance} / 64</span>
    </label>
    <span class="muted hint">{t('similar.thresholdHint')}</span>
    {#if scope === null}
      <label class="disk">
        <span class="sr-only">{t('similar.disk')}</span>
        <select
          aria-label={t('similar.disk')}
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
    {/if}
    <button class="icon refresh" onclick={load} disabled={loading} title={t('similar.refresh')} aria-label={t('similar.refresh')}><Icon name="refresh" size={15} /></button>
  </div>

  <p class="list-head">
    <span>{t('similar.noDelete')}</span>
    {#if view && view.groups.length > 0}
      <span class="mono muted">{t(kind === 'images' ? 'similar.summary' : 'similar.summaryVideos', { count: formatCount(view.totalGroups, lang) })}</span>
    {/if}
  </p>

  {#if view && view.pending > 0}
    <p class="pending" role="status">{t(kind === 'images' ? 'similar.pending' : 'similar.pendingVideos', { count: formatCount(view.pending, lang) })}</p>
  {/if}

  {#if view === null}
    <p class="empty card">{loading ? t('similar.loading') : ''}</p>
  {:else if view.groups.length === 0}
    <p class="empty card">{t('similar.nothing')}</p>
  {:else}
    {#each view.groups.slice(0, shown) as g (g.key)}
      {@const maxDiff = Math.max(...g.files.map((f) => f.distance))}
      <article class="group card" aria-label={name(g.files[0]!)}>
        <header>
          <h2>{t(kind === 'images' ? 'similar.images' : 'similar.videos', { count: formatCount(g.count, lang) })}</h2>
          <span class="mono muted">{t('similar.groupDiff', { value: maxDiff })}</span>
        </header>
        <ul class="tiles">
          {#each g.files as f (f.entryId)}
            {@const vol = volumes.find((v) => v.id === f.volumeId)}
            <li class="tile" class:offline={!f.online}>
              <div class="thumb" use:lazyThumb={f}>
                {#if thumbs[f.entryId]}
                  <img src={thumbs[f.entryId]} alt={name(f)} />
                {:else if !f.online}
                  <span class="placeholder">{t('similar.noPreview')}</span>
                {:else}
                  <span class="placeholder" aria-hidden="true"></span>
                {/if}
                {#if f.online}
                  <button class="reveal icon" title={t('similar.open')} aria-label={t('similar.open')} onclick={() => reveal(f)}>
                    <Icon name="open" size={14} />
                  </button>
                {/if}
              </div>
              <div class="info">
                <strong class="name" title={f.fullPath}>{name(f)}</strong>
                <span class="mono details">{details(f)}</span>
                <span class="where" style:--sticker={vol ? stickerColor(vol) : undefined}>
                  <span class="sticker" class:ring={!f.online}></span>
                  {f.location ? `${f.disk} · ${f.location}` : f.disk}
                  {#if !f.online}<span class="badge">{t('similar.offline')}</span>{/if}
                </span>
                <span class="mono folder" title={f.path}>{folder(f)}</span>
              </div>
            </li>
          {/each}
        </ul>
        {#if g.count > g.files.length}
          <p class="muted">{t('similar.moreFiles', { count: formatCount(g.count - g.files.length, lang) })}</p>
        {/if}
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
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 18px;
    flex-wrap: wrap;
    padding: 12px 16px;
  }
  .threshold {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text);
  }
  .threshold input {
    width: 180px;
  }
  .threshold .mono {
    font-weight: 400;
  }
  .hint {
    font-size: 12px;
  }
  .disk {
    margin: 0 0 0 auto;
  }
  .disk select {
    width: auto;
  }
  .refresh {
    width: 32px;
    height: 32px;
  }
  .list-head {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin: 0;
    font-size: 12px;
    color: var(--text-2);
  }
  .pending {
    margin: 0;
    padding: 10px 16px;
    background: var(--surface);
    border-radius: var(--radius-sm);
    font-size: 12.5px;
  }
  .group {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .group header {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .group h2 {
    font-size: 16px;
  }
  .group header .mono {
    font-size: 12px;
  }
  .tiles {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 12px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .thumb {
    position: relative;
    height: 140px;
    border-radius: 10px;
    background: var(--chip);
    overflow: hidden;
    display: grid;
    place-items: center;
  }
  .offline .thumb {
    background: transparent;
    border: 1.5px dashed var(--border);
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .placeholder {
    padding: 12px;
    text-align: center;
    font-size: 12px;
    color: var(--text-3);
  }
  .reveal {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 28px;
    height: 28px;
    background: var(--surface);
    opacity: 0.9;
  }
  .info {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .name,
  .folder {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .details {
    font-size: 11.5px;
    color: var(--text-2);
  }
  .where {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-2);
  }
  .where .sticker {
    width: 9px;
    height: 9px;
  }
  .badge {
    font-size: 10.5px;
    padding: 1px 6px;
    border-radius: 5px;
    background: var(--chip);
  }
  .folder {
    font-size: 11px;
    color: var(--text-3);
  }
  .more {
    align-self: center;
  }
</style>
