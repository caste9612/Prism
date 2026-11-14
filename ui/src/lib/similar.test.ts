import { describe, expect, it } from 'vitest';
import type { SimilarFileView } from './bindings/SimilarFileView';
import type { SimilarGroupView } from './bindings/SimilarGroupView';
import { canPick, pickTotals, togglePick, type SimilarSelection } from './similar';

const file = (entryId: number, size: number, online = true, permanent = false): SimilarFileView => ({
  entryId,
  volumeId: 1,
  disk: 'C:',
  location: null,
  path: `p\\${entryId}.jpg`,
  fullPath: `C:\\p\\${entryId}.jpg`,
  width: 100,
  height: 100,
  size,
  durationMs: null,
  online,
  permanent,
  distance: 0,
});
const group = (key: string, files: SimilarFileView[]): SimilarGroupView => ({ key, files, count: files.length });

// REVIEW D67: files of a group of similar files can be chosen for deletion, never the last one
// of the group nor one on a disk that is not connected; the request lists what stays.
describe('deleting similar files', () => {
  it('one file of each group always stays', () => {
    const g = group('a', [file(1, 100), file(2, 50), file(3, 10)]);
    let s: SimilarSelection = new Map();
    s = togglePick(s, g, g.files[1]!);
    s = togglePick(s, g, g.files[2]!);
    expect([...s.get('a')!]).toEqual([2, 3]);
    expect(canPick(g, g.files[0]!, s.get('a')!)).toBe(false);
    expect(togglePick(s, g, g.files[0]!)).toBe(s);
    // Taking one back frees another.
    s = togglePick(s, g, g.files[2]!);
    expect(canPick(g, g.files[0]!, s.get('a')!)).toBe(true);
    expect(togglePick(togglePick(new Map(), g, g.files[1]!), g, g.files[1]!).size).toBe(0);
  });

  it('files on disconnected disks are not chosen', () => {
    const g = group('a', [file(1, 100), file(2, 50, false), file(3, 10)]);
    expect(canPick(g, g.files[1]!, new Set())).toBe(false);
    expect(togglePick(new Map(), g, g.files[1]!).size).toBe(0);
  });

  it('totals and the request to the backend', () => {
    const a = group('a', [file(1, 100), file(2, 50)]);
    const b = group('b', [file(3, 1000), file(4, 400, true, true), file(5, 7)]);
    let s: SimilarSelection = new Map();
    s = togglePick(s, a, a.files[1]!);
    s = togglePick(s, b, b.files[1]!);
    expect(pickTotals([a, b], s)).toEqual({
      count: 2,
      bytes: 450,
      permanent: true,
      deletions: [
        { delete: [2], keep: [1] },
        { delete: [4], keep: [3, 5] },
      ],
    });
    expect(pickTotals([a, b], new Map())).toEqual({ count: 0, bytes: 0, permanent: false, deletions: [] });
  });
});
