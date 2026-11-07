# Prism — Diario di avanzamento

> Da leggere all'inizio di ogni sessione di sviluppo, insieme a `CLAUDE.md`.
> In cima: lo stato attuale. Sotto: il diario, dal più recente al più vecchio.

## Stato attuale

- **Tappa**: T5 completata (2026-11-07). L'app si prova seguendo `docs/REVIEW.md` §4.
- **Prossimo passo**: T6 — hash e duplicati: job di hash in background a priorità minima con
  sospensione a batteria opzionale (F06, ADR-0011), gruppi di duplicati online e offline (F10),
  regole di selezione (F10-AC3), eliminazione completa con la regola dell'ultima copia
  (F16-AC4), stato dei job e pausa degli hash nel tray (F19-AC2). Nessuna sosta (CLAUDE.md).
- **Bloccanti**: nessuno. Il runner amministratore è configurato e verificato (attività
  `PrismDevAdminTests`, disco di test PRISMTEST): i test elevati si lanciano con
  `schtasks /run /tn PrismDevAdminTests` senza intervento dell'utente. Il montaggio del VHDX non
  sopravvive al riavvio: lo ricollega il runner stesso; la lettera di unità può cambiare.
- **Decisioni in sospeso**: Q7 firma del codice (utente, entro T10), Q11 ramo principale (T10).
  ADR-0010 (indice di ricerca) e ADR-0013 (video) le decide lo sviluppo con benchmark e prototipi.
- **Modalità di lavoro** (aggiornata 2026-11-07, ~15:30): autonomia totale **senza soste**
  fino alla 1.0; la sosta di fine T3 è annullata; le decisioni si prendono in autonomia e si
  registrano in `docs/REVIEW.md`, che guida l'unica revisione dell'utente (stasera). Una
  sessione di supervisione sorveglia il loop. Push consentito solo del branch `rewrite` (vedi
  CLAUDE.md).

## Diario

### 2026-11-07 — T5 completata: ricerca nell'app

