# Prism — Architettura

> Stato: **bozza v1** (2026-11-07). Le decisioni motivate sono nelle ADR in `docs/adr/`.
> Questo documento descrive *come* è fatto il sistema; la specifica funzionale è in
> `docs/SPEC.md`. Va aggiornato nello stesso commit di ogni modifica strutturale.

## 1. Vista d'insieme

```
┌──────────────────────── prism.exe (elevato, una sola istanza) ────────────────────────┐
│                                                                                       │
│  ┌──────────────── UI (WebView2) ────────────────┐     ┌──────── Tray / hotkey ───────┐ │
│  │ Svelte 5 + TS: catalogo, ricerca, treemap,    │     │ menu, scorciatoia globale,   │ │
│  │ duplicati, backup, salute, impostazioni       │     │ notifiche                    │ │
│  └───────────────▲───────────────┬───────────────┘     └──────────────▲───────────────┘ │
│        eventi    │               │ comandi tipizzati (IPC)           │                 │
│  ┌───────────────┴───────────────▼───────────────────────────────────┴───────────────┐ │
│  │ prism-app (Tauri): comandi sottili, traduzione eventi, finestre, tray, pipe server │ │
│  └───────────────────────────────────────▲────────────────────────────────────────────┘ │
│                                          │ API Rust                                      │
│  ┌───────────────────────────────────────┴────────────────────────────────────────────┐ │
│  │ prism-core: Engine                                                                  │ │
│  │  ├─ catalog   (volumi, voci, aggregati cartelle)    ├─ jobs     (coda, priorità,    │ │
│  │  ├─ store     (SQLite: 1 writer + N lettori)         │            annullamento)      │ │
│  │  ├─ scan      (enumerazione → riconciliazione)       ├─ search   (indice nomi, filtri)│ │
│  │  ├─ hashing   (BLAKE3, cache invalidabile)           ├─ duplicates (gruppi, regole)  │ │
│  │  ├─ media     (impronte immagini/video)              ├─ backup   (verifica, confronto,│ │
│  │  └─ fileops   (Cestino, copia verificata, registro)  │            copia mancanti)    │ │
│  └───────────────────────────────────────▲────────────────────────────────────────────┘ │
│                                          │ trait di piattaforma                          │
│  ┌───────────────────────────────────────┴────────────────────────────────────────────┐ │
│  │ prism-win: volumi e identità, MFT, journal USN, SMART, Cestino (IFileOperation),    │ │
│  │ avvio non elevato, attività pianificata, priorità di background. Tutto l'unsafe è qui│ │
│  └─────────────────────────────────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────────────────────────────┘
        ▲ named pipe                                   ▲ avvio all'accesso senza UAC
┌───────┴─────────────────┐                     ┌──────┴─────────────────────────────┐
│ prism-shell.exe         │  ← menu contestuale │ Attività pianificata "Prism"       │
│ (non elevato, minimo)   │    di Esplora risorse│ (privilegi più elevati, all'accesso)│
└─────────────────────────┘                     └────────────────────────────────────┘
```

## 2. Crate del workspace (ADR-0002)

| Crate | Ruolo | Dipende da |
|---|---|---|
| `prism-core` | Logica di dominio, archivio SQLite, job, ricerca, duplicati, backup. Nessun `unsafe`, nessuna API Win32 diretta: la piattaforma passa da trait (`VolumeSource`, `Enumerator`, `ChangeFeed`, `Recycler`, `HealthProbe`...). Testabile con implementazioni finte. | — |
| `prism-win` | Implementazioni Windows dei trait: rilevamento e identità dei volumi, lettore MFT, journal USN, SMART, Cestino, avvio non elevato, attività pianificata. Unico crate con `unsafe`, ogni blocco con commento `// SAFETY:`. | `prism-core` |
| `prism-cli` | Strumento di sviluppo: scansioni, ricerche, benchmark, diagnostica e test manuali senza interfaccia grafica. Non è un prodotto pubblico. | `prism-core`, `prism-win` |
| `prism-app` | Applicazione Tauri 2: comandi IPC, eventi, finestre, tray, scorciatoia, server della named pipe. Nessuna logica di dominio. | `prism-core`, `prism-win` |
| `prism-shell` | Piccolo eseguibile **non elevato** invocato dal menu contestuale (`prism-shell analyze "<percorso>"` o `duplicates`): inoltra la richiesta all'istanza attiva via named pipe, oppure avvia l'attività pianificata `Prism` e poi la inoltra (T9). | `prism-win` |
| `ui/` | Frontend Svelte 5 + Vite + TypeScript. | — |

## 3. Modello dati (ADR-0005)

Archivio SQLite in modalità WAL, nella cartella dati dell'app. Lo schema è versionato con
migrazioni SQL incorporate (`crates/prism-core/src/store/migrations/`) e `PRAGMA user_version`;
una migrazione già pubblicata non si modifica, se ne aggiunge una nuova.

**Schema in uso (versione 11)** — testo completo in `0001_initial.sql`,
`0002_volume_bus.sql` (aggiunge `bus_type`), `0003_entries_volume_index.sql` (indice sul solo
volume al posto di `(volume_id, file_ref)`, ADR-0017) e `0004_actions_log.sql` (registro delle
azioni, T4), `0005_content_hashes.sql` (hash del contenuto, T6), `0006_image_prints.sql`
(impronte delle immagini, T8), `0007_video_prints.sql` (impronte dei video, T8),
`0008_disk_health.sql` (salute dei dischi, T9), `0009_share_servers.sql` (nomi dei server
delle condivisioni di rete, REVIEW D59), `0010_volume_disk.sql` (disco fisico di ogni volume,
REVIEW D64) e `0011_disk_media.sql` (SSD o disco rigido, REVIEW D65):

```sql
volumes (
  id, kind,                         -- fixed | removable | network
  fs_type, fs_serial, volume_guid, unc_path, os_label, capacity, free_space,   -- ADR-0004
  bus_type,                         -- usb | sata | nvme | virtual | ...  (v2, ADR-0016)
  user_name, location, notes, color,                                           -- F03
  status,                           -- new | indexing | indexed | ignored | reindex_needed
  last_mount,                       -- ultima lettera o percorso visto (mai una chiave)
  first_seen_at, last_seen_at, last_indexed_at,   -- millisecondi Unix
  usn_journal_id, usn_next,         -- journal USN (solo NTFS, T2)
  settings,                         -- JSON: esclusioni per volume, politica hash
  disk_key                          -- disco fisico "modello|seriale" (v10, REVIEW D64)
) STRICT

entries (
  id,                               -- assegnato dall'archivio, mai riusato
  volume_id  REFERENCES volumes,
  parent_id  REFERENCES entries,    -- NULL solo per la radice del volume (nome "")
  name,                             -- nome memorizzato (escape reversibile, ADR-0015)
  is_dir, size, alloc_size,         -- alloc_size NULL se non noto (walker)
  created_at, modified_at,          -- FILETIME (100 ns dal 1601), piena precisione
  attrs,                            -- FILE_ATTRIBUTE_*
  file_ref                          -- riferimento MFT, NULL per il walker
) STRICT
UNIQUE INDEX entries_parent_name (parent_id, name)    -- nome esatto (ADR-0015)
INDEX        entries_volume      (volume_id)          -- v3; nessun indice su file_ref:
                                                      -- ricerche per riferimento con una
                                                      -- mappa in memoria (FileRefMap)

dir_stats (entry_id PRIMARY KEY REFERENCES entries ON DELETE CASCADE,
           total_size, total_alloc, file_count, dir_count) STRICT   -- solo file contati

settings (key PRIMARY KEY, value) STRICT

actions_log (id, at, action,        -- v4: recycle | delete (F16-AC6, ADR-0012)
             volume_id,             -- senza chiave esterna: sopravvive alla rimozione del volume
             path, size,            -- percorso completo al momento dell'azione
             outcome, detail) STRICT  -- done | skipped_changed | skipped_missing | failed
INDEX actions_log_at (at)

content_hashes (entry_id PRIMARY KEY REFERENCES entries ON DELETE CASCADE,
                size, modified_at,    -- lo stato del file per cui l'hash vale (ADR-0011)
                quick, full,          -- BLAKE3: dimensione + 64 KiB iniziali e finali; tutto
                computed_at) STRICT   -- v5 (T6)
INDEX content_hashes_full (size, full), content_hashes_quick (size, quick)

image_prints (entry_id PRIMARY KEY REFERENCES entries ON DELETE CASCADE,
              size, modified_at,      -- lo stato del file per cui l'impronta vale
              dhash,                  -- 64 bit; NULL se il file non si decodifica
              width, height, computed_at) STRICT   -- v6 (T8)

video_prints (entry_id PRIMARY KEY REFERENCES entries ON DELETE CASCADE,
              size, modified_at, duration_ms, width, height,
              frames,                 -- 5 impronte da 8 byte; NULL se non si decodifica
              computed_at) STRICT     -- v7 (T8)

disk_health (disk_key,                 -- "modello|numero di serie" del disco fisico
             at, status, temperature, power_on_hours, wear, reallocated, pending,
             media_errors, spare, reasons,
             PRIMARY KEY (disk_key, at)) STRICT   -- v8 (T9)

share_servers (address PRIMARY KEY,      -- il server come scritto nei percorsi UNC (un IP)
               name, seen_at) STRICT     -- il suo nome ("ZimaOS.local"); v9 (REVIEW D59)

disk_media (disk_key PRIMARY KEY,        -- "modello|numero di serie", come disk_health
            media,                       -- ssd | hdd
            rpm) STRICT                  -- giri al minuto dei dischi rigidi, se dichiarati; v11
```

