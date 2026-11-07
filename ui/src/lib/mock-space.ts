// Simulated space data for the sample disks (browser development, tests): a deterministic tree
// per volume with the same shapes as the backend (src/lib/bindings).
import type { CrumbView } from './bindings/CrumbView';
import type { LargeFileView } from './bindings/LargeFileView';
import type { SpaceEntryView } from './bindings/SpaceEntryView';
import type { SpaceFolderView } from './bindings/SpaceFolderView';
import type { StatRowView } from './bindings/StatRowView';
import type { TreemapNodeView } from './bindings/TreemapNodeView';
import type { TypeStatsView } from './bindings/TypeStatsView';
import { CATEGORIES } from './treemap';

interface MockNode {
  id: number;
  name: string;
  isDir: boolean;
  size: number;
  files: number;
  modified: number;
  parent: MockNode | null;
  children: MockNode[];
}

const CATEGORY_OF: Record<string, string> = {
  jpg: 'images', png: 'images', cr2: 'images',
  mp4: 'videos', mkv: 'videos',
  mp3: 'audio', flac: 'audio',
  pdf: 'documents', docx: 'documents', xlsx: 'documents', txt: 'documents',
  zip: 'archives', '7z': 'archives',
  iso: 'disk_images', vhdx: 'disk_images',
  rs: 'code', ts: 'code', json: 'code',
  exe: 'programs', dll: 'programs',
};

export function extensionOf(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 && i < name.length - 1 ? name.slice(i + 1).toLowerCase() : '';
}

export function categoryOf(name: string): string {
  return CATEGORY_OF[extensionOf(name)] ?? 'other';
}

/** Folder → [file extension, typical size] of the files generated in it. */
const FOLDERS: [string, string, number][] = [
  ['Video', 'mkv', 1_800_000_000],
  ['Foto', 'jpg', 6_000_000],
  ['Foto RAW', 'cr2', 28_000_000],
  ['Musica', 'flac', 32_000_000],
  ['Documenti', 'pdf', 900_000],
  ['Archivi', 'zip', 120_000_000],
  ['Progetti', 'rs', 14_000],
  ['Programmi', 'dll', 2_400_000],
  ['Immagini disco', 'iso', 4_200_000_000],
  ['Varie', 'bin', 300_000],
];

/** A small deterministic random generator (mulberry32). */
function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const trees = new Map<number, Map<number, MockNode>>();

function build(volumeId: number): Map<number, MockNode> {
  const random = rng(volumeId * 7919);
  const index = new Map<number, MockNode>();
  let next = volumeId * 1_000_000;
  const now = Date.UTC(2026, 10, 7);
  const add = (parent: MockNode | null, name: string, isDir: boolean, size = 0): MockNode => {
    const n: MockNode = {
      id: next++, name, isDir, size, files: isDir ? 0 : 1,
      modified: now - Math.floor(random() * 900) * 86_400_000, parent, children: [],
    };
    parent?.children.push(n);
    index.set(n.id, n);
    return n;
  };
  const root = add(null, '', true);
  FOLDERS.forEach(([name, ext, typical], k) => {
    if (random() < 0.15 && k > 2) return;
    const folder = add(root, name, true);
    const subs = 1 + Math.floor(random() * 4);
    for (let s = 0; s < subs; s++) {
      const sub = add(folder, `${2021 + s}`, true);
      const count = 3 + Math.floor(random() * 12);
      for (let f = 0; f < count; f++) {
        const size = Math.max(1, Math.floor(typical * (0.2 + random() * 1.6)));
        add(sub, `${name.toLowerCase().replace(/\s/g, '-')}-${s}${f}.${ext}`, false, size);
      }
    }
  });
  add(root, 'pagefile.sys', false, 4_000_000_000);
  // Folder totals, bottom-up.
  const total = (n: MockNode): void => {
    if (!n.isDir) return;
    n.children.forEach(total);
    n.size = n.children.reduce((a, c) => a + c.size, 0);
    n.files = n.children.reduce((a, c) => a + c.files, 0);
  };
  total(root);
  return index;
}

function tree(volumeId: number): Map<number, MockNode> {
  let t = trees.get(volumeId);
  if (!t) {
    t = build(volumeId);
    trees.set(volumeId, t);
  }
  return t;
}

function rootOf(volumeId: number): MockNode {
  return [...tree(volumeId).values()].find((n) => n.parent === null)!;
}

function folderOf(volumeId: number, folderId: number | null): MockNode {
  const n = folderId === null ? rootOf(volumeId) : tree(volumeId).get(folderId);
  if (!n || !n.isDir) throw { code: 'not_found', message: `folder ${folderId}` };
  return n;
}

