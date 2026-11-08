# ADR-0017 — Lettore MFT: decisione "go" e ricerche per riferimento senza indice SQL

- **Stato**: Accettata
- **Data**: 2026-11-07
- **Completa**: ADR-0006 (prestazioni MFT da misurare con il prototipo T2)

## Contesto

L'ADR-0006 prevede due enumeratori: il lettore della MFT per i volumi NTFS locali (con
privilegi di amministratore) e il walker per tutto il resto, con la decisione finale sul
lettore MFT rimandata a un prototipo misurato (T2). La SPEC chiede ≥ 1 milione di voci in meno
di 30 s (F04-AC1); il journal USN (ADR-0007) e il riconoscimento degli spostamenti (F05-AC4)
richiedono il **riferimento al file** di ogni voce, che il walker non fornisce.

## Prototipo e misure

Implementazione in `prism-win::mft` (dettagli in ARCHITECTURE §4.1), interamente in codice
sicuro: lettura del dispositivo `\\.\X:` con `seek_read`, parser puro delle strutture NTFS
testato su record sintetici, test elevato sul disco di test che confronta MFT e walker sullo
stesso albero (hard link, nomi in record di estensione, flussi alternativi, nomi Unicode e
con punto finale, file residenti e non).

Misure complete in `docs/benchmarks/2026-11-07-t2-mft.md`. In sintesi (release, NVMe):

| | C: (1,26 M voci) | D: (621 mila voci) |
|---|---|---|
| Enumerazione MFT / walker | 1,6 s / 9,5 s | 0,7 s / 1,4 s |
| Indicizzazione completa MFT / walker | 5,7 s / 14,3 s | 2,1 s / 2,8 s |
| Stesso numero di file e cartelle? | ± 7 (sistema in uso) | identico |

## Decisione

1. **Go**: il lettore MFT è l'enumeratore dei volumi NTFS locali quando Prism è elevato (cioè
   sempre nella versione di release, ADR-0003). Il walker resta per FAT/exFAT/ReFS, unità di
   rete, build di debug non elevate e come ripiego se il lettore MFT rifiuta un volume
   (`ScanError::Unsupported`). La scelta automatica dell'enumeratore spetta al motore dei
   job (T3); la CLI la espone con `--mft`.
2. **Nessun indice SQL sui riferimenti ai file.** L'indice `(volume_id, file_ref)` rendeva gli
   inserimenti 5 volte più lenti, perché i riferimenti arrivano dalla MFT in ordine sparso
   (12,96 s contro 2,43 s per un milione di voci). Migrazione 3: indice sul solo `volume_id`.
   Le ricerche per riferimento usano una `FileRefMap` (coppie ordinate riferimento → voce,
   ~16 B per voce, 0,18 s per milione di voci) caricata dal reconciler solo alla prima ricerca
   necessaria; il journal USN userà la stessa mappa, tenuta in memoria per i volumi NTFS
   collegati.
3. **Semantica** delle voci lette dalla MFT (allineata al walker dove entrambi vedono lo stesso
   dato): nomi DOS 8.3 esclusi; file di metadati NTFS nascosti (record < 24); date e attributi
   da `$STANDARD_INFORMATION` (cartelle con l'attributo directory, attributi vuoti = NORMAL);
   `size` = flusso dati principale; `alloc_size` = cluster di tutti i flussi dati, compresi
   gli alternativi (ADS), 0 per i dati residenti; un file con più hard link compare una volta
   per nome, con lo spazio allocato contato solo sul nome con coppia (genitore, nome) minima.
   Questo chiude per l'MFT i punti aperti dell'ADR-0015 su hard link e ADS.
4. Il lettore legge il **disco**, non la cache del file system: le modifiche non ancora scritte
   compaiono con il journal USN. I test elevati forzano la scrittura (`FlushFileBuffers` sul
   volume di test) prima di leggere.

## Conseguenze

- Indicizzazione completa di C: in 5,7 s invece di 14,3 s; riferimenti ai file disponibili per
  spostamenti e journal USN.
- ~10 B per voce in più nel catalogo (riferimento e spazio allocato).
- Limiti noti del prototipo, da coprire con il lettore completo: una `$MFT` così grande da avere
  essa stessa un `$ATTRIBUTE_LIST` nel record 0 viene rifiutata (`Unsupported`, si usa il
  walker); i file compressi/sparsi usano la "dimensione compressa" dell'attributo, non ancora
  verificata su disco reale; parsing a un solo thread (1,25 s per 2,3 milioni di record:
  parallelizzabile se servirà).
- Memoria massima durante l'indicizzazione ~315 B per voce: da ridurre (istantanea compatta).
