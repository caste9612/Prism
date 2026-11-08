<script lang="ts">
  // The mark of Prism (ADR-0019, REVIEW D52): a prism made of layers on a black tile — the same
  // figure as the application icon. Decorative: the parent carries the accessible name.
  let { size = 24 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
  <rect width="64" height="64" rx="14" fill="#111418" />
  <path d="M32 12 39.37 26H24.63Z" fill="#ffffff" />
  <path d="M23.58 28H40.42L44.11 35H19.89Z" fill="#f04e23" />
  <path d="M18.84 37H45.16L48.32 43H15.68Z" fill="#5b9dff" />
  <path d="M14.63 45H49.37L52 50H12Z" fill="#3fbf6f" />
</svg>
