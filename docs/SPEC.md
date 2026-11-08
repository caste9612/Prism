# Prism — Specifica funzionale

> Stato: **v1, implementata** e pubblicata con Prism 2.0.0 (2026-11-08); i criteri aggiunti
> dopo portano la data. Fonte: decisioni prese con l'utente dal 2026-11-07.
> Ogni funzione ha un ID (`Fxx`) e criteri di accettazione (`Fxx-ACn`) verificabili.
> I test che coprono un criterio ne citano l'ID nel nome o in un commento.
> Le parti marcate **[DA CONFERMARE]** sono proposte non ancora validate dall'utente.

## 1. Visione

Prism è il **catalogo di tutti i dischi** di un utente Windows: interni, esterni, chiavette e
unità di rete. Ogni disco indicizzato resta consultabile anche quando è scollegato. Sopra il
catalogo, Prism offre:

- ricerca istantanea su tutti i dischi, anche offline;
- analisi dello spazio (treemap, statistiche, pulizia guidata);
- duplicati esatti e simili (immagini e video) tra dischi diversi;
- verifica e completamento dei backup (confronto, file mancanti, copia);
- stato di salute dei dischi (SMART).

Il posizionamento rispetto ai concorrenti (WizTree per lo spazio, Everything per la ricerca,
Czkawka/dupeGuru per i duplicati) è il **catalogo offline**: sapere cosa c'è su un disco che non
è collegato, dove si trova fisicamente e se è un backup completo di un altro.

## 2. Principi

1. **I dati dell'utente sono sacri.** Nessuna operazione distruttiva senza conferma esplicita;
   per default si usa il Cestino; prima di eliminare si riverifica lo stato reale del file.
2. **Il backend è l'unica fonte di verità.** L'interfaccia mostra lo stato, non lo ricostruisce.
3. **Mai bloccare l'utente.** I lavori lunghi girano come job in background, annullabili e con
   avanzamento; l'interfaccia resta sempre reattiva.
4. **Un solo modo di fare ogni cosa.** Una sola pipeline di indicizzazione, una sola politica di
   esclusione, un solo percorso di eliminazione.
5. **Trasparenza.** L'utente vede sempre cosa sta facendo Prism e perché.

## 3. Glossario

| Termine | Significato |
|---|---|
| **Disco / volume** | Un file system montato: partizione locale, disco esterno, chiavetta o share di rete. Identificato in modo stabile anche se cambia lettera (vedi ADR-0004). |
| **Catalogo** | L'insieme dei volumi conosciuti e del loro contenuto indicizzato. |
| **Voce** | Un file o una cartella nel catalogo. |
| **Online / offline** | Un volume è online se è collegato e leggibile ora. |
| **Indicizzazione** | Lettura completa della struttura di un volume nel catalogo. |
| **Aggiornamento** | Allineamento del catalogo alle modifiche avvenute su un volume già indicizzato. |
| **Hash** | Impronta BLAKE3 del contenuto di un file; serve per duplicati, verifiche e confronti. |
| **Job** | Un lavoro in background (indicizzazione, hash, verifica, copia...) con stato e avanzamento. |

## 4. Piattaforma e requisiti generali

- **Sistema operativo**: solo **Windows 11**, x64.
- **Privilegi**: Prism gira **sempre come amministratore** (manifest `requireAdministrator`).
  L'avvio con Windows avviene tramite un'attività pianificata "esegui con i privilegi più
  elevati", quindi senza prompt UAC (ADR-0003).
- **Lingue**: inglese e italiano dal primo giorno; struttura i18n pronta per altre lingue.
  Lingua iniziale = lingua di Windows se supportata, altrimenti inglese.
- **Tema**: segue il tema chiaro/scuro di Windows, con scelta manuale nelle impostazioni.
- **Distribuzione**: installer con aggiornamenti automatici. Niente versione portable, winget o
  Store.
- **Pubblico**: app pubblica dalla prima versione della riscrittura (2.0.0, perché le 1.0.x
  erano del vecchio Prism), che include **tutte** le funzioni di questa specifica.

