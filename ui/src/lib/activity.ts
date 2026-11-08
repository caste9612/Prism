// The "In progress" panel of the catalog: what the background jobs are doing now, how far they
// are, since when, and roughly how long is left. The jobs report their phase and progress
// (`JobView`); the speed is measured here, from what was seen in this session.
import type { JobView } from './bindings/JobView';
import type { VolumeView } from './bindings/VolumeView';

/** The jobs shown in the panel; the quick automatic ones (journal, refresh) are not. */
const SHOWN = ['index', 'update', 'copy', 'hash'];

/** The phases the jobs report that have a text in the panel (`activity.phase.*`). */
export const PHASES = ['scanning', 'writing', 'copying', 'hashQuick', 'hashFull', 'prints', 'videoPrints'];

export interface ActivityRow {
  key: string;
  /** `index`, `update`, `copy`, `hash`, or `waiting` (content analysis with files left, idle). */
  kind: string;
  volumeId: number | null;
  /** The phase reported by the job (`hashQuick`, `prints`, `scanning`…), if any. */
  phase: string | null;
  done: number;
  /** 0 when not known. */
  total: number;
  /** 0–100, rounded down; null when the total is not known. */
  percent: number | null;
  queued: boolean;
  /** Since when the job runs, as far as this session has seen it. */
  elapsedMs: number | null;
  /** Estimated time left in the current phase, once the speed is known. */
  etaMs: number | null;
  /** For `waiting`: why the analysis is not running (`paused`, `busy`, `skipped`, `next`). */
  reason: string | null;
}

/** What was first seen of a job: when it started and when its current phase started. */
interface Seen {
  at: number;
  phase: string;
  phaseAt: number;
  phaseDone: number;
}

/** A phase must run this long, and move, before a time left is estimated. */
export const ETA_AFTER_MS = 15_000;

export class ActivityTracker {
  #seen = new Map<number, Seen>();

  /** The rows of the panel at `now`, remembering the jobs for the speed. */
  rows(jobs: JobView[], volumes: VolumeView[], now: number): ActivityRow[] {
    const active = jobs.filter((j) => SHOWN.includes(j.kind) && (j.state === 'running' || j.state === 'queued'));
    for (const id of [...this.#seen.keys()]) if (!active.some((j) => j.id === id)) this.#seen.delete(id);
    const rows: ActivityRow[] = active.map((j) => {
      const queued = j.state === 'queued';
      let seen = this.#seen.get(j.id);
      if (!queued && (!seen || seen.phase !== j.phase)) {
        seen = { at: seen?.at ?? now, phase: j.phase, phaseAt: now, phaseDone: j.done };
        this.#seen.set(j.id, seen);
      }
      const percent = j.total > 0 ? Math.min(100, Math.floor((j.done / j.total) * 100)) : null;
      let etaMs: number | null = null;
      if (seen && j.total > 0 && now - seen.phaseAt >= ETA_AFTER_MS && j.done > seen.phaseDone) {
        const rate = (j.done - seen.phaseDone) / (now - seen.phaseAt);
        etaMs = Math.max(0, (j.total - j.done) / rate);
      }
      return {
        key: `job-${j.id}`,
        kind: j.kind,
        volumeId: j.volumeId,
        phase: PHASES.includes(j.phase) ? j.phase : null,
        done: j.done,
        total: j.total,
        percent,
        queued,
        elapsedMs: seen ? now - seen.at : null,
        etaMs,
        reason: null,
      };
    });
    // Disks with files still to analyse and no job on them now: the row says why (paused,
    // another job on the disk, files in use, next round), so it does not look stuck. Disks the
    // user left out of the analysis are not shown.
    // A disk counted as running whose job is not in the list yet shows the counted progress.
    for (const v of volumes) {
      const h = v.hashes;
      if (!h || !v.online || h.hashed >= h.total || v.status === 'ignored' || h.waiting === 'excluded') continue;
      if (active.some((j) => j.kind === 'hash' && j.volumeId === v.id)) continue;
      rows.push({
        key: `wait-${v.id}`,
        kind: h.running ? 'hash' : 'waiting',
        reason: h.running ? null : (h.waiting ?? 'next'),
        volumeId: v.id,
        phase: null,
        done: h.hashed,
        total: h.total,
        percent: Math.min(100, Math.floor((h.hashed / h.total) * 100)),
        queued: false,
        elapsedMs: null,
        etaMs: null,
      });
    }
    return rows;
  }
}
