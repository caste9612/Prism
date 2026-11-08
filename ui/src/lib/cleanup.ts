// Presentation logic of the cleanup screen (F09), kept out of the component to be testable.
import type { CleanupGroupView } from './bindings/CleanupGroupView';

/** A stable key for a group: one category of one volume (and one browser for caches). */
export function groupKey(g: CleanupGroupView): string {
  return `${g.volumeId}:${g.category}:${g.owner ?? ''}`;
}

/**
 * What is selected when the proposal appears (F09-AC3: everything can be deselected).
 * Temporary files, empty folders and caches are pre-selected; large old files are the user's
 * own files and are never pre-selected; nothing of a blocked group (browser running) is.
 */
export function defaultSelection(g: CleanupGroupView): Set<number> {
  if (g.blockedBy !== null || g.category === 'large_old') return new Set();
  return new Set(g.items.map((i) => i.id));
}

/** Bytes of the selected items of a group. */
export function selectedBytes(g: CleanupGroupView, selected: Set<number>): number {
  return g.items.reduce((a, i) => (selected.has(i.id) ? a + i.size : a), 0);
}

/** The order of the groups on screen: largest recoverable space first, blocked ones last. */
export function orderGroups(groups: CleanupGroupView[]): CleanupGroupView[] {
  return [...groups].sort(
    (a, b) => Number(a.blockedBy !== null) - Number(b.blockedBy !== null) || b.total - a.total,
  );
}
