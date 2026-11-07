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

**Schema in uso (versione 2)** — testo completo in `0001_initial.sql` e
`0002_volume_bus.sql` (aggiunge `bus_type`):

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
INDEX        entries_volume_ref  (volume_id, file_ref)

dir_stats (entry_id PRIMARY KEY REFERENCES entries ON DELETE CASCADE,
           total_size, total_alloc, file_count, dir_count) STRICT   -- solo file contati

settings (key PRIMARY KEY, value) STRICT
```

Le chiavi esterne sono attive sulla connessione di scrittura; `entries` non usa cascate (una
sottostruttura si elimina con una CTE ricorsiva, senza limiti di profondità).

**Tabelle previste nelle tappe successive** (bozza, arriveranno con nuove migrazioni):

```sql
content_hashes (entry_id PRIMARY KEY, size, modified_at, quick BLOB, full BLOB, computed_at)
-- valido solo se size/modified_at coincidono con la voce (ADR-0011)          -- T6
media_prints (entry_id, kind, size, modified_at, print BLOB)                   -- T8
health_samples (volume_id, taken_at, status, temperature, power_on_hours, wear, realloc, pending, raw)  -- T9
jobs (id, kind, volume_id, state, priority, created_at, started_at, finished_at, progress, error, params) -- T3
actions_log (id, at, action, entry_path, volume_id, size, outcome, detail)    -- F16-AC6, T6
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

- **MFT** (NTFS locale, richiede amministratore): lettura diretta della `$MFT` tramite handle
  al volume; elenco completo in un'unica passata sequenziale.
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

### 4.3 Hash (ADR-0011)

Job di background a priorità bassa (`THREAD_MODE_BACKGROUND_BEGIN`, quindi I/O e CPU in
background), attivo anche durante l'uso del PC; sospensione a batteria opzionale. Ordine: prima i file con
dimensioni condivise da altri file (possibili duplicati), poi gli altri. Due livelli:
`quick` (inizio e fine del file) e `full` (BLAKE3 completo). Un hash vale solo per la coppia
(`size`, `modified_at`) con cui è stato calcolato.

### 4.4 Ricerca (ADR-0010, *proposta*)

Indice dei nomi di **tutti** i volumi, online e offline, con ricerca per sottostringa e filtri.
Due candidati da misurare con benchmark (T1–T5): indice in memoria (stile Everything) oppure
FTS5 con tokenizer `trigram`. Il parser dei filtri (F07-AC5) è indipendente dall'indice scelto.

### 4.5 Duplicati, simili, backup

- **Duplicati**: raggruppamento per (`size`, `full`) sulla tabella degli hash validi; le regole
  di selezione (F10-AC3) sono funzioni pure, testate in isolamento.
- **Simili**: hash percettivi a 64 bit indicizzati con un BK-tree (o *multi-index hashing*) per
  le ricerche di distanza di Hamming, senza confronto di tutte le coppie.
- **Verifica, confronto, copia**: lavorano sul catalogo (funzionano anche offline); solo la
  copia richiede i volumi online e passa da `fileops`.

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

## 7. Interfaccia e IPC (ADR-0014)

- Svelte 5 + Vite + TypeScript, senza SvelteKit (finestra principale e finestra di ricerca).
- Tipi TypeScript generati dai tipi Rust (tauri-specta o ts-rs, da scegliere in T3): niente
  interfacce duplicate a mano.
- Lo stato vive nel backend; i componenti si iscrivono agli eventi e rileggono ciò che serve.
- Gli errori del backend arrivano come **codice + parametri**; l'interfaccia li traduce (i18n).
  Anche le etichette del tray sono fornite dall'interfaccia nella lingua scelta.
- Liste lunghe virtualizzate; treemap disegnata su canvas.

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
│  ├─ prism-app/           (T3)
│  └─ prism-shell/         (T9)
├─ ui/                     (T3)
├─ scripts/                check.ps1, script dei test con amministratore
├─ docs/
│  ├─ SPEC.md  ARCHITECTURE.md  TESTING.md  ROADMAP.md  PROGRESS.md
│  ├─ adr/                 decisioni architetturali
│  └─ benchmarks/          risultati delle misure
├─ .github/workflows/      CI
└─ CLAUDE.md               regole di lavoro per le sessioni di sviluppo
```
