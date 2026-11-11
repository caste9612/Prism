# Prism — Diario di avanzamento

> Da leggere all'inizio di ogni sessione di sviluppo, insieme a `CLAUDE.md`.
> In cima: lo stato attuale. Sotto: il diario, dal più recente al più vecchio.

## Stato attuale

- **Stato**: pubblicate la **2.0.0**, la **2.1.0** (2026-11-08), la **2.1.1** (2026-11-09) e
  la **2.1.4** (2026-11-10). Il PC aziendale dell'utente si è aggiornato da solo alla 2.1.1 e ha
  provato la 2.1.2, la 2.1.3 e la 2.1.4 da installer locali. La 2.1.4 comprende la 2.1.2
  (journal fermato da record presi per rovinati, REVIEW L26), la 2.1.3 (aggiornamento mirato che
  rileggeva interi sottoalberi, analisi delle cartelle di rete che non avanzavano, L27) e le
  correzioni del quarto pacchetto (analisi fermate da attese di pochi secondi, giri troppo
  frequenti su C:, L28); il quinto pacchetto la conferma (L29). La 2.1.2 e la 2.1.3 non sono
  state pubblicate. Su `develop` la **2.2.0**, non ancora pubblicata, da provare sul PC
  aziendale con un installer locale: comprende la 2.1.5 (mai pubblicata: correzioni del sesto
  pacchetto, REVIEW L30) e un'analisi del contenuto più veloce sulle cartelle di rete (L31: ogni
  file aperto una volta, file piccoli e immagini letti una volta sola, 4 file alla volta sulla
  rete, nuove righe di log). Il lavoro continua direttamente su `develop`.
- **Prossimo passo**: un pacchetto diagnostico del PC aziendale con la 2.2.0 di prova, per
  misurare il guadagno sulla rete (righe `hashing finished` e `hashing progress`); poi la
  release col via libera dell'utente. Resta aperto REVIEW L29 (2). Prima di avviare l'app di
  sviluppo va chiusa quella installata (CLAUDE.md).
  Consigliata una copia della chiave fuori dal PC (REVIEW U7).