## 5. Funzioni

### F01 — Catalogo dischi (schermata principale)

All'apertura l'utente vede tutti i volumi del catalogo, collegati e scollegati.

Per ogni volume: nome (etichetta utente o nome di sistema), lettera o percorso attuale se
online, tipo (interno, esterno, chiavetta, rete), spazio usato/libero/totale, numero di file,
data dell'ultimo aggiornamento, stato (online, offline, in indicizzazione, errore), stato della
salute SMART se disponibile, avanzamento degli hash.

- **F01-AC1** I volumi offline restano visibili con i dati dell'ultima indicizzazione e la data
  in cui sono stati visti l'ultima volta.
- **F01-AC2** Lo stato online/offline si aggiorna entro 5 secondi dal collegamento o
  scollegamento del disco, senza ricaricare la pagina.
- **F01-AC3** Durante un job su un volume, la sua scheda mostra il tipo di job e l'avanzamento.
- **F01-AC4** Da ogni scheda si raggiungono: treemap, ricerca limitata al volume, etichette,
  salute, rimozione dal catalogo.
- **F01-AC5** Rimuovere un volume dal catalogo elimina solo i dati del catalogo, mai i file sul
  disco, e richiede conferma.

### F02 — Rilevamento dischi e disco nuovo

- **F02-AC1** Quando viene collegato un volume mai visto, Prism mostra una notifica "Nuovo
  disco: indicizzarlo?" con le azioni *Indicizza*, *Non ora*, *Ignora sempre*.
- **F02-AC2** Dalla stessa notifica (o dalla schermata che apre) si può assegnare subito nome ed
  etichette (F03).
- **F02-AC3** Un volume già nel catalogo viene riconosciuto anche se ha cambiato lettera di
  unità (ADR-0004); in caso di ambiguità Prism chiede all'utente.
- **F02-AC4** *Ignora sempre* memorizza il volume come escluso; si può annullare dalle
  impostazioni.
- **F02-AC5** Le unità di rete mappate e i percorsi UNC si possono aggiungere al catalogo
  anche manualmente (nell'app: *Cartella di rete…* nella barra del catalogo, REVIEW L24). Le unità mappate in modo persistente dall'utente sono riconosciute
  automaticamente anche se Prism gira come amministratore, e vengono lette tramite il percorso
  UNC (ADR-0016).

### F03 — Etichette per disco

- **F03-AC1** Per ogni volume l'utente può impostare: nome, posizione fisica (testo libero,
  es. "cassetto 2"), note, colore.
- **F03-AC2** I risultati di ricerca su un volume offline mostrano nome e posizione fisica del
  disco ("Si trova su: Backup Foto — cassetto 2").
- **F03-AC3** Le etichette sono legate all'identità del volume, non alla lettera.

### F04 — Indicizzazione

- **F04-AC1** Sui volumi NTFS locali l'indicizzazione legge direttamente la MFT (ADR-0006).
  Obiettivo: **≥ 1 milione di voci in meno di 30 secondi** su SSD. **Misurato**: 1,26 milioni di
  voci in 5,7 s (ADR-0017).
- **F04-AC2** Sugli altri file system (FAT32, exFAT, ReFS) e sulle unità di rete si usa
  l'enumerazione classica, con gli stessi risultati logici.
- **F04-AC3** Per ogni voce si memorizzano almeno: nome, cartella madre, file/cartella,
  dimensione, spazio occupato su disco (se disponibile), date di creazione e modifica,
  attributi.
- **F04-AC4** Le dimensioni delle cartelle (totale, numero di file e sottocartelle) sono
  disponibili subito dopo l'indicizzazione, senza ricalcoli lenti.
- **F04-AC5** Esiste **una sola** politica di esclusione, usata sia dall'indicizzazione sia
  dagli aggiornamenti. Default: si indicizza tutto, tranne `$RECYCLE.BIN` e
  `System Volume Information` (così la treemap mostra tutto lo spazio usato). L'utente può aggiungere
  esclusioni globali o per volume. Un'esclusione confronta il percorso esatto o il nome esatto
  della cartella, mai una sottostringa.