const bySize = (a: MockNode, b: MockNode) => b.size - a.size || a.name.localeCompare(b.name);

function entry(n: MockNode): SpaceEntryView {
  return { id: n.id, name: n.name, isDir: n.isDir, kind: n.isDir ? 'dir' : categoryOf(n.name), size: n.size, files: n.files, modified: n.isDir ? null : n.modified };
}

function treemapOf(n: MockNode, levels: number): TreemapNodeView {
  const children = levels > 0 ? [...n.children].sort(bySize) : [];
  return {
    id: n.id,
    name: n.name,
    kind: n.isDir ? 'dir' : categoryOf(n.name),
    size: n.size,
    other: 0,
    children: children.map((c) => treemapOf(c, levels - 1)),
  };
}

export function mockSpaceFolder(volumeId: number, folderId: number | null): SpaceFolderView {
  const folder = folderOf(volumeId, folderId);
  const crumbs: CrumbView[] = [];
  for (let n: MockNode | null = folder; n; n = n.parent) crumbs.unshift({ id: n.id, name: n.name });
  return {
    volumeId,
    folderId: folder.id,
    breadcrumb: crumbs,
    items: [...folder.children].sort(bySize).map(entry),
    moreItems: 0,
    treemap: treemapOf(folder, 6),
  };
}

export function mockLargestFiles(volumeId: number, folderId: number | null, limit: number): LargeFileView[] {
  const folder = folderOf(volumeId, folderId);
  const files: MockNode[] = [];
  const walk = (n: MockNode) => (n.isDir ? n.children.forEach(walk) : files.push(n));
  walk(folder);
  return files
    .sort(bySize)
    .slice(0, limit)
    .map((f) => {
      const parts: string[] = [];
      for (let p = f.parent; p && p.parent; p = p.parent) parts.unshift(p.name);
      return { entry: entry(f), folder: parts.join('\\') };
    });
}

const BANDS = [0, 1_000, 100_000, 1_000_000, 100_000_000, 1_000_000_000];

export function mockTypeStats(volumeIds: number[]): TypeStatsView {
  const files: MockNode[] = [];
  for (const v of volumeIds) {
    for (const n of tree(v).values()) if (!n.isDir) files.push(n);
  }
  const add = (map: Map<string, { files: number; bytes: number }>, key: string, size: number) => {
    const t = map.get(key) ?? { files: 0, bytes: 0 };
    t.files += 1;
    t.bytes += size;
    map.set(key, t);
  };
  const cats = new Map<string, { files: number; bytes: number }>();
  const exts = new Map<string, { files: number; bytes: number }>();
  const bands = new Map<string, { files: number; bytes: number }>();
  for (const f of files) {
    add(cats, categoryOf(f.name), f.size);
    add(exts, extensionOf(f.name), f.size);
    const band = [...BANDS].reverse().find((b) => f.size >= b) ?? 0;
    add(bands, String(band), f.size);
  }
  const rows = (keys: string[], map: Map<string, { files: number; bytes: number }>, cat: boolean): StatRowView[] =>
    keys.map((key) => ({ key, category: cat ? key : null, totals: map.get(key) ?? { files: 0, bytes: 0 } }));
  const extRows: StatRowView[] = [...exts.entries()]
    .sort((a, b) => b[1].bytes - a[1].bytes)
    .map(([key, totals]) => ({ key, category: CATEGORY_OF[key] ?? 'other', totals }));
  return {
    total: { files: files.length, bytes: files.reduce((a, f) => a + f.size, 0) },
    categories: rows([...CATEGORIES], cats, true),
    extensions: extRows.slice(0, 25),
    otherExtensions: extRows.slice(25).reduce((a, r) => ({ files: a.files + r.totals.files, bytes: a.bytes + r.totals.bytes }), { files: 0, bytes: 0 }),
    sizeBands: rows(BANDS.map(String), bands, false),
  };
}

/** Whether an entry exists in a simulated volume (open/reveal). */
export function mockHasEntry(volumeId: number, entryId: number): boolean {
  return tree(volumeId).has(entryId);
}

/** Every entry of a simulated volume with its folder path (for the simulated search). */
export function mockEntries(volumeId: number): { id: number; name: string; folder: string; isDir: boolean; size: number; modified: number }[] {
  const out = [];
  for (const n of tree(volumeId).values()) {
    if (n.parent === null) continue;
    const parts: string[] = [];
    for (let p = n.parent; p && p.parent; p = p.parent) parts.unshift(p.name);
    out.push({ id: n.id, name: n.name, folder: parts.join('/'), isDir: n.isDir, size: n.size, modified: n.modified });
  }
  return out;
}
