// Treemap layout (F08-AC1): squarified rectangles (Bruls, Huizing, van Wijk), nested folders
// with a title strip. Pure functions, tested with Vitest; drawing is in Treemap.svelte.
import type { TreemapNodeView } from './bindings/TreemapNodeView';

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A drawn rectangle. `node` is null for the grouped small items of a folder ("other"). */
export interface Cell {
  node: TreemapNodeView | null;
  /** Bytes the cell stands for (for "other", the small items it groups). */
  size: number;
  rect: Rect;
  depth: number;
  /** Folders drawn with a title strip and their children inside. */
  nested: boolean;
}

export interface LayoutOptions {
  /** Height of the title strip of a nested folder. */
  header: number;
  /** Space between a folder's border and its children. */
  pad: number;
  /** Below this width or height a folder is drawn as a plain block. */
  minNested: number;
  /** Rectangles smaller than this (either side) are not drawn. */
  minSide: number;
}

export const DEFAULT_LAYOUT: LayoutOptions = { header: 16, pad: 2, minNested: 40, minSide: 2 };

function worst(row: number[], side: number): number {
  const sum = row.reduce((a, b) => a + b, 0);
  const max = Math.max(...row);
  const min = Math.min(...row);
  const s2 = side * side;
  const sum2 = sum * sum;
  return Math.max((s2 * max) / sum2, sum2 / (s2 * min));
}

/**
 * Places `values` (largest first, all > 0) in `rect`, areas proportional to the values, as
 * close to squares as the algorithm gets. Returns one rectangle per value, in order.
 */
export function squarify(values: number[], rect: Rect): Rect[] {
  const total = values.reduce((a, b) => a + b, 0);
  const out: Rect[] = [];
  if (values.length === 0 || total <= 0 || rect.w <= 0 || rect.h <= 0) return out;
  const scale = (rect.w * rect.h) / total;
  const areas = values.map((v) => v * scale);
  let free: Rect = { ...rect };
  let i = 0;
  while (i < areas.length) {
    const side = Math.min(free.w, free.h);
    const row: number[] = [areas[i]!];
    let j = i + 1;
    while (j < areas.length && worst([...row, areas[j]!], side) <= worst(row, side)) {
      row.push(areas[j]!);
      j++;
    }
    const rowArea = row.reduce((a, b) => a + b, 0);
    if (free.w >= free.h) {
      // A column on the left, as wide as needed.
      const width = free.h > 0 ? rowArea / free.h : 0;
      let y = free.y;
      for (const a of row) {
        const h = width > 0 ? a / width : 0;
        out.push({ x: free.x, y, w: width, h });
        y += h;
      }
      free = { x: free.x + width, y: free.y, w: free.w - width, h: free.h };
    } else {
      // A row on top.
      const height = free.w > 0 ? rowArea / free.w : 0;
      let x = free.x;
      for (const a of row) {
        const w = height > 0 ? a / height : 0;
        out.push({ x, y: free.y, w, h: height });
        x += w;
      }
      free = { x: free.x, y: free.y + height, w: free.w, h: free.h - height };
    }
    i = j;
  }
  return out;
}

/** All the rectangles of a treemap, parents before their children. */
export function layoutTreemap(
  root: TreemapNodeView,
  rect: Rect,
  opts: LayoutOptions = DEFAULT_LAYOUT,
): Cell[] {
  const cells: Cell[] = [];
  placeChildren(root, rect, 0, opts, cells);
  return cells;
}

function placeChildren(
  node: TreemapNodeView,
  rect: Rect,
  depth: number,
  opts: LayoutOptions,
  cells: Cell[],
): void {
  const parts: { node: TreemapNodeView | null; size: number }[] = node.children
    .filter((c) => c.size > 0)
    .map((c) => ({ node: c, size: c.size }));
  if (node.other > 0) parts.push({ node: null, size: node.other });
  parts.sort((a, b) => b.size - a.size);
  const rects = squarify(
    parts.map((p) => p.size),
    rect,
  );
  parts.forEach((p, k) => {
    const r = rects[k]!;
    if (r.w < opts.minSide || r.h < opts.minSide) return;
    const child = p.node;
    const nested =
      child !== null &&
      child.kind === 'dir' &&
      (child.children.length > 0 || child.other > 0) &&
      r.w >= opts.minNested &&
      r.h >= opts.minNested;
    cells.push({ node: child, size: p.size, rect: r, depth, nested });
    if (nested && child) {
      const inner: Rect = {
        x: r.x + opts.pad,
        y: r.y + opts.header,
        w: r.w - 2 * opts.pad,
        h: r.h - opts.header - opts.pad,
      };
      placeChildren(child, inner, depth + 1, opts, cells);
    }
  });
}

/** The deepest cell under a point, for hover and click. */
export function cellAt(cells: Cell[], x: number, y: number): Cell | null {
  for (let i = cells.length - 1; i >= 0; i--) {
    const c = cells[i]!;
    const r = c.rect;
    if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return c;
  }
  return null;
}

/** Colour role of a cell: folders and "other" are neutral, files by category. */
export function colorRole(cell: Cell): string {
  if (cell.node === null) return 'other';
  return cell.node.kind === 'dir' ? 'dir' : cell.node.kind;
}

/** The file categories, in the fixed order of their colours (legend, statistics). */
export const CATEGORIES = [
  'images',
  'videos',
  'audio',
  'documents',
  'archives',
  'disk_images',
  'code',
  'programs',
  'other',
] as const;
