// Simulated backup checks and comparisons (browser development, tests), with the shapes of the
// backend: summaries, one level of the result tree at a time, copy preview and outcome.
import type { CompareSummaryView } from './bindings/CompareSummaryView';
import type { CopyPreviewView } from './bindings/CopyPreviewView';
import type { ReportFileView } from './bindings/ReportFileView';
import type { ReportFilterInput } from './bindings/ReportFilterInput';
import type { ReportFolderView } from './bindings/ReportFolderView';
import type { SideInput } from './bindings/SideInput';
import type { TallyView } from './bindings/TallyView';
import type { VerifySummaryView } from './bindings/VerifySummaryView';
import type { VolumeView } from './bindings/VolumeView';
import { STATUSES } from './backup';

const MB = 1_000_000;

type Item = { rel: string; size: number; status: string; other: string | null; evidence: string | null };
type Report = { kind: 'verify'; source: SideInput; items: Item[] } | { kind: 'compare'; items: Item[] };

const MISSING: [string, number][] = [
  ['Foto\\2024\\IMG_0001.jpg', 4.1 * MB],
  ['Foto\\2024\\IMG_0002.jpg', 3.8 * MB],
  ['Foto\\2024\\Vacanza\\IMG_0100.jpg', 5.2 * MB],
  ['Foto\\2023\\scan.pdf', 1.2 * MB],
  ['Documenti\\tesi.docx', 2.0 * MB],
];

const COMPARED: Item[] = [
  { rel: 'Foto\\2023\\mare.jpg', size: 3 * MB, status: 'same', other: null, evidence: 'content' },
  { rel: 'Foto\\2023\\monti.jpg', size: 2 * MB, status: 'same', other: 'Archivio\\monti-2023.jpg', evidence: 'content' },
  { rel: 'Foto\\2024\\IMG_0001.jpg', size: 4.1 * MB, status: 'onlyA', other: null, evidence: null },
  { rel: 'Documenti\\tesi.docx', size: 2 * MB, status: 'different', other: null, evidence: 'content' },
  { rel: 'Vecchio\\lettera.doc', size: 0.1 * MB, status: 'onlyB', other: null, evidence: null },
];

const tally = (items: Item[]): TallyView => ({ files: items.length, bytes: items.reduce((a, i) => a + i.size, 0) });

/** The file the simulated copy fails on. */
export const FAILING_COPY = 'Foto\\2023\\scan.pdf';

export function mockBackup(volumes: () => VolumeView[]) {
  const reports = new Map<number, Report>();
  const copied = new Set<string>();
  let next = 1;
  const keep = (r: Report) => {
    reports.set(next, r);
    return next++;
  };
  const get = (id: number) => {
    const r = reports.get(id);
    if (!r) throw { code: 'report_expired', message: String(id) };
    return r;
  };
  const volume = (id: number) => {
    const v = volumes().find((x) => x.id === id);
    if (!v) throw { code: 'volume_not_found', message: String(id) };
    return v;
  };
  const same = (a: SideInput, b: SideInput) =>
    a.volumeId === b.volumeId && (a.folder.startsWith(b.folder) || b.folder.startsWith(a.folder));

  return {
    verify(source: SideInput, destinations: SideInput[]): VerifySummaryView {
      if (destinations.length === 0) throw { code: 'no_destination', message: '' };
      if (destinations.some((d) => same(source, d))) throw { code: 'backup_overlap', message: '' };
      volume(source.volumeId);
      destinations.forEach((d) => volume(d.volumeId));
      const items = MISSING.filter(([rel]) => !copied.has(rel)).map(([rel, size]) => ({
        rel,
        size,
        status: 'missing',
        other: null,
        evidence: null,
      }));
      const id = keep({ kind: 'verify', source, items });
      return {
        reportId: id,
        total: { files: 2410, bytes: 18_400 * MB },
        byContent: { files: 2105, bytes: 16_000 * MB },
        byMetadata: { files: 300, bytes: 2_383.7 * MB },
        missing: tally(items),
      };
    },
    compare(a: SideInput, b: SideInput, byContent: boolean): CompareSummaryView {
      if (same(a, b)) throw { code: 'backup_overlap', message: '' };
      const items = COMPARED.map((i) =>
        !byContent && i.other ? { ...i, status: 'onlyA', other: null, evidence: null } : { ...i },
      );
      if (!byContent) items.push({ rel: 'Archivio\\monti-2023.jpg', size: 2 * MB, status: 'onlyB', other: null, evidence: null });
      const id = keep({ kind: 'compare', items });
      return { reportId: id, tallies: STATUSES.map((s) => tally(items.filter((i) => i.status === s))) };
    },
    folder(reportId: number, folder: string, filter: ReportFilterInput): ReportFolderView {
      const r = get(reportId);
      const classes = r.kind === 'verify' ? ['missing'] : [...STATUSES];
      const text = (filter.text ?? '').trim().toLowerCase();
      const shown = r.items.filter(
        (i) =>
          i.size >= filter.minSize &&
          (!text || i.rel.toLowerCase().includes(text)) &&
          (filter.statuses.length === 0 || filter.statuses.includes(i.status)),
      );
      const zero = () => classes.map(() => ({ files: 0, bytes: 0 }));
      const view: ReportFolderView = { folders: [], files: [], moreFiles: 0, tallies: zero() };
      const add = (t: TallyView[], i: Item) => {
        const k = Math.max(0, classes.indexOf(i.status));
        t[k] = { files: t[k]!.files + 1, bytes: t[k]!.bytes + i.size };
      };
      for (const i of shown) {
        const rest = folder ? (i.rel.startsWith(folder + '\\') ? i.rel.slice(folder.length + 1) : null) : i.rel;
        if (rest === null) continue;
        add(view.tallies, i);
        const cut = rest.indexOf('\\');
        if (cut < 0) {
          const file: ReportFileView = { rel: i.rel, name: rest, size: i.size, status: i.status, other: i.other, evidence: i.evidence };
          view.files.push(file);
        } else {
          const key = rest.slice(0, cut);
          let node = view.folders.find((f) => f.key === key);
          if (!node) {
            node = { key, name: key, tallies: zero() };
            view.folders.push(node);
          }
          add(node.tallies, i);
        }
      }
      const bytes = (t: TallyView[]) => t.reduce((a, x) => a + x.bytes, 0);
      view.folders.sort((x, y) => bytes(y.tallies) - bytes(x.tallies));
      return view;
    },
    /** The copy of a check is over: everything but the failing file now has a copy. */
    copied(reportId: number) {
      const r = get(reportId);
      r.items.filter((i) => i.rel !== FAILING_COPY).forEach((i) => copied.add(i.rel));
    },
    preview(reportId: number, dest: SideInput): CopyPreviewView {
      const r = get(reportId);
      if (r.kind !== 'verify') throw { code: 'not_a_check', message: String(reportId) };
      const src = volume(r.source.volumeId);
      const d = volume(dest.volumeId);
      if (!src.online) throw { code: 'volume_offline', message: String(src.id) };
      if (!d.online) throw { code: 'volume_offline', message: String(d.id) };
      const bytes = tally(r.items).bytes;
      const mount = (d.mount ?? '').replace(/\\$/, '');
      const folder = dest.folder.replace(/^\\+|\\+$/g, '');
      return {
        files: r.items.length,
        bytes,
        freeSpace: d.freeSpace,
        enoughSpace: d.freeSpace === null || d.freeSpace >= bytes,
        destPath: folder ? `${mount}\\${folder}` : mount,
      };
    },
  };
}