- App: comando `search` (200 risultati migliori con disco, stato e posizione fisica, oppure
  l'errore con la posizione in caratteri), indici di ricerca mantenuti da un thread dagli
  eventi del catalogo, finestra di ricerca separata con scorciatoia globale (`Ctrl+Space`,
  configurabile; se non è registrabile lo si dice e resta la precedente), voce *Cerca* nel tray.
- Interfaccia: vista di ricerca (pagina del menu e finestra `?view=search`) con menu dei filtri
  che scrivono il testo, errori sottolineati, tastiera (frecce, Invio, Maiusc+Invio, Ctrl+C),
  azioni disattivate sui dischi scollegati. 3 Vitest degli helper, 5 Playwright (F07-AC1/AC2/
  AC4/AC5/AC6/AC7).
- Prova reale: premuto Ctrl+Space con un tasto simulato, la finestra di ricerca è comparsa in
  primo piano; "steam" su D: ha dato 414 risultati, con la cartella "Steam" in cima.

**Report di fine tappa T5**

- Fatto: indice in memoria scelto con benchmark (ADR-0010), linguaggio dei filtri con errori
  posizionati, motore con ordinamento, indici aggiornati in background, ricerca nell'app con
  finestra globale, scorciatoia configurabile e voce nel tray.
- Misure: primi 1000 risultati ≤ 1 ms su C: (1,37 milioni di voci) e ≤ 3 ms su 5,5 milioni;
  60 B per voce in memoria (F07-AC3, NF3).
- Criteri coperti da test automatici: F07-AC2/AC4/AC5/AC6 (unitari ed e2e), F07-AC1/AC7 (e2e
  della vista, prova reale della scorciatoia), F07-AC3 (benchmark).
- Controlli: `scripts/check.ps1` verde.
- Rimandati: aggiornamento incrementale degli indici (L11), default della scorciatoia con i
  metodi di input asiatici (L12).
- Verifica manuale per l'utente: checklist T5 in `docs/REVIEW.md` §4.

### 2026-11-07 — T5: linguaggio di ricerca e motore

- `search::query`: parole libere e filtri di F07-AC5 (dimensioni con unità, estensioni, tipi,
  date assolute e relative nel fuso dell'utente, disco, percorso, file/cartelle, online,
  espressioni regolari), errori con codice, valore e posizione (F07-AC6). 7 test, verdi al primo
  giro (calendario compreso).
- `search::engine`: volumi online e offline (F07-AC4), filtri sull'indice in memoria, ordinamento
  per pertinenza, conteggio fino a 10.000, disco sconosciuto come errore. 5 test; quattro
  aspettative scritte male sull'ordine a pari punteggio corrette secondo la regola documentata
  (nomi più corti, poi ordine del catalogo).
- `search::live::LiveIndexes`: indici ricostruiti in background dagli eventi del catalogo; test
  con costruzione iniziale, ricostruzione dopo un cambio e rimozione del volume.
- Decisione D27 sui dettagli del linguaggio.

### 2026-11-07 — T5: decisione sull'indice di ricerca (ADR-0010)

- `prism-core::search::index::VolumeIndex`: nomi normalizzati in un buffer unico cercati con
  `memchr::memmem`, anche su più thread; genitore, tipo, dimensione e data per i filtri. Test
  F07-AC2 (sottostringa senza maiuscole: "port" trova "report" e "passport").
- `prism-cli search-bench [--copies N] [--fts]`: misure su C: (1,37 milioni di voci) e su 5,5
  milioni simulati. Indice in memoria: 0,57 s di costruzione, 60 B per voce (da 97 B
  restituendo la capacità inutilizzata dei vettori), primi 1000 risultati ≤ 3 ms. FTS5 trigram:
  17,5 s di costruzione, +127 B per voce su disco, niente ricerche sotto i 3 caratteri.
- ADR-0010 accettata: indice in memoria (D26); ricostruzione in background con ritardo massimo
  di ~30 s (L11). Benchmark in `docs/benchmarks/2026-11-07-t5-search.md`.

### 2026-11-07 — T4 completata: schermata Pulizia e guardia dell'app di sviluppo

- App: comandi `cleanup_scan` (categorie per ogni disco collegato e indicizzato, Cestino per
  informazione) e `delete_entries` (Cestino sui volumi fissi, definitivo altrove solo con
  conferma). `prism-win::trash` ora riprova uno per uno gli elementi rimasti se la shell si
  ferma su un file in uso o sparito.
- Interfaccia: schermata *Pulizia* nel menu laterale; 3 Vitest della selezione, 5 Playwright
  (F09-AC1/AC3/AC4, F16-AC1/AC2).
- Test elevato `fileops_real.rs`: eliminazione sicura completa sul disco di test (catalogo,
  file cambiato saltato, cartella vuota e file vecchio nel Cestino vero di PRISMTEST).
- Dati reali (schermata in sola lettura, nessuna eliminazione): su D: comparivano cartelle
  vuote di Steam installato in `D:\Programmi` → nuova regola delle cartelle di programmi (D25).
- **Istruzione della supervisione (S1, priorità alta)**: le build di debug dell'app eliminano
  solo dentro PRISMTEST, `%TEMP%` e `%LOCALAPPDATA%\PrismDev` (`AppState::deletion_guard`); test
  che un'eliminazione fuori sandbox è rifiutata senza toccare il disco (L10).

**Report di fine tappa T4**

- Fatto: analisi dello spazio dal catalogo (treemap, file più grandi, statistiche per tipo,
  anche per dischi scollegati), schermata *Spazio*, eliminazione sicura con Cestino e registro
  delle azioni, pulizia guidata con regole prudenti, apertura di file e cartelle senza
  privilegi dalle liste.
- Misure: su C: (1,37 milioni di voci) treemap 0,48 s, file più grandi 0,14–0,34 s,
  statistiche 0,23 s, scansione della pulizia ~2 s.
- Criteri coperti da test automatici: F08-AC1/AC2/AC3/AC4 (e2e, unitari), F08-AC5 (misura),
  F09-AC1/AC2/AC3/AC4 (unitari, e2e), F16-AC1/AC2 (test elevati sul disco di test, e2e),
  F16-AC3/AC5/AC6 (integrazione).
- Controlli: `scripts/check.ps1` verde, test elevati verdi.
- Rimandati: soglie della pulizia configurabili nelle impostazioni (D24), consultazione del
  registro delle azioni nell'interfaccia (i dati ci sono: `store::actions_log`), regola
  dell'ultima copia (T6).
- Verifica manuale per l'utente: checklist T4 in `docs/REVIEW.md` §4.

### 2026-11-07 — T4: regole della pulizia guidata

- `prism-core::cleanup`: temporanei, cartelle vuote (le più in alto), file grandi e vecchi,
  cache dei browser; aree mai proposte (F09-AC2). 5 test su un catalogo costruito apposta
  (aree protette, nascosti, giunzioni, cartelle con il punto, soglie).
- `fileops`: la verifica delle cartelle vuote guarda tutto il sottoalbero
  (`FolderCheck::NoFiles`); nuovo test con cartelle vuote annidate.
- `prism-win::process::running_process_names` per non toccare le cache di un browser aperto
  (F09-AC4).
- CLI `prism-cli cleanup <volume>`: proposta senza eliminare nulla. Su C: reale: 21.237 file
  temporanei (8,0 GB) in 0,75 s, 102 cartelle vuote, nessun file grande e vecchio fuori dalle
  aree protette, cache di Chrome 843 MB, Edge 25 MB, Firefox 401 MB.
- Trovati sui dati reali e corretti: `C:\inetpub` (creata vuota da Windows come mitigazione
  di sicurezza, da non cancellare), le cartelle che iniziano con `$` e le cartelle dei
  profili utente in sé (`C:\Users\TEMP`) venivano proposte come cartelle vuote (D24).

### 2026-11-07 — T4: eliminazione sicura di base

- `prism-core::fileops::delete_entries` (ADR-0012): identificativi del catalogo → percorsi,
  guardia sandbox su tutti i percorsi prima di agire, verifica sul disco (dimensione e data
  identiche al catalogo; cartelle vuote se richiesto), `Remover`, controllo del disco dopo,
  catalogo aggiornato, esiti nel nuovo registro `actions_log` (schema v4).
- `prism-win::trash`: Cestino con `IFileOperation` (avviso di Windows se un elemento non può
  esservi spostato), eliminazione definitiva, lettura e svuotamento del Cestino di un volume.
- Test: 5 d'integrazione su cartelle temporanee (F16-AC3/AC5/AC6, guardia, errori) e uno
  elevato sul disco di test (F16-AC1/AC2): due elementi nel Cestino di PRISMTEST, uno eliminato
  definitivamente, poi svuotato il solo Cestino del disco di test. Tutti verdi al primo giro.
- Decisione D23: Cestino solo sui volumi fissi locali; altrove definitivo con conferma.

### 2026-11-07 — T4: schermata Spazio

- App: comandi asincroni `space_folder` (cartella, breadcrumb, contenuto e treemap in una
  chiamata), `largest_files`, `type_stats`, `open_entry`, `reveal_entry`; tipi esportati con
  `ts-rs`, categoria di ogni file calcolata nel backend.
- Interfaccia: schermata *Spazio* dalla scheda di un disco indicizzato, con mappa (treemap
  *squarified* su canvas, cartelle annidate, tooltip, legenda), elenco del contenuto, file più
  grandi e statistiche per tipo (disco o catalogo). Palette delle categorie validata con lo
  script della guida di visualizzazione (daltonismo e contrasto) in entrambi i temi (D21).
- Test: 6 Vitest del layout (aree proporzionali, nessuna sovrapposizione, annidamento, punto
  sotto il puntatore), 5 Playwright (F08-AC1/AC2/AC3/AC4, apertura).
- Provato sull'app vera su D: (655.162 voci, indicizzato dall'app in ~8,5 s in debug). Dallo
  screenshot: con 6 livelli fissi le cartelle profonde dei giochi restavano blocchi vuoti →
  treemap che espande per prime le cartelle più grandi a qualunque profondità (D20; C: 11.592
  nodi in 0,48 s).

