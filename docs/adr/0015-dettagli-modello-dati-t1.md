# ADR-0015 — Dettagli del modello dati decisi in T1

- **Stato**: Accettata
- **Data**: 2026-11-07
- **Sostituisce in parte**: ADR-0005 (regola di unicità dei nomi)

## Contesto

L'ADR-0005 lasciava aperti alcuni punti del modello ad albero e prevedeva l'unicità dei nomi
"senza distinzione tra maiuscole e minuscole". Implementando archivio, walker e riconciliazione
(T1) sono emersi vincoli concreti:

- NTFS ammette cartelle **sensibili alle maiuscole** (attivabili per cartella, usate da WSL) e
  nomi nello spazio POSIX: due nomi che differiscono solo per le maiuscole possono coesistere.
  Un vincolo di unicità case-insensitive renderebbe impossibile indicizzarle.
- I nomi di Windows sono sequenze UTF-16 e possono contenere **surrogati spaiati**, non
  rappresentabili in una `String` Rust né in un `TEXT` SQLite: una conversione con perdita
  farebbe collidere nomi diversi e renderebbe irraggiungibili i file.
- NTFS aggiorna **in ritardo** la copia delle date di una cartella conservata nell'indice della
  cartella madre (quella restituita da `FindFirstFile`): due scansioni dello stesso albero
  immutato davano date diverse per le cartelle (misurato nei test: scarti di ~1 ms).
- Win32 elimina punti e spazi finali dai percorsi: le cartelle con nomi come `trap.` (valide su
  NTFS, create da WSL o macOS) risultavano illeggibili.
- Le cancellazioni a cascata di SQLite hanno un limite di profondità; gli alberi di cartelle no.

## Decisione

1. **Unicità per nome esatto**: indice `UNIQUE (parent_id, name)` con confronto binario. Il
   confronto senza maiuscole si usa solo dove ha senso per l'utente: percorsi digitati
   (ricerca esatta poi senza maiuscole) e regole di esclusione. La piegatura delle maiuscole è
   uno-a-uno come in NTFS (`ß` resta `ß`).
2. **Nomi memorizzati con escape reversibile** (`names.rs`): un surrogato spaiato diventa
   `U+FFFF` + 4 cifre esadecimali; un `U+FFFF` letterale diventa `U+FFFF FFFF`. Ogni altro nome è
   invariato. La codifica è iniettiva, quindi nessuna collisione, e il percorso ricostruito
   riporta allo stesso file reale (verificato con un test su disco).
3. **Date in `FILETIME`** (intervalli di 100 ns dal 1601) a piena precisione, per tutte le voci:
   walker e lettore MFT producono gli stessi valori e la validità degli hash (ADR-0011) non è
   indebolita da arrotondamenti. Le date dell'app (prima vista, indicizzazione) sono in
   millisecondi Unix.
4. **Metadati delle cartelle letti dalla cartella stessa** (una chiamata `metadata()` per
   cartella, non per file); i file usano i dati dell'enumerazione (nessuna chiamata per file).
5. **Percorsi verbatim** (`\\?\`) nel walker, così i nomi con punto o spazio finale funzionano.
6. **Giunzioni e collegamenti simbolici** di cartella sono voci del catalogo ma non vengono
   seguiti (niente doppi conteggi né cicli); gli altri reparse point (segnaposto cloud) sono
   voci normali.
7. **Id delle voci assegnati dall'archivio** (contatore atomico inizializzato a `max(id)+1`):
   il reconciler produce operazioni complete senza attendere lo scrittore. Gli id non si
   riusano.
8. **Integrità**: chiavi esterne attive sulla connessione di scrittura; nessuna cascata su
   `entries` (le sottostrutture si eliminano con una CTE ricorsiva, senza limite di
   profondità); `dir_stats` ha `ON DELETE CASCADE` (un solo livello).
9. **Aggregati** (`dir_stats`): contano solo i file; lo spazio occupato usa la dimensione
   logica quando quella allocata non è nota (walker). Si ricalcolano, dopo ogni
   riconciliazione, per le cartelle toccate e i loro antenati, dal basso verso l'alto.
10. **Stato `indexing`** dei volumi mentre il reconciler scrive; un'interruzione lo trasforma in
    `reindex_needed` (all'errore o alla riapertura dell'archivio).

Restano per T2: rappresentazione degli hard link (lo spazio va contato una volta; il walker non
li distingue e nei collegamenti non usati per l'ultima modifica NTFS può riportare dimensioni
vecchie) e flussi alternativi (ADS).

## Conseguenze

- Una rinomina che cambia solo le maiuscole, senza riferimento al file (walker), diventa
  eliminazione + inserimento: il catalogo resta corretto ma gli hash del file vanno ricalcolati.
  Con il riferimento MFT (T2) è una rinomina.
- Su volumi letti con il walker, una cartella spostata o rinominata è vista come nuova (il walker
  non ha riferimenti ai file): stesso effetto sugli hash. Un walker con id dei file
  (`FileIdExtdDirectoryInfo`, in `prism-win`) potrà eliminarlo dove il file system li offre.
- La scansione costa una chiamata in più per cartella (circa il 10% delle voci): accettabile, e
  sulle unità di rete resta lontana dal costo di una chiamata per file.
