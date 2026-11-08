// Presentation logic of the catalog screen (F01), kept out of the components to be testable.
import type { JobView } from './bindings/JobView';
import type { RaidView } from './bindings/RaidView';
import type { VolumeView } from './bindings/VolumeView';
import { t } from './i18n.svelte';

/** Volumes shown as cards: everything except the ignored ones (F02-AC4), connected first. */
export function visibleVolumes(volumes: VolumeView[]): { online: VolumeView[]; offline: VolumeView[] } {
  const shown = volumes.filter((v) => v.status !== 'ignored');
  const byName = (a: VolumeView, b: VolumeView) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: 'base', numeric: true });
  return {
    online: shown.filter((v) => v.online).sort(byName),
    offline: shown.filter((v) => !v.online).sort(byName),
  };
}

/** Connected volumes never indexed and not dismissed: the "new disk" prompts (F02-AC1). */
export function newVolumePrompts(volumes: VolumeView[], dismissed: Set<number>): VolumeView[] {
  return volumes.filter(
    (v) => v.online && v.status === 'new' && v.job === null && !dismissed.has(v.id),
  );
}

/**
 * Name shown for a volume. Without a user or system label the backend falls back to the mount
 * point; like Explorer, show it as "Local disk (C:)" instead of a bare "C:\". A network share is
 * called by its folder name ("Media"): the network icon and the server say the rest
 * (REVIEW D63).
 */
export function displayName(v: VolumeView, kindLabel: string): string {
  const labelled = [v.userName, v.osLabel].some((n) => n !== null && n.trim() !== '');
  if (labelled) return v.name;
  const share = (v.path ?? (v.kind === 'network' ? v.mount : null))?.replace(/\\+$/, '').split('\\').pop();
  if (v.kind === 'network' && share) return share;
  if (v.mount === null) return v.name;
  const shown = driveLetter(v.mount) ?? v.mount.replace(/\\+$/, '').split('\\').pop() ?? v.mount;
  return `${kindLabel} (${shown})`;
}

/** The server of a network share as shown: the name learned for it, or its address. */
export function serverLabel(v: VolumeView): string | null {
  return v.serverName ?? v.server;
}

/**
 * What the card says above the name: the location the user wrote, or for a network share its
 * server ("NAS · ZimaOS.local"), or the kind of disk.
 */
export function whereLabel(v: VolumeView): string {
  const location = v.location?.trim();
  if (location) return location;
  const server = v.kind === 'network' ? serverLabel(v) : null;
  return server ? t('group.nasShort', { server }) : t(`kind.${v.kind}`);
}

/** Cards shown together under one heading (REVIEW D64). */
export interface DiskGroup {
  key: string;
  /** `disk`: partitions of one physical disk; `server`: shares of one server; `single`: a disk whose physical disk is not known yet. */
  kind: 'disk' | 'server' | 'single';
  volumes: VolumeView[];
}

/**
 * The cards grouped by physical disk or by server, in the order of the cards: a group takes the
 * place of its first card.
 */
export function diskGroups(cards: VolumeView[]): DiskGroup[] {
  const groups = new Map<string, DiskGroup>();
  for (const v of cards) {
    const key = v.group ?? `volume:${v.id}`;
    let g = groups.get(key);
    if (!g) {
      g = { key, kind: v.group?.startsWith('net:') ? 'server' : v.group ? 'disk' : 'single', volumes: [] };
      groups.set(key, g);
    }
    g.volumes.push(v);
  }
  return [...groups.values()];
}

/** Heading of a group: the disk model and how it is connected, or the server, or the kind. */
export function groupTitle(g: DiskGroup): string {
  const first = g.volumes[0]!;
  if (g.kind === 'server') {
    const name = first.serverName;
    return name && first.server && name.toLowerCase() !== first.server.toLowerCase()
      ? t('group.nasNamed', { name, address: first.server })
      : t('group.nas', { server: first.server ?? '' });
  }
  if (g.kind === 'disk' && first.diskModel) {
    const bus = first.bus ? t(`group.bus.${first.bus}`) : null;
    return [first.diskModel, mediaLabel(first), bus && !bus.startsWith('group.bus.') ? bus : null].filter(Boolean).join(' · ');
  }
  return t(`kind.${first.kind}`);
}