Nella tabella `settings`, oltre al documento delle impostazioni, `nas_problems:<server>` tiene i
problemi di un NAS già notificati (REVIEW D66). Il login dei NAS **non** sta nel catalogo: è nel
Gestore credenziali di Windows (§9).

Le chiavi esterne sono attive sulla connessione di scrittura; `entries` non usa cascate (una
sottostruttura si elimina con una CTE ricorsiva, senza limiti di profondità).

Il percorso completo di una voce **non è memorizzato**: si ricostruisce risalendo i
`parent_id` (o dall'indice in memoria). Questo elimina la ripetizione del percorso in colonne e
indici, che nel vecchio Prism portava il database a circa 1 KB per file.

Decisioni di dettaglio (unicità per nome esatto, escape dei nomi, date `FILETIME`, metadati
delle cartelle, aggregati) in ADR-0015; hard link NTFS e flussi alternativi (ADS) in §4.1.

## 4. Pipeline

### 4.0 Rilevamento e identità dei volumi (ADR-0004, ADR-0016)

```
VolumeSource::detect()          (prism-win: WinVolumeSource, nessun privilegio richiesto)
   → DetectedVolume { info: chiavi d'identità + informazioni, mounts, disks }
   → match_volumes(rilevati, catalogo)  (prism-core, funzione pura)
   → Known(id) | New | Ambiguous(ids)
   → record_detected: aggiorna il noto / aggiunge il nuovo con stato `new` / l'ambiguo attende
     la scelta dell'utente
```

- Volumi locali: seriale a 64 bit (NTFS/ReFS) o 32 bit, capacità, GUID, tipo di bus (per
  distinguere interni ed esterni), dischi fisici (solo in memoria). Esclusi i volumi senza punto
  di montaggio e le unità ottiche.
- Unità di rete: mappature persistenti dell'utente (`HKCU\Network`) più quelle della sessione;
  identificate e **lette tramite il percorso UNC**, perché un processo elevato non vede le
  lettere mappate dall'utente.
- Abbinamento: GUID + seriale → noto; altrimenti seriale + capacità unici → noto; più candidati
  o capacità diversa senza GUID → ambiguo (decide l'utente, F02-AC3); UNC normalizzato per la
  rete. La lettera non è mai una chiave.

### 4.1 Indicizzazione (ADR-0006)

```
Enumerator (MFT | walker classico)
   → stream di RawEntry { key, parent_key, name, is_dir, size, alloc, times, attrs, file_ref }
   → ExclusionPolicy (unica, ADR-0006)
   → Reconciler: confronta con il catalogo del volume → Insert / Update / Move / Delete
   → Writer (transazioni a lotti)
   → ricalcolo dir_stats lungo i rami toccati
   → aggiornamento indice di ricerca
   → evento catalog-changed
```

Indicizzazione completa e aggiornamento usano **lo stesso Reconciler**: un aggiornamento è
una riconciliazione su un insieme di voci più piccolo. Così sono garantiti gli stessi risultati
(F05-AC3), cosa che il vecchio Prism non garantiva.

- **MFT** (`prism-win::mft::MftEnumerator`; NTFS locale, richiede amministratore, ADR-0017):
  lettura diretta della `$MFT` dal dispositivo `\\.\X:` (solo codice sicuro: `seek_read`) in
  un'unica passata sequenziale a blocchi da 4 MiB, seguendo i run della `$MFT` (anche
  frammentata). Parser puro (`mft/parse.rs`: boot sector, record con fixup, attributi, run
  list) testato su record sintetici. I record di estensione si uniscono al record base per
  riferimento (così `$ATTRIBUTE_LIST` non va decodificato); tabella compatta per colonne e nomi
  in un unico buffer; albero dai `$FILE_NAME` (genitore + nome) con la stessa politica di
  esclusione. Semantica allineata al walker (verificata da un test elevato sul disco di test):
  nomi DOS esclusi, file di metadati NTFS nascosti, date e attributi da
  `$STANDARD_INFORMATION`. In più: riferimento al file, spazio allocato (con i flussi
  alternativi; i dati residenti non occupano cluster), hard link con lo spazio contato su un
  solo nome. Legge il disco, non la cache: le modifiche ancora in cache arrivano dopo, con il
  journal USN.
- **Walker classico** (`scan::WalkEnumerator`; FAT/exFAT/ReFS, unità di rete, fallback):
  enumerazione parallela delle cartelle usando i metadati restituiti dall'enumerazione stessa,
  **senza** una chiamata `metadata()` per file (su SMB ogni chiamata in più è un viaggio di
  rete). Dettagli (ADR-0015): una sola chiamata per **cartella** per leggerne le date reali;
  percorsi verbatim `\\?\`; giunzioni e link simbolici registrati ma non seguiti; i thread
  elencano soltanto, un unico collettore costruisce l'albero.

Ogni enumeratore produce uno `ScanTree`: istantanea completa di una cartella e del suo
contenuto, con le esclusioni già applicate. Le cartelle non leggibili (accesso negato, errore a
metà, scomparse) sono marcate `Unreadable`: la riconciliazione **non** elimina dal catalogo i
figli che non ha visto.
L'istantanea tiene ogni voce in 64 byte (metadati con valori riservati al posto degli
`Option`) e tutti i nomi in un unico buffer; il lettore MFT la alloca una volta sola (REVIEW
D49, L1).

**Reconciler** (`reconcile::reconcile`), visita in ampiezza; per ogni cartella abbina i figli
scansionati a quelli del catalogo: (1) per riferimento al file nella stessa cartella, prima
con lo stesso nome (i nomi di un file con più hard link nella stessa cartella tengono le loro
voci, REVIEW L30), poi con un altro nome (rinomina sul posto), (2) per nome esatto se i
riferimenti non si contraddicono, (3) per riferimento altrove nel
volume (spostamento: la voce mantiene id e dati collegati, F05-AC4). I figli rinominati o
il cui nome è preso da un'altra voce passano prima per un nome temporaneo (i nomi sono unici
per cartella); le eliminazioni avvengono alla fine, salvo voci rivendicate da uno spostamento.
L'ordine in ampiezza garantisce che uno spostamento non crei mai cicli. Al termine si
ricalcolano gli aggregati delle cartelle toccate e dei loro antenati. Il bersaglio è la radice
del volume (indicizzazione completa) o una cartella esistente (aggiornamento parziale).

**Stato del volume** (`indexer::index`): l'enumerazione non scrive nulla (se fallisce o viene
annullata il catalogo resta com'era); durante la scrittura il volume è `indexing`; se la
riconciliazione non termina diventa `reindex_needed`, e così anche dopo un crash, alla
riapertura dell'archivio (F04-AC6).

### 4.2 Aggiornamento (ADR-0007)

| Situazione | Meccanismo |
|---|---|
| NTFS locale, Prism attivo | Lettura continua del journal USN dall'ultimo `usn_next` |
| NTFS ricollegato, journal valido | Lettura del journal dall'ultimo `usn_next` |
| Journal ricreato o sovrascritto (`usn_journal_id` diverso o `usn_next` < primo USN) | Riconciliazione completa |
| Non NTFS locale, Prism attivo | Sorveglianza cartelle (`ReadDirectoryChangesW`) + riconciliazione mirata |
| Unità di rete, rimovibili ricollegate | Riconciliazione completa (walker) |
| Volume NTFS senza journal attivo | Enumerazione completa al ricollegamento; sorveglianza cartelle mentre Prism è attivo (Prism non crea journal sui dischi dell'utente) |

Componenti del journal (`prism-win`, ADR-0018):

- `usn::query_journal` / `check_position` / `read_journal`: stato del journal e record da una
  posizione salvata (parser puro `parse_usn_buffer` per `USN_RECORD_V2`/`V3`). La posizione
  (`volumes.usn_journal_id`, `usn_next`; `store::set_usn_position`) si legge prima di una
  lettura completa della MFT e si salva dopo una riconciliazione riuscita.
- `mft::fetch_file`: stato attuale di un file (record base ed estensioni, anche con
  `$ATTRIBUTE_LIST` non residente letto dai cluster) tramite `FSCTL_GET_NTFS_FILE_RECORD`,
  coerente con la cache del file system e con la stessa semantica della lettura completa
  (codice `entry_meta` condiviso). Windows restituisce questi record con i fixup già
  applicati: si riapplicano solo se tutti i settori finiscono col numero di sequenza, perché
  un settore può finire con quel valore anche come dato (`parse::fixups_pending`, REVIEW L26).
  Un file che non si legge in modo coerente fa ritentare l'aggiornamento senza avanzare; così
  anche due file cambiati letti con lo stesso nome nella stessa cartella
  (`changes::name_claimed_twice`: gli stati si leggono uno dopo l'altro, e un programma può
  rinominare un file sopra un altro fra le due letture; applicati, i due nomi si
  scontrerebbero nel catalogo, REVIEW L30). Se i tentativi falliscono per 10 minuti, il monitor
  rilegge il volume dalla MFT.
- `prism-core::changes::apply_changes`: dai file cambiati (`FileChange`: stato attuale e
  genitori nominati dai record) agli elenchi **parziali** delle cartelle toccate
  (`Listing::Partial`: solo i file cambiati che ora sono lì, con le voci che quei file avevano
  nella cartella; sottocartelle `NotListed`), applicati con `reconcile_changes` (più bersagli
  in una sola esecuzione, ordinati per profondità reale, eliminazioni rimandate alla fine).
  Il riconciliatore confronta solo quelle voci e i nomi occupati, senza leggere tutta la
  cartella: un giro costa come le modifiche, non come le cartelle (REVIEW D50). La mappa dei
  riferimenti (`file_ref_map`, non c'è un indice SQL: ADR-0017) si carica una volta per giro
  e passa al riconciliatore.
- `prism-win::ntfs`: `index_ntfs` (posizione del journal letta prima della lettura della MFT e
  salvata dopo) e `update_ntfs` (journal → `apply_changes`; ripiego su `index_ntfs` se la
  posizione manca o è persa, o se le modifiche non si possono applicare: es. cartella con
  contenuto ripristinata dal Cestino; il motivo va nel log, `volume read again in full`).

Sorveglianza cartelle (volumi senza journal, ADR-0007):

- `prism-win::watch::FolderWatcher`: `ReadDirectoryChangesW` tramite la libreria `notify`;
  `next_batch` raccoglie gli eventi in lotti (fino a una pausa di silenzio) come percorsi
  relativi alla radice; un traboccamento del buffer chiede una rienumerazione completa.
- `prism-core::refresh`: per ogni percorso cambiato, la cartella del catalogo più vicina al suo
  genitore, e il percorso stesso se è una cartella del catalogo (ignorando le zone escluse; una
  cartella che non esiste più cede il posto al suo antenato esistente). Di ognuna si rilegge solo
  l'**elenco** (`Enumerator::list`, una lettura della cartella): le sottocartelle già nel
  catalogo restano com'erano (i loro cambi arrivano con le loro notifiche), quelle nuove si
  leggono per intero; poi una sola riconciliazione con lo stesso reconciler, genitori prima delle
  sottocartelle. Rileggere tutto il sottoalbero, come fino alla 2.1.2, trasformava ogni cambio in
  una cartella di primo livello di un file server in una rilettura di 3,2 milioni di voci
  (REVIEW L27). Verificato da test di proprietà su disco, da un test che conta le letture e da un
  test end-to-end con il watcher.

### 4.3 Hash (ADR-0011, T6)

`prism-core::hashing::hash_volume` calcola gli hash di un volume in tre passate (F06-AC4):

1. hash **rapido** (BLAKE3 di dimensione, primi e ultimi 64 KiB) dei file la cui dimensione è
   condivisa con un altro file del catalogo, dal più grande;
2. hash **completo** dei file con dimensione e hash rapido condivisi (i possibili duplicati);
3. hash completo di tutti gli altri file (servirà alla verifica dei backup, T7), dal più piccolo.

Subito dopo aver letto un file si confrontano dimensione e data sul disco con il catalogo: se
il file è cambiato l'hash non si salva (lo si ricalcola dopo l'aggiornamento del catalogo). Gli
hash si salvano a gruppi in `content_hashes` con lo stato del file; ogni lettura li considera
validi solo se dimensione e data coincidono con la voce (F06-AC3), e restano quando il volume va
offline (F06-AC6). La passata si ferma pulita tra un file e l'altro su richiesta (pausa,
batteria) o annullamento; il chiamante la esegue in modalità background di Windows (priorità
minima di CPU e disco, F06-AC2).

In memoria restano solo gli id dei file ancora da fare, già ordinati (`store::quick_work`
ordina in SQL; la seconda passata tiene id, dimensione e "possibile duplicato"): ogni file si
rilegge dal catalogo quando tocca a lui (`store::hash_state`), così un file cancellato o già
analizzato nel frattempo si salta, e la cache dei percorsi delle cartelle si svuota oltre 4096
voci. Su un volume di 629.000 file l'analisi occupa ~12 MiB invece di ~190 (REVIEW D48, NF3).

Nell'app (`prism-app/src/hash_jobs.rs`) un thread, 20 s dopo l'avvio e poi ogni 30 s:

- conta per ogni volume indicizzato file e file con hash valido (ogni 5 minuti, ogni minuto
  mentre l'analisi gira: il conteggio costa centinaia di ms su un catalogo grande) e lo mette
  sulle schede (`VolumeView.hashes`, F06-AC5);
- accoda un job `hash` a priorità *background* per ogni volume collegato, indicizzato, con
  lavoro da fare, non escluso dall'utente (F21-AC3) e senza altri job, al massimo ogni 2
  minuti dopo un giro interrotto e ogni 10 dopo un giro completo (REVIEW L28). I file modificati negli ultimi 10 minuti (o fino a 10 minuti nel futuro, orologio di
  un server avanti) aspettano un giro successivo e contano come saltati: un file riscritto di
  continuo non si ricalcola a ogni giro (REVIEW L27). Anche le unità di rete (REVIEW D58): il loro job occupa la risorsa `net:<server>`,
  quindi le cartelle di uno stesso NAS si leggono una alla volta;
- per ogni volume con file ancora da leggere e nessun job dice perché aspetta
  (`HashesView.waiting`, REVIEW D61): escluso, in pausa, un altro job sul disco, file che
  l'ultimo giro completo non ha potuto leggere (cambiati durante la lettura o illeggibili,
  `skipped`), oppure il prossimo giro.

Il job entra in modalità background di Windows (`prism_win::process::BackgroundMode`) e si
ferma tra due file se l'utente mette in pausa (tray o impostazioni), se il PC va a batteria e
l'opzione lo chiede (`prism_win::process::on_battery`), o se un job più urgente aspetta lo
stesso disco (`JobEngine::should_yield`, REVIEW D31). Non serve un record persistente per
riprendere dopo un riavvio: l'avanzamento è la tabella degli hash stessa, e il giro successivo
riparte dai file senza hash.

### 4.4 Ricerca (ADR-0010)

Indice dei nomi di **tutti** i volumi, online e offline, con ricerca per sottostringa e filtri.
Scelto con i benchmark l'**indice in memoria** (`prism-core::search::index::VolumeIndex`, uno
per volume): nomi normalizzati in un buffer unico cercati con `memchr::memmem`, più genitore,
tipo, dimensione e data per i filtri; 60 B per voce, costruzione 0,57 s per 1,37 milioni di
voci, primi 1000 risultati in ≤ 3 ms anche su 5,5 milioni
(`docs/benchmarks/2026-11-07-t5-search.md`).

- `search::live::LiveIndexes`: gli indici in uso; un thread segue gli eventi del catalogo e
  ricostruisce l'indice di un volume cambiato (subito se manca, poi al massimo ogni 30 s,
  REVIEW L11). Le ricerche leggono un'istantanea e non aspettano mai una ricostruzione.
- `search::query`: il linguaggio (F07-AC5/AC6). Parole libere = sottostringhe del nome, tutte
  richieste; filtri `chiave:valore` in AND (`size`, `ext`, `type`, `modified`, `disk`, `path`,
  `is`, `online`, `regex`, con alcuni alias italiani). Poiché `:` non può stare in un nome di
  file, ogni `parola:` è un filtro: uno sconosciuto è un errore con codice, valore e posizione;
  una sola lettera (`d:`) indica l'unità. Le date digitate sono giorni locali (lo scarto dal
  fuso orario arriva dall'interfaccia).
- `search::engine`: sceglie i volumi (`disk:` sconosciuto = errore, `online:`), trova i
  candidati con la parola più lunga, controlla le altre parole e i filtri sull'indice (il
  filtro `path:` ricostruisce il percorso della cartella, una volta per cartella), conta fino a
  un tetto (10.000) e ordina: nome identico, nome che inizia con la prima parola, inizio di
  parola, altrove; poi nomi più corti, poi ordine del catalogo.
- **App**: comando `search` asincrono (testo e scarto dal fuso dell'utente) che restituisce i
  200 risultati migliori con nome, cartella, percorso completo, disco, stato e posizione fisica
  dei dischi scollegati, oppure l'errore del linguaggio con la posizione in caratteri.
  Finestra di ricerca separata (`search_window.rs`, etichetta `search`, sempre in primo piano,
  creata nascosta all'avvio, stessa cartella WebView2) aperta e nascosta dalla scorciatoia
  globale (`tauri-plugin-global-shortcut`, default `Ctrl+Space`, nelle impostazioni; se non è
  registrabile l'interfaccia lo dice e propone di cambiarla, la scorciatoia precedente resta
  attiva) e dalla voce *Cerca* del tray. L'interfaccia è la stessa pagina con `?view=search`;
  nella finestra principale la ricerca è la casella della barra in alto (ADR-0019).

### 4.5 Duplicati, simili, backup

- **Duplicati nell'app** (T6): il comando `duplicate_groups` (filtri e regole) restituisce i 300
  gruppi più grandi con i totali di tutti (`duplicates::find` costruisce i percorsi solo per i
  gruppi mostrati) e la selezione proposta dalle regole; `delete_duplicates` controlla la
  regola dell'ultima copia su tutta la selezione, poi elimina disco per disco attraverso
  `fileops` con l'hash ricontrollato e la guardia di debug (REVIEW D32).
- **Duplicati** (`prism-core::duplicates`): gruppi per (`size`, `full`) sugli hash validi, su
  tutti i volumi anche offline, con copie collegate e non, e spazio recuperabile; filtri per
  dimensione, tipo, volume e cartella (F10-AC5). Regole di selezione pure (F10-AC3): le regole
  "tieni" (sul volume X, la più vecchia, la più recente, il percorso più corto) scelgono in
  ordine la copia che resta, "mai in" protegge una cartella; le copie su dischi scollegati non
  si selezionano mai e una copia resta sempre (F10-AC4). All'eliminazione `check_last_copies`
  rifiuta di togliere l'ultima copia e chiede conferma se resterebbero solo copie offline
  (F16-AC4); `fileops` ricalcola l'hash completo sul file e salta quelli cambiati (F16-AC3).
- **Immagini simili** (`prism-core::media`, T8, F11): ogni immagine ha un'impronta percettiva
  a 64 bit (*difference hash*: l'immagine ridotta a 9×8 livelli di grigio mediando le aree, un
  bit per "più chiaro del vicino a destra"), valida come gli hash per una dimensione e una data
  del file. JPEG, PNG, GIF, WebP, BMP e TIFF si decodificano con il crate `image`; i RAW delle
  fotocamere dall'anteprima JPEG incorporata (`media::raw`: nei RAW basati su TIFF dalle
  directory, `JPEGInterchangeFormat` o strisce JPEG anche nelle sotto-directory; negli altri
  contenitori, come CR3, cercando i flussi JPEG), scegliendo la più piccola con il lato corto
  di almeno 400 px; la risoluzione mostrata è quella dell'anteprima più grande. Un file che non
  si decodifica resta segnato finché non cambia; oltre 512 MB non si decodifica, e non conta
  tra le immagini da fare (REVIEW L27: contate, facevano ripartire l'analisi ogni 2 minuti su
  una cartella di vetrini virtuali). I gruppi
  (`media::bktree::groups`) uniscono le immagini entro una distanza di Hamming con un BK-tree:
  ogni immagine cerca le vicine visitando solo i rami con distanza compatibile, senza
  confrontare tutte le coppie (F11-AC2). Nell'app le impronte si calcolano nello stesso job di
  background degli hash, dopo di essi (`hash_jobs.rs`); `similar_images` restituisce i 200
  gruppi con più immagini (la più grande per prima, con la distanza dalla prima) e
  `image_thumbnail` una miniatura JPEG di 192 px dei file collegati, come byte grezzi
  (`tauri::ipc::Response`), con una cache in memoria di 600 miniature.
- **Video simili** (`prism-core::media::video`, `prism-win::video`, T8, F12, ADR-0013): Media
  Foundation decodifica il primo flusso video (NV12, o RGB32 se il decoder non lo offre); al 10,
  30, 50, 70 e 90 % della durata si salta e si decodifica fino al fotogramma del tempo esatto (al
  massimo 300 fotogrammi oltre il fotogramma chiave), si tiene la luminanza e se ne calcola il
  *difference hash*. L'impronta (durata, dimensioni, 5 hash) si salva in `video_prints` con la
  stessa regola di validità delle altre. Il nucleo non chiama Windows: riceve il decodificatore
  come `VideoDecoder` (`MfVideoDecoder` nell'app, uno finto nei test). Due video sono simili se
  le durate differiscono al massimo di 2 s o del 3 % e la distanza media per fotogramma resta
  entro la soglia; i gruppi si formano scorrendo i video ordinati per durata. Nell'app le
  impronte dei video si calcolano nello stesso job, dopo quelle delle immagini; la miniatura è
  il fotogramma a metà, a colori. Le passate di immagini e video condividono il ciclo
  (`run_pass`: file invariato prima e dopo la lettura, salvataggio a gruppi, pausa).
- **Verifica, confronto, copia** (`prism-core::backup`, T7): lavorano sul catalogo, quindi
  anche con i dischi scollegati. `store::subtree_files` legge i file sotto una cartella con il
  percorso relativo e l'hash completo se valido (una CTE ricorsiva sulle cartelle).
  `verify` (F13) indicizza le destinazioni per (dimensione, hash) e per (nome senza maiuscole,
  dimensione) e per ogni file della sorgente cerca una copia: per contenuto se ha un hash e una
  destinazione ha lo stesso; altrimenti per nome, dimensione e data (±2 s, risoluzione FAT) se
  almeno uno dei due lati non ha hash; con hash su entrambi i lati e contenuti diversi non è
  una copia. Il report conta i file verificati per contenuto e per metadati (F13-AC3).
  `compare` (F14) abbina per percorso relativo oppure per contenuto: prima i percorsi uguali
  con lo stesso contenuto, poi lo stesso hash altrove (file spostati o rinominati), poi nome,
  dimensione e data senza hash, poi i percorsi uguali rimasti come "diversi". `folder_view`
  restituisce un livello dell'albero dei risultati con i totali per stato di ogni sottocartella
  (F13-AC4, F14-AC3). Sorgente e destinazione che si contengono sono rifiutate.
- **Backup nell'app** (`prism-app/src/backup_state.rs`, T7): `backup_verify` e
  `backup_compare` leggono il catalogo in un thread bloccante e tengono in memoria gli ultimi 4
  risultati; l'interfaccia li sfoglia un livello alla volta con `backup_folder` (cartelle con i
  totali, al massimo 500 file per livello, ricerca nel percorso, dimensione minima, stati del
  confronto). `copy_preview` controlla che sorgente e destinazione siano collegate, che la
  destinazione non stia dentro la sorgente, la guardia di debug e lo spazio libero;
  `copy_start` accoda un job `copy` (priorità di indicizzazione, dischi di sorgente e
  destinazione) che scrive nel registro delle azioni e poi aggiorna il catalogo della cartella
  di destinazione; `copy_result` restituisce l'esito, con gli errori file per file.
- **Copia verificata** (`backup::copy`, F15): ricrea la struttura sotto la cartella scelta,
  scrive in `<nome>.prism-part` calcolando l'hash di ciò che legge, rilegge la copia e la
  rinomina solo se coincide; mantiene la data di modifica (così il controllo per metadati
  riconosce la copia). Un file esistente non si sovrascrive mai: se ha lo stesso contenuto conta
  come già copiato (ripresa dopo un annullamento), altrimenti si salta o si copia come
  `nome (2).ext`. L'annullamento ferma tra due blocchi e toglie il file parziale; ogni scrittura
  passa dalla guardia sandbox quando c'è (debug e test).

### 4.6 Analisi dello spazio (F08, T4)

`prism-core::space` legge solo il catalogo, quindi funziona anche con i volumi offline:

- **Contenuto di una cartella** ordinato per dimensione (`folder_items`): figli con la
  dimensione del sottoalbero dagli aggregati `dir_stats` (un file con più hard link conta una
  volta); dimensione logica, non allocata.
- **Treemap** (`treemap`): albero potato in cui si espandono **per prime le cartelle più
  grandi, a qualunque profondità** (coda a priorità per dimensione): gli elementi sotto lo
  0,002 % della cartella finiscono in `other`, al massimo 12.000 nodi; la profondità (64) è
  solo un limite di sicurezza. Con un numero fisso di livelli le cartelle profonde (giochi,
  dischi virtuali) restavano blocchi vuoti. Le cartelle non espanse si disegnano come blocchi
  pieni.
- **Interfaccia** (`ui/src/lib/treemap.ts`, `components/Treemap.svelte`): disposizione
  *squarified* (Bruls, Huizing, van Wijk) in TypeScript, cartelle annidate con una striscia del
  titolo, disegno su canvas; colori dei file per categoria (palette categoriale validata in
  entrambi i temi, ordine fisso, `--cat-*` in `app.css`), etichette sui blocchi abbastanza
  grandi, tooltip, legenda. Il canvas è nascosto agli screen reader: lo stesso contenuto e le
  stesse azioni sono nell'elenco accanto (NF7). I comandi (`space_folder`, `largest_files`,
  `type_stats`) sono asincroni e leggono il catalogo fuori dal thread principale (NF1);
  `open_entry`/`reveal_entry` aprono file e cartelle senza privilegi (ADR-0003).
- **File più grandi** di un sottoalbero (`largest_files`): per la radice una scansione per
  volume; per una sottocartella una CTE ricorsiva con ordine di join fissato (vedi il
  benchmark: senza, SQLite sceglieva un piano quadratico).
- **Statistiche per tipo** (`type_stats`): una passata sui file del volume o del catalogo,
  raggruppati per estensione in memoria, poi per categoria (`category_of`) e per fascia di
  dimensione; qui ogni nome conta (anche gli hard link).
- I comandi passano identificativi del catalogo; `folder_of_volume` verifica che la cartella
  appartenga al volume indicato (ADR-0003).

Misure su C: (1,37 milioni di voci): treemap 0,48 s (11.592 nodi), file più grandi
0,14–0,34 s, statistiche 0,23 s (`docs/benchmarks/2026-11-07-t4-space.md`).

### 4.7 Eliminazione sicura (ADR-0012, F16, T4)

`prism-core::fileops::delete_entries` è l'unico percorso di eliminazione:

1. riceve identificativi del catalogo di **un** volume (mai la radice) e ne ricava i percorsi
   dal percorso di accesso attuale; una voce dentro un'altra cartella selezionata viene tolta
   (va via con la cartella);
2. con una `SandboxGuard` (test e comandi di sviluppo) controlla **tutti** i percorsi prima di
   toccare qualcosa;
3. confronta ogni elemento sul disco con il catalogo (`scan::disk_meta`, gli stessi metadati
   del walker): file con dimensione e data di modifica uguali, cartelle ancora cartelle (e
   vuote, se la pulizia propone cartelle vuote); altrimenti l'elemento è saltato e segnalato;
4. passa i percorsi verificati a un `Remover`: nell'app `prism-win::trash::ShellRemover`
   (Cestino con `IFileOperation`; se un elemento non può andare nel Cestino Windows chiede
   all'utente invece di eliminarlo in silenzio) oppure eliminazione definitiva, solo dove il
   Cestino non esiste e con conferma esplicita;
5. controlla il disco dopo l'operazione (fa fede quello che è rimasto), toglie dal catalogo le
   voci eliminate e ricalcola gli aggregati delle cartelle madri, scrive ogni esito in
   `actions_log`.

Nell'app (`AppState::delete_entries`, comando `delete_entries`) il Cestino si usa sui volumi
fissi locali; altrove l'eliminazione è definitiva solo con la conferma esplicita
dell'interfaccia. Le build di **debug** passano sempre una `SandboxGuard`
(`AppState::deletion_guard`): solo il volume PRISMTEST (riconosciuto dall'etichetta), `%TEMP%`
e `%LOCALAPPDATA%\PrismDev`; la release non ha guardia. La stessa guardia vale per ogni
operazione distruttiva dell'app, compresa la copia verso una destinazione (§4.5).

La regola dell'ultima copia dei duplicati (F16-AC4) e il controllo dell'hash sono in §4.5.

### 4.8 Pulizia guidata (F09, T4)

`prism-core::cleanup` calcola le categorie sul catalogo (tenuto aggiornato da journal e
sorveglianza cartelle); l'eliminazione passa da `fileops`, che ricontrolla ogni elemento sul
disco.

- **Temporanei**: file sotto `%TEMP%` e `%SystemRoot%\Temp` modificati da più di un giorno (i
  più recenti possono essere in uso; quelli in uso li rifiuta comunque la shell).
- **Cartelle vuote**: le più in alto senza alcun file nel sottoalbero (rimuoverle toglie anche
  le sottocartelle vuote); al momento di eliminare si verifica che sul disco non ci siano file
  né collegamenti (`FolderCheck::NoFiles`).
- **Grandi e vecchi**: file di almeno 500 MB non modificati da un anno (valori di default).
- **Cache dei browser**: solo cartelle di cache note (Chromium: `Cache`, `Code Cache`,
  `GPUCache` dei profili e le cache degli shader; Firefox: `cache2` dei profili), mai profili,
  cookie, password o cronologia; proposte solo se il browser non è in esecuzione
  (`prism-win::process::running_process_names`, F09-AC4).
- **Mai proposti** (F09-AC2): `DEFAULT_PROTECTED` (Windows, programmi, `ProgramData`,
  ripristino, `Users\*\AppData` tranne temporanei e cache, `inetpub` creato da Windows come
  mitigazione di sicurezza…), le cartelle dei profili utente in sé, elementi nascosti o di
  sistema e tutto ciò che contengono, collegamenti e giunzioni, nomi che iniziano con `.` o `$`,
  e tutto ciò che sta sotto una cartella di programmi installata altrove (una cartella che
  contiene direttamente una `.dll`: giochi, Steam, app portatili).
- **Schermata** *Pulizia* (`ui/src/components/CleanupView.svelte`, comandi `cleanup_scan` e
  `delete_entries`): gruppi per categoria e disco con spiegazione e totale, elementi
  deselezionabili (preselezionati tranne i file grandi e vecchi), conferma prima di agire,
  risultato con spostati, saltati e non riusciti; spazio occupato dal Cestino per informazione.

Su C: (1,37 milioni di voci) la scansione completa richiede circa 2 s (cartelle vuote 1,1 s,
temporanei 0,75 s); `prism-cli cleanup <volume>` mostra la proposta senza eliminare nulla.

## 5. Accesso ai dati (ADR-0008)

- **Un solo thread scrittore** possiede la connessione in scrittura: riceve operazioni da un
  canale, le raggruppa in transazioni (per numero o per tempo) e, dopo ogni commit, pubblica le
  notifiche di modifica.
- **Un pool di connessioni in sola lettura** serve i comandi dell'interfaccia: con WAL le
  letture non aspettano mai lo scrittore (NF1).
- Nessun lock globale sul database: è l'errore architetturale principale del vecchio Prism.

Implementazione (`store::Store`, T1):

- `Store::write(closure)`: transazione sincrona sullo scrittore, risultato dopo il commit; usa
  una **corsia prioritaria**, così le scritture interattive non restano in coda dietro i lotti.
- `WriteSession::submit(lotto)`: lotti di `WriteOp` asincroni, ognuno nella propria
  transazione, su una coda limitata (un produttore veloce viene rallentato invece di riempire
  la memoria). Dopo il primo lotto fallito i successivi della stessa sessione vengono saltati;
  `flush()` attende e restituisce l'errore.
- `Store::read(closure)`: connessione in sola lettura dal pool, sul thread chiamante.
- `Store::subscribe()`: eventi `CatalogChanged`, `VolumeChanged`, `VolumeRemoved` pubblicati
  solo dopo il commit.
- Un job dello scrittore che va in panico annulla la propria transazione senza fermare lo
  scrittore.
- All'apertura: migrazioni, volumi `indexing` → `reindex_needed`, e `PRAGMA quick_check` se la
  sessione precedente non si è chiusa in modo pulito (indicatore in `settings`). Alla chiusura
  (`Store::close`, che l'app chiama all'uscita e prima di installare un aggiornamento perché
  Tauri termina il processo senza rilasciare lo stato; oppure al rilascio dello `Store`):
  indicatore azzerato e checkpoint del WAL.
- `check_consistency` verifica gli invarianti (genitori, radici, aggregati ricalcolati da zero,
  raggiungibilità, nomi temporanei): usata dai test e dalla diagnostica.

## 6. Motore dei job (ADR-0009)

- Tipi (`JobSpec::kind`): `index` (prima indicizzazione), `update` (aggiornamento chiesto o al
  ritorno del disco), `journal` (journal NTFS ogni 15 s), `refresh` (cartelle sorvegliate),
  `hash` (analisi del contenuto e poi impronte di immagini e video), `copy` (copia dei file
  mancanti). Verifica e confronto dei backup, pulizia ed eliminazione girano subito nel catalogo
  senza job (REVIEW D36); salute e NAS sono thread a parte (§9).
- Priorità: *interattiva* (chiesta dall'utente) > *indicizzazione* > *background*.
- Al massimo un job pesante per **disco fisico** alla volta, per non far lavorare un HDD in
  parallelo su più fronti.
- Ogni job ha un token di annullamento e un reporter di avanzamento. Il motore è in memoria
  (nessuna tabella `jobs`, REVIEW D10): un lavoro interrotto si riconosce dallo stato del
  catalogo e riparte da lì (vedi sotto, NF4).
- Eventi verso l'interfaccia, limitati a circa 4 al secondo: `jobs-changed`, `volumes-changed`,
  `settings-changed`, `update-changed`, `search-focus`, `shell-request`.

Implementazione (`prism-core::jobs::JobEngine`, T3): alcuni thread di lavoro prendono il job in
coda più urgente i cui dischi fisici sono liberi (FIFO a parità di priorità); ogni job riceve
un `JobContext` (token di annullamento, avanzamento con fase); un job in coda annullato non
parte, uno in esecuzione riceve la richiesta di stop; un job che va in panico risulta fallito.
`subscribe` notifica i cambi di stato; l'avanzamento si legge con `jobs()` a frequenza limitata.
Il motore è in memoria (REVIEW D10). `should_yield(id)` dice a un job se un job più urgente
aspetta uno dei suoi dischi: i job di background (hash) lo chiedono ogni mezzo secondo e
cedono il disco invece di far aspettare un'indicizzazione per ore (T6, REVIEW D31); quando
tutti i thread sono occupati e un job più urgente potrebbe partire, cede il posto il job meno
urgente partito per ultimo (REVIEW L25: con molti dischi quattro analisi tenevano tutti i
thread e gli aggiornamenti dal journal aspettavano ore). I job di background però non prendono
mai l'ultimo thread (`background_fits`, REVIEW L27): un aggiornamento trova sempre un thread
libero, e la regola precedente scatta solo mentre quel thread è occupato da un altro lavoro
urgente, e solo dopo che il job in attesa ha aspettato 30 s (`WORKER_GRACE`, REVIEW L28: i
lavori urgenti che tengono il thread di solito durano secondi, e fermare l'analisi a ogni
attesa la faceva ripartire ogni 3 minuti). Un job fallito finisce anche nel log. I job di
background non contano come "disco occupato" per l'app: aggiornamenti dal journal, dalla
sorveglianza e richieste dell'utente partono comunque (`AppState::foreground_jobs`); il job
`journal` non riserva il disco (legge pochi record ogni 15 s), altrimenti l'analisi gli
cederebbe il passo a ogni giro, e lo stesso vale per un `refresh` di poche cartelle (su una
cartella di rete affollata arriva ogni pochi secondi): solo la rilettura completa riserva il
disco o il server. Gli hash non hanno bisogno di un record persistente per
riprendere (§4.3).

### 6.1 Lavori interrotti e quando si rifanno

**Interruzione** (annullamento, Prism chiuso, PC spento, disco scollegato):

- **Indicizzazione e aggiornamento.** La lettura del disco non scrive nulla: interrotta lì, il
  catalogo resta com'era. La scrittura avviene a lotti, ognuno in una transazione: interrotta
  a metà, il volume diventa `reindex_needed` (un arresto improvviso lo lascia `indexing`, che
  l'apertura successiva trasforma in `reindex_needed`). Il catalogo resta consultabile e
  coerente lotto per lotto; alla prossima occasione (sotto) il volume si rilegge per intero.
- **Analisi del contenuto e impronte.** Il lavoro fatto si salva a gruppi (ogni 256 file per
  gli hash, a ogni blocco di file per le impronte): un'interruzione perde al più l'ultimo gruppo. Il giro successivo riparte dai file senza hash valido (un hash
  vale finché dimensione e data del file sono quelle per cui è stato calcolato); i file già
  analizzati non si rileggono mai, finché non cambiano.
- **Copia del backup.** File temporanei `.prism-part`, rinominati solo dopo la verifica: una
  copia interrotta non lascia file a metà, e rifatta riconosce i file già copiati dal contenuto.

**Quando si rilegge cosa** (`monitor.rs`, `hash_jobs.rs`, `watchers.rs`):

| Cosa | Quando | Quanto lavoro |
|---|---|---|
| Rilevamento dei dischi | ogni 2 s | nessuna lettura del contenuto |
| Aggiornamento di un disco indicizzato | all'avvio di Prism e ogni volta che il disco torna (collegato, NAS acceso), o con *Aggiorna* | NTFS (Prism elevato): dal journal, secondi; se il journal non copre più la posizione salvata, il volume era `reindex_needed` o le modifiche non si applicano una per una, rilettura completa della MFT (secondi anche per milioni di voci). Altri file system e NAS: rilettura completa dell'albero delle cartelle (solo nomi e metadati, non il contenuto: ~45 s per 600.000 voci sul NAS dell'utente) |
| Journal dei dischi NTFS collegati | ogni 15 s | solo i record nuovi |
| Cartelle sorvegliate (FAT, exFAT, ReFS, NAS) | dopo ogni gruppo di cambi (fino a 5 s) | solo l'elenco delle cartelle cambiate, e per intero le sottocartelle nuove (REVIEW L27); ogni ora una riga del log riassume aggiornamenti, percorsi e tempo speso (L28); quando Windows perde notifiche o i percorsi in attesa sono più di 10.000, tutto il volume, ma non prima di 30 minuti dalla rilettura completa precedente né di 10 volte quanto è durata (al massimo 6 ore; REVIEW L25) |
| Analisi del contenuto | un giro ogni 30 s controlla; un disco riparte se ha file da fare, nessun altro lavoro sopra e almeno 2 minuti dall'inizio dell'ultimo giro se si era interrotto, 10 se era completo (REVIEW L28; 30 minuti se restano solo file in uso, illeggibili, cambiati rispetto al catalogo o modificati negli ultimi 10 minuti, e stampe di file cambiati; REVIEW L26, L27) | solo i file senza hash valido, mai quelli solo nel cloud (OneDrive) o offline, né quelli modificati da meno di 10 minuti; le analisi lasciano sempre libero un thread del motore, e cedono il disco entro mezzo secondo a un lavoro più urgente, il thread quando sono tutti occupati e quel lavoro aspetta da 30 s |
| Avanzamento dell'analisi sulle schede | ogni 5 minuti (1 mentre gira), subito dopo un'indicizzazione | conteggio nel catalogo |
| Indici di ricerca | dopo un cambio del catalogo, al massimo ogni 30 s per volume | ricostruzione in memoria del volume (L11) |
| Salute dei dischi del PC | 10 s dopo l'avvio, poi ogni ora e appena si collega un disco | lettura SMART |
| NAS ZimaOS | 10 s dopo l'avvio, poi ogni ora (ogni 5 minuti se l'ultima lettura è fallita) | due chiamate al NAS |
| Spazio libero dei NAS | ogni 30 s (10 s se non risponde) | una chiamata |
| Disco NTFS collegato rimasto `reindex_needed` | dopo 30 minuti, senza aspettare il prossimo avvio | rilettura della MFT |
| Disco NTFS i cui aggiornamenti dal journal falliscono di fila | dopo 10 minuti di fallimenti, qualunque ne sia la causa (REVIEW L26) | rilettura della MFT, che fa ripartire il journal da quel momento |
| Aggiornamenti di Prism | un minuto dopo l'avvio, poi ogni 24 ore (dopo un'ora se il controllo è fallito) | il `latest.json` della release |

Prism non riparte mai da zero da solo con l'analisi del contenuto: si rifà per intero solo
dopo *Rimuovi dal catalogo* o il reset del catalogo (che tolgono gli hash). La rilettura
completa dell'albero a ogni avvio per i dischi non NTFS e i NAS è il prezzo di non avere un
journal: senza, Prism non sa che cosa è cambiato mentre era chiuso.

## 7. Interfaccia e IPC (ADR-0014)

- Svelte 5 + Vite + TypeScript, senza SvelteKit (finestra principale e finestra di ricerca).
- Tipi TypeScript generati dai tipi Rust con **ts-rs** (REVIEW D11): niente interfacce
  duplicate a mano.
- Lo stato vive nel backend; i componenti si iscrivono agli eventi e rileggono ciò che serve.
- Gli errori del backend arrivano come **codice + parametri**; l'interfaccia li traduce (i18n).
  Anche le etichette del tray sono fornite dall'interfaccia nella lingua scelta.
- Liste lunghe limitate invece che virtualizzate (200 risultati di ricerca, 300 gruppi di
  duplicati, 500 file per livello, con i totali di tutto; REVIEW D28, D32, D36); treemap
  disegnata su canvas.

Implementazione (T3):

- `crates/prism-app` (binario `prism`): `main.rs` apre il catalogo (`catalog.db` nella cartella
  dati: in debug `%LOCALAPPDATA%\PrismDev\app`, in release la cartella dati locale
  dell'identificativo `com.prism.catalog`), avvia il motore dei job, il tray, il plugin di
  istanza singola e i thread di sorveglianza; la chiusura della finestra la nasconde (F19-AC1).
  `dto.rs` contiene i tipi esposti all'interfaccia (`VolumeView`, `JobView`, `Settings`,
  `AppInfo`, `ApiError` con codice); `commands.rs` sono comandi Tauri sottili che delegano a
  `state.rs` (stato condiviso, invio dei job di indicizzazione) e a `prism-core`/`prism-win`.
- `monitor.rs`: ogni 2 s rileva i volumi e li confronta con il catalogo (nuovi inseriti con
  stato `new`, riconosciuti aggiornati, ambigui in attesa della scelta dell'utente, REVIEW D43); alla
  riconnessione di un volume indicizzato accoda un aggiornamento (F05-AC2); ogni 15 s applica
  il journal ai volumi NTFS indicizzati e collegati (F05-AC1, job `journal`); inoltra
  all'interfaccia gli eventi `jobs-changed` (al massimo ~4 al secondo) e `volumes-changed`.
- Identità ambigue (F02-AC3, `identity.rs`): `monitor::apply_detection` (separata dal
  rilevamento, così si prova senza finestra) tiene i volumi `Ambiguous` in attesa con i
  candidati; la scelta dell'utente (un candidato, o un disco nuovo) si applica al giro
  successivo come `Known` o `New`, e il disco scelto prende il GUID di quello collegato.
- `netprobe.rs`: le unità di rete contano come collegate solo quando il server risponde; capacità
  e spazio libero si chiedono in thread separati con un tempo massimo di 5 s (un server spento
  può bloccare la chiamata per decine di secondi), ogni 30 s se raggiungibili e ogni 10 s se no.
- Cartelle di rete a un nuovo indirizzo (REVIEW D59): un NAS raggiunto per IP può avere un
  altro indirizzo dopo un riavvio. `netnames.rs` impara il nome del server quando risponde
  (`prism_win::net::host_name`, ricerca inversa di Windows: DNS, mDNS, LLMNR, NetBIOS) e lo
  salva in `share_servers`; quando una cartella del catalogo non risponde più,
  `monitor::moved_shares` cerca il nome (`prism_win::net::ipv4_addresses`) e prova la stessa
  cartella ai nuovi indirizzi. Una cartella con il nome di una del catalogo trovata a un altro
  server (così, o mappata con una lettera) non corrisponde per percorso: `match_volumes` la
  rende ambigua e l'utente conferma ("Cartella di rete a un nuovo indirizzo?"); scelta la
  cartella del catalogo, questa prende il nuovo percorso. In più `shares.rs` permette di dare
  il nuovo percorso a mano dalla scheda del disco, dopo aver controllato che risponda e quante
  delle voci principali del catalogo ci sono.
- `watchers.rs`: per i volumi indicizzati senza un journal utilizzabile (FAT, exFAT, ReFS, unità
  di rete) un `FolderWatcher` raccoglie i percorsi cambiati e un job `refresh` rilegge le
  cartelle interessate (`prism-core::refresh`); i cambi nella cartella dati di Prism sono
  ignorati. I job automatici `journal` e `refresh` non compaiono sulle schede.
- `notices.rs`: i dischi nuovi trovati mentre la finestra è nascosta diventano una notifica di
  Windows (`prism-win::notify`), una sola per più dischi, con i testi inviati dall'interfaccia
  (`set_native_texts`, insieme alle voci del tray); a finestra visibile basta il riquadro.
- `tray.rs`: Apri, Cerca, una riga non cliccabile con il numero di job in corso (aggiornata dal
  relè dei job solo quando cambia), Sospendi/Riprendi l'analisi del contenuto (salva
  l'impostazione e avvisa l'interfaccia con `settings-changed`), Esci (F19-AC2).
- Le impostazioni (F21) sono un documento JSON nella tabella `settings` (chiave
  `app.settings`). L'attività pianificata `Prism` (`prism-win::autostart`, ADR-0003) resta
  sempre registrata nelle build di rilascio, perché il menu di Esplora risorse la usa per
  avviare Prism senza UAC; l'opzione di avvio con Windows ne accende o spegne solo il trigger
  di accesso (REVIEW D45). Con `--autostart` la finestra parte nascosta.
- Menu di Esplora risorse (F18, `shell_requests.rs`): l'app serve la named pipe
  `\\.\pipe\prism-<utente>` (`prism-dev-` in sviluppo) con un elenco di accesso esplicito
  (SYSTEM, amministratori, l'utente) e l'etichetta di integrità media, così `prism-shell` non
  elevato può scriverci; una richiesta `azione<TAB>percorso` porta la finestra in primo piano
  ed emette `shell-request`; l'interfaccia chiede `locate_path` (la cartella più profonda del
  catalogo che contiene il percorso, su un disco collegato) e apre lo spazio di quella
  cartella o i duplicati filtrati su di essa. Le voci del menu (`prism-win::shell_menu`, sotto
  `HKCU\Software\Classes`: cartelle, unità, sfondo di una cartella) le registra l'installer
  (T10) e le toglie il disinstallatore (F18-AC3).
- La finestra principale è creata in `main.rs` dalla sua configurazione (`create: false`), con
  la cartella dati di WebView2 accanto al catalogo.
- `ui/`: Svelte 5 + Vite + TypeScript. `src/lib/api.ts` è l'unico punto di contatto con il
  backend (`invoke`/`listen`); fuori da Tauri usa un backend simulato (`src/lib/mock.ts`) con
  dati d'esempio, per lo sviluppo nel browser e i test. `i18n.svelte.ts` (EN/IT, lingua di
  sistema), `theme.ts` (tema di Windows o scelto), `catalog.ts` e `format.ts` (logica di
  presentazione testata con Vitest), componenti in `src/components/`. I tipi generati stanno in
  `src/lib/bindings/` (rigenerati da `cargo test -p prism-app`, `TS_RS_EXPORT_DIR` in
  `.cargo/config.toml`; la CI verifica che siano aggiornati).

Interfaccia "B+C" (T11, ADR-0019):

- **Navigazione** (`src/lib/nav.svelte.ts`): un solo stato `route` — `catalog`, `disk`
  (volume, scheda, cartella), `all` (funzioni su tutto il catalogo), `search`, `settings` — e
  `visit`, che cresce a ogni navigazione per riaprire una schermata con nuovi parametri. Nessun
  menu laterale: `TopBar.svelte` (logo, percorso, casella di ricerca, pulsante dei dischi nuovi
  con il suo riquadro, impostazioni), poi la schermata, poi la barra di stato in `App.svelte`.
- **Catalogo** (`CatalogView.svelte`, `DiskCard.svelte`): filtri, ordine e colori in
  `catalog.ts` (`catalogCards`, `filterCounts`, `stickerColor`, `knownLocations`,
  `diskQuery`); la vista Schede/Elenco resta nel `localStorage` (solo una comodità). Schede e
  righe si raggruppano per disco fisico o per server (`diskGroups`, `groupTitle`, REVIEW D64)
  secondo `VolumeView.group`; le cartelle di rete si chiamano col nome della cartella, con
  l'icona di rete al posto della lettera e "NAS · server" sopra il nome (`whereLabel`, D63). Le barre
  per categoria vengono da `categories.svelte.ts`: chiede `type_stats` di un disco alla volta in
  sottofondo e tiene il risultato almeno 10 minuti (un milione di file costa circa un secondo).
  Le *Ultime azioni* vengono dal comando `recent_actions` (registro delle azioni, F16-AC6),
  raggruppate in operazioni da `actions.ts`.
- **Disco** (`DiskView.svelte`): intestazione e schede; ogni scheda riusa la schermata della
  funzione con il parametro `scope` (l'id del disco) — Duplicati con il filtro sul disco, Simili
  con il disco scelto, Backup con il disco come sorgente, Pulizia con le sole proposte del
  disco — più `HealthView` e `TypesView`.
- **Stile**: variabili CSS in `app.css` (chiaro e scuro, `--accent` è l'inchiostro delle
  selezioni, `--signal` l'arancione), classi comuni (`.pills`, `.stats`, `.actionbar`,
  `.sticker`, `table.data`); caratteri inclusi da `src/fonts.ts` (`@fontsource`, OFL). La mappa
  (`Treemap.svelte`, canvas) mescola il colore della categoria con la superficie e scrive in
  inchiostro.
- **Icona**: `scripts/make-icons.py` disegna il prisma a strati da una geometria su 64 unità
  (la stessa di `Logo.svelte`) e scrive le icone di `crates/prism-app/icons/`.
- **Test end-to-end**: i percorsi comuni (aprire un disco, una scheda, il blocco del catalogo,
  le impostazioni) stanno in `ui/e2e/nav.ts`.

## 8. Sicurezza (ADR-0003)

Prism gira elevato, quindi la superficie d'attacco della webview va ridotta al minimo:

- CSP restrittiva, nessun contenuto remoto, nessun plugin `fs` o `shell` generico in Tauri.
- I comandi che agiscono su file ricevono **identificativi del catalogo**, non percorsi liberi;
  i percorsi scritti dall'utente (la cartella di destinazione della copia, REVIEW D36) vengono
  validati nel backend (disco collegato, non dentro la sorgente, sandbox in sviluppo). La pipe
  del menu di Esplora risorse accetta solo `analyze`/`duplicates` con un percorso, che serve
  soltanto a scegliere cosa mostrare.
- File e cartelle si aprono **senza** privilegi di amministratore (tramite la shell del
  desktop), per non lanciare programmi dell'utente come amministratore.
- Il trascinamento da Esplora risorse verso Prism è bloccato da Windows (UIPI): è una
  conseguenza accettata dell'esecuzione elevata.
- NAS ZimaOS (REVIEW D66): Prism si collega solo ai server delle cartelle di rete del catalogo,
  sulla porta 80 del pannello web (HTTP in chiaro sulla rete di casa, come il pannello stesso),
  con il login dato dall'utente; dopo il login solo richieste `GET`. Il login si conserva con
  `CredWriteW` nel Gestore credenziali di Windows (cifrato per l'utente, visibile e cancellabile
  in *Credenziali di Windows* come `Prism/ZimaOS/<server>`; le build di sviluppo usano
  `PrismDev/ZimaOS/`), mai nel catalogo né nei log; l'interfaccia riceve solo il nome utente.

## 8.1 Installazione (T10, F22)

`scripts\build-installer.ps1` compila `prism-shell` e poi, con la CLI di Tauri, l'app e il suo
installer NSIS (`target\release\bundle\nsis\Prism_<versione>_x64-setup.exe`):

- installazione per tutta la macchina (l'installer è elevato e può registrare l'attività
  pianificata con i privilegi più alti), lingue italiano e inglese, WebView2 installato con il
  bootstrapper di Microsoft se manca (F22-AC1);
- `crates\prism-app\windows\installer-hooks.nsh`: dopo l'installazione copia
  `prism-shell.exe` accanto a `prism.exe` e lo esegue con `--install` (voci del menu di Esplora
  risorse per l'utente, nella lingua di Windows, e attività `Prism` con avvio all'accesso);
  prima della disinstallazione `--uninstall` le toglie (F18-AC3, F22-AC2);
- la disinstallazione di Tauri chiede se cancellare i dati dell'applicazione: se la casella
  resta vuota il catalogo resta (F22-AC2);
- le scorciatoie del menu Start e del desktop hanno l'AppUserModelID `com.prism.catalog`, lo
  stesso delle notifiche della versione di rilascio: un clic su una notifica apre Prism
  (REVIEW L9, da verificare su un PC pulito).

La firma del codice (Q7) si aggiunge con `bundle.windows.signCommand` (o
`certificateThumbprint`) in `tauri.conf.json`, senza modifiche al codice.

### Aggiornamenti (F22-AC3)

`crates/prism-app/src/updates.rs` usa `tauri-plugin-updater`: un controllo un minuto dopo
l'avvio e poi ogni 24 ore; se c'è una versione più recente la scarica in sottofondo e la tiene
in memoria. *Impostazioni › Aggiornamenti* mostra la versione pronta e le sue note e la
installa solo dopo la conferma dell'utente: l'installer NSIS parte in modalità `passive` e
Prism riparte. Prima di installare, il plugin verifica la firma del pacchetto (minisign) con
la chiave pubblica; un pacchetto con una firma sbagliata non viene installato.

Lo schema è lo stesso di Orbit (D57). La chiave pubblica e l'indirizzo del manifest sono in
`plugins.updater` di `tauri.conf.json`:
`https://github.com/caste9612/Prism/releases/latest/download/latest.json`, cioè il
`latest.json` dell'ultima release di GitHub (una release senza `latest.json`, in bozza o
pre-release, è invisibile alle copie installate). Controllano solo le build di release: quella
di sviluppo non si collega mai e le impostazioni lo dicono. È l'unica connessione verso
Internet di Prism (NF6); in rete locale parla solo con i NAS di cui l'utente ha dato il login
(§8, REVIEW D66).

