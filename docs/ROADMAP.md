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

## T1 — Catalogo di base (core, senza privilegi)  `[x]`

- [x] Schema SQLite e migrazioni; test di migrazione
- [x] Archivio: thread scrittore + pool di lettori (ADR-0008)
- [x] Modello volumi/voci, ricostruzione percorsi, `dir_stats` incrementali
- [x] `ExclusionPolicy` unica (F04-AC5) con le esclusioni di default decise (Q2)
- [x] `WalkEnumerator` parallelo senza `metadata()` per file
- [x] `Reconciler` unico; test di proprietà "aggiornamento = reindicizzazione" (F05-AC3)
- [x] `SandboxGuard` per le operazioni distruttive (TESTING S2)
- [x] CLI: `scan`, `ls`, `du`, `stats` su un catalogo di sviluppo (più `volumes`, `check`,
      `bench`)
- [x] Benchmark in sola lettura su C: e D: e confronto con il vecchio Prism (tempo, dimensione
      del catalogo per voce): `docs/benchmarks/2026-11-07-t1-walker.md`

Criteri coperti: F01-AC5, F04-AC2..AC7, F05-AC3, F05-AC4 (riferimenti simulati), NF2 (prima
misura: 108–129 B per voce senza indice di ricerca), NF4 (parziale).
**Verifica utente**: lanciare il benchmark su altri dischi, se si vuole.

## T2 — Windows: volumi, MFT, USN  `[x]` (due ottimizzazioni rimandate, REVIEW L1/L2)

- [x] Rilevamento volumi e identità (ADR-0004, ADR-0016); unità di rete e UNC (anche da
      processo elevato, via `HKCU\Network`); test elevato del cambio di lettera (F02-AC3)
- [x] Runner amministratore e disco virtuale di test (TESTING §4): configurato dall'utente e
      verificato (`schtasks /run` senza UAC, test elevati passati sul volume PRISMTEST)
- [x] Prototipo lettore MFT, misure su VHDX e su C:/D: (sola lettura); decisione go/no-go:
      **go** (ADR-0017; C: 5,7 s contro 14,3 s del walker)
- [~] `MftEnumerator` completo: fatti frammentazione della `$MFT`, record di estensione
      (`$ATTRIBUTE_LIST`), hard link, ADS, confronto con il walker sul disco di test; restano
      `$MFT` con `$ATTRIBUTE_LIST` nel record 0, verifica di file compressi/sparsi
      (rimandato: REVIEW L2)
- [ ] Istantanea `ScanTree` compatta (memoria massima oggi ~315 B per voce, NF3) — rimandata:
      REVIEW L1
- [x] Journal USN: lettura, posizione salvata, gestione journal non valido, recupero dei
      singoli file e applicazione tramite il `Reconciler` (più bersagli, cartelle non elencate),
      con test di equivalenza simulati e reali (ADR-0018)
- [x] Sorveglianza cartelle per i volumi non NTFS (`notify`, aggiornamento mirato dei
      sottoalberi, test end-to-end e di proprietà su disco)

Criteri coperti: F02-AC3, F02-AC5, F04-AC1, F05-AC1/AC2/AC3/AC4.
**Verifica utente**: collegare/scollegare un disco esterno.

## T3 — Motore dei job e app minima

- [x] Motore dei job (ADR-0009) con priorità e annullamento (la ripresa arriva con i job di
      background in T6, REVIEW D10)
- [x] `prism-app` Tauri 2: manifest elevato, istanza singola, tray, chiusura nel tray
- [ ] Prototipi: WebView2 elevata (fatto: l'app gira elevata), attività pianificata, apertura
      non elevata (ADR-0003)
- [x] Frontend Svelte 5: struttura, i18n EN/IT, tema, tipi generati, IPC
- [x] Schermata catalogo (F01), disco nuovo (F02), etichette (F03), impostazioni di base (F21)
- [ ] Rifiniture del monitor: sonda delle unità di rete (REVIEW L5), sorveglianza cartelle per
      i volumi non NTFS (L6), notifica di sistema per i dischi nuovi (L8)
- [x] Test e2e con IPC simulato (Playwright, `ui/e2e/`)

Criteri coperti: F01, F02, F03, F19, F21 (parziale).
**Sosta di revisione annullata** (decisione dell'utente, 2026-11-07): lo sviluppo prosegue fino
alla 1.0 e l'utente fa una sola revisione finale con `docs/REVIEW.md`.
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
