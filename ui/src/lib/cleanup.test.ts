import { describe, expect, it } from 'vitest';
import type { CleanupGroupView } from './bindings/CleanupGroupView';
import { defaultSelection, groupKey, orderGroups, selectedBytes } from './cleanup';

function group(category: string, sizes: number[], extra: Partial<CleanupGroupView> = {}): CleanupGroupView {
  const items = sizes.map((size, i) => ({ id: i + 1, path: `f${i}`, isDir: false, size, files: 1, modified: null }));
  return {
    category,
    volumeId: 1,
    owner: null,
    blockedBy: null,
    permanent: false,
    items,
    count: items.length,
    total: sizes.reduce((a, b) => a + b, 0),
    ...extra,
  };
}

describe('cleanup selection', () => {
  // F09-AC3: proposals start selected (except the user's large old files) and can be changed.
  it('pre-selects temporary files, empty folders and caches, never large old files', () => {
    expect([...defaultSelection(group('temp', [1, 2]))]).toEqual([1, 2]);
    expect([...defaultSelection(group('empty_folders', [0]))]).toEqual([1]);
    expect([...defaultSelection(group('browser_cache', [5], { owner: 'Chrome' }))]).toEqual([1]);
    expect(defaultSelection(group('large_old', [9])).size).toBe(0);
  });

  // F09-AC4: nothing of a running browser's cache is selected.
  it('blocked groups start empty', () => {
    expect(defaultSelection(group('browser_cache', [5], { owner: 'Firefox', blockedBy: 'Firefox' })).size).toBe(0);
  });

  it('selected bytes, keys and order', () => {
    const g = group('temp', [10, 20, 30]);
    expect(selectedBytes(g, new Set([1, 3]))).toBe(40);
    const chrome = group('browser_cache', [1], { owner: 'Chrome' });
    expect(groupKey(chrome)).toBe('1:browser_cache:Chrome');
    const blocked = group('browser_cache', [500], { owner: 'Firefox', blockedBy: 'Firefox' });
    const big = group('large_old', [100]);
    expect(orderGroups([g, blocked, big]).map((x) => x.category)).toEqual(['large_old', 'temp', 'browser_cache']);
  });
});
