import { describe, expect, it } from 'vitest';
import type { DupFileView } from './bindings/DupFileView';
import type { DupGroupView } from './bindings/DupGroupView';
import { hashPercent } from './catalog';
import { canSelect, onlyOfflineLeft, proposed, ruleComplete, toggle, totals } from './duplicates';
import { mockDeleteDuplicates, mockDuplicates } from './mock-duplicates';
import { sampleVolumes } from './mock';

function file(entryId: number, online = true, extra: Partial<DupFileView> = {}): DupFileView {
  return {
    entryId,
    volumeId: 1,
    disk: 'D',
    location: null,
    path: `f${entryId}`,
    fullPath: `D:\\f${entryId}`,
    modified: null,
    online,
    permanent: false,
    ...extra,
  };
}

function group(files: DupFileView[], selected: number[] = [], size = 10): DupGroupView {
  return { key: `g${files[0]?.entryId}`, size, recoverable: size * (files.length - 1), kind: 'other', files, selected };
}

describe('duplicate selection', () => {
  // F10-AC4: copies on disconnected disks are never selectable; one copy always stays.
  it('offline copies and the last copy cannot be selected', () => {
    const g = group([file(1), file(2), file(3, false)]);
    let s = proposed([g]);
    expect(canSelect(g, g.files[2]!, s.get(g.key)!)).toBe(false);
    s = toggle(s, g, g.files[0]!);
    s = toggle(s, g, g.files[1]!);
    // The offline copy stays: both connected copies may go, with a warning.
    expect([...s.get(g.key)!]).toEqual([1, 2]);
    expect(onlyOfflineLeft(g, s.get(g.key)!)).toBe(true);

    const two = group([file(4), file(5)]);
    let t = toggle(proposed([two]), two, two.files[0]!);
    expect(toggle(t, two, two.files[1]!)).toBe(t);
    expect(canSelect(two, two.files[1]!, t.get(two.key)!)).toBe(false);
    t = toggle(t, two, two.files[0]!);
    expect(t.get(two.key)!.size).toBe(0);
  });

  it('totals count the selected copies and whether some deletion is permanent', () => {
    const a = group([file(1), file(2, true, { permanent: true })], [2], 100);
    const b = group([file(3), file(4)], [], 7);
    let s = proposed([a, b]);
    expect(totals([a, b], s)).toEqual({ count: 1, bytes: 100, permanent: true, ids: [2] });
    s = toggle(s, b, b.files[1]!);
    expect(totals([a, b], s)).toEqual({ count: 2, bytes: 107, permanent: true, ids: [2, 4] });
  });

  it('rules need what their kind needs', () => {
    expect(ruleComplete({ kind: 'keepOldest', volumeId: null, text: null })).toBe(true);
    expect(ruleComplete({ kind: 'keepOnVolume', volumeId: null, text: null })).toBe(false);
    expect(ruleComplete({ kind: 'keepOnVolume', volumeId: 2, text: null })).toBe(true);
    expect(ruleComplete({ kind: 'neverIn', volumeId: null, text: '  ' })).toBe(false);
    expect(ruleComplete({ kind: 'neverIn', volumeId: null, text: 'Backup' })).toBe(true);
    expect(ruleComplete({ kind: 'nope', volumeId: null, text: null })).toBe(false);
  });

  // F06-AC5: 100% only when every file is analysed.
  it('hash progress is rounded down', () => {
    expect(hashPercent({ total: 1000, hashed: 999 })).toBe(99);
    expect(hashPercent({ total: 1000, hashed: 1000 })).toBe(100);
    expect(hashPercent({ total: 0, hashed: 0 })).toBe(100);
  });
});

describe('simulated duplicates follow the backend rules', () => {
  const volumes = sampleVolumes();
  const all = { minSize: 0, category: null, volumeId: null, folder: null };

  it('filters, rules and offline copies', () => {
    const view = mockDuplicates(volumes, all, [{ kind: 'keepShortestPath', volumeId: null, text: null }], new Set());
    expect(view.groups.map((g) => g.kind)).toEqual(['videos', 'disk_images', 'archives', 'images', 'documents']);
    const video = view.groups[0]!;
    // Kept: the shortest path on Data; the copy on the disconnected backup is never selected.
    expect(video.selected).toEqual([103]);
    const album = view.groups[2]!;
    expect(album.selected).toEqual([]);
    expect(mockDuplicates(volumes, { ...all, category: 'images' }, [], new Set()).groups).toHaveLength(1);
    expect(mockDuplicates(volumes, { ...all, folder: 'da ordinare' }, [], new Set()).groups).toHaveLength(1);
    expect(mockDuplicates(volumes, { ...all, minSize: 1_000_000_000 }, [], new Set()).groups).toHaveLength(1);
    const never = mockDuplicates(volumes, all, [{ kind: 'neverIn', volumeId: null, text: 'archivio' }], new Set());
    expect(never.groups[0]!.selected).toEqual([]);
  });

  // F16-AC4 in the simulated backend, as in prism-core::duplicates::check_last_copies.
  it('the last copy and offline-only copies are refused', () => {
    const gone = new Set<number>();
    expect(() => mockDeleteDuplicates(volumes, [501, 502], false, false, gone)).toThrow();
    // Deleting the two connected copies of the photo leaves only the one on the backup disk.
    expect(() => mockDeleteDuplicates(volumes, [401, 402], false, false, gone)).toThrowError();
    try {
      mockDeleteDuplicates(volumes, [401, 402], false, false, gone);
    } catch (e) {
      expect((e as { code: string }).code).toBe('only_offline_copies');
    }
    const r = mockDeleteDuplicates(volumes, [401, 402], true, false, gone);
    expect(r.done).toBe(2);
    expect(gone).toEqual(new Set([401, 402]));
  });
});