### 2026-11-07 — T4: analisi dello spazio dal catalogo

- `prism-core::space`: contenuto di una cartella per dimensione, treemap potata e riempita in
  ampiezza, file più grandi di un sottoalbero, statistiche per categoria, estensione e fascia
  di dimensione (per volume o per tutto il catalogo), breadcrumb, controllo che una cartella
  appartenga al volume. 7 test unitari (F08-AC1/AC2/AC3, ripartizione del budget di nodi,
  somme coerenti a ogni livello).
- CLI: `prism-cli space <cartella>` con i tempi. Su C: (1,37 milioni di voci): treemap 0,22 s,
  file più grandi 0,14–0,34 s, statistiche 0,23 s (`docs/benchmarks/2026-11-07-t4-space.md`).
- **Difetti trovati con le misure e corretti**: (1) l'assemblaggio della treemap perdeva la
  radice (scoperto dai test); (2) i file più grandi di una sottocartella impiegavano oltre
  2 minuti per un piano di SQLite quadratico (indice automatico su `is_dir`): ora 0,34 s con
  ordine di join fissato.

### 2026-11-07 — T3 completata: notifiche dei dischi nuovi

- `prism-win::notify`: notifiche di Windows (toast WinRT con il crate `windows`) e
  registrazione dell'identificativo dell'app per l'utente (solo build di release; in sviluppo
  l'identificativo di Windows PowerShell, per non scrivere nel registro).
