# ADR-0011 — Hash del contenuto in background e invalidazione

- **Stato**: Accettata
- **Data**: 2026-11-07

## Contesto

Nel vecchio Prism, quando un file veniva modificato, l'aggiornamento azzerava l'hash parziale
ma non quello completo: un file modificato con stessa dimensione e stessi primi 64 KB poteva
risultare "duplicato" del suo vecchio gemello, con rischio di eliminare l'unica copia buona.
Nel catalogo reale, inoltre, nessun hash era memorizzato.

## Decisione

- Tabella `content_hashes` separata, con la coppia (`size`, `modified_at`) per cui l'hash è
  stato calcolato. **Un hash è valido solo se la coppia coincide con la voce attuale**; il
  controllo è nella query, non nel codice chiamante.
- Due livelli: `quick` (BLAKE3 della dimensione + primi e ultimi 64 KB) per scartare in fretta
  i non duplicati; `full` (BLAKE3 di tutto il contenuto) per confermare.
- Calcolo in background (ADR-0009): prima i file con dimensione condivisa da altri file, poi
  tutti gli altri.
- Prima di qualunque eliminazione basata sui duplicati, l'hash completo viene **ricalcolato**
  sul file reale (F16-AC3).

### Aggiornamento 2026-11-11 (REVIEW L31)

Su un file server aziendale l'analisi di 2,5 milioni di file ha richiesto 21 ore e mezza, un
file alla volta: il tempo se ne andava ad aspettare il server per ogni file più che a leggerlo.
Ogni file si apre una volta sola (dimensione e data dal file aperto); i file fino a 1 MiB si
leggono interi nella prima passata, che ne tiene anche l'hash completo; le immagini lette
intere si stampano dagli stessi byte; sulle unità di rete si leggono 4 file alla volta. Gli
hash restano gli stessi (stesso BLAKE3 sugli stessi byte); un file letto intero ha anche l'hash
rapido, che non costa altro.

### Aggiornamento 2026-11-13 (REVIEW L33)

La dimensione e la data si leggevano dal file aperto solo dopo il contenuto: tre file sempre in
uso, 38 GB in tutto, venivano letti fino in fondo ogni 30 minuti per scoprire alla fine che la
data non era quella del catalogo. Un file letto a pezzi per l'hash completo (oltre 1 MiB) viene
ora interrogato prima, e non si legge se dimensione o data non sono quelle del catalogo; resta
il controllo dopo la lettura, per i file che cambiano mentre si leggono. I file fino a 1 MiB
non si interrogano prima: su una cartella di rete la domanda costa quanto il contenuto.

## Conseguenze

- Duplicati, verifica backup e confronto per contenuto diventano istantanei una volta
  calcolati gli hash, anche con i dischi offline.
- Il primo calcolo completo su dischi grandi richiede ore: va reso visibile, interrompibile e
  poco invasivo.