- **Bloccanti**: nessuno.
- **Decisioni in sospeso**: firma del codice (U1, per ora nessuna, come Orbit); riduzioni della
  dimensione del catalogo (L23, l'utente: "non serve" per ora).
- **Modalità di lavoro** (aggiornata 2026-11-08): autonomia; le decisioni si registrano in
  `docs/REVIEW.md`. Commit e push direttamente su `develop`, niente pull request (CLAUDE.md). Il
  runner amministratore (`PrismDevAdminTests`) resta finché l'utente non lo rimuove (U6).

## Diario

### 2026-11-11 — 2.2.0: analisi più veloce sulle cartelle di rete

- Dai tempi del sesto pacchetto: l'archivio di rete (2,5 milioni di file) ha richiesto 27 ore,
  21 e mezza per gli hash un file alla volta; le impronte, con 4 thread, andavano 3 volte più
  veloci sugli stessi file. Ogni file ora si apre una volta, i file piccoli e le immagini si
  leggono una volta sola (hash e impronta dagli stessi byte), sulla rete 4 file alla volta;
  l'avanzamento di immagini e video si conta senza caricare tutti i file. Nuove righe di log
  per misurare (REVIEW L31). `prism-cli hash-bench` confronta la lettura della 2.1 con quella
  nuova: su un SSD NVMe la lettura in parallelo non cambia nulla di misurabile, quindi sui
  dischi locali si legge un file alla volta.

### 2026-11-11 — 2.1.5: dal pacchetto di una giornata intera

- Sesto pacchetto del PC aziendale, con la 2.1.4 pubblicata (26 ore e mezza): nessuna analisi
  interrotta, analisi della cartella d'archivio finita, `Organ` in corso da 28 ore. Un
  aggiornamento di C: dal journal è fallito alle 05:11 UTC (`UNIQUE constraint failed`): due
  file cambiati letti con lo stesso nome, uno rinominato sopra l'altro fra le due letture;
  riprodotto in un test, ora l'aggiornamento riprova al giro dopo. Ogni rilettura completa di
  C: scambiava le voci dei nomi di un file con più hard link nella stessa cartella (1.881 voci
  "spostate" ogni volta): corretto l'abbinamento. Nel log il motivo delle riletture complete,
  in `system.txt` voci e avanzamento dell'analisi di ogni disco (REVIEW L30).

### 2026-11-10 — 2.1.4: analisi che non si fermano per attese di pochi secondi

- Quarto pacchetto del PC aziendale, con la 2.1.3 di prova (3 ore e mezza in orario di
  lavoro): confermate le correzioni della 2.1.3, nessun errore. Restavano l'analisi di `Organ`
  interrotta a ogni giro e un giro di analisi ogni 3 minuti su C:. Ora un'analisi cede il suo
  thread solo a un job che aspetta da 30 s, e dopo un giro completo il successivo parte dopo 10
  minuti; ogni ora il log riassume il lavoro degli aggiornamenti delle cartelle (REVIEW L28).
- Quinto pacchetto, con la 2.1.4 di prova (2 ore e 14 minuti di pomeriggio): nessun errore,
  l'analisi di `Organ` mai interrotta, un giro su C: ogni 10-11 minuti, gli aggiornamenti di
  `Organ` 2 secondi di lavoro all'ora. Pubblicata la 2.1.4 col via libera dell'utente (REVIEW
  L29).

### 2026-11-10 — 2.1.3: la sorveglianza rilegge solo le cartelle cambiate

- Terzo pacchetto diagnostico del PC aziendale, con la 2.1.2 di prova (53 minuti di log in
  orario di lavoro): la correzione del journal funziona, nessun aggiornamento fallito e l'analisi
  di C: ha recuperato.
- In orario di lavoro però `Organ` (3,2 milioni di voci) si rileggeva ancora per intero, una
  lettura dopo l'altra. La causa era l'aggiornamento mirato, che rileggeva tutto il sottoalbero
  della cartella cambiata: un cambio in una cartella di primo livello vuol dire la radice. La
  diagnosi di L25 (notifiche perse) era sbagliata, e il secondo pacchetto, di notte, non lo
  mostrava. Ora di ogni cartella cambiata si rilegge solo l'elenco, e per intero solo le
  sottocartelle nuove (REVIEW L27).
- Corretti anche: immagini oltre 512 MB contate come da fare (giri a vuoto ogni 2 minuti),
  analisi interrotte dagli aggiornamenti del journal quando i thread erano tutti occupati,
  aggiornamenti di poche cartelle che fermavano le analisi dello stesso server, file riscritti
  di continuo ricalcolati a ogni giro.

### 2026-11-10 — 2.1.2: il journal fermato da un falso record rovinato

- Secondo pacchetto diagnostico del PC aziendale: l'aggiornamento automatico 2.1.0 → 2.1.1 è
  riuscito e le cartelle di rete non si rileggono più per intero ogni pochi minuti, ma gli
  aggiornamenti dal journal di C: e D: sono falliti ogni 16 secondi per tutta la notte
  (`update sequence mismatch in sector 1`).
- Test diagnostico in sola lettura su C: e D: di questo PC (runner amministratore, poi tolto):
  `FSCTL_GET_NTFS_FILE_RECORD` restituisce sempre i record con i fixup applicati, e 70 record su
  ~35.000 avevano il numero di sequenza alla fine di un settore come dato; la regola "misto =
  rovinato" li scartava. Corretta la regola, aggiunta la rilettura della MFT dopo 10 minuti di
  aggiornamenti falliti, e l'attesa di 30 minuti per i giri di analisi con soli file cambiati
  rispetto al catalogo (REVIEW L26). Nuovo test elevato in sola lettura sul disco di sistema
  (`usn.rs`).
- L'utente ha chiesto di provare la correzione sul PC aziendale prima di una release:
  installer compilato in locale con `build-installer.ps1 -NoUpdateSignature`.

### 2026-11-09 — 2.1.1 dal pacchetto diagnostico di un altro PC

- La 2.1.0 è uscita il 2026-11-08 (release `v2.1.0`, approvata dall'utente). L'utente l'ha
  installata su un PC aziendale (9,15 milioni di voci, due file server Windows) e ha mandato il
  pacchetto diagnostico della giornata.
- Dal log: riletture complete ripetute di una cartella di rete da 3,2 milioni di voci, thread
  del motore tutti occupati dalle analisi, giri di analisi a vuoto ogni 2 minuti, file di
  OneDrive solo nel cloud trattati come illeggibili, C: rimasto da rileggere senza traccia, un
  controllo degli aggiornamenti fallito e non ripetuto. Tutto corretto nella 2.1.1 (REVIEW L25).

### 2026-11-08 — Dimensione del catalogo, lavori interrotti, documentazione, 2.1.0

- Domande dell'utente: quanto pesa il database e che cosa succede quando un lavoro lungo si
  interrompe. Misurato su una copia del catalogo installato (`docs/benchmarks/2026-11-08-catalog-size.md`):
  580 MB per 3,5 milioni di voci, ~120 B per voce più ~165 B per file analizzato; il 17% del
  catalogo è la seconda copia del NAS in `ZimaOS-HD\.media` (L19). Possibili riduzioni (L23):
  l'utente ha detto che non servono. Interruzioni e frequenze descritte in ARCHITECTURE §6.1.
- Revisione di tutta la documentazione (con un agente che ha confrontato documenti e codice):
  REVIEW §0 e voci superate, ROADMAP (T10 e T11 chiuse, sezione "Dopo la 2.0.0"), CLAUDE.md,
  SPEC (obiettivi misurati, domande decise), ARCHITECTURE (job reali, tabelle, struttura delle
  cartelle), TESTING, stato delle ADR 0003, 0009, 0014.
- Trovato nella revisione: F02-AC5 (cartelle di rete aggiunte a mano) mancava nell'app.
  Aggiunto *Cartella di rete…* (L24), utile per la prova dell'utente su un altro PC.
- Client del NAS: legge una risposta fino alla sua lunghezza invece di aspettare la chiusura
  della connessione (riprovato sul NAS vero).
- Prova dell'app vera (U8): l'utente ha collegato il NAS nell'app di sviluppo; pilotata poi
  l'app: RAID sulle tre cartelle, dischi nella scheda *Salute*, cartelle già nel catalogo
  rifiutate da *Cartella di rete…*.
- Versione 2.1.0, note in `docs/releases/2.1.0.md`.

### 2026-11-08 — Tipo dei dischi e RAID del NAS

- Richiesta dell'utente: il tipo dei dischi e i RAID (sul suo NAS ZimaOS, due RAID 1 di dischi
  di due tipi). Prima un giro di sola lettura sul NAS con il login dato dall'utente per vedere
  cosa ZimaOS dice: RAID con livello, stato, attività e dischi; dischi con modello, tipo,
  collegamento, temperatura, ore e salute (un HGST senza SMART leggibile). Nessuna API
  documentata: si usano le chiamate del pannello web.
- Scelta dell'utente: "Dischi PC + NAS ZimaOS". **D65**: SSD/HDD e giri dei dischi del PC
  (schema 11). **D66**: login del NAS nelle impostazioni, salvato nel Gestore credenziali di
  Windows; RAID sulle schede e nell'intestazione del NAS, dischi nella scheda *Salute*,
  notifica quando un RAID o un disco peggiora; lettura oraria. F17-AC5/AC6 nella SPEC.
- `reqwest` (già nel progetto per gli aggiornamenti) va in panico senza un fornitore
  crittografico: per l'HTTP in chiaro del NAS basta un client minimo su `TcpStream`, provato sul
  NAS vero.
- Non provato nell'app vera: il Prism installato è aperto ed elevato (istanza unica), quindi
  l'app di sviluppo non parte (U8). Coperto da test unitari, test dell'app con un NAS finto,
  e2e con il backend simulato e la lettura del NAS vero.

### 2026-11-08 — Preparazione del rilascio 2.0.0

- Ripresa dopo uno spegnimento accidentale del PC: nessun dato perso (albero pulito, ultimo
  commit già su GitHub).
- Installer `Prism_2.0.0_x64-setup.exe` costruito (6 MB, senza firma né aggiornamenti).
- `scripts/sandbox-test.ps1`: prova dell'installer in Windows Sandbox, un Windows pulito e usa e
  getta (REVIEW U3, TESTING §4bis); Sandbox va attivata una volta dall'utente.
- Note di rilascio in `docs/releases/2.0.0.md`.
- Domanda dell'utente: C: compare sempre come disco nuovo. Non è un errore di riconoscimento
  (una sola scheda dal 2026-11-07, ritrovata a ogni avvio): nel catalogo di sviluppo C: non è
  mai stato né indicizzato né ignorato. Il testo "nuovo" resta (scelta dell'utente).
- Aggiornamenti come Orbit (D57): chiave pubblica e indirizzo in `tauri.conf.json`, workflow
  `release.yml` con approvazione, environment `release` creato su GitHub (solo `develop`),
  `scripts/new-update-key.ps1` per la chiave (da lanciare dall'utente). Niente firma del codice
  per ora, come Orbit.
- Dalle domande dell'utente durante la prova dell'app:
  - il NAS aveva cambiato indirizzo dopo un riavvio (.65 → .57): ora una cartella di rete si
    ritrova dal nome del server o si sposta a mano (D59); l'utente ha messo una prenotazione
    DHCP e il NAS è tornato a .65;
  - le unità di rete si analizzano anche loro, una per server alla volta (D58); trovato e chiuso
    il rischio di eliminare l'unica copia di un file visto da due percorsi (D62: hard link, o
    ZimaOS-HD che mostra in `.media` le altre condivisioni);
  - *Attività in corso* dice perché un'analisi aspetta (D61);
  - multithreading misurato: le impronte delle foto scalano quasi linearmente, ora usano un
    quarto dei core, da 1 a 4 thread (D60, `docs/benchmarks/2026-11-08-prints-threads.md`);
  - le anteprime di *Simili* non si vedevano nell'app vera per la CSP (L20): corretto e aggiunto
    un controllo alla prova dell'app vera; nelle schede di un disco il conteggio di quello che
    resta da analizzare è ora del disco (L21);
  - cartelle di rete col nome della cartella e l'icona di rete (D63); schede raggruppate per
    disco fisico e per NAS (D64): sul PC dell'utente C:, D: ed E: stanno sotto "Samsung SSD 990
    PRO 2TB · NVMe · 3 partizioni".
- Prova dell'app vera: 14 su 14 (con le anteprime); test elevati verdi.
- Rilascio: chiave degli aggiornamenti creata (password casuale in `~\.tauri`), PR #2 unita in
  `develop` dall'utente, workflow *Release* avviato e approvato dall'utente. Vecchio Prism
  1.0.1 (portatile) e i suoi dati spostati nel Cestino su richiesta dell'utente.
- README aggiornato con sette screenshot presi dall'interfaccia con il backend simulato
  (`npm --prefix ui run screens`, `docs/screenshots/`): dati inventati. Tolto dal backend
  simulato il numero di serie di un disco reale; il NAS simulato ora ha un indirizzo IP e un
  nome imparato, come quelli veri.
- **Release 2.0.0 pubblicata** (13:26 UTC, run 42483929205): installer 6.428.090 byte,
  `latest.json` servito da `releases/latest`, firma fatta con la chiave dell'app (verificato
  l'identificativo). Installer scaricato in `Downloads`; l'installazione aspetta il clic UAC
  dell'utente.
- Su richiesta dell'utente: niente più pull request, si lavora direttamente su `develop`
  (CLAUDE.md); README rifatto sul modello di quello di Orbit (intestazione con badge, *Why*,
  *Install*, *Features* con screenshot, *Footprint* con le misure dei benchmark, *Stack & key
  decisions*, scorciatoie); screenshot orizzontali 1440×900 in una cornice da finestra; NAS
  simulato rinominato "NAS Media"; nel catalogo i gruppi di dischi si affiancano quando ci
  stanno (D64).
- La 2.0.0 pubblicata è installata sul PC dell'utente: attività `\Prism`, voci del menu di
  Esplora risorse e catalogo nuovo verificati (REVIEW U3). Chiave degli aggiornamenti copiata,
  e password spostata, nella cartella delle credenziali dell'utente su sua richiesta; verificato
  che la chiave si apre (REVIEW U7).
- Pulizia chiesta dall'utente (REVIEW L22): il runner amministratore ora scollega il disco
  PRISMTEST a fine giro (prima restava collegato e il Prism installato l'ha visto come disco
  nuovo); disco scollegato; eliminati benchmark, esperimenti in `%TEMP%` e log vecchi (~1,3 GB).

### 2026-11-08 — Versione 2.0.0, icona ovunque, prova dell'app vera

- Versione **2.0.0** (scelta dall'utente: le release 1.0.x pubblicate sono del vecchio Prism).
- Icona: l'eseguibile di sviluppo aveva ancora quella vecchia, perché `tauri-build` non guarda i
  file delle icone; ora `build.rs` li guarda. L'icona delle notifiche (release) si riscrive
  quando cambia.
- Nuova **prova dell'app vera** prima di un rilascio (`ui/e2e-real/`, TESTING §4bis): 13 controlli
  su catalogo, schede del disco, ricerca, menu di Esplora risorse, etichette, impostazioni, e le
  operazioni sui file del solo disco PRISMTEST. Test elevati rifatti con il runner: verdi.
- Difetti trovati e corretti grazie alla prova: la mappa tornava alla radice a ogni
  aggiornamento in sottofondo (L18); i file nuovi si analizzavano fino a 5 minuti dopo un
  aggiornamento (D54); con nomi lunghi la tabella accanto alla mappa allargava la schermata
  (D56).
- Su segnalazione dell'utente: riquadro *Attività in corso* con fase, conteggi, tempo trascorso e
  stima (D53); finestra minima 960 × 640 e interfaccia che si adatta (D56).
- NAS dell'utente: le condivisioni senza lettera mappata ora si riconoscono dal percorso di rete
  (D55); indicizzata ZimaOS-HD (537.009 file in 4 minuti, via rete); limite L19.

### 2026-11-08 — Merge su develop (U2, parte)

- Su richiesta dell'utente `rewrite` va su `develop` (ramo predefinito del repository) con una
  pull request. Da `develop` arrivava un solo commit (il README del vecchio Prism con i suoi
  screenshot): tenuto il README nuovo, tolti gli screenshot della vecchia interfaccia, riga di
  stato del README aggiornata (le release 1.0.0 e 1.0.1 sono del vecchio Prism).
- Resta il rilascio (U2): numero di versione maggiore di 1.0.1 (consigliato 2.0.0), dopo U7 e U3.

### 2026-11-08 — T11: nuova interfaccia costruita

- Fondamenta: variabili CSS chiare e scure del linguaggio B+C, caratteri inclusi
  (`@fontsource`: Inter, Space Grotesk, IBM Plex Mono), barra in alto, navigazione dal disco
  (`nav.svelte.ts`), niente menu laterale.
- Catalogo: blocco "Tutto il catalogo", schede compatte con colore e posizione, barra per
  categorie (calcolata in sottofondo un disco alla volta), filtri, ordine, vista elenco, dischi
  nuovi in un riquadro a comparsa, attività in corso, ultime azioni, barra di stato.
- Backend: comando `recent_actions` (registro delle azioni per le *Ultime azioni*), con test.
- Schermata del disco con le schede Spazio (mappa a riquadri tinti e tabella con colonne),
  Duplicati, Simili, Backup, Pulizia, Salute (nuova, al posto della finestra), Tipi di file;
  le schermate di funzione hanno il parametro `scope` per limitarsi a un disco. Ricerca,
  Impostazioni (sezioni a sinistra, interruttori), finestre e avvisi nel nuovo stile;
  *Cerca in questo disco…* nel menu "⋯" (F01-AC4).
- Icona "prisma a strati" generata da `scripts/make-icons.py` (stessa geometria del logo).
- Test: 46 unitari (nuovi: ultime azioni, filtri, ordine, colori, barre), 48 end-to-end
  riscritti sui nuovi percorsi con le stesse verifiche (`ui/e2e/nav.ts` per i percorsi comuni).
- Trovato durante il lavoro: con il disco come filtro, la schermata Duplicati ricordava quel
  disco anche per tutto il catalogo; ora il disco della scheda non si ricorda.

### 2026-11-08 — Revisione dell'utente: nuova interfaccia (T11)

- L'utente ha provato l'app di sviluppo e ha bocciato l'interfaccia (aspetto, colori,
  disposizione). Insieme abbiamo disegnato dei mockup su una tela di claude.ai: tre stili, la
  versione "B+C", tutte le schermate e le proposte d'icona. Approvati: versione B+C, tutte le
  schermate, icona "prisma a strati" (ADR-0019, REVIEW D51, D52, U5).
- Aperta la tappa T11 nella ROADMAP.

### 2026-11-07 — T10 completata (lavoro autonomo)

**Report di fine tappa T10**

- Fatto: installer NSIS con menu di Esplora risorse, attività di avvio e scelta di tenere il
  catalogo (D46); aggiornamenti firmati, installati solo su conferma, e manifest firmato dallo
  script di rilascio (D47); chiusura pulita del catalogo all'uscita (L16); NF2 e NF3 misurati
  con 5,7 milioni di voci e memoria dell'analisi del contenuto ridotta (D48, L17); istantanea
  `ScanTree` compatta (D49, L1); aggiornamento dal journal con elenchi parziali (D50, L4); file
  compressi e sparsi verificati nel lettore MFT (L2); documentazione per l'utente nel README.
- Criteri coperti: F22-AC1/AC2 (installer costruito e controllato nello script generato;
  installazione da provare su un PC pulito), F22-AC3, F18-AC3 (agganci dell'installer), NF2,
  NF3, NF4 (uscita pulita), NF6.
- Controlli: `scripts/check.ps1` verde (ora anche clippy sui test elevati); test elevati
  passati sul disco PRISMTEST; test lungo del journal passato; CI verde su GitHub.
- Resta all'utente: U1 firma, U2 merge e 1.0, U3 prova su un PC pulito con la checklist T10,
  U4 dischi fisici, U5 giudizio sull'interfaccia, U6 rimozione del runner, U7 chiavi e
  pubblicazione degli aggiornamenti.

### 2026-11-07 — T10: file compressi e sparsi nel lettore MFT (L2)

- Test elevato `mft.rs`: un file compresso (`compact /c`) e uno sparso (`fsutil sparse`) sul
  disco PRISMTEST, sul percorso già accettato dalla guardia. Il lettore MFT dà dimensione e
  attributi uguali al walker e uno spazio occupato molto sotto la dimensione. Primo tentativo
  fallito perché gli strumenti non accettano i percorsi `\\?\`.
- `check.ps1` ora controlla con clippy anche i test elevati: la `ScanTree` compatta (D49)
  aveva lasciato in `tests/usn.rs` un `&*` che nessun controllo vedeva.

### 2026-11-07 — T10: aggiornamento dal journal più leggero (L4)

- Misura su C: con tempi per fase (nuovi log di debug in `update_ntfs` e `apply_changes`):
  anche con 10 file cambiati un giro costava ~0,7 s, perché la cartella Temp (115.792 file)
  si rileggeva e si confrontava per intero a ogni giro.
- `Listing::Partial` e `reconcile::reconcile_changes` (D50): per ogni cartella toccata solo i
  file cambiati e le voci che avevano lì; la mappa dei riferimenti passa dal pianificatore al
  riconciliatore. Ora 12–20 voci confrontate e ~0,32 s per giro (quasi tutto la mappa).
- Errore mio, trovato dalla misura: una prima versione cercava i riferimenti con query SQL,
  ma l'indice sui riferimenti non c'è più dal T2 (D6): ogni ricerca leggeva tutto il volume e
  un giro non finiva. Tolte le query, la mappa resta l'unico modo (ADR-0017).
- Prove: test di proprietà (160 casi), test lungo (migliaia di casi), test elevati sul disco
  PRISMTEST con modifiche casuali vere confrontate con la reindicizzazione completa.

### 2026-11-07 — T10: documentazione per l'utente

- `README.md`: che cosa fa Prism (schermate e menu di Esplora risorse), sicurezza dei file,
  installazione e privilegi, aggiornamenti, privacy (nessuna telemetria, dove stanno catalogo e
  log, pacchetto diagnostico solo su richiesta), limiti noti (HEVC, unità di rete, salute dei
  dischi USB), disinstallazione con il catalogo tenuto. Ogni affermazione controllata
  sull'interfaccia: corrette due prime stesure (la pausa a batteria è un'opzione, la ricerca non
  ha un filtro per cartella).

### 2026-11-07 — T10: memoria durante l'indicizzazione (L1)

- `ScanTree` compatta (D49): nodi da 64 byte e nomi in un unico buffer; `ScanNode` è ora una
  vista per valore con gli stessi campi (cambiati solo tre `&*` nel riconciliatore e nei
  test). Il lettore MFT alloca l'istantanea una volta sola. Nuovo test sui valori dei nodi.
- Indicizzazione completa di C: (1,5 milioni di voci): picco da 426,7 a 333,6 MiB con la MFT
  (e 0,6 s più veloce), da 347,0 a 248,8 MiB con il walker; il confronto con il codice
  precedente è fatto sulla stessa macchina subito prima. L1 ridotto; il resto dopo la 1.0.

### 2026-11-07 — T10: prestazioni finali, NF2 e NF3

- `prism-cli replicate --entries N`: aggiunge copie non collegate dei volumi a un catalogo di
  sviluppo (identità inventata, solo dentro `PrismDev`); `prism-cli mem-bench`: memoria del
  nucleo a regime; `Store::release_memory`. Test sulla replica (coerenza, identità, totali).
- Misure con 5.693.221 voci (`docs/benchmarks/2026-11-07-t10-memory.md`): 120 B per voce sul
  disco, 173 con l'indice di ricerca in memoria (NF2); nucleo 311 MiB.
- L'app con lo stesso catalogo occupava ~495 MiB: ~190 venivano dall'analisi del contenuto di
  D:, che teneva lo stato di tutti i 629.000 file. Ora tiene solo gli id dei file da fare e
  rilegge ogni file quando tocca a lui (D48): l'app resta a 322–326 MiB (NF3 rispettato);
  l'analisi continua (11.776 hash rapidi in ~80 s). Tolto anche un costo quadratico a fine
  prima passata (L17). Il test di ordine F06-AC4 ora prova la query sul catalogo.
- Resta la memoria durante l'indicizzazione (L1).

### 2026-11-07 — T10: aggiornamenti automatici (F22-AC3)

- `crates/prism-app/src/updates.rs` con `tauri-plugin-updater`: controllo un minuto dopo
  l'avvio e ogni 24 ore, download in sottofondo, installazione solo dopo conferma
  (*Impostazioni › Aggiornamenti*: stato, note, *Controlla ora*, *Installa e riavvia…*).
  Chiave pubblica e indirizzo di `latest.json` si danno alla compilazione: senza, la build
  non si collega mai (REVIEW D47, U7).
- `scripts/build-installer.ps1`: con `TAURI_SIGNING_PRIVATE_KEY` firma l'installer e scrive
  `latest.json` (senza BOM) per la release `v<versione>`.
- Prove: unità (configurazione), e2e (conferma, *Annulla*), app di sviluppo reale (le
  impostazioni dicono che la build non controlla; nel log "automatic updates not configured").
  Costruzione completa con una chiave di prova nella cartella temporanea: firma verificata con
  `minisign-verify` (lo stesso controllo del plugin), installer alterato rifiutato, indirizzo
  compilato in `prism.exe`; poi ricostruito l'installer senza chiavi (5,5 MB: +1 MB per lo
  stack di rete del plugin).
- Errori trovati: `update` era già il nome della funzione delle impostazioni (pagina rotta in
  e2e); un secondo `role="status"` rompeva il test della diagnostica (ora `aria-live`); la CLI
  di Tauri controlla la firma appena creata con la chiave pubblica della configurazione, che è
  vuota: lo script la passa in un file di configurazione aggiuntivo solo per quella build.
- Problema trovato e risolto (REVIEW L16): l'aggiornamento chiude Prism con
  `process::exit`, e lo stesso fa Tauri alla fine del ciclo degli eventi (*Esci*): lo `Store`
  non veniva mai rilasciato, quindi ogni avvio dopo un'uscita normale rifaceva il controllo di
  integrità del catalogo. Ora `Store::close` (test `nf4_explicit_close_records_a_clean_exit_without_drop`)
  si chiama su `RunEvent::Exit` e nel `on_before_exit` dell'aggiornamento; anche il file di
  log viene chiuso all'uscita. Il tray non si raggiunge con l'automazione su questa macchina
  (YasbBar al posto della barra delle applicazioni): verifica nella checklist T3.

### 2026-11-07 — T10: installer

- `crates/prism-app/tauri.conf.json`: pacchetto NSIS per tutta la macchina, italiano e inglese,
  WebView2 con il bootstrapper; agganci `windows/installer-hooks.nsh` che copiano
  `prism-shell.exe` e lo eseguono con `--install`/`--uninstall` (menu di Esplora risorse nella
  lingua di Windows e attività pianificata). `scripts/build-installer.ps1`.
- Installer costruito: `Prism_0.1.0_x64-setup.exe`, 4,5 MB. Controllato lo script generato
  (agganci inseriti, casella dei dati dell'applicazione, AppUserModelID sulle scorciatoie).
  Non eseguito sulla macchina di sviluppo: la prova è REVIEW U3 con la checklist T10.
- Errore trovato alla prima costruzione: il comando dell'interfaccia di Tauri gira dalla
  cartella `crates`, non da `crates/prism-app`; percorso corretto.

### 2026-11-07 — T9 completata: menu di Esplora risorse

- `prism-win::pipe` (named pipe con accesso esplicito all'utente non elevato),
  `prism-win::shell_menu` (voci sotto `HKCU\Software\Classes`, provate su una chiave di prova),
  `autostart::run` e attività `Prism` sempre registrata con il solo trigger legato
  all'impostazione.
- Nuovo crate `prism-shell`: inoltra `analyze`/`duplicates` all'app, o la avvia con l'attività
  pianificata e poi inoltra.
- App: server della pipe, comando `locate_path`; l'interfaccia apre lo spazio della cartella o
  i duplicati filtrati su di essa. 3 Playwright.
- Un mio errore durante il lavoro: uno script di modifica riapriva `autostart.rs` in scrittura
  prima di rileggerlo e lo svuotava; ripristinato da git due volte (la seconda perché lo
  script di correzione, passato per heredoc, aveva perso le barre rovesciate e non trovava la
  riga). Nessun commit coinvolto.

- Prova sull'app vera (sviluppo, elevata): sulle schede di C:, D: ed E: (partizioni dello
  stesso Samsung SSD 990 PRO) "Salute: buona · 41 °C", con la finestra dei valori e una riga di
  storia; PRISMTEST "Salute: non disponibile". Il file di log del giorno si scrive (Esplora
  risorse ne mostra la dimensione aggiornata solo alla chiusura). Pacchetto diagnostico
  esportato dalle impostazioni e aperto con `zipfile` di Python: CRC e date corretti,
  `system.txt` leggibile. Menu di Esplora risorse: `prism-shell analyze D:\Progetti` avviato
  con un token ristretto non amministratore (`runas /trustlevel:0x20000`) ha raggiunto l'app
  elevata attraverso la pipe, che ha aperto la mappa di `Data › Progetti`. Ritocco: nella storia
  della salute lo stato senza il prefisso "Salute:".

**Report di fine tappa T9**

- Fatto: diagnostica (log giornalieri tenuti 14 giorni, pacchetto con anteprima), reset del
  catalogo con conferma, scelta per le identità ambigue, salute dei dischi con storia e
  notifiche, menu di Esplora risorse con `prism-shell` e pipe.
- Criteri coperti da test automatici: F02-AC3, F17-AC1…AC4, F18-AC1/AC2, F20-AC1…AC3,
  F21-AC4; F18-AC3 con l'installer (T10).
- Controlli: `scripts/check.ps1` verde.
- Verifica manuale per l'utente: checklist T9 in `docs/REVIEW.md` §4.

### 2026-11-07 — T9: salute dei dischi (SMART)

- Schema v8: `disk_health`. `prism-core::health`: interpretazione delle pagine NVMe e degli
  attributi ATA, stato con i motivi, peggioramenti. `prism-win::health`: lettura con gli IOCTL
  di Windows, motivo quando i dati non ci sono (USB, disco virtuale, diritti, disco che non li
  fornisce).
- App: thread orario, storia, notifica di peggioramento con i testi dell'interfaccia; comando
  `disk_health`. Interfaccia: indicatore sulle schede e finestra con valori e storia.
- Lettura reale (solo lettura) del disco di questa macchina: Samsung SSD 990 PRO, buono,
  42 °C, 2.444 ore, usura 1 %, nessun errore; il disco virtuale PRISMTEST "non disponibile".
- Errori miei trovati dai test prima del commit: l'indice del numero di disco nel comando SMART
  (byte 12, non 11).

### 2026-11-07 — T9: scelta per le identità ambigue

- `identity.rs` e `monitor::apply_detection`: i volumi ambigui restano in attesa con i loro
  candidati; la scelta dell'utente si applica al rilevamento successivo e aggiorna il disco
  scelto con il GUID di quello collegato. Comandi `ambiguous_volumes` e `resolve_ambiguous`.
- Interfaccia: riquadro "Quale disco è …?" nella schermata *Dischi*. Test dell'app con due
  cloni nel catalogo, 1 Playwright. REVIEW L7 risolto.

### 2026-11-07 — T9: diagnostica e reset del catalogo

- Log su file: uno al giorno in `logs` nella cartella dati, 14 tenuti (`tracing-appender`).
- `prism-core::diagnostics`: archivio ZIP non compresso scritto da Prism (CRC-32, date DOS),
  elenco dei log; `prism-win::system::windows_version`. App: anteprima del pacchetto
  (`system.txt` e log), esportazione senza sovrascrivere, apertura della cartella dei log.
- `store::reset_catalog` e comando `reset_catalog` (job fermati, stato in memoria svuotato,
  impostazioni e registro delle azioni conservati). Interfaccia: gruppi *Diagnostica* e
  *Catalogo* nelle impostazioni, con anteprima e conferma esplicita. 2 Playwright.
- Dal test che rilegge l'archivio: la voce della directory centrale aveva 4 byte di troppo
  (12 invece di 8 tra la lunghezza del nome e gli attributi esterni); corretto prima del commit.

### 2026-11-07 — T8 completata: video simili

- Schema v7: `video_prints`. `prism-core::media::video`: estensioni, passata in background con
  un `VideoDecoder` (il nucleo resta senza Windows), gruppi per durata e distanza media; la
  passata delle immagini e quella dei video condividono ora lo stesso ciclo.
- `prism-win::video`: `MfVideoDecoder` per le impronte, `frame_bgrx` per le miniature a colori;
  le clip di prova codificate con gli encoder di Windows sono disponibili anche ai test
  dell'app (funzione `test-support`).
- App: il job di background calcola le impronte dei video dopo quelle delle immagini; comandi
  `similar_videos` e `video_thumbnail`. Interfaccia: scheda *Video* in *Simili*, con durata.
- Test: unitari dei gruppi, passata con decodificatore finto, test dell'app con clip H.264 e
  WMV vere (stessa clip in due formati raggruppata, l'altra no), e2e della scheda.

**Report di fine tappa T8**

- Fatto: immagini simili (impronte percettive, RAW dall'anteprima incorporata, BK-tree, soglia
  regolabile, anteprime affiancate) e video simili (Media Foundation scelta con un prototipo e
  misure sui video reali, 5 fotogrammi per video, durata e risoluzione, miniature).
- Criteri coperti da test automatici: F11-AC1…AC4, F12-AC1…AC3.
- Controlli: `scripts/check.ps1` verde.
- Limiti: video HEVC solo con l'estensione di Windows (L15); unità di rete non analizzate (L13).
- Verifica manuale per l'utente: checklist T8 in `docs/REVIEW.md` §4.

### 2026-11-07 — T8: prototipo video, ADR-0013 decisa (Media Foundation)

- `prism-win::video::sample_frames`: lettore di sorgente di Media Foundation, uscita NV12 (o
  RGB32 se il decoder non la offre), luminanza dei fotogrammi ai punti richiesti, decodifica
  fino al tempo esatto dopo il salto. Comando di sviluppo `prism-cli video-frames`.
- Misure in sola lettura sui video reali di D: (`docs/benchmarks/2026-11-07-t8-video.md`):
  nessun errore su MP4 H.264 e WebM; la prima versione in RGB32 impiegava 3,9 s per video,
  con NV12 486 ms. Test con gli encoder di Windows: la stessa clip ricodificata più piccola e
  in WMV resta a 0,2 bit per fotogramma, una clip diversa a 31,6.
- ADR-0013 accettata: Media Foundation; HEVC solo con l'estensione dello Store (L15).
- Da un errore mio corretto prima del commit: un controllo dei limiti con precedenze sbagliate
  avrebbe scartato tutte le righe dei fotogrammi memorizzati dal basso.

### 2026-11-07 — T8: immagini simili nell'app

- Il job di background, finiti gli hash, calcola le impronte delle immagini del volume; il
  conteggio per volume tiene anche le immagini da analizzare.
- Comandi `similar_images` (gruppi entro una soglia, filtro per disco, la più grande per
  prima) e `image_thumbnail` (miniatura JPEG come byte grezzi, con cache). Test dell'app su
  immagini generate in una cartella temporanea.
- Schermata *Simili*: soglia regolabile, filtro per disco, anteprime caricate quando entrano
  nella vista, dischi scollegati senza anteprima. 2 Playwright.

### 2026-11-07 — T8: impronte delle immagini (nucleo)

- Schema v6: `image_prints` (impronta, risoluzione, stato del file per cui vale).
- `prism-core::media`: *difference hash* a 64 bit, decodifica con il crate `image` (JPEG, PNG,
  GIF, WebP, BMP, TIFF), anteprime dei RAW (`media::raw`, contenitori TIFF e scansione dei
  flussi JPEG per gli altri), passata in background su un volume con salvataggio a gruppi,
  BK-tree e gruppi per distanza. 2 unitari (il BK-tree trova esattamente ciò che trova il
  confronto di tutte le coppie) e 2 d'integrazione su immagini generate.
- Un'aspettativa sbagliata in un test del BK-tree (1 e 2^40 differiscono di 2 bit, non sono
  lontani) corretta con valori davvero lontani.

### 2026-11-07 — T7 completata: backup nell'app

- App (`backup_state.rs`, `backup_views.rs`): comandi `backup_verify`, `backup_compare`,
  `backup_folder` (un livello dell'albero con i totali, ricerca, filtri), `copy_preview` (dischi
  collegati, destinazione fuori dalla sorgente, guardia di debug, spazio libero),
  `copy_start` (job `copy` con registro delle azioni e aggiornamento del catalogo della
  destinazione), `copy_result`. 2 test dell'app su cartelle temporanee: verifica → albero →
  anteprima → copia → nuova verifica senza mancanti; copia fuori sandbox rifiutata in debug.
- Interfaccia: schermata *Backup* con *Verifica un backup* (sorgente, più destinazioni anche
  scollegate, riepilogo per contenuto/metadati/mancanti, albero, copia con anteprima,
  avanzamento, annullamento, esito con errori e ripresa) e *Confronta* (per percorso o per
  contenuto, totali per stato usati come filtri, albero). 4 Vitest, 5 Playwright.
- Dai test e2e: il percorso della destinazione si aggiornava solo uscendo dal campo, e
  l'anteprima restava quella vecchia; ora segue la digitazione.
- Prova sull'app vera, solo su PRISMTEST (pilotata con UI Automation): sorgente
  `F:\prism-t6`, backup `F:\prism-t7-backup` con una sola foto copiata; la verifica ha trovato
  la foto per contenuto e due file mancanti in `Documenti`; anteprima (2 file, 2,7 MB, 4,3 GB
  liberi), copia verificata con le date originali, nuova verifica: "Ogni file ha una copia"
  (per metadati, finché l'analisi del contenuto non passa sulle copie). Ne sono venuti tre
  miglioramenti: la verifica si ripete da sola dopo una copia (quando il catalogo della
  destinazione è aggiornato); senza file mancanti l'albero vuoto non compare; il controllo
  "destinazione dentro la sorgente" confronta componenti interi (`F:\prism-t6x` non è dentro
  `F:\prism-t6`), con test.

**Report di fine tappa T7**

- Fatto: verifica dei backup sul catalogo (anche con destinazioni scollegate) per contenuto o
  per metadati con la quota di ciascuno, confronto per percorso o per contenuto con i file
  spostati, albero dei risultati con i totali, copia verificata dei mancanti (mai
  sovrascrivere, salta o rinomina, annullabile, riprendibile, errori elencati).
- Criteri coperti da test automatici: F13-AC1…AC5, F14-AC1…AC3, F15-AC1…AC5 (core, app, e2e).
- Controlli: `scripts/check.ps1` verde.
- Rimandati: risultati su disco e verifiche annullabili (L14); analisi delle unità di rete
  (L13), che lascia i backup su NAS al confronto per metadati.
- Verifica manuale per l'utente: checklist T7 in `docs/REVIEW.md` §4.

### 2026-11-07 — T7: verifica, confronto e copia (nucleo)

- `store::subtree_files`: i file sotto una cartella con percorso relativo e hash valido.
- `prism-core::backup`: `verify` (F13), `compare` per percorso o per contenuto (F14),
  `folder_view` per l'albero dei risultati con i totali; `backup::copy` con file temporaneo,
  rilettura, data mantenuta, conflitti, annullamento e ripresa (F15). 3 unitari e 6
  d'integrazione su file veri, tutti nella sandbox del test.
- Trovato dai test: la copia provava a creare la cartella di destinazione anche quando esiste
  già; se la destinazione è la radice della sandbox la guardia (giustamente) rifiuta, e un
  test d'annullamento restava ad aspettare per sempre il primo byte. Ora le cartelle si creano
  solo se mancano, e il test non aspetta più di 20 s.

### 2026-11-07 — T6 completata: analisi del contenuto e duplicati nell'app

- `prism-win::process`: modalità background del thread (`BackgroundMode`, CPU e disco a
  priorità minima, F06-AC2) e stato della batteria (`on_battery`).
- `prism-core::jobs`: `should_yield`, un job di background cede il disco a un job più urgente
  (test con quattro thread: altro disco e stessa priorità non contano). `hashing` usa
  l'avanzamento del job, con le fasi `hashQuick` e `hashFull`. `duplicates::find`: totali di
  tutti i gruppi, percorsi solo per quelli mostrati.
- App: thread dell'analisi (`hash_jobs.rs`), job `hash` a priorità background che si ferma per
  pausa, batteria o un'indicizzazione in attesa; avanzamento per disco sulle schede; nel tray
  la riga dei job in corso e Sospendi/Riprendi; impostazioni di pausa, batteria e dischi
  esclusi; comandi `duplicate_groups` e `delete_duplicates` (ultima copia, conferma per le
  sole copie offline, hash ricontrollato, guardia di debug). Il journal e la sorveglianza non
  aspettano più gli hash; rimuovere un disco li ferma.
- Interfaccia: schermata *Duplicati* (filtri, regole, selezione con i limiti di F10-AC4,
  conferme), avanzamento sulle schede, impostazioni dell'analisi. 4 Vitest, 6 Playwright.
- Test dell'app su file veri in `%TEMP%`: analisi in pausa, disco escluso, job di background
  che non blocca gli aggiornamenti, avanzamento sulla scheda; gruppi con la selezione delle
  regole, ultima copia e file unico rifiutati, eliminazione definitiva solo con conferma su un
  disco rimovibile, registro delle azioni.
- Trovato da un test e2e: l'avviso "analisi in pausa" sulla schermata dei duplicati leggeva lo
  stato caricato con l'elenco e restava vecchio dopo *Riprendi*; ora segue l'impostazione.
- Prova sull'app vera (debug, elevata): su D: (576.077 file) l'analisi si fermava dopo 293 file
  perché l'aggiornamento dal journal, accodato ogni 15 s, riservava lo stesso disco e l'hash gli
  cedeva il passo. Ora il job `journal` non riserva il disco (test
  `journal_updates_do_not_reserve_the_disk`); dopo la correzione l'analisi di D: è andata avanti
  ininterrotta per 4 minuti, a priorità background, con "Analisi del contenuto: 0% · in corso"
  sulla scheda. Su PRISMTEST (unico disco dove si prova a eliminare) sei file di prova in
  `F:\prism-t6`: indicizzati, analizzati, due gruppi nella schermata *Duplicati* con la regola
  "tieni la più vecchia"; le tre copie proposte (controllate via UI Automation: tutte su
  PRISMTEST) sono finite nel Cestino di F: con la conferma, e le copie tenute sono rimaste.
  Il menu del tray non è raggiungibile con UI Automation da questa sessione: resta nella
  checklist T6 (punto 2).

**Report di fine tappa T6**

- Fatto: hash in tre passate con validità legata a dimensione e data (ADR-0011), job di
  background che cede il disco, pausa dal tray e dalle impostazioni, sospensione a batteria,
  dischi esclusi, duplicati online e offline con filtri e regole, eliminazione con la regola
  dell'ultima copia e il contenuto ricontrollato.
- Criteri coperti da test automatici: F06-AC1/AC3/AC4/AC5/AC6 (integrazione e app), F06-AC2
  (modalità background verificata dal test di `prism-win`), F10-AC1…AC5 (core, app, e2e),
  F16-AC3/AC4 (core, app, e2e), F19-AC2 e F21-AC3 (app, e2e della vista).
- Controlli: `scripts/check.ps1` verde.
- Rimandati: analisi delle unità di rete (L13).
- Verifica manuale per l'utente: checklist T6 in `docs/REVIEW.md` §4.

### 2026-11-07 — T6: hash del contenuto e duplicati (nucleo)

- Schema v5: `content_hashes` (hash rapido e completo con lo stato del file per cui valgono).
- `prism-core::hashing`: tre passate (F06-AC4), controllo che il file non sia cambiato durante
  la lettura, salvataggio a gruppi, pausa e annullamento tra un file e l'altro. 2 unitari e 4
  d'integrazione su file veri (stessa dimensione e stesso hash rapido ma contenuto diverso,
  invalidazione dopo una modifica, file cambiati o spariti, pausa).
- `prism-core::duplicates`: gruppi su tutti i volumi anche offline con spazio recuperabile,
  filtri, regole di selezione, regola dell'ultima copia; `fileops` ricalcola l'hash prima di
  eliminare un duplicato. 3 unitari e 3 d'integrazione (un file con stessa dimensione e data
  ma contenuto diverso non viene eliminato). Un'aspettativa sbagliata corretta: con "percorso
  più corto" resta la copia offline, se è la più corta (D29).
- Supervisione: nuova regola in CLAUDE.md (`Cargo.lock` nello stesso commit del codice, CI
  rossa su `736db7c`); rispettata da qui in poi.
- Test instabile trovato e spiegato: `process::tests::memory_usage_is_reported` verificava che
  il picco del working set fosse ≥ del valore corrente, ma Windows non aggiorna i due contatori
  insieme e sotto carico il picco può restare indietro (fallito una volta in `check.ps1`, mai da
  solo in 5 ripetizioni). Il test ora verifica ciò che l'API garantisce (contatori riportati);
  il motivo è scritto nel test.

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
