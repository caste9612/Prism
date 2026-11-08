import { describe, expect, it } from 'vitest';
import type { ActionView } from './bindings/ActionView';
import { BATCH_GAP_MS, batchActions } from './actions';
import { barParts } from './categories.svelte';
import { catalogCards, diskQuery, filterCounts, knownLocations, LABEL_COLORS, stickerColor } from './catalog';
import { sampleVolumes } from './mock';

// The catalog of the B+C interface (ADR-0019): last actions, filters, order, colours, bars.

const record = (at: number, o: Partial<ActionView> = {}): ActionView => ({
  at,
  action: 'recycle',
  volumeId: 2,
  path: 'D:\\x',
  size: 10,
  outcome: 'done',
  ...o,
});

describe('last actions', () => {
  it('one line per operation: same action and disk, records close in time', () => {
    const now = 1_000_000_000;
    const lines = batchActions([
      record(now),
      record(now - 1_000),
      record(now - 2_000, { outcome: 'skipped_changed', size: 0 }),
      record(now - 3_000, { outcome: 'failed', size: 0 }),
      // Another disk, then the same disk much earlier: two more operations.
      record(now - 4_000, { volumeId: 1, size: 5 }),
      record(now - 4_000 - BATCH_GAP_MS - 1, { volumeId: 1, action: 'delete', size: 7 }),
    ]);
    expect(lines).toEqual([
      { at: now, action: 'recycle', volumeId: 2, done: 2, bytes: 20, skipped: 1, failed: 1 },
      { at: now - 4_000, action: 'recycle', volumeId: 1, done: 1, bytes: 5, skipped: 0, failed: 0 },
      { at: now - 4_000 - BATCH_GAP_MS - 1, action: 'delete', volumeId: 1, done: 1, bytes: 7, skipped: 0, failed: 0 },
    ]);
  });

  it('a long operation stays one line, and the list stops at the limit', () => {
    const now = 2_000_000_000;
    const long = Array.from({ length: 50 }, (_, i) => record(now - i * 30_000));
    expect(batchActions(long)).toHaveLength(1);
    const many = Array.from({ length: 10 }, (_, i) => record(now - i * 10 * BATCH_GAP_MS));
    expect(batchActions(many, 4)).toHaveLength(4);
    expect(batchActions([])).toEqual([]);
  });
});

describe('catalog cards', () => {
  const volumes = sampleVolumes(0);

  it('filters: connected local disks, network, in the drawer; ignored never shown', () => {
    expect(filterCounts(volumes)).toEqual({ all: 5, online: 3, network: 1, offline: 1 });
    const ignored = volumes.map((v) => (v.id === 5 ? { ...v, status: 'ignored' } : v));
    expect(filterCounts(ignored).all).toBe(4);
    expect(catalogCards(ignored, 'all', 'name').map((v) => v.id)).not.toContain(5);
    expect(catalogCards(volumes, 'network', 'name').map((v) => v.name)).toEqual(['NAS Big-Storage']);
    expect(catalogCards(volumes, 'offline', 'name').map((v) => v.name)).toEqual(['Backup Foto']);
  });

  it('connected first, then network, then the drawer; within each the chosen order', () => {
    expect(catalogCards(volumes, 'all', 'name').map((v) => v.name)).toEqual(['Data', 'KINGSTON', 'Windows', 'NAS Big-Storage', 'Backup Foto']);
    // Space used: Data (648 GB) before Windows (255 GB) before the stick (13 GB).
    expect(catalogCards(volumes, 'online', 'used').map((v) => v.name)).toEqual(['Data', 'Windows', 'KINGSTON']);
    // By location: disks without one last.
    const placed = volumes.map((v) => (v.id === 1 ? { ...v, location: 'Armadio' } : v));
    expect(catalogCards(placed, 'online', 'location').map((v) => v.name)).toEqual(['Windows', 'Data', 'KINGSTON']);
  });

  it('every disk has a sticker colour: its own, or a stable one from its id', () => {
    expect(stickerColor({ id: 3, color: '#f59e0b' })).toBe('#f59e0b');
    expect(stickerColor({ id: 7, color: null })).toBe(stickerColor({ id: 7, color: '  ' }));
    expect(stickerColor({ id: 7, color: null })).toMatch(/^#[0-9a-f]{6}$/);
    expect(new Set(LABEL_COLORS.map((c) => c.key)).size).toBe(LABEL_COLORS.length);
  });

  it('locations already used are suggested once each, sorted', () => {
    const v = sampleVolumes(0).map((x, i) => ({ ...x, location: ['ufficio', 'cassetto 2', 'Ufficio', null, ' '][i] ?? null }));
    expect(knownLocations(v)).toEqual(['cassetto 2', 'ufficio']);
  });

  it('a search limited to one disk', () => {
    expect(diskQuery({ name: 'Backup Foto' })).toBe('disk:"Backup Foto" ');
    expect(diskQuery({ name: 'Odd "name"' })).toBe('disk:"Odd name" ');
  });
});

describe('coloured bars of the cards', () => {
  it('the used part of the disk split in proportion to the categories', () => {
    const parts = barParts(0.6, [
      { category: 'videos', bytes: 300 },
      { category: 'images', bytes: 100 },
    ]);
    expect(parts.map((p) => p.category)).toEqual(['videos', 'images']);
    expect(parts[0]!.width).toBeCloseTo(0.45);
    expect(parts[1]!.width).toBeCloseTo(0.15);
  });

  it('nothing when the used space or the categories are unknown', () => {
    expect(barParts(null, [{ category: 'videos', bytes: 1 }])).toEqual([]);
    expect(barParts(0.5, [])).toEqual([]);
  });
});