- `prism-app::notices`: i dischi mai visti trovati con la finestra nascosta diventano una sola
  notifica, nei testi inviati dall'interfaccia (`set_native_texts`, che sostituisce
  `set_tray_labels`); all'avvio aspetta i testi fino a 20 s, poi usa l'inglese. Test unitari.
- Prova reale: catalogo di sviluppo azzerato e avvio nascosto con `--autostart` → una sola
  notifica "7 nuovi dischi trovati" con l'elenco, letta dalla cronologia del Centro notifiche.

**Report di fine tappa T3**

- Fatto: motore dei job; app Tauri 2 (manifest elevato in release, istanza singola, tray,
  chiusura nel tray, avvio con Windows con attività pianificata, avvio nascosto); interfaccia
  Svelte 5 (catalogo con schede e avanzamento, dischi nuovi, etichette, rimozione, impostazioni,
  EN/IT, tema); aggiornamento continuo (journal ogni 15 s, sorveglianza cartelle per i volumi
  senza journal, sonda delle unità di rete); apertura senza privilegi; notifiche dei dischi
  nuovi. Prototipi di ADR-0003 completati.
- Criteri coperti da test automatici: F01-AC1/AC3/AC5, F02-AC1/AC4, F03-AC1, F21-AC1 (e2e e
  Vitest), F19-AC3 (test elevato), più quelli di T2 riusati dall'app. Verificati sull'app vera
  con script: F19-AC1, F19-AC4, aggiornamento automatico dal journal, apertura non elevata,
  notifica.
- Controlli: `scripts/check.ps1` verde (Rust, Vitest, Playwright), test elevati verdi, CI verde.
- Rimandati: finestra di scelta per le identità ambigue (REVIEW L7), apertura di Prism dal clic
  sulla notifica (L9, installer), collegamenti della scheda a treemap/ricerca/salute (con T4,
  T5, T9), voci "Cerca" e stato dei job nel menu del tray (F19-AC2, con T5/T6), reset del
  catalogo (F21-AC4).