- **F04-AC6** L'indicizzazione di un volume può essere annullata; un'indicizzazione annullata
  o interrotta (crash, disco scollegato) non lascia il catalogo in uno stato incoerente: il
  volume mantiene i dati precedenti oppure viene marcato "da reindicizzare".
- **F04-AC7** Durante l'indicizzazione l'app resta usabile: ricerca e navigazione degli altri
  volumi rispondono normalmente.

### F05 — Aggiornamento automatico

- **F05-AC1** Mentre Prism è in esecuzione, le modifiche sui volumi NTFS locali compaiono nel
  catalogo entro pochi secondi (lettura del journal USN, ADR-0007).
- **F05-AC2** Quando un volume già indicizzato si ricollega, Prism lo aggiorna
  automaticamente: con il journal USN se è NTFS e il journal è ancora valido, altrimenti con
  una nuova enumerazione confrontata con il catalogo.
- **F05-AC3** Un aggiornamento produce lo stesso catalogo che produrrebbe una nuova
  indicizzazione completa dello stesso stato del disco (test di equivalenza).
- **F05-AC4** Spostamenti e rinomine di cartelle sono gestiti come tali, senza perdere hash e
  dati associati ai file contenuti.

### F06 — Hash del contenuto in background

- **F06-AC1** Dopo l'indicizzazione, Prism calcola in background gli hash BLAKE3 dei file, con
  priorità di I/O e CPU bassa.
- **F06-AC2** Il calcolo gira **sempre**, anche mentre l'utente usa il PC, ma con la priorità
  minima di disco e CPU (modalità background di Windows), così da non rallentare il lavoro.
  La sospensione a batteria è un'opzione delle impostazioni, disattivata per default (F21-AC3).
- **F06-AC3** Un hash vale solo finché dimensione e data di modifica del file non cambiano;
  se cambiano, l'hash viene invalidato e ricalcolato.
- **F06-AC4** L'ordine di calcolo privilegia i file che potrebbero avere duplicati (stessa
  dimensione di altri file), poi tutti gli altri.
- **F06-AC5** L'avanzamento è visibile per volume e si può mettere in pausa dal tray.
- **F06-AC6** Gli hash restano nel catalogo quando il volume va offline.

### F07 — Ricerca (Ctrl+Space) e filtri avanzati

- **F07-AC1** Una scorciatoia globale (default `Ctrl+Space`, configurabile) apre la finestra
  di ricerca da qualunque applicazione. Se la scorciatoia non è registrabile, Prism lo segnala
  e propone di cambiarla.
- **F07-AC2** La ricerca trova i nomi per **sottostringa**, senza distinguere maiuscole e
  minuscole: "port" trova "report.pdf".
- **F07-AC3** Latenza: risultati entro **50 ms (p95)** su un catalogo di 5 milioni di voci.
  **Misurato**: primi risultati entro 3 ms su 5,5 milioni di voci (ADR-0010).
- **F07-AC4** I risultati includono i volumi **offline**, marcati come tali, con nome e
  posizione fisica del disco.
- **F07-AC5** Filtri con sintassi testuale e controlli UI equivalenti:
  `size:>1GB`, `size:10MB..1GB`, `ext:pdf,docx`, `type:image|video|audio|document|archive|code`,
  `modified:>2024-01-01`, `modified:<30d`, `disk:"Backup Foto"`, `path:Documenti`,
  `is:dir`, `is:file`, `online:yes|no`, `regex:...`.
  Filtri diversi si combinano in AND.
- **F07-AC6** Un errore di sintassi in un filtro viene mostrato all'utente, non produce zero
  risultati silenziosi.
- **F07-AC7** Dai risultati: apri file, apri cartella in Esplora risorse, copia percorso; le
  azioni su file di volumi offline sono disabilitate. I file si aprono **senza** privilegi di
  amministratore (ADR-0003).

