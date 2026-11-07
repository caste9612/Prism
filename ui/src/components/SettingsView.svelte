<script lang="ts">
  // F21 (first part): language, theme, start with Windows, ignored disks (F02-AC4), about.
  import type { AppInfo } from '../lib/bindings/AppInfo';
  import type { Settings } from '../lib/bindings/Settings';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { languages, t } from '../lib/i18n.svelte';
  import { volumeName } from '../lib/catalog';

  let {
    settings,
    volumes,
    info,
    onchange,
    onerror,
  }: {
    settings: Settings;
    volumes: VolumeView[];
    info: AppInfo | null;
    onchange: (s: Settings) => void;
    onerror: (message: string) => void;
  } = $props();

  const ignored = $derived(volumes.filter((v) => v.status === 'ignored'));

  function update(patch: Partial<Settings>) {
    onchange({ ...settings, ...patch });
  }

  async function unignore(v: VolumeView) {
    try {
      await backend.setIgnored(v.id, false);
    } catch (e) {
      onerror(errorMessage(e));
    }
  }
</script>

<section class="settings">
  <h1>{t('settings.title')}</h1>

  <div class="group">
    <h2>{t('settings.general')}</h2>
    <div class="row">
      <label for="set-language">{t('settings.language')}</label>
      <select id="set-language" value={settings.language} onchange={(e) => update({ language: e.currentTarget.value })}>
        <option value="system">{t('settings.system')}</option>
        {#each languages as l (l)}
          <option value={l}>{t(`language.${l}`)}</option>
        {/each}
      </select>
    </div>
    <div class="row">
      <label for="set-theme">{t('settings.theme')}</label>
      <select id="set-theme" value={settings.theme} onchange={(e) => update({ theme: e.currentTarget.value })}>
        <option value="system">{t('settings.system')}</option>
        <option value="light">{t('settings.light')}</option>
        <option value="dark">{t('settings.dark')}</option>
      </select>
    </div>
    <div class="row check">
      <input id="set-autostart" type="checkbox" checked={settings.autostart} onchange={(e) => update({ autostart: e.currentTarget.checked })} />
      <div>
        <label for="set-autostart">{t('settings.autostart')}</label>
        <p class="muted hint">{t('settings.autostartHint')}</p>
      </div>
    </div>
    <div class="row">
      <label for="set-shortcut">{t('settings.shortcut')}</label>
      <div>
        <input
          id="set-shortcut"
          value={settings.searchShortcut}
          spellcheck="false"
          onchange={(e) => update({ searchShortcut: e.currentTarget.value.trim() || 'Ctrl+Space' })}
        />
        <p class="muted hint">{t('settings.shortcutHint')}</p>
        {#if info?.shortcutError}
          <p class="warn" role="alert">{t('settings.shortcutError', { value: info.shortcutError })}</p>
        {/if}
      </div>
    </div>
  </div>

  <div class="group">
    <h2>{t('settings.ignored')}</h2>
    {#if ignored.length === 0}
      <p class="muted">{t('settings.ignoredEmpty')}</p>
    {:else}
      <ul class="ignored">
        {#each ignored as v (v.id)}
          <li>
            <span>{volumeName(v)}{#if v.mount}{' '}<span class="muted">· {v.mount}</span>{/if}</span>
            <button onclick={() => unignore(v)}>{t('settings.stopIgnoring')}</button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  {#if info}
    <div class="group">
      <h2>{t('settings.about')}</h2>
      <p>{t('settings.version', { version: info.version })}</p>
      <p class="muted">{info.elevated ? t('settings.elevated') : t('settings.notElevated')}</p>
      <p class="muted path">{t('settings.dataDir', { dir: info.dataDir })}</p>
    </div>
  {/if}
</section>

<style>
  .settings {
    max-width: 640px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  h1 {
    font-size: 24px;
  }
  .group {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px 18px;
    display: grid;
    gap: 12px;
  }
  h2 {
    font-size: 15px;
  }
  .row {
    display: grid;
    grid-template-columns: 180px 1fr;
    align-items: center;
    gap: 12px;
  }
  .row label {
    margin: 0;
    color: var(--text);
    font-size: 14px;
  }
  .row.check {
    grid-template-columns: auto 1fr;
    align-items: start;
  }
  .row.check input {
    width: 18px;
    height: 18px;
    margin-top: 2px;
  }
  .warn {
    margin: 4px 0 0;
    color: var(--warn);
    font-size: 13px;
  }
  .hint {
    margin: 2px 0 0;
    font-size: 12.5px;
  }
  .group p {
    margin: 0;
  }
  .path {
    word-break: break-all;
    font-size: 12.5px;
  }
  .ignored {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }
  .ignored li {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
</style>