- Verifica manuale per l'utente: checklist T3 in `docs/REVIEW.md` §4.

### 2026-11-07 — T3: apertura senza privilegi

- `prism-win::shell`: apertura di file, cartelle e programmi con i diritti normali
  dell'utente tramite `IShellDispatch2::ShellExecute` del desktop (crate `windows`, ADR-0003).
  Test elevato con controllo di confronto: lo stesso `cmd /c whoami /groups` risulta a
  integrità alta se avviato direttamente dal runner e media se avviato tramite la shell.
- App: pulsante *Apri in Esplora risorse* sulle schede dei dischi collegati (comando
  `open_volume` con l'id del catalogo); provato sull'app vera con PRISMTEST: la finestra si
  apre nel processo `explorer` del desktop. Test e2e: pulsante assente sui dischi scollegati.

### 2026-11-07 — T3: avvio con Windows e finestra dell'app

- `prism-win::autostart`: attività pianificata all'accesso con i privilegi più elevati,
  registrata con `schtasks.exe` da una definizione XML (UTF-16); test unitari della
  definizione e test elevato con un'attività **disattivata** in `\PrismDevTests\` (registrata,
  riletta, sostituita e rimossa). Primo tentativo fallito: `schtasks` apre il file XML in modo
  esclusivo e il mio handle era ancora aperto; corretto chiudendolo prima.
- App: l'opzione "Avvia Prism con Windows" registra o rimuove l'attività `Prism` e Prism la
  riallinea a ogni avvio; con `--autostart` la finestra parte nascosta nel tray (verificato:
  finestra principale invisibile, riaperta da un secondo avvio). Le build di sviluppo non creano
  mai l'attività (REVIEW D18).