### F08 — Analisi dello spazio: treemap e statistiche

- **F08-AC1** La treemap di un volume o di una cartella mostra le sottocartelle in proporzione
  allo spazio occupato, con navigazione avanti e indietro (drill-down e breadcrumb).
- **F08-AC2** Accanto alla treemap c'è l'elenco ordinato per dimensione delle sottocartelle e
  dei file più grandi.
- **F08-AC3** Le statistiche per tipo di file mostrano la distribuzione per categoria, per
  estensione e per fascia di dimensione, per volume o per l'intero catalogo.
- **F08-AC4** Treemap e statistiche funzionano anche per i volumi offline.
- **F08-AC5** L'apertura di una treemap di un volume da 1 milione di voci richiede meno di
  1 secondo. **Misurato**: 0,48 s su 1,37 milioni di voci (ARCHITECTURE §4.6).

### F09 — Pulizia guidata

- **F09-AC1** Prism propone queste categorie di spazio recuperabile, ognuna con dimensione
  totale e spiegazione:
  - **file temporanei** di Windows e dell'utente (`%TEMP%`, `Windows\Temp`), solo file non in uso;
  - **cartelle vuote**;
  - **file grandi e vecchi**: sopra una dimensione e non modificati da un periodo, entrambi
    configurabili (default proposto: ≥ 500 MB, non modificati da 1 anno);
  - **cache di browser e app**: solo le cartelle di cache note (es. `Cache`, `Code Cache`,
    `GPUCache` dei profili di Chrome, Edge, Firefox), **mai** cookie, password, cronologia o
    dati dei profili;
  - in più, a titolo informativo: spazio occupato dal Cestino e collegamento ai duplicati (F10).
- **F09-AC2** Nessuna categoria include file di sistema o cartelle di programmi.
- **F09-AC4** Le cache di un browser si propongono solo se quel browser non è in esecuzione;
  altrimenti la categoria mostra "chiudi il browser per pulire".
- **F09-AC3** L'utente vede e può deselezionare ogni elemento prima di procedere; le
  eliminazioni seguono F16.

### F10 — Duplicati esatti e regole di selezione

- **F10-AC1** I gruppi di duplicati sono file con stessa dimensione e stesso hash completo,
  anche su volumi diversi, anche offline.
- **F10-AC2** Un gruppo indica quante copie sono online e quante offline, e lo spazio
  recuperabile.
- **F10-AC3** Regole di selezione automatica configurabili e combinabili, ad esempio: tieni la
  copia sul volume X; tieni la più vecchia / la più recente; tieni quella con il percorso più
  corto; non selezionare mai file in una certa cartella.
- **F10-AC4** In ogni gruppo resta sempre almeno una copia non selezionata; il backend rifiuta
  una richiesta che eliminerebbe tutte le copie (F16-AC4).
- **F10-AC5** Filtri per dimensione minima, tipo, volume e cartella.

### F11 — Immagini simili

- **F11-AC1** Prism trova immagini visivamente simili (ridimensionate, ricompresse, con piccole
  modifiche) tramite hash percettivo, con soglia di somiglianza regolabile.
- **F11-AC2** La ricerca dei simili non confronta tutte le coppie: usa una struttura di indice
  adatta alla distanza di Hamming, per restare veloce su centinaia di migliaia di immagini.
- **F11-AC3** I gruppi mostrano anteprime affiancate, risoluzione e dimensione.
- **F11-AC4** Formati: JPEG, PNG, GIF, WebP, BMP, TIFF e **RAW delle fotocamere** (CR2, CR3,
  NEF, ARW, DNG, ORF, RW2...) tramite l'anteprima JPEG incorporata nel file. HEIC/HEIF e AVIF
  non sono supportati.

### F12 — Video simili

- **F12-AC1** Prism trova video con lo stesso contenuto anche se ricodificati o in risoluzioni
  diverse, tramite un'impronta calcolata su più fotogrammi.
