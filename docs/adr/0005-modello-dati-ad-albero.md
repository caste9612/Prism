# ADR-0005 — Modello dati ad albero

- **Stato**: Accettata (hard link, nomi Unicode e flussi alternativi da definire in T1/T2)
- **Data**: 2026-11-07

## Contesto

Il vecchio schema aveva una riga per file con il percorso completo come chiave `UNIQUE`; le
cartelle venivano ricostruite spezzando le stringhe dopo ogni scansione (tabella
`folder_sizes` ricalcolata da zero). Effetti: database enorme, rinomina di una cartella =
aggiornamento di tutte le righe sottostanti, incoerenze tra radice `C:` e `C:\`.

## Decisione

- Tabella `entries` con `parent_id`: ogni voce memorizza solo il proprio nome. Il percorso
  completo si ricostruisce risalendo l'albero.
- Le cartelle sono voci come le altre (`is_dir = 1`) e hanno aggregati in `dir_stats`
  (dimensione totale, spazio occupato, numero di file e cartelle), aggiornati **solo lungo i
  rami toccati** da una modifica.
- Su NTFS si memorizza il riferimento MFT (`file_ref`), che permette di riconoscere spostamenti
  e rinomine (F05-AC4) e di applicare il journal USN.
- Unicità: (`volume_id`, `parent_id`, `name`) senza distinzione tra maiuscole e minuscole.

Obiettivo di dimensione: ≤ 250 byte per voce, indice di ricerca incluso (NF2), da validare con
il prototipo.

## Conseguenze

- Una rinomina o uno spostamento è un aggiornamento di una riga.
- La ricostruzione dei percorsi ha un costo: serve una cache (o l'indice in memoria) per i
  casi frequenti (risultati di ricerca, report).
- Da decidere: rappresentazione degli hard link (più nomi per un `file_ref`, spazio contato
  una volta), confronto case-insensitive oltre l'ASCII (regole di NTFS `$UpCase`), flussi
  alternativi (probabilmente solo come dimensione aggiuntiva).
