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
migrazioni SQL incorporate e `PRAGMA user_version`. Schema iniziale (bozza, si consolida in T1):

```sql
volumes (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL,          -- fixed | removable | network
  fs_type       TEXT,                   -- NTFS, exFAT, FAT32, ReFS, ...
  fs_serial     INTEGER,                -- seriale del file system (64 bit su NTFS)
  volume_guid   TEXT,                   -- \\?\Volume{...}\ (per macchina)
  unc_path      TEXT,                   -- \\server\share per le unità di rete
  os_label      TEXT,                   -- etichetta di Windows
  capacity      INTEGER, free_space INTEGER,
  -- etichette utente (F03)
  user_name TEXT, location TEXT, notes TEXT, color TEXT,
  -- stato
  status        TEXT NOT NULL,          -- new | indexed | ignored | reindex_needed
  last_mount    TEXT,                   -- ultima lettera o percorso visto
  first_seen_at INTEGER, last_seen_at INTEGER, last_indexed_at INTEGER,
  -- journal USN (solo NTFS)
  usn_journal_id INTEGER, usn_next INTEGER,
  settings      TEXT                    -- JSON: esclusioni, politica hash
)

entries (
  id         INTEGER PRIMARY KEY,
  volume_id  INTEGER NOT NULL REFERENCES volumes(id),
  parent_id  INTEGER REFERENCES entries(id),  -- NULL solo per la radice del volume
  name       TEXT NOT NULL,
  is_dir     INTEGER NOT NULL,
  size       INTEGER NOT NULL,                -- dimensione logica
  alloc_size INTEGER,                         -- spazio occupato su disco
  created_at INTEGER, modified_at INTEGER,
  attrs      INTEGER,
  file_ref   INTEGER                          -- riferimento MFT (NTFS), NULL altrove
)
-- unicità: (volume_id, parent_id, name) case-insensitive; indice su (volume_id, file_ref)

dir_stats (entry_id PRIMARY KEY, total_size, total_alloc, file_count, dir_count)

content_hashes (entry_id PRIMARY KEY, size, modified_at, quick BLOB, full BLOB, computed_at)
-- valido solo se size/modified_at coincidono con la voce (ADR-0011)

media_prints (entry_id, kind, size, modified_at, print BLOB)   -- immagini e video simili

health_samples (volume_id, taken_at, status, temperature, power_on_hours, wear, realloc, pending, raw)

jobs (id, kind, volume_id, state, priority, created_at, started_at, finished_at, progress, error, params)

actions_log (id, at, action, entry_path, volume_id, size, outcome, detail)   -- F16-AC6

settings (key PRIMARY KEY, value)
```

Il percorso completo di una voce **non è memorizzato**: si ricostruisce risalendo i
`parent_id` (o dall'indice in memoria). Questo elimina la ripetizione del percorso in colonne e
indici, che nel vecchio Prism portava il database a circa 1 KB per file.

Punti ancora da definire in T1/T2: rappresentazione degli hard link NTFS (più nomi per lo
stesso `file_ref`; lo spazio va contato una sola volta), confronto case-insensitive dei nomi
oltre l'ASCII, gestione dei flussi alternativi (ADS).

## 4. Pipeline

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
- **Walker classico** (FAT/exFAT/ReFS, unità di rete, fallback): enumerazione parallela delle
  cartelle usando i metadati restituiti dall'enumerazione stessa, **senza** una chiamata
  `metadata()` per file (su SMB ogni chiamata in più è un viaggio di rete).

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
│  ├─ prism-core/
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
