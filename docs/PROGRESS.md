# Prism — Diario di avanzamento

> Da leggere all'inizio di ogni sessione di sviluppo, insieme a `CLAUDE.md`.
> In cima: lo stato attuale. Sotto: il diario, dal più recente al più vecchio.

## Stato attuale

- **Tappa**: T1 completata (2026-11-07). Report sotto.
- **Prossimo passo**: T2 — rilevamento volumi e identità (ADR-0004), poi prototipo del lettore
  MFT con misure sul VHDX e su C:/D: (sola lettura) e decisione go/no-go.
- **Da fare in T2 emerso in T1**: istantanea `ScanTree` più compatta (oggi ~245 B per voce di
  picco durante l'indicizzazione: per 5 milioni di voci sarebbe ~1,2 GB); hard link e ADS
  (ADR-0015).
- **Bloccanti**: nessuno. Il runner amministratore è configurato e verificato (attività
  `PrismDevAdminTests`, disco di test PRISMTEST): i test elevati si lanciano con
  `schtasks /run /tn PrismDevAdminTests` senza intervento dell'utente. Il montaggio del VHDX non
  sopravvive al riavvio: lo ricollega il runner stesso; la lettera di unità può cambiare.
- **Decisioni in sospeso**: Q7 firma del codice (utente, entro T10), Q11 ramo principale (T10).
  ADR-0010 (indice di ricerca) e ADR-0013 (video) le decide lo sviluppo con benchmark e prototipi.
- **Modalità di lavoro**: autonomia completa; unica sosta per la revisione a fine T3; push
  consentito solo del branch `rewrite` (vedi CLAUDE.md).

## Diario

### 2026-11-07 — T1 completata: CLI e benchmark

- `prism-cli`: `scan`, `volumes`, `ls`, `du`, `stats`, `check`, `bench` su un catalogo di
  sviluppo in `%LOCALAPPDATA%\PrismDev\` (opzione `--db` per usarne un altro). `bench` usa un
  catalogo temporaneo e lo elimina tramite la guardia sandbox.
- `prism-win`: memoria del processo e stato di elevazione (primo codice `unsafe`, con `SAFETY`).
- Due test NF4 in più: chiusura non pulita rilevata e controllo d'integrità superato; catalogo
  danneggiato rifiutato all'apertura invece di essere usato.
- **Benchmark** (`docs/benchmarks/2026-11-07-t1-walker.md`, release, NVMe Samsung 990 PRO):
  C: 1.249.469 voci in 17,2 s, 129 B per voce, picco 307 MiB; D: 615.492 voci in 3,8 s,
  108 B per voce. Il vecchio Prism occupava ≈ 1.094 B per file (8–10 volte di più) e la sua
  unica scansione completa registrata andava a ≈ 2.500 file/s (contro ≈ 160.000 voci/s).

**Report di fine tappa T1**

- Fatto: tutto il contenuto della tappa (vedi ROADMAP). Criteri coperti da test automatici:
  F01-AC5, F04-AC2..AC7, F05-AC3, F05-AC4 (con riferimenti simulati; su NTFS reale in T2),
  NF2 (prima misura), NF4 (parziale: ripresa dei job in T3).
- Controlli: `scripts/check.ps1` verde (fmt, clippy `-D warnings`, 66 test tra unitari,
  integrazione e proprietà).
- Verifica manuale per l'utente (facoltativa): lanciare
  `cargo run --release -p prism-cli -- bench <disco>` su altri dischi (es. E:, unità di rete
  X:/Y:/Z:) e confrontare con la tabella; è in sola lettura.

### 2026-11-07 — T1: core del catalogo

- `prism-core`: modello (`model`), nomi con escape reversibile (`names`), politica di
  esclusione unica (`exclusion`), archivio SQLite con scrittore unico, corsia prioritaria,
  sessioni di lotti asincroni, pool di lettori ed eventi (`store`), `ScanTree` e
  `WalkEnumerator` parallelo (`scan`), reconciler unico con spostamenti per riferimento
  (`reconcile`), indicizzazione con stato del volume sempre veritiero (`indexer`), guardia
  sandbox (`sandbox`), annullamento (`cancel`).
- Schema v1 (`0001_initial.sql`) e decisioni di dettaglio in **ADR-0015** (sostituisce la
  regola di unicità case-insensitive dell'ADR-0005: NTFS ammette cartelle sensibili alle
  maiuscole).
- Problemi trovati dai test e corretti: (1) deadlock del walker annullato (i thread saltavano i
  lavori senza rispondere); (2) NTFS aggiorna in ritardo le date delle cartelle nell'indice
  della cartella madre, quindi due scansioni dello stesso albero differivano → le date delle
  cartelle si leggono dalla cartella stessa; (3) cartelle con punto o spazio finale illeggibili
  con i percorsi Win32 → percorsi verbatim (il test fallisce senza la correzione, verificato).
- Test: 36 unitari, 10 sul walker reale, 10 sul reconciler con file system in memoria con
  riferimenti (192 + 192 casi casuali: aggiornamento completo e parziale = reindicizzazione,
  id conservati negli spostamenti), 1 di proprietà su disco reale (32 casi). `check.ps1` verde.
- Scoperta d'ambiente: in questa sessione la shell è **elevata** (in T0 non lo era). Le ACL non
  bastano quindi a simulare cartelle illeggibili (vedi TESTING); CLAUDE.md aggiornato: non dare
  per scontato lo stato di elevazione, i test admin passano sempre dal runner.

### 2026-11-07 — Runner amministratore configurato

- L'utente ha confermato il prompt UAC di `scripts/admin/install-admin-runner.ps1`: creato
  `%LOCALAPPDATA%\PrismDev\prism-test.vhdx` (4 GB espandibile, NTFS, etichetta PRISMTEST,
  montato su F:) e registrata l'attività `PrismDevAdminTests`.
- Verifica dalla shell non elevata: `schtasks /run /tn PrismDevAdminTests` è partita senza UAC,
  `last.status` = `passed`; nel log `runner_is_elevated` e `test_volume_is_mounted` passano con
  `PRISM_TEST_VOLUME=F:\`.
- CI su GitHub verde; `actions/checkout` portato alla v5 per l'avviso di deprecazione di Node 20.

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
