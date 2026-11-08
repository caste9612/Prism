import { describe, expect, it } from 'vitest';
import type { JobView } from './bindings/JobView';
import { ActivityTracker, ETA_AFTER_MS } from './activity';
import { formatDuration } from './format';
import { sampleVolumes } from './mock';

// The "In progress" panel (ADR-0019): phases, time since the start, time left.

const job = (o: Partial<JobView>): JobView => ({
  id: 1,
  kind: 'hash',
  volumeId: 2,
  state: 'running',
  done: 0,
  total: 0,
  phase: '',
  error: null,
  ...o,
});

describe('in progress panel', () => {
  it('a running job shows its phase, the time since it started and, once measured, the time left', () => {
    const tracker = new ActivityTracker();
    const t0 = 1_000_000;
    let [row] = tracker.rows([job({ phase: 'prints', done: 100, total: 1_100 })], [], t0);
    expect(row).toMatchObject({ kind: 'hash', phase: 'prints', percent: 9, elapsedMs: 0, etaMs: null });
    // 20 s later 200 more done: 10 per second, 800 left → 80 s.
    [row] = tracker.rows([job({ phase: 'prints', done: 300, total: 1_100 })], [], t0 + 20_000);
    expect(row!.elapsedMs).toBe(20_000);
    expect(row!.etaMs).toBeCloseTo(80_000);
    // A new phase starts its own measure; the time since the start of the job goes on.
    [row] = tracker.rows([job({ phase: 'videoPrints', done: 0, total: 50 })], [], t0 + 30_000);
    expect(row).toMatchObject({ phase: 'videoPrints', percent: 0, elapsedMs: 30_000, etaMs: null });
  });

  it('no time left before the speed is known, or without a total', () => {
    const tracker = new ActivityTracker();
    tracker.rows([job({ kind: 'index', phase: 'scanning', done: 10, total: 0 })], [], 0);
    const [row] = tracker.rows([job({ kind: 'index', phase: 'scanning', done: 5_000, total: 0 })], [], ETA_AFTER_MS + 1);
    expect(row).toMatchObject({ percent: null, etaMs: null });
    // An unknown phase has no text.
    expect(tracker.rows([job({ phase: 'journal' })], [], 0)[0]!.phase).toBeNull();
  });

  it('queued and automatic jobs; finished jobs are forgotten', () => {
    const tracker = new ActivityTracker();
    const rows = tracker.rows(
      [job({ id: 1, kind: 'update', state: 'queued' }), job({ id: 2, kind: 'journal' }), job({ id: 3, state: 'done' })],
      [],
      0,
    );
    expect(rows.map((r) => [r.kind, r.queued])).toEqual([['update', true]]);
  });

  it('a connected disk with files left and no job waits for its next round', () => {
    const tracker = new ActivityTracker();
    const volumes = sampleVolumes(0).map((v) => (v.id === 1 ? { ...v, hashes: { total: 1_000, hashed: 990, running: false } } : v));
    const rows = tracker.rows([], volumes, 0);
    expect(rows).toEqual([expect.objectContaining({ kind: 'waiting', volumeId: 1, percent: 99, done: 990, total: 1_000 })]);
    // Counted as running before its job reaches the list: the counted progress, no phase.
    const counted = volumes.map((v) => (v.id === 1 ? { ...v, hashes: { total: 1_000, hashed: 500, running: true } } : v));
    expect(new ActivityTracker().rows([], counted, 0)).toEqual([expect.objectContaining({ kind: 'hash', volumeId: 1, percent: 50, phase: null })]);
    // While its job runs, the job row is shown instead.
    const running = tracker.rows([job({ volumeId: 1, phase: 'hashFull', done: 1, total: 10 })], volumes, 0);
    expect(running.map((r) => r.kind)).toEqual(['hash']);
  });

  it('durations are short and the same in both languages', () => {
    expect(formatDuration(4_000)).toBe('4 s');
    expect(formatDuration(12 * 60_000)).toBe('12 min');
    expect(formatDuration(65 * 60_000)).toBe('1 h 05 min');
  });
});