- **F12-AC2** Formati minimi: MP4/MOV (H.264, H.265), MKV, AVI, WMV, decodificati da Media Foundation (ADR-0013): H.265 solo con l'estensione HEVC
  di Windows (REVIEW L15).
- **F12-AC3** Il calcolo delle impronte è un job in background come gli hash (F06).

### F13 — Verifica backup

- **F13-AC1** L'utente sceglie un volume o cartella sorgente e uno o più volumi o cartelle di
  destinazione; Prism elenca i file della sorgente che non hanno una copia nelle destinazioni.
- **F13-AC2** La verifica funziona anche con le destinazioni **offline**, usando il catalogo.
- **F13-AC3** Il confronto usa il contenuto (hash) quando disponibile su entrambi i lati,
  altrimenti nome, dimensione e data; il report dice esplicitamente quanta parte è stata
  verificata per contenuto e quanta solo per metadati.
- **F13-AC4** I file mancanti sono raggruppati per cartella, con dimensioni, ricerca e filtri.
- **F13-AC5** Da un report si avvia la copia dei file mancanti (F15).

### F14 — Confronto tra dischi o cartelle

- **F14-AC1** Confronto tra due volumi o cartelle (anche offline): solo in A, solo in B,
  diversi (stesso percorso relativo, contenuto diverso), identici.
- **F14-AC2** Modalità di abbinamento: per percorso relativo, oppure per contenuto
  (rileva file spostati o rinominati).
- **F14-AC3** Il risultato è navigabile ad albero con i totali per cartella.

### F15 — Copia dei file mancanti

- **F15-AC1** Disponibile solo se sorgente e destinazione sono online.
- **F15-AC2** Prima della copia: anteprima con numero di file, dimensione totale e verifica
  dello spazio libero nella destinazione.
- **F15-AC3** La struttura delle cartelle viene ricreata sotto una cartella di destinazione
  scelta dall'utente.
- **F15-AC4** Nessun file esistente viene mai sovrascritto; in caso di conflitto l'utente
  sceglie tra *salta* e *rinomina*.
- **F15-AC5** Ogni file copiato viene verificato (hash) dopo la copia; gli errori sono
  elencati alla fine; la copia è annullabile e riprendibile.

### F16 — Eliminazione sicura (trasversale)

Vale per ogni eliminazione avviata da Prism (duplicati, pulizia, simili).

- **F16-AC1** Per default i file vanno nel **Cestino** di Windows.
- **F16-AC2** Sulle unità di rete il Cestino non esiste: l'eliminazione è definitiva e
  l'utente deve confermarla esplicitamente, con un avviso chiaro.
- **F16-AC3** Subito prima di eliminare, Prism verifica che il file sul disco corrisponda al
  catalogo (dimensione, data e, per i duplicati, hash del contenuto); se non corrisponde, il
  file viene saltato e segnalato.
- **F16-AC4** Il backend rifiuta di eliminare l'ultima copia di un gruppo di duplicati; se le
  altre copie sono solo su volumi offline serve una conferma esplicita aggiuntiva.
- **F16-AC5** L'interfaccia indica i file per identificativo del catalogo, mai per percorso
  libero; è il backend a risolvere il percorso (ADR-0003).
- **F16-AC6** Ogni eliminazione viene registrata in un registro delle azioni consultabile
  (data, file, dimensione, esito, Cestino o definitiva).

### F17 — Salute dei dischi (SMART)

- **F17-AC1** Per i dischi che lo supportano: stato complessivo (buono, attenzione, critico),
  temperatura, ore di funzionamento, usura (SSD), settori riallocati e in attesa (HDD).
- **F17-AC2** I valori vengono letti periodicamente e conservati con una storia minima, per
  rilevare peggioramenti (es. aumento dei settori riallocati).
- **F17-AC3** Se lo stato diventa *critico* o peggiora, Prism mostra una notifica.
- **F17-AC4** Se lo SMART non è disponibile (tipico dietro adattatori USB), Prism lo dice
  invece di mostrare dati vuoti.
