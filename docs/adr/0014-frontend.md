# ADR-0014 — Frontend: Svelte 5, IPC tipizzato, i18n, tema

- **Stato**: Accettata. Scelte in T3: tipi generati con `ts-rs` e i18n con file JSON propri
  (REVIEW D11); un solo punto d'ingresso, la finestra di ricerca è la stessa pagina con
  `?view=search`; liste lunghe limitate invece che virtualizzate (ARCHITECTURE §7); aspetto e
  navigazione da ADR-0019
- **Data**: 2026-11-07

## Contesto

Il vecchio frontend usava SvelteKit (inutile in una SPA desktop), interfacce TypeScript
riscritte a mano e spesso non allineate al backend, e una pagina principale di 1.700 righe con
la logica di stato dentro.

## Decisione

- **Svelte 5** (runes) + **Vite** + TypeScript, senza SvelteKit. Due punti d'ingresso: finestra
  principale e finestra di ricerca.
- **Tipi generati** dal Rust (tauri-specta oppure ts-rs): comandi ed eventi hanno un'unica
  definizione.
- **Stato nel backend**: store sottili che leggono tramite comandi e si aggiornano con gli
  eventi di ADR-0009.
- **i18n** inglese e italiano dal primo componente: nessuna stringa visibile scritta
  direttamente nel markup. Il backend restituisce codici di errore con parametri.
- **Tema**: segue `prefers-color-scheme` di Windows, con scelta manuale; colori definiti come
  variabili CSS, contrasto verificato in entrambi i temi (NF7).
- **Prestazioni**: liste virtualizzate; treemap su canvas.
- **Test**: Vitest per componenti e store; Playwright per i flussi end-to-end con IPC simulato.

## Conseguenze

- Meno codice di raccordo e nessuna divergenza tra tipi Rust e TypeScript.
- La scelta delle librerie (i18n, generazione dei tipi, virtualizzazione) si fa in T3 con un
  piccolo prototipo.
