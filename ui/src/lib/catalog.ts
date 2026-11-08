// Presentation logic of the catalog screen (F01), kept out of the components to be testable.
import type { JobView } from './bindings/JobView';
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
 * point; like Explorer, show it as "Local disk (C:)" instead of a bare "C:\".
 */
export function displayName(v: VolumeView, kindLabel: string): string {
  const labelled = [v.userName, v.osLabel].some((n) => n !== null && n.trim() !== '');
  if (labelled || v.mount === null) return v.name;
  // A network share reached by its path (no letter): its share name, "Network drive (Foto)".
  const shown = driveLetter(v.mount) ?? v.mount.replace(/\\+$/, '').split('\\').pop() ?? v.mount;
  return `${kindLabel} (${shown})`;
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
