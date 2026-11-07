# Prism — Diario di avanzamento

> Da leggere all'inizio di ogni sessione di sviluppo, insieme a `CLAUDE.md`.
> In cima: lo stato attuale. Sotto: il diario, dal più recente al più vecchio.

## Stato attuale

- **Tappa**: T0 completata.
- **Prossimo passo**: T1 — schema SQLite, archivio con scrittore unico, walker, riconciliazione.
- **Bloccanti**: nessuno per T1. Per T2 serve che l'utente esegua una volta
  `scripts/admin/install-admin-runner.ps1` da PowerShell amministratore.
- **Decisioni in sospeso**: Q7 firma del codice (utente, entro T10), Q11 ramo principale (T10).
  ADR-0010 (indice di ricerca) e ADR-0013 (video) le decide lo sviluppo con benchmark e prototipi.
- **Modalità di lavoro**: autonomia completa; unica sosta per la revisione a fine T3; push
  consentito solo del branch `rewrite` (vedi CLAUDE.md).

## Diario

### 2026-11-07 — T0 Fondamenta

- Analisi completa del vecchio Prism; dati reali dal suo catalogo: 919.090 file, 958 MB di
  database (circa 1 KB per file), indice incoerente tra scansioni complete e incrementali,
  6 scansioni rimaste "running", 0 hash memorizzati.
- Decisioni di prodotto prese con l'utente (riassunte in SPEC §1, §4, §7): catalogo dischi come
  identità, solo Windows, sempre amministratore con avvio da attività pianificata, pubblica da
  subito, tutte le funzioni nella 1.0, installer con aggiornamenti automatici, EN/IT, tema di
  sistema, hash in background, Cestino di default, conferma per i dischi nuovi, home = catalogo.
- Creato il branch `rewrite` (da `refactor/cleanup`) nel worktree `D:\Progetti\Github\Prism-next`;
  rimosso il vecchio codice (resta nella storia git).
- Scritti SPEC, ARCHITECTURE, TESTING, ROADMAP, ADR 0001–0014, CLAUDE.md, README.
- Creato il workspace Cargo con `prism-core`, `prism-win`, `prism-cli`; lint comuni; toolchain
  Rust 1.92.0 fissata; `scripts/check.ps1`; CI su GitHub Actions (Windows).
- Scelta per i test con amministratore: runner tramite attività pianificata + disco virtuale
  di test, da configurare con l'utente in T2 (TESTING §4).
- Verificato che `scripts/check.ps1` si ferma con un errore quando un controllo fallisce
  (prova con formattazione volutamente sbagliata, poi ripristinata).
- Decise con l'utente: solo Windows 11 (Q1); esclusioni di default solo `$RECYCLE.BIN` e
  `System Volume Information` (Q2); nessuna telemetria nella 1.0 (Q9); avvio con Windows attivo
  per default (Q10).
- Decise con l'utente (secondo giro): hash sempre attivi a priorità minima (Q3); pulizia con
  temporanei, cartelle vuote, file grandi e vecchi, cache di browser/app (Q4); immagini RAW
  oltre ai formati base, niente HEIC/AVIF (Q5); licenza MIT, repository pubblico (Q8); firma
  del codice decisa dall'utente verso la fine (Q7); scelte tecniche delegate allo sviluppo;
  sosta di revisione solo a fine T3; push consentito solo del branch `rewrite`.
- Scritti in anticipo gli script del runner amministratore (`scripts/admin/`) e i test di
  verifica (`prism-win/tests/admin_runner.rs`); da shell non elevata i test falliscono con
  "Accesso negato" come previsto. Documentata la verifica dell'interfaccia sull'app vera
  (debug `asInvoker`, WebDriver, screenshot).