/**
 * What the physical disk is (REVIEW D65): "SSD", "HDD 5400 rpm", "HDD"; null when not known, and
 * for a USB stick (flash, but not what anyone calls an SSD).
 */
export function mediaLabel(v: Pick<VolumeView, 'media' | 'rpm' | 'kind'>): string | null {
  if (v.kind === 'stick') return null;
  if (v.media === 'ssd') return t('media.ssd');
  if (v.media === 'hdd') return v.rpm ? t('media.hddRpm', { rpm: v.rpm }) : t('media.hdd');
  return null;
}

/** The array of a NAS share in a few words (REVIEW D66): "RAID 1 · OK", "RAID 1 · rebuilding 40%". */
export function raidLabel(r: RaidView): string {
  const what = r.level !== null ? t('raid.level', { level: r.level }) : t('raid.single');
  if (r.activity) {
    const known = ['resync', 'recovery', 'reshape', 'check'].includes(r.activity) ? r.activity : 'other';
    return `${what} · ${t(`raid.activity.${known}`)}${r.progress !== null ? ` ${r.progress}%` : ''}`;
  }
  return `${what} · ${t(`raid.short.${r.state}`)}`;
}

/** The colour class of a RAID state, as the health ones. */
export function raidTone(state: string): 'good' | 'warning' | 'critical' {
  return state === 'problem' ? 'critical' : state === 'attention' ? 'warning' : 'good';
}

/** The worst state of the arrays of some NAS shares, or null when none was read. */
export function worstRaid(volumes: VolumeView[]): string | null {
  const rank = (s: string) => (s === 'problem' ? 2 : s === 'attention' ? 1 : 0);
  let worst: string | null = null;
  for (const v of volumes) {
    if (v.raid && (worst === null || rank(v.raid.state) > rank(worst))) worst = v.raid.state;
  }
  return worst;
}

/** The drive letter of a mount, "X:", or null for a network path or a folder. */
export function driveLetter(mount: string | null): string | null {
  const m = mount === null ? null : /^([A-Za-z]:)\\?$/.exec(mount);
  return m ? m[1]!.toUpperCase() : null;
}

/** [`displayName`] in the current language. */
export function volumeName(v: VolumeView): string {
  return displayName(v, t(`kind.${v.kind}`));
}

/**
 * From this many prompts on (typically the first launch, when every disk is new) they are shown
 * as one compact panel with "Index all" instead of one large banner each.
 */
export const GROUP_PROMPTS_FROM = 3;

export function groupPrompts(prompts: VolumeView[]): boolean {
  return prompts.length >= GROUP_PROMPTS_FROM;
}

/**
 * The job shown on a volume card (F01-AC3): an indexing or update in progress. The automatic
 * background updates (`journal`, `refresh`) run often and quickly and are not shown; mirrors
 * `shown_on_card` in `prism-app/src/state.rs`.
 */
export function cardJob(jobs: JobView[], volumeId: number): JobView | null {
  return (
    jobs.find(
      (j) =>
        j.volumeId === volumeId &&
        (j.kind === 'index' || j.kind === 'update') &&
        (j.state === 'running' || j.state === 'queued'),
    ) ?? null
  );
}

/** The main action of a card: index a volume never indexed, update one already indexed. */
export function primaryAction(v: VolumeView): 'index' | 'update' | null {
  if (!v.online || v.job !== null || v.status === 'ignored') return null;
  return v.status === 'indexed' ? 'update' : 'index';
}

/** Colours offered for the volume labels (F03-AC1, ADR-0019), with their names (i18n keys). */
export const LABEL_COLORS: { key: string; color: string }[] = [
  { key: 'ink', color: '#111418' },
  { key: 'blue', color: '#2a78d6' },
  { key: 'green', color: '#1f8f4e' },
  { key: 'yellow', color: '#e5b000' },
  { key: 'orange', color: '#e8622c' },
  { key: 'pink', color: '#d1588a' },
  { key: 'purple', color: '#6250d6' },
  { key: 'grey', color: '#9aa0a8' },
];

