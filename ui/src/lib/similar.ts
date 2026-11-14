// Deleting files of groups of similar images and videos (REVIEW D67): the selection of the
// screen and what it asks the backend. Similar files are not identical copies, so the only
// protection is that at least one file of each group stays; the backend checks it again.
import type { SimilarDeletion } from './bindings/SimilarDeletion';
import type { SimilarFileView } from './bindings/SimilarFileView';
import type { SimilarGroupView } from './bindings/SimilarGroupView';

/** The files chosen for deletion, by group key. */
export type SimilarSelection = Map<string, Set<number>>;

/** Whether `file` may be chosen in `group`: on a connected disk, and not the last file shown. */
export function canPick(group: SimilarGroupView, file: SimilarFileView, picked: Set<number>): boolean {
  return file.online && (picked.has(file.entryId) || picked.size + 1 < group.files.length);
}

/** The selection with `file` chosen, or no longer chosen. */
export function togglePick(selection: SimilarSelection, group: SimilarGroupView, file: SimilarFileView): SimilarSelection {
  const picked = new Set(selection.get(group.key) ?? []);
  if (picked.has(file.entryId)) picked.delete(file.entryId);
  else if (canPick(group, file, picked)) picked.add(file.entryId);
  else return selection;
  const next = new Map(selection);
  if (picked.size === 0) next.delete(group.key);
  else next.set(group.key, picked);
  return next;
}

/** What is chosen: files, bytes, whether some are on disks without a Recycle Bin, and the request. */
export function pickTotals(groups: SimilarGroupView[], selection: SimilarSelection) {
  let count = 0;
  let bytes = 0;
  let permanent = false;
  const deletions: SimilarDeletion[] = [];
  for (const g of groups) {
    const picked = selection.get(g.key);
    if (!picked || picked.size === 0) continue;
    const doomed = g.files.filter((f) => picked.has(f.entryId));
    count += doomed.length;
    bytes += doomed.reduce((s, f) => s + f.size, 0);
    permanent ||= doomed.some((f) => f.permanent);
    deletions.push({
      delete: doomed.map((f) => f.entryId),
      keep: g.files.filter((f) => !picked.has(f.entryId)).map((f) => f.entryId),
    });
  }
  return { count, bytes, permanent, deletions };
}
