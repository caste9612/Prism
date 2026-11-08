// Presentation helpers of the backup screen (F13–F15).
import type { JobView } from './bindings/JobView';
import type { TallyView } from './bindings/TallyView';
import type { VerifySummaryView } from './bindings/VerifySummaryView';

/** Statuses of a comparison, in the order of `CompareSummaryView.tallies` (F14-AC1). */
export const STATUSES = ['onlyA', 'onlyB', 'different', 'same'] as const;
export type CompareStatus = (typeof STATUSES)[number];

/** Share of `part` in `whole`, as a whole percentage rounded down (100 only when complete). */
export function percent(part: TallyView, whole: TallyView): number {
  if (whole.files <= 0) return 0;
  return Math.min(100, Math.floor((part.files / whole.files) * 100));
}

/** F13-AC3: how much of the source was checked by content, by metadata, and what is missing. */
export function coverage(s: VerifySummaryView) {
  return {
    content: percent(s.byContent, s.total),
    metadata: percent(s.byMetadata, s.total),
    missing: s.missing.files,
    complete: s.missing.files === 0,
  };
}

/** The steps of a folder path (stored names separated by `\`), for the breadcrumb. */
export function crumbs(folder: string): { key: string; name: string }[] {
  const parts = folder.split('\\').filter((p) => p !== '');
  return parts.map((name, i) => ({ key: parts.slice(0, i + 1).join('\\'), name }));
}

/** The folder one level up. */
export function parentFolder(folder: string): string {
  const parts = folder.split('\\').filter((p) => p !== '');
  parts.pop();
  return parts.join('\\');
}

/** The child folder `key` of `folder`. */
export function childFolder(folder: string, key: string): string {
  return folder ? `${folder}\\${key}` : key;
}

/** The copy job, while it is known to the engine. */
export function copyJob(jobs: JobView[], id: number | null): JobView | null {
  if (id === null) return null;
  return jobs.find((j) => j.id === id) ?? null;
}
