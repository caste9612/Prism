# ADR-0001 — Riscrittura da zero, stesso stack

- **Stato**: Accettata
- **Data**: 2026-11-07

## Contesto

Un'analisi completa del vecchio Prism (Tauri 2 + Rust + SvelteKit + SQLite) ha mostrato
problemi strutturali, non bug isolati:

- modello dati basato sul percorso come stringa e sulla lettera di unità; nel database reale
  circa 1 KB per file (919.090 file → 958 MB), con il percorso ripetuto in colonna, 3 indici e FTS;
- una sola connessione SQLite dietro un `Mutex` globale, tenuta per minuti dai lavori lunghi;
- tre pipeline di scansione con regole di esclusione diverse (completa, incrementale, coda): il
  catalogo cambiava contenuto a seconda dell'ultima scansione eseguita;
- hash dei file mai invalidati (rischio di falsi duplicati), cancellazione in blocco che
  ignorava la verifica sul database;
- stato ricostruito nel frontend a partire da circa 9 eventi sovrapposti, in una pagina di
  1.700 righe;
- documentazione staccata dal codice; 38 test, quasi tutti su funzioni di utilità.

## Decisione

Riscrivere da zero backend, modello dati e interfaccia, **mantenendo lo stack**: Rust,
Tauri 2, SQLite (WAL), Svelte (passando alla versione 5). Il lavoro avviene sul branch
`rewrite` dello stesso repository; il vecchio codice resta nella storia git.

Si riusano come riferimento (non come codice copiato alla cieca):

- il rilevamento delle unità Win32, inclusa l'enumerazione WNet e il controllo delle unità di
  rete con timeout in parallelo;
- l'approccio dei duplicati a fasi (dimensione → hash parziale → hash completo);
- le idee di interfaccia: treemap squarified, finestra di ricerca globale, tray.

## Conseguenze

- Tempo iniziale per tornare alle funzioni esistenti, compensato da un'architettura che regge
  le funzioni nuove (catalogo offline, USN, SMART, backup).
- La documentazione nasce prima del codice e si aggiorna nello stesso commit.
