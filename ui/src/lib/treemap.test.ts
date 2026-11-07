import { describe, expect, it } from 'vitest';
import type { TreemapNodeView } from './bindings/TreemapNodeView';
import { cellAt, layoutTreemap, squarify, type Rect } from './treemap';

const area = (r: Rect) => r.w * r.h;
const inside = (r: Rect, p: Rect, eps = 1e-6) =>
  r.x >= p.x - eps && r.y >= p.y - eps && r.x + r.w <= p.x + p.w + eps && r.y + r.h <= p.y + p.h + eps;
const overlap = (a: Rect, b: Rect, eps = 1e-6) =>
  a.x < b.x + b.w - eps && b.x < a.x + a.w - eps && a.y < b.y + b.h - eps && b.y < a.y + a.h - eps;

function node(id: number, size: number, kind = 'dir', children: TreemapNodeView[] = [], other = 0): TreemapNodeView {
  return { id, name: `n${id}`, kind, size, other, children };
}

describe('squarify', () => {
  it('areas are proportional, inside the rectangle, without overlaps', () => {
    const values = [500, 300, 120, 50, 20, 7, 2, 1];
    const rect = { x: 10, y: 20, w: 400, h: 250 };
    const rects = squarify(values, rect);
    expect(rects).toHaveLength(values.length);
    const total = values.reduce((a, b) => a + b, 0);
    rects.forEach((r, i) => {
      expect(area(r)).toBeCloseTo((values[i]! / total) * area(rect), 6);
      expect(inside(r, rect)).toBe(true);
    });
    for (let i = 0; i < rects.length; i++) {
      for (let j = i + 1; j < rects.length; j++) {
        expect(overlap(rects[i]!, rects[j]!), `${i} ${j}`).toBe(false);
      }
    }
    // The covered area is the whole rectangle.
    expect(rects.reduce((a, r) => a + area(r), 0)).toBeCloseTo(area(rect), 6);
  });

  it('keeps rectangles close to squares', () => {
    const rects = squarify([1, 1, 1, 1], { x: 0, y: 0, w: 200, h: 200 });
    for (const r of rects) expect(r.w / r.h).toBeCloseTo(1, 6);
  });

  it('nothing to place', () => {
    expect(squarify([], { x: 0, y: 0, w: 10, h: 10 })).toEqual([]);
    expect(squarify([1], { x: 0, y: 0, w: 0, h: 10 })).toEqual([]);
  });
});

describe('treemap layout', () => {
  // F08-AC1: folders nest their children inside, below a title strip.
  it('nests children inside their folder and groups small items', () => {
    const tree = node(1, 1000, 'dir', [
      node(2, 600, 'dir', [node(4, 400, 'videos'), node(5, 150, 'images')], 50),
      node(3, 300, 'documents'),
    ], 100);
    const rect = { x: 0, y: 0, w: 600, h: 400 };
    const cells = layoutTreemap(tree, rect, { header: 16, pad: 2, minNested: 40, minSide: 1 });
    const byId = new Map(cells.filter((c) => c.node).map((c) => [c.node!.id, c]));
    const folder = byId.get(2)!;
    expect(folder.nested).toBe(true);
    for (const id of [4, 5]) {
      const c = byId.get(id)!;
      expect(c.depth).toBe(1);
      expect(inside(c.rect, folder.rect)).toBe(true);
      expect(c.rect.y).toBeGreaterThanOrEqual(folder.rect.y + 16);
    }
    // "Other" cells for the root (100) and the folder (50).
    expect(cells.filter((c) => c.node === null)).toHaveLength(2);
    // Top-level cells cover the whole area in proportion.
    const top = cells.filter((c) => c.depth === 0);
    expect(top.reduce((a, c) => a + area(c.rect), 0)).toBeCloseTo(area(rect), 6);
    expect(area(byId.get(2)!.rect) / area(rect)).toBeCloseTo(0.6, 6);
  });

  it('small folders are plain blocks; tiny cells are skipped', () => {
    const tree = node(1, 1000, 'dir', [
      node(2, 999, 'dir', [node(4, 999, 'videos')]),
      node(3, 1, 'dir', [node(5, 1, 'images')]),
    ]);
    const cells = layoutTreemap(tree, { x: 0, y: 0, w: 100, h: 100 }, { header: 16, pad: 2, minNested: 40, minSide: 2 });
    expect(cells.find((c) => c.node?.id === 3)).toBeUndefined();
    expect(cells.find((c) => c.node?.id === 2)?.nested).toBe(true);
  });

  it('finds the deepest cell under the pointer', () => {
    const tree = node(1, 100, 'dir', [node(2, 100, 'dir', [node(3, 100, 'videos')])]);
    const cells = layoutTreemap(tree, { x: 0, y: 0, w: 200, h: 200 });
    expect(cellAt(cells, 100, 100)?.node?.id).toBe(3);
    expect(cellAt(cells, 100, 5)?.node?.id).toBe(2); // title strip of the folder
    expect(cellAt(cells, 500, 500)).toBeNull();
  });
});
