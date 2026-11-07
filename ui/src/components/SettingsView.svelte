<script lang="ts">
  // F21: language, theme, start with Windows, search shortcut, content analysis (F06-AC5,
  // F21-AC3), ignored disks (F02-AC4), about.
  import type { AppInfo } from '../lib/bindings/AppInfo';
  import type { Settings } from '../lib/bindings/Settings';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import type { DiagnosticsView } from '../lib/bindings/DiagnosticsView';
  import type { UpdateView } from '../lib/bindings/UpdateView';
  import { onMount } from 'svelte';
  import { backend, errorMessage } from '../lib/api';
  import { formatBytes, formatCount } from '../lib/format';
  import Dialog from './Dialog.svelte';
  import { language, languages, t } from '../lib/i18n.svelte';
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
  const lang = $derived(language());
  let diagnostics = $state<DiagnosticsView | null>(null);
  let exported = $state<string | null>(null);
  let resetting = $state(false);
  let updates = $state<UpdateView | null>(null);
  let confirmUpdate = $state(false);

  onMount(() => {
    let off: (() => void) | undefined;
    const read = () => void backend.updateStatus().then((u) => (updates = u), () => {});
    read();
    void backend.onUpdateChanged(read).then((f) => (off = f));
    return () => off?.();
  });

  async function checkNow() {
    try {
      updates = await backend.checkUpdates();
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  async function installNow() {
    confirmUpdate = false;
    try {
      await backend.installUpdate();
      updates = await backend.updateStatus();
    } catch (e) {
      onerror(errorMessage(e));
    }
  }
  let resetUnderstood = $state(false);

  async function openLogs() {
    try {
      await backend.openLogs();
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  async function previewPackage() {
    try {
      diagnostics = await backend.diagnosticsPreview();
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  async function exportPackage() {
    diagnostics = null;
    try {
      exported = await backend.exportDiagnostics();
    } catch (e) {
      onerror(errorMessage(e));
    }
  }

  async function resetCatalog() {
    resetting = false;
    try {
      const n = await backend.resetCatalog();
      onerror(t('settings.resetDone', { count: formatCount(n, lang) }));
    } catch (e) {
      onerror(errorMessage(e));
    }
  }
  // Network drives are never analysed (REVIEW D30): no choice to offer for them.
  const analysable = $derived(volumes.filter((v) => v.lastIndexedAt !== null && v.kind !== 'network'));

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
    <h2>{t('settings.hashing')}</h2>
    <p class="muted hint">{t('settings.hashingHint')}</p>
    <div class="row check">
      <input id="set-hash-paused" type="checkbox" checked={settings.hashingPaused} onchange={(e) => update({ hashingPaused: e.currentTarget.checked })} />
      <label for="set-hash-paused">{t('settings.hashingPaused')}</label>
    </div>
    <div class="row check">
      <input
        id="set-hash-battery"
        type="checkbox"
        checked={settings.pauseHashingOnBattery}
        onchange={(e) => update({ pauseHashingOnBattery: e.currentTarget.checked })}
      />
      <label for="set-hash-battery">{t('settings.hashingBattery')}</label>
    </div>
    {#if analysable.length > 0}
      <fieldset class="disks">
        <legend>{t('settings.hashingDisks')}</legend>
        {#each analysable as v (v.id)}
          <label class="disk">
            <input
              type="checkbox"
              checked={!settings.hashingExcluded.includes(v.id)}
              onchange={(e) =>
                update({
                  hashingExcluded: e.currentTarget.checked
                    ? settings.hashingExcluded.filter((id) => id !== v.id)
                    : [...settings.hashingExcluded, v.id],
                })}
            />
            <span>{volumeName(v)}{#if v.mount}{' '}<span class="muted">· {v.mount}</span>{/if}</span>
          </label>
        {/each}
      </fieldset>
    {/if}
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

  {#if updates}
    <div class="group">
      <h2>{t('updates.title')}</h2>
      {#if !updates.enabled}
        <p class="muted hint">{t('updates.notConfigured')}</p>
      {:else}
        <p aria-live="polite">
          {#if updates.ready && updates.available}
            {t('updates.ready', { version: updates.available })}
          {:else if updates.available}
            {t('updates.downloading', { version: updates.available })}
          {:else if updates.checking}
            {t('updates.checking')}
          {:else}
            {t('updates.upToDate')}
          {/if}
        </p>
        {#if updates.notes && updates.available}
          <p class="muted hint">{updates.notes}</p>
        {/if}
        {#if updates.error}
          <p class="warn">{t('updates.failed')}</p>
        {/if}
        <div class="buttons">
          {#if updates.ready}
            <button class="primary" onclick={() => (confirmUpdate = true)}>{t('updates.install')}</button>
          {/if}
          <button onclick={checkNow} disabled={updates.checking}>{t('updates.checkNow')}</button>
        </div>
      {/if}
    </div>
  {/if}

  <div class="group">
    <h2>{t('settings.diagnostics')}</h2>
    <p class="muted hint">{t('settings.diagnosticsHint')}</p>
    <div class="buttons">
      <button onclick={openLogs}>{t('settings.openLogs')}</button>
      <button onclick={previewPackage}>{t('settings.exportDiagnostics')}</button>
    </div>
    {#if exported}
      <p class="muted path" role="status">{t('settings.exported', { path: exported })}</p>
    {/if}
  </div>

  <div class="group">
    <h2>{t('settings.catalog')}</h2>
    <p class="muted hint">{t('settings.resetHint')}</p>
    <div class="buttons">
      <button class="danger-outline" onclick={() => ((resetting = true), (resetUnderstood = false))}>{t('settings.reset')}</button>
    </div>
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

<Dialog open={diagnostics !== null} title={t('settings.packageTitle')} onclose={() => (diagnostics = null)}>
  {#if diagnostics}
    <p>{t('settings.packageIntro')}</p>
    <ul class="files">
      {#each diagnostics.files as f (f.name)}
        <li><span>{f.name}</span><span class="muted">{formatBytes(f.size, lang)}</span></li>
      {/each}
    </ul>
    <pre class="system" aria-label="system.txt">{diagnostics.system}</pre>
    <p class="muted">{t('settings.packageWhere', { dir: diagnostics.destination })}</p>
  {/if}
  {#snippet actions()}
    <button onclick={() => (diagnostics = null)}>{t('actions.cancel')}</button>
    <button class="primary" onclick={exportPackage}>{t('settings.exportNow')}</button>
  {/snippet}
</Dialog>

<Dialog open={confirmUpdate} title={t('updates.confirmTitle', { version: updates?.available ?? '' })} onclose={() => (confirmUpdate = false)}>
  <p>{t('updates.confirm')}</p>
  {#snippet actions()}
    <button onclick={() => (confirmUpdate = false)}>{t('actions.cancel')}</button>
    <button class="primary" onclick={installNow}>{t('updates.installNow')}</button>
  {/snippet}
</Dialog>

<Dialog open={resetting} title={t('settings.resetTitle')} onclose={() => (resetting = false)}>
  <p>{t('settings.resetWhat')}</p>
  <p><strong>{t('settings.resetKeep')}</strong></p>
  <label class="understand">
    <input type="checkbox" checked={resetUnderstood} onchange={(e) => (resetUnderstood = e.currentTarget.checked)} />
    {t('settings.resetUnderstood')}
  </label>
  {#snippet actions()}
    <button onclick={() => (resetting = false)}>{t('actions.cancel')}</button>
    <button class="primary danger" disabled={!resetUnderstood} onclick={resetCatalog}>{t('settings.resetNow')}</button>
  {/snippet}
</Dialog>

<style>
  .buttons {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .danger-outline {
    color: var(--danger);
  }
  .files {
    list-style: none;
    margin: 0 0 8px;
    padding: 0;
    font-size: 13px;
  }
  .files li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
  .system {
    max-height: 220px;
    overflow: auto;
    background: var(--surface-2);
    border-radius: 8px;
    padding: 8px 10px;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .understand {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 8px 0 0;
    color: var(--text);
  }
  .understand input {
    width: auto;
  }
  .disks {
    border: 0;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .disks legend {
    font-size: 14px;
    margin-bottom: 6px;
  }
  .disk {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    color: var(--text);
    font-size: 14px;
  }
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
