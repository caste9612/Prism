# Prism — Roadmap

> Tappe **interne**: ognuna produce qualcosa di completo e verificabile, ma la prima versione
> pubblica (1.0) esce solo alla fine, con tutte le funzioni della SPEC.
> Ogni tappa termina con: documentazione aggiornata, controlli verdi (`scripts/check.ps1`) e una
> **checklist di verifica per l'utente**. Lo stato dettagliato è in `docs/PROGRESS.md`.

Legenda: `[x]` fatto · `[ ]` da fare · `[~]` in corso

## T0 — Fondamenta  `[x]`

- [x] Branch `rewrite` nello stesso repository, in una cartella separata (worktree)
- [x] Specifica funzionale con criteri di accettazione (`docs/SPEC.md`)
- [x] Architettura (`docs/ARCHITECTURE.md`) e ADR 0001–0014
- [x] Strategia di test e regole di sicurezza (`docs/TESTING.md`)
- [x] Regole di lavoro per le sessioni di sviluppo (`CLAUDE.md`)
- [x] Workspace Cargo (`prism-core`, `prism-win`, `prism-cli`), lint comuni, toolchain fissata
- [x] Script dei controlli (`scripts/check.ps1`) e CI GitHub Actions (attiva al primo push)
- [x] Decisioni anticipate per lavorare in autonomia fino alla 1.0 (SPEC §8)

**Verifica utente**: facoltativa, leggere `docs/SPEC.md`. Fatto: esecuzione una tantum di
`scripts/admin/install-admin-runner.ps1` (2026-11-07).

## T1 — Catalogo di base (core, senza privilegi)  `[~]`

- [x] Schema SQLite e migrazioni; test di migrazione
- [x] Archivio: thread scrittore + pool di lettori (ADR-0008)
- [x] Modello volumi/voci, ricostruzione percorsi, `dir_stats` incrementali
- [x] `ExclusionPolicy` unica (F04-AC5) con le esclusioni di default decise (Q2)
- [x] `WalkEnumerator` parallelo senza `metadata()` per file
- [x] `Reconciler` unico; test di proprietà "aggiornamento = reindicizzazione" (F05-AC3)
- [x] `SandboxGuard` per le operazioni distruttive (TESTING S2)
- [ ] CLI: `scan`, `ls`, `du`, `stats` su un catalogo di sviluppo
- [ ] Benchmark in sola lettura su C: e D: e confronto con il vecchio Prism (tempo, dimensione
      del catalogo per voce)

Criteri coperti: F04-AC2..AC7, F05-AC3, NF2 (prima misura), NF4 (parziale).
**Verifica utente**: lanciare il benchmark su altri dischi, se si vuole.

## T2 — Windows: volumi, MFT, USN

- [ ] Rilevamento volumi e identità (ADR-0004); unità di rete e UNC
- [x] Runner amministratore e disco virtuale di test (TESTING §4): configurato dall'utente e
      verificato (`schtasks /run` senza UAC, test elevati passati sul volume PRISMTEST)
- [ ] Prototipo lettore MFT, misure su VHDX e su C:/D: (sola lettura); decisione go/no-go
- [ ] `MftEnumerator` completo (frammentazione, `$ATTRIBUTE_LIST`, hard link)
- [ ] Journal USN: lettura, applicazione tramite `Reconciler`, gestione journal non valido
- [ ] Sorveglianza cartelle per i volumi non NTFS

Criteri coperti: F02-AC3, F04-AC1, F05-AC1/AC2/AC4.
**Verifica utente**: collegare/scollegare un disco esterno.

## T3 — Motore dei job e app minima

- [ ] Motore dei job (ADR-0009) con priorità, annullamento, ripresa
- [ ] `prism-app` Tauri 2: manifest elevato, istanza singola, tray, chiusura nel tray
- [ ] Prototipi: WebView2 elevata, attività pianificata, apertura non elevata (ADR-0003)
- [ ] Frontend Svelte 5: struttura, i18n EN/IT, tema, tipi generati, IPC
- [ ] Schermata catalogo (F01), disco nuovo (F02), etichette (F03), impostazioni di base (F21)
- [ ] Test e2e con IPC simulato

Criteri coperti: F01, F02, F03, F19, F21 (parziale).
**Sosta obbligatoria**: a fine T3 lo sviluppo si ferma per la revisione dell'interfaccia da parte
dell'utente; è l'unica sosta prevista prima della 1.0.
**Verifica utente**: aspetto e usabilità della schermata catalogo; avvio con Windows.

## T4 — Spazio

- [ ] Treemap su canvas con navigazione, elenco dei più grandi (F08)
- [ ] Statistiche per tipo, estensione e fascia di dimensione (F08-AC3)
- [ ] Pulizia guidata (F09): temporanei, cartelle vuote, grandi e vecchi, cache browser/app

## T5 — Ricerca

- [ ] Benchmark e decisione sull'indice di ricerca (ADR-0010)
- [ ] Parser dei filtri avanzati con errori espliciti (F07-AC5/AC6)
- [ ] Finestra di ricerca globale con scorciatoia configurabile (F07-AC1)
- [ ] Risultati offline con nome e posizione del disco (F07-AC4)

## T6 — Hash e duplicati

- [ ] Job di hash in background a priorità minima, sospensione a batteria opzionale (F06)
- [ ] Gruppi di duplicati online e offline (F10)
- [ ] Regole di selezione (F10-AC3)
- [ ] Eliminazione sicura completa: Cestino, riverifica, ultima copia, registro azioni (F16)

## T7 — Backup

- [ ] Verifica backup anche con destinazioni offline (F13)
- [ ] Confronto tra dischi o cartelle (F14)
- [ ] Copia dei file mancanti verificata, annullabile, riprendibile (F15)

## T8 — Media

- [ ] Immagini simili con indice per distanza di Hamming, inclusi i RAW (F11)
- [ ] Prototipi e decisione per i video (ADR-0013), poi video simili (F12)

## T9 — Integrazione con il sistema

- [ ] SMART con storia e notifiche (F17)
- [ ] `prism-shell` e menu contestuale di Esplora risorse (F18)
- [ ] Diagnostica: log a rotazione, pacchetto diagnostico (F20)

## T10 — Distribuzione e 1.0

- [ ] Installer: attività pianificata, menu contestuale, WebView2, disinstallazione (F22)
- [ ] Aggiornamenti automatici firmati (F22-AC3)
- [ ] Firma del codice con il certificato scelto dall'utente (Q7), ramo principale (Q11)
- [ ] Test su un PC Windows pulito; passaggio completo delle checklist manuali
- [ ] Merge di `rewrite` sul ramo principale e rilascio 1.0
