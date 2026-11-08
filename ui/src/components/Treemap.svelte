<script lang="ts">
  // Treemap on a canvas (F08-AC1): folders nested with a title strip, files coloured by
  // category. Clicking a folder opens it; hovering shows name and size. The list next to the
  // map offers the same content to the keyboard and to screen readers (NF7).
  import type { TreemapNodeView } from '../lib/bindings/TreemapNodeView';
  import { cellAt, colorRole, layoutTreemap, type Cell } from '../lib/treemap';

  let {
    tree,
    label,
    describe,
    onopen,
    onselect,
  }: {
    tree: TreemapNodeView;
    /** Accessible name of the map. */
    label: string;
    /** Tooltip lines for a cell. */
    describe: (cell: Cell) => string[];
    onopen: (folderId: number) => void;
    onselect?: (entryId: number) => void;
  } = $props();

  let box: HTMLDivElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  let width = $state(0);
  let height = $state(0);
  let hover = $state<Cell | null>(null);
  let pointer = $state({ x: 0, y: 0 });
  let themeTick = $state(0);

  const cells = $derived(
    width > 0 && height > 0 ? layoutTreemap(tree, { x: 0, y: 0, w: width, h: height }) : [],
  );

  $effect(() => {
    if (!box) return;
    const ro = new ResizeObserver(([entry]) => {
      if (!entry) return;
      width = Math.floor(entry.contentRect.width);
      height = Math.floor(entry.contentRect.height);
    });
    ro.observe(box);
    // Redraw when the theme changes (colours come from CSS variables).
    const mo = new MutationObserver(() => themeTick++);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  });

  function fit(ctx: CanvasRenderingContext2D, text: string, max: number): string | null {
    if (max < 24) return null;
    if (ctx.measureText(text).width <= max) return text;
    let lo = 0;
    let hi = text.length;
    while (lo < hi) {
      const mid = Math.ceil((lo + hi) / 2);
      if (ctx.measureText(text.slice(0, mid) + '…').width <= max) lo = mid;
      else hi = mid - 1;
    }
    return lo > 1 ? text.slice(0, lo) + '…' : null;
  }

  /** `a` mixed into `b`: `share` of `a` (both `#rrggbb`; anything else is returned as is). */
  function mix(a: string, b: string, share: number): string {
    const rgb = (c: string) => (/^#[0-9a-f]{6}$/i.test(c) ? [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16)) : null);
    const ca = rgb(a);
    const cb = rgb(b);
    if (!ca || !cb) return a;
    const m = ca.map((v, i) => Math.round(v * share + cb[i]! * (1 - share)));
    return `rgb(${m[0]}, ${m[1]}, ${m[2]})`;
  }

  $effect(() => {
    void themeTick;
    if (!canvas || !box) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.round(width * dpr));
    canvas.height = Math.max(1, Math.round(height * dpr));
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const css = getComputedStyle(box);
    const color = (name: string) => css.getPropertyValue(name).trim() || '#888';
    const surface = color('--surface');
    // B+C tiles (ADR-0019): the category colour mixed with the surface, labels in ink.
    const share = (parseFloat(color('--tile-mix')) || 30) / 100;
    const tile = (name: string) => mix(color(name), surface, share);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = surface;
    ctx.fillRect(0, 0, width, height);
    ctx.font = '500 11.5px Inter, "Segoe UI", sans-serif';
    ctx.textBaseline = 'middle';
    for (const cell of cells) {
      const { x, y, w, h } = cell.rect;
      const role = colorRole(cell);
      ctx.fillStyle = role === 'dir' ? color(cell.depth % 2 === 0 ? '--cat-dir' : '--cat-dir-2') : tile(`--cat-${role}`);
      // A small surface gap around every block keeps neighbours apart.
      const bw = Math.max(0, w - 2);
      const bh = Math.max(0, h - 2);
      ctx.beginPath();
      if (bw > 10 && bh > 10) ctx.roundRect(x + 1, y + 1, bw, bh, 4);
      else ctx.rect(x + 1, y + 1, bw, bh);
      ctx.fill();
      const name = cell.node?.name ?? '';
      if (cell.nested) {
        ctx.fillStyle = color('--text-2');
        const text = fit(ctx, name, w - 10);
        if (text) ctx.fillText(text, x + 6, y + 9);
      } else if (cell.node !== null && role !== 'dir' && w > 60 && h > 22) {
        ctx.fillStyle = color('--cat-label');
        const text = fit(ctx, name, w - 12);
        if (text) ctx.fillText(text, x + 7, y + 12);
      } else if (role === 'dir' && w > 60 && h > 20) {
        ctx.fillStyle = color('--text-2');
        const text = fit(ctx, name, w - 12);
        if (text) ctx.fillText(text, x + 7, y + 12);
      }
    }
    if (hover) {
      const { x, y, w, h } = hover.rect;
      ctx.strokeStyle = color('--signal');
      ctx.lineWidth = 2;
      ctx.strokeRect(x + 1, y + 1, Math.max(0, w - 2), Math.max(0, h - 2));
    }
  });

  function move(e: PointerEvent) {
    pointer = { x: e.offsetX, y: e.offsetY };
    hover = cellAt(cells, e.offsetX, e.offsetY);
  }

  function click(e: MouseEvent) {
    const cell = cellAt(cells, e.offsetX, e.offsetY);
    const node = cell?.node;
    if (!node) return;
    if (node.kind === 'dir') onopen(node.id);
    else onselect?.(node.id);
  }

  const tip = $derived(hover ? describe(hover) : []);
  const tipLeft = $derived(Math.min(pointer.x + 14, Math.max(0, width - 260)));
  const tipTop = $derived(pointer.y + 18 > height - 60 ? pointer.y - 56 : pointer.y + 18);
</script>

<!-- The canvas is a pointer shortcut: the same content and actions are in the list beside it,
     so screen readers get the figure's name and skip the drawing. -->
<div class="treemap" bind:this={box} role="figure" aria-label={label}>
  <canvas
    bind:this={canvas}
    style:width="{width}px"
    style:height="{height}px"
    aria-hidden="true"
    onpointermove={move}
    onpointerleave={() => (hover = null)}
    onclick={click}
    class:pointer={hover?.node?.kind === 'dir'}
  ></canvas>
  {#if hover && tip.length > 0}
    <div class="tip" style:left="{tipLeft}px" style:top="{tipTop}px" role="tooltip">
      {#each tip as line, i (i)}
        <div class:strong={i === 0}>{line}</div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .treemap {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 240px;
    overflow: hidden;
    border-radius: var(--radius-sm);
  }
  canvas {
    display: block;
  }
  canvas.pointer {
    cursor: pointer;
  }
  .tip {
    position: absolute;
    pointer-events: none;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow);
    padding: 6px 9px;
    font-size: 12px;
    max-width: 260px;
    overflow-wrap: anywhere;
  }
  .strong {
    font-weight: 600;
  }
</style>