/** Colours given to the disks without one of their own, so each card is told apart. */
const AUTO_COLORS = ['#2a78d6', '#1f8f4e', '#e5b000', '#e8622c', '#d1588a', '#6250d6'];

/** The colour of the disk's sticker: the user's, or a stable one from its id. */
export function stickerColor(v: Pick<VolumeView, 'id' | 'color'>): string {
  if (v.color !== null && v.color.trim() !== '') return v.color;
  return AUTO_COLORS[Math.abs(v.id) % AUTO_COLORS.length] ?? '#9aa0a8';
}

/** Percentage of analysed files, rounded down: 100 only when every file is done (F06-AC5). */
export function hashPercent(h: { total: number; hashed: number }): number {
  if (h.total <= 0) return 100;
  return Math.min(100, Math.floor((h.hashed / h.total) * 100));
}

/** The filters of the catalog (ADR-0019): connected local disks, network, in the drawer. */
export type DiskFilter = 'all' | 'online' | 'network' | 'offline';
export type DiskSort = 'location' | 'name' | 'used';

/** Network shares stay under "Network" when the server does not answer: they are not in a drawer. */
function group(v: VolumeView): 'online' | 'network' | 'offline' {
  if (v.kind === 'network') return 'network';
  return v.online ? 'online' : 'offline';
}

/** The state of a disk that is not connected: in the drawer, or a share whose server is off. */
export function offlineState(v: Pick<VolumeView, 'kind' | 'lastSeenAt'>, when: (ms: number) => string): string {
  const network = v.kind === 'network';
  if (v.lastSeenAt === null) return t(network ? 'card.unreachable' : 'card.inDrawer');
  return t(network ? 'card.unreachableSeen' : 'card.inDrawerSeen', { when: when(v.lastSeenAt) });
}

/** How many disks each filter shows (ignored disks are never shown, F02-AC4). */
export function filterCounts(volumes: VolumeView[]): Record<DiskFilter, number> {
  const shown = volumes.filter((v) => v.status !== 'ignored');
  return {
    all: shown.length,
    online: shown.filter((v) => group(v) === 'online').length,
    network: shown.filter((v) => group(v) === 'network').length,
    offline: shown.filter((v) => group(v) === 'offline').length,
  };
}

/**
 * The cards of the catalog for a filter and an order: connected disks first, then network,
 * then the ones in the drawer; within each group by the chosen order.
 */
export function catalogCards(volumes: VolumeView[], filter: DiskFilter, sort: DiskSort): VolumeView[] {
  const name = (v: VolumeView) => volumeName(v);
  const cmpName = (a: VolumeView, b: VolumeView) =>
    name(a).localeCompare(name(b), undefined, { sensitivity: 'base', numeric: true });
  const usedOf = (v: VolumeView) =>
    v.capacity !== null && v.freeSpace !== null ? v.capacity - v.freeSpace : v.indexedSize;
  const cmp = (a: VolumeView, b: VolumeView): number => {
    if (sort === 'used') return usedOf(b) - usedOf(a) || cmpName(a, b);
    if (sort === 'location') {
      const la = a.location?.trim() || '￿';
      const lb = b.location?.trim() || '￿';
      return la.localeCompare(lb, undefined, { sensitivity: 'base' }) || cmpName(a, b);
    }
    return cmpName(a, b);
  };
  const order = { online: 0, network: 1, offline: 2 };
  return volumes
    .filter((v) => v.status !== 'ignored' && (filter === 'all' || group(v) === filter))
    .sort((a, b) => order[group(a)] - order[group(b)] || cmp(a, b));
}

/** Physical locations already used by other disks, for the suggestions of the labels dialog. */
export function knownLocations(volumes: VolumeView[]): string[] {
  const seen = new Map<string, string>();
  for (const v of volumes) {
    const l = v.location?.trim();
    if (l && !seen.has(l.toLowerCase())) seen.set(l.toLowerCase(), l);
  }
  return [...seen.values()].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' }));
}

/** The search text limited to one disk (F01-AC4): `disk:"Name" `, ready to type after it. */
export function diskQuery(v: Pick<VolumeView, 'name'>): string {
  return `disk:"${v.name.replace(/"/g, '')}" `;
}
