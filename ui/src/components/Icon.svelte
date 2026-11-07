<script lang="ts">
  // Small set of line icons (24×24, stroke), decorative: always paired with visible text or an
  // accessible label on the parent.
  const paths: Record<string, string> = {
    internal: 'M4 6h16v12H4z M7 15h.01 M11 15h6',
    external: 'M6 3h12v18H6z M10 18h4 M9 7h6',
    stick: 'M9 2h6v5H9z M7 7h10v13a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2z M12 11v5',
    network: 'M4 5h16v6H4z M4 13h16v6H4z M7 8h.01 M7 16h.01 M12 19v3',
    folder: 'M3 6h6l2 2h10v11H3z',
    disks: 'M4 4h16v7H4z M4 13h16v7H4z M7 7.5h.01 M7 16.5h.01',
    settings:
      'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z',
    refresh: 'M20 11a8 8 0 1 0-2.3 5.7 M20 4v7h-7',
    tag: 'M3 12V4h8l10 10-8 8z M7.5 7.5h.01',
    trash: 'M4 7h16 M9 7V4h6v3 M6 7l1 13h10l1-13',
    close: 'M6 6l12 12 M18 6L6 18',
    pin: 'M12 22s7-6.3 7-12a7 7 0 0 0-14 0c0 5.7 7 12 7 12z M12 12.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z',
    plus: 'M12 5v14 M5 12h14',
    open: 'M14 4h6v6 M20 4l-9 9 M18 14v6H4V6h6',
    back: 'M15 18l-6-6 6-6',
    up: 'M12 19V5 M5 12l7-7 7 7',
    space: 'M3 3h18v18H3z M3 12h10 M13 3v18 M13 15h8',
  };
  let { name, size = 18 }: { name: string; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name] ?? ''} />
</svg>
