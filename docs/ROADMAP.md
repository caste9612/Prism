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

## T3 — Motore dei job e app minima  `[x]`

- [x] Motore dei job (ADR-0009) con priorità e annullamento (la ripresa arriva con i job di
      background in T6, REVIEW D10)
- [x] `prism-app` Tauri 2: manifest elevato, istanza singola, tray, chiusura nel tray
- [x] Prototipi (ADR-0003, esiti nell'ADR): WebView2 elevata, attività pianificata (avvio con
      Windows, F19-AC3), apertura non elevata tramite la shell
- [x] Frontend Svelte 5: struttura, i18n EN/IT, tema, tipi generati, IPC
- [x] Schermata catalogo (F01), disco nuovo (F02), etichette (F03), impostazioni di base (F21)
- [x] Rifiniture del monitor: sonda delle unità di rete (REVIEW L5), sorveglianza cartelle per
      i volumi non NTFS (L6), job automatici nascosti dalle schede
- [x] Notifica di sistema per i dischi nuovi a finestra nascosta (REVIEW L8, D19)
- [x] Test e2e con IPC simulato (Playwright, `ui/e2e/`)

Criteri coperti: F01 (AC4 per le parti che arrivano con T4/T5/T9), F02 (AC3 senza la finestra
di scelta per le identità ambigue, REVIEW L7), F03-AC1/AC3, F19-AC1/AC3/AC4, F21-AC1/AC4
(parziale: manca il reset del catalogo).
**Sosta di revisione annullata** (decisione dell'utente, 2026-11-07): lo sviluppo prosegue fino
alla 1.0 e l'utente fa una sola revisione finale con `docs/REVIEW.md`.
**Verifica utente**: aspetto e usabilità della schermata catalogo; avvio con Windows.

## T4 — Spazio  `[x]`

- [x] Treemap su canvas con navigazione, elenco dei più grandi (F08)
- [x] Statistiche per tipo, estensione e fascia di dimensione (F08-AC3)
- [x] Eliminazione sicura di base (ADR-0012, F16-AC1/2/3/5/6): Cestino, verifica sul disco,
      registro delle azioni; l'ultima copia dei duplicati (F16-AC4) arriva con T6
- [x] Pulizia guidata (F09): temporanei, cartelle vuote, grandi e vecchi, cache dei browser
- [x] Guardia sandbox nelle build di debug dell'app (istruzione della supervisione, REVIEW S1)

Criteri coperti: F08-AC1/AC2/AC3/AC4/AC5, F09-AC1/AC2/AC3/AC4, F16-AC1/AC2/AC3/AC5/AC6.
**Verifica utente**: leggibilità della treemap; prova di pulizia sul disco di test (REVIEW §4).

## T5 — Ricerca  `[x]`

- [x] Benchmark e decisione sull'indice di ricerca (ADR-0010: indice in memoria)
- [x] Parser dei filtri avanzati con errori espliciti (F07-AC5/AC6); motore con ordinamento e
      conteggio limitato; indici mantenuti in background
- [x] Finestra di ricerca globale con scorciatoia configurabile (F07-AC1)
- [x] Risultati offline con nome e posizione del disco (F07-AC4)
- [x] Voce *Cerca* nel menu del tray (F19-AC2, rimandata da T3)

Criteri coperti: F07-AC1/AC2/AC3/AC4/AC5/AC6/AC7, F19-AC2 (parte *Cerca*).
**Verifica utente**: la scorciatoia Ctrl+Space da altri programmi; leggibilità dei risultati.

## T6 — Hash e duplicati  `[x]`

- [x] Job di hash in background a priorità minima, in modalità background di Windows, che cede
      il disco alle indicizzazioni; pausa, sospensione a batteria opzionale, dischi esclusi (F06,
      F21-AC3)
- [x] Gruppi di duplicati online e offline, schermata con filtri (F10)
- [x] Regole di selezione (F10-AC3)
- [x] Eliminazione sicura completa: Cestino, riverifica, ultima copia, registro azioni (F16)
- [x] Stato dei job e Pausa/Riprendi hash nel menu del tray (F19-AC2, rimandata da T3)

Criteri coperti: F06-AC1…AC6, F10-AC1…AC5, F16-AC3/AC4, F19-AC2, F21-AC3.
**Verifica utente**: checklist T6 in `docs/REVIEW.md` §4 (eliminazione solo sul disco di test).

## T7 — Backup

- [~] Verifica backup anche con destinazioni offline (F13): nucleo fatto (`prism-core::backup`);
      schermata da fare
- [~] Confronto tra dischi o cartelle (F14): nucleo fatto; schermata da fare
- [~] Copia dei file mancanti verificata, annullabile, riprendibile (F15): nucleo fatto; job e
      schermata da fare

## T8 — Media

- [ ] Immagini simili con indice per distanza di Hamming, inclusi i RAW (F11)
- [ ] Prototipi e decisione per i video (ADR-0013), poi video simili (F12)

## T9 — Integrazione con il sistema

- [ ] SMART con storia e notifiche (F17)
- [ ] `prism-shell` e menu contestuale di Esplora risorse (F18)
- [ ] Diagnostica: log a rotazione, pacchetto diagnostico (F20)
- [ ] Finestra di scelta per le identità ambigue dei volumi (F02-AC3, REVIEW L7) e reset del
      catalogo con conferma (F21-AC4), rimandati da T3

## T10 — Distribuzione e 1.0

- [ ] Installer: attività pianificata, menu contestuale, WebView2, disinstallazione (F22),
      identificativo e attivatore delle notifiche (REVIEW L9)
- [ ] Aggiornamenti automatici firmati (F22-AC3)
- [ ] Firma del codice con il certificato scelto dall'utente (Q7), ramo principale (Q11)
- [ ] Test su un PC Windows pulito; passaggio completo delle checklist manuali
- [ ] Merge di `rewrite` sul ramo principale e rilascio 1.0