**La chiave privata** (minisign, protetta da password) sta in `~\.tauri\prism-updater.key`,
fuori dal repository, accanto a quelle di Orbit e Zenith; la crea una volta
`scripts\new-update-key.ps1` (chiede la password, o con `-GeneratePassword` ne genera una e la
salva in `prism-updater.key.password.txt`), che mette chiave e password anche nei secret
`TAURI_SIGNING_PRIVATE_KEY` e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` dell'environment `release`
del repository GitHub. I secret non si possono rileggere: la copia di sicurezza (file e password
insieme, fuori dal PC) la tiene l'utente. Persa la chiave o la password, le copie installate non
accettano più aggiornamenti: serve una chiave nuova e una reinstallazione a mano. Per cambiarla
in modo pianificato si pubblica prima una versione con la nuova chiave pubblica, firmata con la
vecchia.

**Rilascio.** `scripts\build-installer.ps1` controlla che la versione sia la stessa in
`tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, `ui\package.json` e `ui\package-lock.json`,
chiede la password (o la legge da `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) e la prova firmando un
file di prova prima di compilare; poi firma l'installer (`.sig`) e scrive `latest.json` accanto,
con il primo paragrafo di `docs\releases\<versione>.md` come note mostrate in *Impostazioni*.
Con `-NoUpdateSignature` costruisce un installer solo da provare. Di solito però si pubblica da
GitHub Actions (`.github\workflows\release.yml`), a mano e mai da una pull request:

```powershell
gh workflow run release.yml --ref develop                  # prova a secco: i file restano come artefatto
gh workflow run release.yml --ref develop -f publish=true  # release v<versione> su GitHub
```

L'environment `release` vale solo per `develop` e aspetta l'approvazione dell'utente prima di
dare i secret al job di compilazione (Windows). Con *publish* un secondo job controlla che
`latest.json` sia della versione giusta e con la firma dell'installer, che la release non esista
già, e crea la release `v<versione>` sul commit compilato, con le note di
`docs\releases\<versione>.md` e i tre file: installer, `.sig`, `latest.json`.

## 9. Errori, log, diagnostica

- `thiserror` nei crate di libreria, errori con codice stabile per l'interfaccia.
- `tracing` con log su file a rotazione giornaliera e conservazione limitata (F20, T9):
  `tracing-appender` scrive `prism.AAAA-MM-GG.log` in `logs` nella cartella dati (in sviluppo
  `%LOCALAPPDATA%\PrismDev\app\logs`) e ne tiene 14; la console resta per lo sviluppo.
- Salute dei dischi (F17, T9): `prism-win::health` legge identità e bus del disco fisico
  (`IOCTL_STORAGE_QUERY_PROPERTY`), per NVMe la pagina di log della salute
  (`StorageDeviceProtocolSpecificProperty`), per ATA/SATA attributi, soglie e stato SMART
  (`SMART_RCV_DRIVE_DATA`, `SMART_SEND_DRIVE_COMMAND`); USB e dischi virtuali risultano "non
  disponibile" con il motivo. `prism-core::health` interpreta i dati (stato buono, attenzione,
  critico con i motivi; peggioramento tra due letture). Nell'app (`health_state.rs`) un thread
  legge ogni ora i dischi dei volumi locali collegati, salva una lettura in `disk_health` quando
  cambia qualcosa (non la temperatura) o una volta al giorno, tiene 400 giorni e mostra una
  notifica quando un disco peggiora o è critico la prima volta che lo si vede.
- Tipo dei dischi (F17-AC5, REVIEW D65): NVMe è sempre SSD; per gli altri
  `StorageDeviceSeekPenaltyProperty` dice se il disco gira (si legge senza diritti particolari,
  spesso non risponde dietro un adattatore USB) e, dove i comandi SMART ATA passano, la parola 217
  di IDENTIFY DEVICE dà la velocità di rotazione. Il risultato si salva in `disk_media`; una
  velocità letta una volta resta anche quando una lettura successiva non può chiederla.
- NAS ZimaOS (F17-AC6, REVIEW D66): `zimaos.rs` fa il login (`POST /v1/users/login`) e legge
  `GET /v2/local_storage/raid` e `/v2/local_storage/disk`, le stesse chiamate del pannello web
  (non documentate: ogni campo è facoltativo e una risposta diversa è `unsupported`, mai un
  problema dei dischi), con un client HTTP/1.1 minimo su `TcpStream` (niente dipendenze nuove:
  `reqwest` dell'aggiornamento non ha un fornitore crittografico configurato). Una cartella
  corrisponde al volume del NAS con lo stesso nome; la condivisione di sistema `ZimaOS-HD` alla
  partizione `/DATA`. Un disco senza temperatura né ore è "SMART non disponibile"; un membro di
  un RAID assente dall'elenco dei dischi è "mancante". `nas_state.rs` rilegge ogni NAS con un
  login ogni ora (ogni 5 minuti se l'ultima lettura è fallita, tenendo l'ultima buona) in un
  thread `prism-nas`; i problemi (RAID non ok, disco non in salute, guasto o mancante) sono
  chiavi stabili, e quelle nuove rispetto alla lettura precedente danno una notifica.
- Pacchetto diagnostico (`prism-core::diagnostics`, `prism-app/src/diagnostics_state.rs`):
  `system.txt` (versione, Windows, diritti, contatori del catalogo, impostazioni, dischi con le
  loro voci e l'avanzamento dell'analisi, job in corso col loro disco, avanzamento e durata,
  REVIEW L30) e i log, in uno ZIP senza compressione scritto da Prism stesso (nessuna libreria in
  più). L'interfaccia mostra prima l'elenco dei file e il testo di `system.txt`; il file va
  nella cartella Download (in sviluppo nella cartella dati) senza mai sostituirne uno esistente.
- Reset del catalogo (F21-AC4): `store::reset_catalog` toglie ogni volume come la rimozione di
  un singolo disco (gli indici di ricerca seguono gli eventi); l'app prima ferma i job, poi
  svuota lo stato in memoria; impostazioni e registro delle azioni restano.
- Nessun pannello di debug nell'app: job, fasi e tempi si vedono nel riquadro *Attività in
  corso* e nei log (il monitor egui del vecchio progetto non è stato rifatto).

## 10. Test

Strategia completa in `docs/TESTING.md`. In breve: test unitari e d'integrazione su cartelle
temporanee per tutto ciò che è in `prism-core`; test di `prism-win` che richiedono
amministratore eseguiti su un disco virtuale di test dedicato; test end-to-end
dell'interfaccia con backend simulato; benchmark registrati in `docs/benchmarks/`.

## 11. Struttura delle cartelle

```
Prism-next/
├─ Cargo.toml  Cargo.lock  rust-toolchain.toml  clippy.toml   workspace, lint comuni
├─ README.md  LICENSE  CLAUDE.md                               presentazione, MIT, regole di lavoro
├─ crates/
│  ├─ prism-core/   model, names, exclusion, volumes (riconoscimento), scan (walker),
│  │                reconcile, changes, refresh, indexer, store (SQLite e migrazioni), jobs,
│  │                search, space, cleanup, fileops, sandbox, hashing, duplicates, media
│  │                (immagini e video), backup, health, diagnostics, cancel; tests/ di
│  │                integrazione e di proprietà
│  ├─ prism-win/    volumes, mft, usn, ntfs, watch, health (SMART, tipo del disco), trash,
│  │                shell, shell_menu, pipe, autostart, notify, process, system, net, files,
│  │                video (Media Foundation), credentials (Gestore credenziali), ioctl;
│  │                tests/ con amministratore (feature `admin-tests`)
│  ├─ prism-cli/    strumento di sviluppo: scan, ls, du, stats, check, bench, replicate,
│  │                mem-bench, search-bench, print-bench
│  ├─ prism-app/    app Tauri 2: commands, state (+ state_tests), dto, monitor, watchers,
│  │                netprobe, netnames, shares, identity, hash_jobs, health_state, nas_state,
│  │                zimaos, backup_state, similar_state, *_views, notices, tray, updates,
│  │                search_window, shell_requests, diagnostics_state
│  └─ prism-shell/  eseguibile non elevato del menu di Esplora risorse (F18)
├─ ui/              interfaccia Svelte 5: src/components, src/lib (logica, simulatore,
│                   bindings generati, locales), e2e/ (Playwright con il backend simulato),
│                   e2e-real/ (app vera), screens/ (screenshot del README)
├─ scripts/         check.ps1, build-installer.ps1, new-update-key.ps1, sandbox-test.ps1,
│                   make-icons.py, admin/ (runner dei test con amministratore)
├─ docs/
│  ├─ SPEC.md  ARCHITECTURE.md  TESTING.md  ROADMAP.md  PROGRESS.md  REVIEW.md
│  ├─ adr/          decisioni architetturali
│  ├─ benchmarks/   risultati delle misure
│  ├─ releases/     note di ogni versione
│  └─ screenshots/  immagini del README (`npm run screens`)
└─ .github/workflows/   ci.yml (controlli), release.yml (rilascio)
```