- **F17-AC5** *(2026-11-08, richiesta dell'utente)* Ogni disco fisico dice che cos'è: SSD o
  disco rigido, con la velocità di rotazione quando il disco la dichiara; l'informazione resta
  anche a disco scollegato (REVIEW D65).
- **F17-AC6** *(2026-11-08, richiesta dell'utente)* Per le cartelle di un NAS ZimaOS di cui
  l'utente ha dato il login nelle impostazioni: il RAID su cui sta la cartella (livello e stato)
  sulla scheda e nell'intestazione del NAS, e nella scheda *Salute* i dischi del RAID con tipo,
  temperatura, ore e stato. Il NAS si rilegge ogni ora; un RAID o un disco che peggiora dà una
  notifica. Il login è conservato cifrato nel Gestore credenziali di Windows e Prism sul NAS
  legge soltanto (REVIEW D66).

### F18 — Menu contestuale di Esplora risorse

- **F18-AC1** Clic destro su una cartella o un'unità: *Analizza con Prism* (apre la treemap
  su quella cartella) e *Cerca duplicati qui*.
- **F18-AC2** Se Prism è già in esecuzione, la richiesta arriva all'istanza attiva; se non lo
  è, Prism viene avviato senza prompt UAC (attività pianificata) e poi esegue la richiesta.
- **F18-AC3** L'installer registra le voci e il disinstallatore le rimuove.

### F19 — Tray, esecuzione in background, avvio con Windows

- **F19-AC1** Chiudere la finestra nasconde Prism nell'area di notifica; i job continuano.
- **F19-AC2** Menu del tray: Apri Prism, Cerca, stato dei job in corso, Pausa/Riprendi hash
  in background, Esci.
- **F19-AC3** L'avvio con Windows (attività pianificata) si attiva o disattiva dalle
  impostazioni; per default è attivo.
- **F19-AC4** Una sola istanza: avviare Prism quando è già aperto porta in primo piano
  l'istanza esistente.

### F20 — Diagnostica e log

- **F20-AC1** Log su file a rotazione giornaliera, con conservazione limitata (es. 14 giorni).
- **F20-AC2** Dalle impostazioni: apri cartella dei log; esporta un pacchetto diagnostico
  (ZIP con log e informazioni di sistema), mostrando prima cosa contiene, perché i log
  includono percorsi di file.
- **F20-AC3** Gli errori mostrati all'utente sono comprensibili e localizzati; i dettagli
  tecnici sono nei log.

### F21 — Impostazioni

- **F21-AC1** Lingua, tema, scorciatoia di ricerca, avvio con Windows.
- **F21-AC2** Esclusioni globali e per volume (F04-AC5).
- **F21-AC3** Politica degli hash in background: attiva/disattiva, sospensione a batteria
  (default: disattivata), per volume si può disattivare.
- **F21-AC4** Volumi ignorati (F02-AC4) e reset del catalogo, con conferma.

### F22 — Installazione e aggiornamento

- **F22-AC1** Installer per Windows che: installa l'app, registra l'attività pianificata di
  avvio, registra il menu contestuale, installa WebView2 se manca.
- **F22-AC2** Il disinstallatore rimuove attività e menu contestuale e chiede se conservare il
  catalogo.
- **F22-AC3** Aggiornamenti automatici: controllo all'avvio e ogni 24 ore, download in
  background, installazione solo dopo conferma dell'utente; pacchetti di aggiornamento firmati.
- **F22-AC4** Eseguibili e installer firmati con un certificato di firma del codice.
  **Deciso** (REVIEW U1): la 2.0.0 esce senza firma, come Orbit; si aggiunge con
  `bundle.windows.signCommand` senza toccare il codice.

## 6. Requisiti non funzionali

| ID | Requisito |
|---|---|
| NF1 | **Reattività**: nessun comando dell'interfaccia attende più di 100 ms a causa di un job in background. |
| NF2 | **Dimensione del catalogo**: obiettivo ≤ 250 byte per voce, indice di ricerca incluso; il vecchio Prism occupava circa 1 KB per voce. **Misurato**: ~120 B per voce senza l'analisi del contenuto (l'indice di ricerca è in memoria, D26), ~250 B con il contenuto di tutti i file analizzato (catalogo vero dell'utente, `docs/benchmarks/2026-11-08-catalog-size.md`): al limite. |
| NF3 | **Memoria**: ≤ 500 MB con 5 milioni di voci nel catalogo. **Misurato**: ~325 MiB con 5,7 milioni di voci, indice di ricerca compreso (`docs/benchmarks/2026-11-07-t10-memory.md`). |
| NF4 | **Affidabilità**: un crash o lo spegnimento durante un job non corrompono il catalogo; all'avvio successivo i job interrotti vengono ripresi o annullati in modo pulito. |
| NF5 | **Sicurezza**: nessuna operazione su percorsi arbitrari passati dall'interfaccia; CSP restrittiva; nessun contenuto remoto nella webview (ADR-0003). |
| NF6 | **Privacy**: nessuna telemetria; l'unica connessione verso Internet è il controllo degli aggiornamenti; in rete locale Prism parla solo con i NAS di cui l'utente ha dato il login (F17-AC6). Il catalogo resta sul PC. |
| NF7 | **Accessibilità**: navigazione completa da tastiera, contrasto adeguato in entrambi i temi, etichette per gli screen reader. |
| NF8 | **Testabilità**: ogni criterio di accettazione ha almeno un test automatico, oppure una voce nella checklist di verifica manuale della tappa. |

## 7. Fuori ambito

- Export CSV/JSON (rimosso).
- Rilevamento dei dischi sovrapposti (rimosso).
- Monitor egui separato (rimosso; per la diagnosi ci sono i log e il pacchetto diagnostico, F20).
- Cronologia dello spazio nel tempo, avvisi generici (disco pieno, backup vecchio), hardlink
  al posto dell'eliminazione, anteprima file, ricerche salvate e tag, riga di comando pubblica
  (la CLI esiste solo come strumento di sviluppo).
- Ricerca nel contenuto dei file.
- macOS e Linux; versioni portable, winget, Microsoft Store.

## 8. Domande aperte

| # | Domanda | Quando serve deciderla |
|---|---|---|
| Q1 | ~~Versione minima di Windows~~ — **decisa** (2026-11-07): solo Windows 11 x64. | — |
| Q2 | ~~Esclusioni di default~~ — **decisa** (2026-11-07): solo `$RECYCLE.BIN` e `System Volume Information`. | — |
| Q3 | ~~Quando calcolare gli hash~~ — **decisa** (2026-11-07): sempre, a priorità minima (F06-AC2). | — |
| Q4 | ~~Categorie della pulizia~~ — **decisa** (2026-11-07): temporanei, cartelle vuote, grandi e vecchi, cache browser/app (F09). | — |
| Q5 | ~~Formati immagine aggiuntivi~~ — **decisa** (2026-11-07): solo RAW delle fotocamere (F11-AC4). | — |
| Q6 | ~~Video simili: Media Foundation o ffmpeg~~ — **decisa** (2026-11-07): Media Foundation (ADR-0013, REVIEW D39). | — |
| Q7 | ~~Firma del codice~~ — **decisa** (2026-11-08, REVIEW U1): nessuna per la 2.0.0, come Orbit; il progetto è predisposto per aggiungerla con una configurazione. | — |
| Q8 | ~~Licenza~~ — **decisa** (2026-11-07): MIT, open source, repository pubblico. | — |
| Q9 | ~~Telemetria~~ — **decisa** (2026-11-07): nessuna (NF6). | — |
| Q10 | ~~Avvio con Windows di default~~ — **decisa** (2026-11-07): attivo (F19-AC3). | — |
| Q11 | ~~Ramo principale finale~~ — **deciso** (2026-11-08): `develop`, dove è stato unito `rewrite` e da cui partono le release. | — |
