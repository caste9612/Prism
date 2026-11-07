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
│  │  ├─ hashing   (BLAKE3, cache invalidabile)           ├─ dupes    (gruppi, regole)    │ │
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
| `prism-app` | Applicazione Tauri 2: comandi IPC, eventi, finestre, tray, scorciatoia, server della named pipe. Nessuna logica di dominio. *(Arriva in T3.)* | `prism-core`, `prism-win` |
| `prism-shell` | Piccolo eseguibile **non elevato** invocato dal menu contestuale: inoltra la richiesta all'istanza attiva via named pipe, oppure avvia l'attività pianificata. *(Arriva in T9.)* | — |
| `ui/` | Frontend Svelte 5 + Vite + TypeScript. *(Arriva in T3.)* | — |

## 3. Modello dati (ADR-0005)

Archivio SQLite in modalità WAL, nella cartella dati dell'app. Lo schema è versionato con
migrazioni SQL incorporate (`crates/prism-core/src/store/migrations/`) e `PRAGMA user_version`;
una migrazione già pubblicata non si modifica, se ne aggiunge una nuova.

**Schema in uso (versione 5)** — testo completo in `0001_initial.sql`,
`0002_volume_bus.sql` (aggiunge `bus_type`), `0003_entries_volume_index.sql` (indice sul solo
volume al posto di `(volume_id, file_ref)`, ADR-0017) e `0004_actions_log.sql` (registro delle
azioni, T4) e `0005_content_hashes.sql` (hash del contenuto, T6):

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
  settings                          -- JSON: esclusioni per volume, politica hash
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
```

Le chiavi esterne sono attive sulla connessione di scrittura; `entries` non usa cascate (una
sottostruttura si elimina con una CTE ricorsiva, senza limiti di profondità).

**Tabelle previste nelle tappe successive** (bozza, arriveranno con nuove migrazioni):

```sql
media_prints (entry_id, kind, size, modified_at, print BLOB)                   -- T8
health_samples (volume_id, taken_at, status, temperature, power_on_hours, wear, realloc, pending, raw)  -- T9
jobs (id, kind, volume_id, state, priority, created_at, started_at, finished_at, progress, error, params) -- T6 (REVIEW D10)
```

Il percorso completo di una voce **non è memorizzato**: si ricostruisce risalendo i
`parent_id` (o dall'indice in memoria). Questo elimina la ripetizione del percorso in colonne e
indici, che nel vecchio Prism portava il database a circa 1 KB per file.

Decisioni di dettaglio (unicità per nome esatto, escape dei nomi, date `FILETIME`, metadati
delle cartelle, aggregati) in ADR-0015. Restano da definire in T2: hard link NTFS (più nomi per
lo stesso `file_ref`; lo spazio va contato una sola volta) e flussi alternativi (ADS).

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

**Reconciler** (`reconcile::reconcile`), visita in ampiezza; per ogni cartella abbina i figli
scansionati a quelli del catalogo: (1) per riferimento al file nella stessa cartella,
(2) per nome esatto se i riferimenti non si contraddicono, (3) per riferimento altrove nel
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
  (codice `entry_meta` condiviso).
- `prism-core::changes::apply_changes`: dai file cambiati (`FileChange`: stato attuale e
  genitori nominati dai record) agli elenchi superficiali delle cartelle toccate (figli
  invariati con i metadati del catalogo, sottocartelle `NotListed`), applicati con
  `reconcile_many` (più bersagli in una sola esecuzione, ordinati per profondità reale,
  eliminazioni rimandate alla fine).
- `prism-win::ntfs`: `index_ntfs` (posizione del journal letta prima della lettura della MFT e
  salvata dopo) e `update_ntfs` (journal → `apply_changes`; ripiego su `index_ntfs` se la
  posizione manca o è persa, o se le modifiche non si possono applicare: es. cartella con
  contenuto ripristinata dal Cestino).

Sorveglianza cartelle (volumi senza journal, ADR-0007):

- `prism-win::watch::FolderWatcher`: `ReadDirectoryChangesW` tramite la libreria `notify`;
  `next_batch` raccoglie gli eventi in lotti (fino a una pausa di silenzio) come percorsi
  relativi alla radice; un traboccamento del buffer chiede una rienumerazione completa.
- `prism-core::refresh`: per ogni percorso cambiato, la cartella del catalogo più vicina al suo
  genitore (ignorando le zone escluse); restano solo le più in alto, e ognuna si rienumera come
  sottoalbero con lo stesso reconciler (una cartella che non esiste più cede il posto al suo
  antenato esistente). Verificato da un test di proprietà su disco e da un test end-to-end con
  il watcher.

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

Nell'app (`prism-app/src/hash_jobs.rs`) un thread, 20 s dopo l'avvio e poi ogni 30 s:

- conta per ogni volume indicizzato file e file con hash valido (ogni 5 minuti, ogni minuto
  mentre l'analisi gira: il conteggio costa centinaia di ms su un catalogo grande) e lo mette
  sulle schede (`VolumeView.hashes`, F06-AC5);
- accoda un job `hash` a priorità *background* per ogni volume locale collegato, indicizzato,
  con lavoro da fare, non escluso dall'utente (F21-AC3) e senza altri job, al massimo ogni 2
  minuti; le unità di rete non si leggono in background (REVIEW D30).

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
  nella finestra principale la ricerca è anche una voce del menu laterale.

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
- **Simili**: hash percettivi a 64 bit indicizzati con un BK-tree (o *multi-index hashing*) per
  le ricerche di distanza di Hamming, senza confronto di tutte le coppie.
- **Verifica, confronto, copia**: lavorano sul catalogo (funzionano anche offline); solo la
  copia richiede i volumi online e passa da `fileops`.

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
e `%LOCALAPPDATA%\PrismDev`; la release non ha guardia. La stessa guardia varrà per ogni
futura operazione distruttiva dell'app (spostamento, copia verso, sovrascrittura).

La regola dell'ultima copia dei duplicati (F16-AC4) e il controllo dell'hash arrivano con T6.

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
  sessione precedente non si è chiusa in modo pulito (indicatore in `settings`). Alla chiusura:
  checkpoint del WAL.
- `check_consistency` verifica gli invarianti (genitori, radici, aggregati ricalcolati da zero,
  raggiungibilità, nomi temporanei): usata dai test e dalla diagnostica.

## 6. Motore dei job (ADR-0009)

- Tipi: `IndexVolume`, `UpdateVolume`, `HashContent`, `MediaPrints`, `VerifyBackup`,
  `CompareTrees`, `CopyMissing`, `CleanupScan`, `DeleteEntries`, `PollHealth`.
- Priorità: *interattiva* (chiesta dall'utente) > *indicizzazione* > *background*.
- Al massimo un job pesante per **disco fisico** alla volta, per non far lavorare un HDD in
  parallelo su più fronti.
- Ogni job ha un token di annullamento, un reporter di avanzamento e un record persistente in
  `jobs`. All'avvio i job interrotti vengono ripresi (hash, impronte) o annullati in modo pulito
  (NF4).
- Eventi verso l'interfaccia, limitati a circa 4 al secondo: `job-updated`, `catalog-changed`
  (volume, tipo di modifica), `volume-status`, `notification`.

Implementazione (`prism-core::jobs::JobEngine`, T3): alcuni thread di lavoro prendono il job in
coda più urgente i cui dischi fisici sono liberi (FIFO a parità di priorità); ogni job riceve
un `JobContext` (token di annullamento, avanzamento con fase); un job in coda annullato non
parte, uno in esecuzione riceve la richiesta di stop; un job che va in panico risulta fallito.
`subscribe` notifica i cambi di stato; l'avanzamento si legge con `jobs()` a frequenza limitata.
Il motore è in memoria (REVIEW D10). `should_yield(id)` dice a un job se un job più urgente
aspetta uno dei suoi dischi: i job di background (hash) lo chiedono ogni mezzo secondo e
cedono il disco invece di far aspettare un'indicizzazione per ore (T6, REVIEW D31). I job di
background non contano come "disco occupato" per l'app: aggiornamenti dal journal, dalla
sorveglianza e richieste dell'utente partono comunque (`AppState::foreground_jobs`); il job `journal` non riserva il disco (legge pochi record ogni 15 s), altrimenti l'analisi gli cederebbe il passo a ogni giro. Gli hash
non hanno bisogno di un record persistente per riprendere (§4.3).

## 7. Interfaccia e IPC (ADR-0014)

- Svelte 5 + Vite + TypeScript, senza SvelteKit (finestra principale e finestra di ricerca).
- Tipi TypeScript generati dai tipi Rust con **ts-rs** (REVIEW D11): niente interfacce
  duplicate a mano.
- Lo stato vive nel backend; i componenti si iscrivono agli eventi e rileggono ciò che serve.
- Gli errori del backend arrivano come **codice + parametri**; l'interfaccia li traduce (i18n).
  Anche le etichette del tray sono fornite dall'interfaccia nella lingua scelta.
- Liste lunghe virtualizzate; treemap disegnata su canvas.

Implementazione (T3):

- `crates/prism-app` (binario `prism`): `main.rs` apre il catalogo (`catalog.db` nella cartella
  dati: in debug `%LOCALAPPDATA%\PrismDev\app`, in release la cartella dati locale
  dell'identificativo `com.prism.catalog`), avvia il motore dei job, il tray, il plugin di
  istanza singola e i thread di sorveglianza; la chiusura della finestra la nasconde (F19-AC1).
  `dto.rs` contiene i tipi esposti all'interfaccia (`VolumeView`, `JobView`, `Settings`,
  `AppInfo`, `ApiError` con codice); `commands.rs` sono comandi Tauri sottili che delegano a
  `state.rs` (stato condiviso, invio dei job di indicizzazione) e a `prism-core`/`prism-win`.
- `monitor.rs`: ogni 2 s rileva i volumi e li confronta con il catalogo (nuovi inseriti con
  stato `new`, riconosciuti aggiornati, ambigui solo registrati nel log, REVIEW D13); alla
  riconnessione di un volume indicizzato accoda un aggiornamento (F05-AC2); ogni 15 s applica
  il journal ai volumi NTFS indicizzati e collegati (F05-AC1, job `journal`); inoltra
  all'interfaccia gli eventi `jobs-changed` (al massimo ~4 al secondo) e `volumes-changed`.
- `netprobe.rs`: le unità di rete contano come collegate solo quando il server risponde; capacità
  e spazio libero si chiedono in thread separati con un tempo massimo di 5 s (un server spento
  può bloccare la chiamata per decine di secondi), ogni 30 s se raggiungibili e ogni 10 s se no.
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
  `app.settings`). L'opzione di avvio con Windows registra o rimuove l'attività pianificata
  `Prism` (`prism-win::autostart`, ADR-0003); con `--autostart` la finestra parte nascosta.
- La finestra principale è creata in `main.rs` dalla sua configurazione (`create: false`), con
  la cartella dati di WebView2 accanto al catalogo.
- `ui/`: Svelte 5 + Vite + TypeScript. `src/lib/api.ts` è l'unico punto di contatto con il
  backend (`invoke`/`listen`); fuori da Tauri usa un backend simulato (`src/lib/mock.ts`) con
  dati d'esempio, per lo sviluppo nel browser e i test. `i18n.svelte.ts` (EN/IT, lingua di
  sistema), `theme.ts` (tema di Windows o scelto), `catalog.ts` e `format.ts` (logica di
  presentazione testata con Vitest), componenti in `src/components/`. I tipi generati stanno in
  `src/lib/bindings/` (rigenerati da `cargo test -p prism-app`, `TS_RS_EXPORT_DIR` in
  `.cargo/config.toml`; la CI verifica che siano aggiornati).

## 8. Sicurezza (ADR-0003)

Prism gira elevato, quindi la superficie d'attacco della webview va ridotta al minimo:

- CSP restrittiva, nessun contenuto remoto, nessun plugin `fs` o `shell` generico in Tauri.
- I comandi che agiscono su file ricevono **identificativi del catalogo**, non percorsi liberi;
  i percorsi scelti dall'utente (es. cartella di destinazione per la copia) passano da dialoghi
  nativi e vengono validati nel backend.
- File e cartelle si aprono **senza** privilegi di amministratore (tramite la shell del
  desktop), per non lanciare programmi dell'utente come amministratore.
- Il trascinamento da Esplora risorse verso Prism è bloccato da Windows (UIPI): è una
  conseguenza accettata dell'esecuzione elevata.

## 9. Errori, log, diagnostica

- `thiserror` nei crate di libreria, errori con codice stabile per l'interfaccia.
- `tracing` con log su file a rotazione giornaliera e conservazione limitata (F20).
- In sviluppo, un pannello di debug interno mostra job, code e tempi (sostituisce il monitor
  egui del vecchio progetto).

## 10. Test

Strategia completa in `docs/TESTING.md`. In breve: test unitari e d'integrazione su cartelle
temporanee per tutto ciò che è in `prism-core`; test di `prism-win` che richiedono
amministratore eseguiti su un disco virtuale di test dedicato; test end-to-end
dell'interfaccia con backend simulato; benchmark registrati in `docs/benchmarks/`.

## 11. Struttura delle cartelle

```
Prism-next/
├─ Cargo.toml              workspace, lint comuni
├─ crates/
│  ├─ prism-core/          model, names, exclusion, scan (walker), reconcile, indexer,
│  │                       store (SQLite), sandbox (guardia per i test), cancel
│  ├─ prism-win/
│  ├─ prism-cli/
│  ├─ prism-app/           app Tauri 2: comandi, stato, sorveglianza volumi, tray
│  └─ prism-shell/         (T9)
├─ ui/                     interfaccia Svelte 5 (src/components, src/lib, bindings generati)
├─ scripts/                check.ps1, script dei test con amministratore
├─ docs/
│  ├─ SPEC.md  ARCHITECTURE.md  TESTING.md  ROADMAP.md  PROGRESS.md
│  ├─ adr/                 decisioni architetturali
│  └─ benchmarks/          risultati delle misure
├─ .github/workflows/      CI
└─ CLAUDE.md               regole di lavoro per le sessioni di sviluppo
```
