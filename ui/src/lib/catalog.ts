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
  return `${kindLabel} (${v.mount.replace(/\\$/, '')})`;
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

/** Colours offered for the volume labels (F03-AC1). */
export const LABEL_COLORS = ['#ef4444', '#f59e0b', '#10b981', '#0ea5e9', '#6366f1', '#d946ef', '#64748b'];