- Scoperto con le prove: anche le build di debug creavano
  `%LOCALAPPDATA%\com.prism.catalog\EBWebView` (cartella dati di WebView2 legata
  all'identificativo). Ora la finestra è creata dal codice con la cartella WebView2 accanto al
  catalogo (`PrismDev\app\webview` in sviluppo). La cartella creata dalle prove precedenti
  (solo dati di WebView2 delle build di sviluppo) è stata rimossa.
- Esiti dei prototipi registrati in ADR-0003.

### 2026-11-07 — T3: unità di rete e sorveglianza cartelle nell'app

- `prism-app::netprobe`: le unità di rete contano come collegate solo quando il server
  risponde; capacità e spazio libero letti in thread separati con un tempo massimo di 5 s
  (`prism-win::volumes::disk_space`). Sull'app vera le tre unità di rete mostrano ora 500 GB,
  1,0 TB e 105 GB (REVIEW L5 risolto). Test unitari con condivisioni simulate raggiungibili,
  irraggiungibili e lente.
- `prism-app::watchers`: sorveglianza cartelle per i volumi indicizzati senza journal
  utilizzabile (FAT, exFAT, ReFS, unità di rete), con job `refresh`; ignorati i cambi nella
  cartella dati di Prism, che altrimenti si alimenterebbero da soli (REVIEW L6 risolto; la
  prova con una chiavetta vera è nella checklist).
- I job automatici (`journal` ogni 15 s, `refresh`) non compaiono più sulle schede: prima la
  scheda di un volume NTFS indicizzato avrebbe mostrato "Aggiornamento" per un attimo ogni 15 s.

### 2026-11-07 — T3: test end-to-end dell'interfaccia

- Playwright (`ui/e2e/catalog.spec.ts`, 7 test) sul backend simulato: dischi scollegati visibili
  con data dell'ultimo avvistamento (F01-AC1), disco nuovo indicizzato con avanzamento
  (F02-AC1, F01-AC3), *Non ora*, *Ignora sempre* e ripristino dalle impostazioni (F02-AC4),
  etichette (F03-AC1), rimozione con conferma (F01-AC5), lingua e tema (F21-AC1). In
  `scripts/check.ps1` e nella CI.
- Difetti trovati scrivendo i test e corretti: i due dialoghi avevano lo stesso id del titolo
  (nome accessibile sbagliato), le schede non avevano un nome accessibile, e un disco scollegato
  mostrava la data dell'ultimo aggiornamento invece di quella in cui era stato visto (F01-AC1).
- `npm audit`: 2 avvisi di gravità bassa in `deepmerge`, dipendenza del plugin Svelte usato
  solo per la compilazione (nessuna correzione disponibile; non finisce nell'app).

### 2026-11-07 — T3: app Tauri e interfaccia

- `prism-core::jobs::JobEngine` (commit precedente): priorità, un job pesante per disco
  fisico, annullamento, avanzamento con fase; in memoria (REVIEW D10).
- `crates/prism-app`: Tauri 2.12, manifest `requireAdministrator` in release e `asInvoker` in
  debug, istanza singola, tray (Apri/Esci, etichette tradotte dall'interfaccia), chiusura nel
  tray, CSP restrittiva, solo i permessi `core:default`. Sorveglianza dei volumi ogni 2 s,
  aggiornamento alla riconnessione, journal ogni 15 s, eventi verso l'interfaccia
  (ARCHITECTURE §7). Tipi TypeScript generati con `ts-rs` (REVIEW D11).
- `ui/`: Svelte 5 + Vite 8 + TypeScript; schermata catalogo con schede (spazio, file, stato,
  avanzamento del job), riquadri dei dischi nuovi (pannello unico con *Indicizza tutti* quando
  sono tanti, D14), etichette, rimozione con conferma, impostazioni (lingua, tema, avvio con
  Windows, dischi ignorati, informazioni). i18n EN/IT con test di parità delle chiavi, tema di
  Windows o scelto. Backend simulato per il browser e i test.
- Test: 13 test Vitest (i18n, formattazione, logica del catalogo, backend simulato; F01-AC1,
  F02-AC1, F02-AC4), tipi verificati con `svelte-check --fail-on-warnings`, tutti in
  `scripts/check.ps1` e nella CI (nuovo job dell'interfaccia, con controllo che i tipi generati
  siano aggiornati).
- Prove sull'app vera dalla shell elevata (TESTING §4bis): indicizzazione di PRISMTEST con la
  MFT cliccando *Indicizza* tramite UI Automation; file creati sul disco di test comparsi da
  soli nella scheda in meno di 20 s (journal); istanza singola e chiusura nel tray verificate.
  Dagli screenshot corretti: troppi riquadri al primo avvio (7 dischi nuovi → pannello unico),
  nomi "C:\" per i dischi senza etichetta (D15), separatori mangiati dagli spazi di Svelte.
- Registrate in REVIEW le decisioni D11–D17 e i limiti L5–L8; scritta la sezione §4 "Come
  provare l'app" con la checklist di T3.

### 2026-11-07 — T2 completata: sorveglianza cartelle

- `prism-win::watch::FolderWatcher` (libreria `notify`, REVIEW D7) e `prism-core::refresh`
  (cartelle del catalogo più vicine ai percorsi cambiati, solo le più in alto, rienumerate come
  sottoalberi; REVIEW D8).
- Test: unitario sulla scelta delle cartelle, di proprietà su disco (modifiche casuali →
  aggiornamento mirato = reindicizzazione) ed end-to-end con il watcher reale su una cartella
  temporanea.
- Nuove regole dal commit di supervisione `48614fa`: niente soste fino alla 1.0, decisioni
  registrate in `docs/REVIEW.md`. Registrate le decisioni D6–D9 e i limiti noti L1–L4.
- Incidente: un mio comando con un `cat >` senza input è rimasto bloccato 16 minuti
  (segnalato dalla supervisione); fermato, regola in CLAUDE.md rispettata da qui in avanti.

**Report di fine tappa T2**

- Fatto: rilevamento e identità dei volumi (anche unità di rete da processo elevato), lettore
  MFT (5,7 s per 1,26 milioni di voci su C:), journal USN (lettura e applicazione, 253 record su
  C: in 0,6 s), sorveglianza cartelle. Rimandati: istantanea compatta (L1), `$MFT` enormi (L2).
- Criteri coperti da test automatici: F02-AC3 (elevato, cambio di lettera reale), F02-AC5,
  F04-AC1, F05-AC1/AC2/AC3/AC4 (simulati e su NTFS reale).
- Controlli: `scripts/check.ps1` verde; test elevati tramite il runner verdi.
- Verifica manuale per l'utente: collegare e scollegare un disco esterno (REVIEW U4).

### 2026-11-07 — T2: applicazione del journal USN al catalogo

- `prism-core::changes::apply_changes`: dai file cambiati agli elenchi superficiali delle
  cartelle toccate; `reconcile_many` (più bersagli in una sola esecuzione, eliminazioni
  rimandate) e `Listing::NotListed` nel reconciler.
- `prism-win::ntfs`: `index_ntfs` salva la posizione del journal letta **prima** della lettura
  della MFT; `update_ntfs` applica il journal e ricade sulla lettura completa quando serve.
  CLI: `scan --mft` salva la posizione, nuovo comando `update`.
- Test: proprietà sul file system simulato (160 casi × 3 giri, con Cestino escluso e
  ripristini), stress da 3.000 semi × 4 giri (manuale), test elevato sul disco di test con
  modifiche reali (4 giri, ripetuto 3 volte): aggiornamento dal journal = reindicizzazione.
- **Difetti trovati dai test e corretti**: (1) il simulatore non applicava le esclusioni come un
  vero enumeratore (corretto il simulatore); (2) mancavano tra le cartelle toccate i genitori
  nominati dai record (es. file creato e poi spostato); (3) una cartella bersaglio spostata in
  zona esclusa non veniva rimossa e la sua esclusione si calcolava sul percorso del catalogo
  invece che su quello reale.
- **Scoperta**: NTFS scrive in ritardo nei record MFT le date delle cartelle; due letture in
  momenti diversi possono differire di pochi millisecondi senza alcun record nel journal
  (documentato in ADR-0018; il test elevato forza la scrittura prima di confrontare).
- Su C: reale: 253 record applicati in 0,6 s, catalogo coerente.

### 2026-11-07 — T2: lettura del journal USN

- `prism-win::usn`: stato del journal, verifica della posizione salvata (journal ricreato o
  sovrascritto → riconciliazione completa), lettura dei record con parser puro per
  `USN_RECORD_V2`/`V3` (4 test su buffer sintetici).
- `prism-win::mft::fetch_file`: stato attuale di un singolo file via
  `FSCTL_GET_NTFS_FILE_RECORD` (coerente con la cache), con i record di estensione;
  `entry_meta` condiviso con la lettura completa, così i metadati sono identici.
- **Scoperta**: con 41 hard link la `$ATTRIBUTE_LIST` non sta nel record (non residente). Ora
  si legge dai cluster del disco; se non è ancora coerente (il file system non l'ha scritta)
  si ottiene `FetchError::Stale` ("riprova"), mai dati sbagliati.
- Catalogo: posizione del journal per volume (`Volume::usn`, `set_usn_position`).
- **ADR-0018**: progetto dell'applicazione del journal (file toccati letti con `fetch_file`,
  elenchi superficiali delle cartelle toccate, una sola esecuzione del reconciler con più
  bersagli, ripiego sulla riconciliazione completa). Prism non crea journal sui dischi
  dell'utente.
- Test elevato tramite il runner (passato): creazione, rinomina (stesso riferimento),
  cancellazione e hard link visti nel journal; il recupero del file con 41 nomi dà, nome per
  nome, gli stessi metadati della lettura completa della MFT. Il journal è stato creato solo su
  PRISMTEST (C:, D:, E: hanno già il proprio).

### 2026-11-07 — T2: lettore MFT, decisione "go"

- `prism-win::mft`: parser puro delle strutture NTFS (boot sector, record con fixup,
  attributi, run list) con 8 test su record sintetici; `MftEnumerator` che legge `\\.\X:` in
  codice sicuro, unisce i record di estensione, gestisce hard link e ADS e produce lo stesso
  `ScanTree` del walker (4 test unitari sulla costruzione dell'albero).
- **Decisione go** (ADR-0017): C: 1,26 milioni di voci indicizzate in 5,7 s contro 14,3 s del
  walker (enumerazione 1,6 s contro 9,5 s); su D: i due metodi trovano esattamente gli stessi
  file e cartelle. Benchmark in `docs/benchmarks/2026-11-07-t2-mft.md`.
- **Problema trovato e risolto con un esperimento misurato**: con l'MFT la scrittura del
  catalogo era 3 volte più lenta. Causa: l'indice `(volume_id, file_ref)` riceveva inserimenti
  sparsi (13,0 s contro 2,4 s per un milione di voci). Migrazione 3: indice sul solo volume;
  le ricerche per riferimento usano una `FileRefMap` in memoria caricata solo se serve
  (0,18 s per milione di voci).
- Tabella dei record MFT compatta: memoria massima su C: da 676 a 397 MiB, costruzione
  dell'albero da 0,80 a 0,39 s.
- `ScanError::Unsupported` per i volumi che un enumeratore non può leggere (il chiamante usa il
  walker); CLI: `scan`/`bench --mft`; `names::encode_wide` per convertire i nomi UTF-16 senza
  allocazioni intermedie.
- Test elevato tramite il runner (passato): MFT e walker danno gli stessi file, dimensioni, date
  e attributi su un albero creato sul disco di test con 42 hard link (record di estensione),
  flussi alternativi, nomi Unicode e con punto finale; lo spazio dei 42 link è contato una volta.

### 2026-11-07 — T2: rilevamento dei volumi e identità

- `prism-core::volumes`: `DetectedVolume`, trait `VolumeSource`, abbinamento puro
  `match_volumes` (noto / nuovo / ambiguo) e `record_detected`. Regole in **ADR-0016**.
- `prism-win::volumes::WinVolumeSource`: volumi locali (seriale a 64 bit NTFS/ReFS da
  `FILE_ID_INFO` senza privilegi, bus del disco, dischi fisici), esclusi quelli senza punto di
  montaggio (ripristino, EFI); unità di rete dalle mappature persistenti dell'utente e della
  sessione.
- **Scoperta**: un processo elevato non vede le unità mappate dall'utente (`net use` vuoto nella
  shell elevata, mentre l'utente ha X:, Y:, Z: sul NAS). Poiché Prism gira sempre elevato, le
  mappature si leggono da `HKCU\Network` e le condivisioni si leggono via UNC (verificato).
- Schema v2: colonna `bus_type` (interno/esterno, F01). Il tipo "esterno" si ricava dal bus
  perché Windows riporta i dischi USB come fissi.
- CLI: nuovo comando `detect` (con `--record`); `scan` e `bench` di una radice di volume usano
  l'identità vera; le unità di rete si leggono via UNC.
- Test: 8 unitari sull'abbinamento (F02-AC3 lettera cambiata, cloni ambigui, GUID decisivo,
  ridimensionamento, file system diverso, rete per UNC F02-AC5), 3 di rilevamento reale non
  elevati (seriale a 64 bit coerente con quello a 32 bit di Windows, stabilità, abbinamento con
  sé stessi), 1 **elevato** che cambia davvero la lettera del disco di test PRISMTEST e verifica
  che l'identità resti la stessa (passato tramite il runner; lettera ripristinata a F:).
- Benchmark di rete (`docs/benchmarks/2026-11-07-t2-network.md`): X: (21.302 voci) via UNC in
  7,3 s a freddo, 1,2 s a caldo.

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
