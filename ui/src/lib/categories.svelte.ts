// What each disk holds by file category, for the coloured bars of the catalog cards (ADR-0019).
// The backend counts it from the catalog on demand (about a second for a million files), so the
// disks are asked one at a time in the background and the result is kept for a while: until
// then a card shows a plain bar.
import type { VolumeView } from './bindings/VolumeView';
import { backend } from './api';

export interface Segment {
  /** Category key: `images`, `videos`… */
  category: string;
  bytes: number;
}

/** A disk is counted again at most this often, even if it changes. */
export const RECOUNT_AFTER_MS = 10 * 60_000;

interface Entry {
  segments: Segment[];
  /** When it was counted, and the indexing it reflects. */
  at: number;
  stamp: number | null;
}

class CategoryCache {
  #entries = $state<Record<number, Entry>>({});
  #queue: number[] = [];
  #busy = false;

  /** The composition of a disk, biggest category first, or null while not counted yet. */
  segments(volumeId: number): Segment[] | null {
    return this.#entries[volumeId]?.segments ?? null;
  }

  /** Counts the indexed disks not counted yet, or changed since and counted long enough ago. */
  request(volumes: VolumeView[], now = Date.now()): void {
    for (const v of volumes) {
      if (v.lastIndexedAt === null || v.status === 'ignored' || this.#queue.includes(v.id)) continue;
      const e = this.#entries[v.id];
      if (e && (e.stamp === v.lastIndexedAt || now - e.at < RECOUNT_AFTER_MS)) continue;
      this.#queue.push(v.id);
    }
    void this.#drain(volumes);
  }

  async #drain(volumes: VolumeView[]): Promise<void> {
    if (this.#busy) return;
    this.#busy = true;
    try {
      while (this.#queue.length > 0) {
        const id = this.#queue.shift()!;
        const stamp = volumes.find((v) => v.id === id)?.lastIndexedAt ?? null;
        try {
          const stats = await backend.typeStats(id);
          const segments = stats.categories
            .filter((c) => c.totals.bytes > 0)
            .map((c) => ({ category: c.key, bytes: c.totals.bytes }))
            .sort((a, b) => b.bytes - a.bytes);
          this.#entries = { ...this.#entries, [id]: { segments, at: Date.now(), stamp } };
        } catch {
          // A disk removed meanwhile, or the catalog busy: its card keeps the plain bar.
        }
      }
    } finally {
      this.#busy = false;
    }
  }
}

export const categories = new CategoryCache();

/**
 * Widths (0–1) of the coloured parts of a disk bar: the used fraction of the disk, divided in
 * proportion to the categories; null when the used space is unknown.
 */
export function barParts(used: number | null, segments: Segment[]): { category: string; width: number }[] {
  const total = segments.reduce((s, x) => s + x.bytes, 0);
  if (used === null || total <= 0) return [];
  return segments.map((s) => ({ category: s.category, width: (used * s.bytes) / total }));
}
