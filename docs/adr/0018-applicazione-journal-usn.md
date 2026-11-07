# ADR-0018 — Applicazione del journal USN al catalogo

- **Stato**: Accettata (lettura del journal implementata; applicazione in corso in T2)
- **Data**: 2026-11-07
- **Rifinisce**: ADR-0007

## Contesto

L'ADR-0007 decide di aggiornare i volumi NTFS leggendo il journal USN dall'ultima posizione
salvata. Restava da decidere **come** trasformare i record del journal in modifiche del
catalogo mantenendo la garanzia di F05-AC3 (aggiornamento = reindicizzazione completa) e la
semantica del lettore MFT (ADR-0017). Vincoli emersi implementando la lettura (T2):

- un record USN dice *quale* file è cambiato (riferimento, genitore, nome, motivo), non le sue
  nuove dimensioni o date;
- la lettura grezza del disco (come fa il lettore MFT) può essere indietro rispetto alla cache
  del file system; `FSCTL_GET_NTFS_FILE_RECORD` invece passa dal file system ed è coerente;
- un file con molti hard link ha i nomi in record di estensione elencati da un
  `$ATTRIBUTE_LIST` che può essere **non residente** (verificato: 41 nomi bastano); quella lista
  si legge dai cluster del disco e può quindi essere momentaneamente indietro;
- la cartella che contiene un file modificato cambia anch'essa (data di modifica) senza un
  record proprio nel journal.

## Decisione

**Posizione.** Prima di ogni lettura completa della MFT si legge lo stato del journal
(`FSCTL_QUERY_USN_JOURNAL`); dopo una riconciliazione riuscita si salva la posizione
(`volumes.usn_journal_id`, `usn_next`). I record scritti durante la lettura vengono quindi
riapplicati: l'applicazione è basata sullo stato attuale dei file, quindi ripeterla non cambia il
risultato. Quando il volume diventa `reindex_needed` la posizione si cancella.

**Aggiornamento.**

1. `check_position`: se il journal non esiste più, è stato ricreato (ID diverso) o la posizione
   è stata sovrascritta → **riconciliazione completa** (lettura MFT).
2. Lettura dei record dalla posizione salvata (`FSCTL_READ_USN_JOURNAL`).
3. **File toccati**: i riferimenti dei record. Per ognuno si legge lo **stato attuale** con
   `fetch_file` (record base + estensioni, stessa semantica della lettura completa: stesso
   codice `entry_meta`). Un file che non esiste più è cancellato. Se una lista di attributi non
   residente non è ancora coerente sul disco (`FetchError::Stale`) l'aggiornamento si ferma
   senza avanzare la posizione e riprova più tardi; dopo alcuni tentativi, riconciliazione
   completa.
4. **Cartelle toccate**: i genitori nel catalogo delle voci dei file toccati (vecchie
   posizioni), i genitori attuali di tutti i loro nomi (nuove posizioni, e tutti i nomi dei file
   con hard link, perché il nome che conta lo spazio può cambiare), e i genitori indicati dai
   record.
5. Per ogni cartella toccata si costruisce un **elenco superficiale** dei figli: i figli già nel
   catalogo con i loro metadati del catalogo (se fossero cambiati, sarebbero nel journal), più
   lo stato letto dei file toccati che ora stanno lì; i metadati della cartella stessa si
   rileggono (la sua data di modifica cambia senza record). Le sottocartelle non toccate sono
   marcate **non elencate**: il reconciler non tocca il loro contenuto.
6. Una cartella che entra nel catalogo con un **rinomina** (non una creazione), cioè arriva da
   una zona esclusa o non indicizzata (es. ripristino dal Cestino), porta con sé un contenuto
   senza record: in questo caso si fa una riconciliazione completa (caso raro; ottimizzabile in
   seguito con un'enumerazione del solo sottoalbero).
7. Tutti gli elenchi passano in **una sola esecuzione del reconciler** con più bersagli,
   ordinati per profondità reale: le eliminazioni restano rimandate alla fine, così uno
   spostamento tra due cartelle toccate conserva l'id (F05-AC4), e una cartella viene spostata
   solo quando i suoi nuovi antenati sono già al loro posto (niente cicli).
8. Se la riconciliazione riesce si salva la nuova posizione; altrimenti il volume diventa
   `reindex_needed` (come per l'indicizzazione completa).

**Mai modifiche ai dischi dell'utente**: Prism non crea né ridimensiona journal. I volumi senza
journal attivo si aggiornano con una nuova enumerazione al ricollegamento e, mentre Prism è
attivo, con la sorveglianza delle cartelle (ADR-0007). Solo il disco di test PRISMTEST riceve un
journal, creato dai test.

## Conseguenze

- Gli aggiornamenti costano in proporzione alle modifiche, non alla dimensione del volume: una
  lettura di record più un `FSCTL_GET_NTFS_FILE_RECORD` per file toccato e un elenco dal
  catalogo per cartella toccata.
- Il reconciler acquisisce due capacità: più bersagli in una sola esecuzione e lo stato
  "non elencata" per le cartelle (`Listing::NotListed`), da cui non si cancella nulla.
- La verifica è la stessa di T1: dopo modifiche casuali, catalogo aggiornato dal journal =
  catalogo reindicizzato da zero (file system simulato con riferimenti e disco di test reale).
