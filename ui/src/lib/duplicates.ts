// Selection of the copies to delete on the duplicates screen (F10-AC3/AC4). The backend proposes
// a selection with the rules; the user may change it, within two limits enforced here and again
// by the backend: copies on disconnected disks cannot be selected, and every group keeps at
// least one copy.
import type { DupFileView } from './bindings/DupFileView';
import type { DupGroupView } from './bindings/DupGroupView';
import type { RuleInput } from './bindings/RuleInput';

export type Selection = Map<string, Set<number>>;

/** The selection the rules proposed. */
export function proposed(groups: DupGroupView[]): Selection {
  return new Map(groups.map((g) => [g.key, new Set(g.selected)]));
}

/** Whether a copy can be checked now: connected, and not the last unchecked copy. */
export function canSelect(group: DupGroupView, file: DupFileView, chosen: Set<number>): boolean {
  if (!file.online) return false;
  if (chosen.has(file.entryId)) return true;
  return group.files.filter((f) => !chosen.has(f.entryId)).length > 1;
}

/** Checks or unchecks a copy; refused changes leave the selection as it is. */
export function toggle(selection: Selection, group: DupGroupView, file: DupFileView): Selection {
  const chosen = new Set(selection.get(group.key) ?? []);
  if (chosen.has(file.entryId)) chosen.delete(file.entryId);
  else if (canSelect(group, file, chosen)) chosen.add(file.entryId);
  else return selection;
  return new Map(selection).set(group.key, chosen);
}

/** Copies, bytes and disks involved in the current selection. */
export function totals(groups: DupGroupView[], selection: Selection) {
  let count = 0;
  let bytes = 0;
  let permanent = false;
  const ids: number[] = [];
  for (const g of groups) {
    const chosen = selection.get(g.key);
    if (!chosen) continue;
    for (const f of g.files) {
      if (!chosen.has(f.entryId)) continue;
      count += 1;
      bytes += g.size;
      permanent ||= f.permanent;
      ids.push(f.entryId);
    }
  }
  return { count, bytes, permanent, ids };
}

/** Whether the copies left in a group would all be on disconnected disks (F16-AC4). */
export function onlyOfflineLeft(group: DupGroupView, chosen: Set<number>): boolean {
  const left = group.files.filter((f) => !chosen.has(f.entryId));
  return chosen.size > 0 && left.length > 0 && left.every((f) => !f.online);
}

/** The kinds of rule the screen offers, in the order shown. */
export const RULE_KINDS = ['keepOnVolume', 'keepOldest', 'keepNewest', 'keepShortestPath', 'neverIn'] as const;
export type RuleKind = (typeof RULE_KINDS)[number];

/** A rule is complete when it has what its kind needs. */
export function ruleComplete(r: RuleInput): boolean {
  if (r.kind === 'keepOnVolume') return r.volumeId !== null;
  if (r.kind === 'neverIn') return (r.text ?? '').trim() !== '';
  return RULE_KINDS.includes(r.kind as RuleKind);
}

/** Size thresholds of the filter, in bytes. */
export const MIN_SIZES = [0, 100_000, 1_000_000, 10_000_000, 100_000_000, 1_000_000_000];
