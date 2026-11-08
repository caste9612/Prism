// The "last actions" panel of the catalog: the actions log (F16-AC6) has one record per file;
// the panel shows one line per operation.
import type { ActionView } from './bindings/ActionView';

export interface ActionBatch {
  /** Time of the newest record of the operation. */
  at: number;
  /** `recycle` or `delete`. */
  action: string;
  volumeId: number | null;
  /** Files handled, and their size. */
  done: number;
  bytes: number;
  /** Left in place because they changed or disappeared, or that failed. */
  skipped: number;
  failed: number;
}

/** Records closer than this belong to the same operation. */
export const BATCH_GAP_MS = 60_000;

/**
 * Groups the records (newest first, as the backend returns them) into operations: same action
 * and disk, each record within [`BATCH_GAP_MS`] of the previous one. At most `limit` lines.
 */
export function batchActions(records: ActionView[], limit = 4): ActionBatch[] {
  const batches: ActionBatch[] = [];
  let last: { batch: ActionBatch; at: number } | null = null;
  for (const r of records) {
    const same =
      last !== null &&
      last.batch.action === r.action &&
      last.batch.volumeId === r.volumeId &&
      Math.abs(last.at - r.at) <= BATCH_GAP_MS;
    if (!same) {
      if (batches.length === limit) break;
      const batch: ActionBatch = { at: r.at, action: r.action, volumeId: r.volumeId, done: 0, bytes: 0, skipped: 0, failed: 0 };
      batches.push(batch);
      last = { batch, at: r.at };
    }
    const b = last!.batch;
    last!.at = r.at;
    if (r.outcome === 'done') {
      b.done += 1;
      b.bytes += r.size;
    } else if (r.outcome === 'failed') {
      b.failed += 1;
    } else {
      b.skipped += 1;
    }
  }
  return batches;
}
