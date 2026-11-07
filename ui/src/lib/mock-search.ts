// Simulated search (browser development, tests): substring search over the simulated trees of
// the sample disks, with a few filters and the same error shape as the backend.
import type { SearchHitView } from './bindings/SearchHitView';
import type { SearchResultView } from './bindings/SearchResultView';
import type { VolumeView } from './bindings/VolumeView';
import { categoryOf, mockEntries } from './mock-space';

const KNOWN = new Set(['size', 'ext', 'type', 'modified', 'disk', 'path', 'is', 'online', 'regex']);

export function mockSearch(text: string, volumes: VolumeView[]): SearchResultView {
  const started = performance.now();
  const terms: string[] = [];
  let disk: string | null = null;
  let online: boolean | null = null;
  let exts: string[] | null = null;
  let kind: 'dir' | 'file' | null = null;
  for (const m of text.matchAll(/(?:([A-Za-z_]+):)?("[^"]*"|\S+)?/g)) {
    if (m[0] === '') continue;
    const key = m[1]?.toLowerCase();
    const raw = (m[2] ?? '').replace(/^"|"$/g, '');
    const start = m.index ?? 0;
    if (!key) {
      if (raw) terms.push(raw.toLowerCase());
      continue;
    }
    if (key.length > 1 && !KNOWN.has(key)) {
      return { hits: [], total: 0, capped: false, ms: 0, error: { code: 'unknown_filter', value: m[1]!, start, end: start + m[1]!.length } };
    }
    if (key === 'disk') disk = raw.toLowerCase();
    else if (key.length === 1) disk = `${key.toUpperCase()}:`.toLowerCase();
    else if (key === 'online') online = raw === 'yes';
    else if (key === 'ext') exts = raw.split(',').map((e) => e.replace(/^\./, '').toLowerCase());
    else if (key === 'is') kind = raw === 'dir' ? 'dir' : 'file';
  }
  const chosen = volumes.filter((v) => v.lastIndexedAt !== null && v.status !== 'ignored');
  const byDisk = disk === null ? chosen : chosen.filter((v) => v.name.toLowerCase() === disk || (v.mount ?? '').toLowerCase().startsWith(disk!));
  if (disk !== null && byDisk.length === 0) {
    const start = text.toLowerCase().indexOf(disk);
    return { hits: [], total: 0, capped: false, ms: 0, error: { code: 'unknown_disk', value: disk, start, end: start + disk.length } };
  }
  const hits: SearchHitView[] = [];
  for (const v of byDisk.filter((x) => online === null || x.online === online)) {
    for (const e of mockEntries(v.id)) {
      const name = e.name.toLowerCase();
      if (!terms.every((t) => name.includes(t))) continue;
      if (kind === 'dir' && !e.isDir) continue;
      if (kind === 'file' && e.isDir) continue;
      if (exts && !exts.includes(name.split('.').pop() ?? '')) continue;
      if (terms.length === 0 && !exts && !kind) continue;
      const folder = e.folder.replaceAll('/', '\\');
      hits.push({
        volumeId: v.id,
        entryId: e.id,
        name: e.name,
        folder,
        fullPath: [(v.mount ?? '').replace(/\\$/, ''), folder, e.name].filter(Boolean).join('\\'),
        isDir: e.isDir,
        kind: e.isDir ? 'dir' : categoryOf(e.name),
        size: e.size,
        modified: e.isDir ? null : e.modified,
        online: v.online,
        disk: v.name,
        location: v.location,
      });
    }
  }
  hits.sort((a, b) => a.name.length - b.name.length || a.name.localeCompare(b.name));
  return { hits: hits.slice(0, 200), total: hits.length, capped: false, error: null, ms: performance.now() - started };
}
