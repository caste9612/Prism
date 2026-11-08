<script lang="ts">
  // The NAS of the catalog in the settings (REVIEW D66): for each one whether Prism has its login,
  // when it was read and how its arrays are; connect with user and password (checked by reading
  // the NAS once, then kept in the Windows Credential Manager) or disconnect.
  import type { NasServerView } from '../lib/bindings/NasServerView';
  import type { VolumeView } from '../lib/bindings/VolumeView';
  import { backend, errorMessage } from '../lib/api';
  import { raidTone } from '../lib/catalog';
  import { ago } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import Dialog from './Dialog.svelte';

  let { volumes, onerror }: { volumes: VolumeView[]; onerror: (message: string) => void } = $props();

  let servers = $state<NasServerView[]>([]);
  // Read again when the catalog changes (a share added, a NAS read again).
  const raidKey = $derived(JSON.stringify(volumes.filter((v) => v.kind === 'network').map((v) => [v.id, v.server, v.raid])));
  $effect(() => {
    void raidKey;
    void backend.nasServers().then(
      (s) => (servers = s),
      (e) => onerror(errorMessage(e)),
    );
  });

  let connecting = $state<NasServerView | null>(null);
  let user = $state('');
  let password = $state('');
  let busy = $state(false);
  let failure = $state<string | null>(null);

  function open(s: NasServerView) {
    connecting = s;
    user = s.user ?? '';
    password = '';
    failure = null;
  }

  async function connect(e: SubmitEvent) {
    e.preventDefault();
    if (!connecting || busy) return;
    busy = true;
    failure = null;
    try {
      await backend.nasConnect(connecting.server, user, password);
      connecting = null;
      servers = await backend.nasServers();
    } catch (err) {
      failure = errorMessage(err);
    } finally {
      password = '';
      busy = false;
    }
  }

  async function disconnect(s: NasServerView) {
    try {
      await backend.nasDisconnect(s.server);
      servers = await backend.nasServers();
    } catch (err) {
      onerror(errorMessage(err));
    }
  }

  const when = (ms: number) => {
    const a = ago(ms);
    return a.key === 'time.justNow' ? t(a.key) : t(a.key, { count: a.count });
  };
  const title = (s: NasServerView) => (s.name && s.name.toLowerCase() !== s.server.toLowerCase() ? `${s.name} · ${s.server}` : s.server);
</script>

<p class="muted hint">{t('nas.hint')}</p>
{#if servers.length === 0}
  <p class="muted">{t('nas.empty')}</p>
{:else}
  <ul class="servers">
    {#each servers as s (s.server)}
      <li>
        <div class="who">
          <div class="name">{title(s)}</div>
          <div class="muted line">
            {#if s.shares > 0}{t('nas.shares', { count: s.shares })} · {/if}
            {#if s.connected}
              {t('nas.connected', { user: s.user ?? '' })}{#if s.readAt !== null}{' · '}{t('nas.readAt', { when: when(s.readAt) })}{/if}
            {:else}
              {t('nas.notConnected')}
            {/if}
          </div>
          {#if s.connected && s.error}
            <div class="warn line">{t(`nas.error.${s.error}`)}</div>
          {:else if s.connected && s.state}
            <div class="line">
              <span class="state {raidTone(s.state)}">{t(`raid.word.${s.state}`)}</span>
              <span class="muted">· {t('nas.summary', { storages: s.storages, disks: s.disks })}</span>
            </div>
          {/if}
        </div>
        {#if s.connected}
          <button onclick={() => disconnect(s)}>{t('nas.disconnect')}</button>
        {:else}
          <button onclick={() => open(s)}>{t('nas.connect')}</button>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

<Dialog open={connecting !== null} title={t('nas.connectTitle', { server: connecting ? title(connecting) : '' })} onclose={() => (connecting = null)}>
  <form id="nas-login" class="login" onsubmit={connect}>
    <p>{t('nas.connectIntro')}</p>
    <label for="nas-user">{t('nas.user')}</label>
    <input id="nas-user" autocomplete="username" spellcheck="false" bind:value={user} />
    <label for="nas-password">{t('nas.password')}</label>
    <input id="nas-password" type="password" autocomplete="current-password" bind:value={password} />
    {#if failure}<p class="warn" role="alert">{failure}</p>{/if}
  </form>
  {#snippet actions()}
    <button onclick={() => (connecting = null)}>{t('actions.cancel')}</button>
    <button class="primary" type="submit" form="nas-login" disabled={busy || user.trim() === '' || password === ''}>
      {busy ? t('nas.connecting') : t('nas.connectNow')}
    </button>
  {/snippet}
</Dialog>

<style>
  .hint {
    font-size: 12.5px;
    margin: 2px 0 0;
  }
  .servers {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .servers li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 0;
    border-top: 1px solid var(--line);
  }
  .who {
    min-width: 0;
  }
  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .line {
    font-size: 12.5px;
    margin-top: 2px;
  }
  .state.good {
    color: var(--ok);
  }
  .state.warning {
    color: var(--warn);
  }
  .state.critical {
    color: var(--danger);
  }
  .warn {
    color: var(--signal-strong);
  }
  .login {
    display: grid;
    gap: 6px;
  }
  .login p {
    margin: 0 0 6px;
  }
  .login label {
    margin: 6px 0 0;
    font-size: 13px;
    color: var(--text);
  }
</style>
