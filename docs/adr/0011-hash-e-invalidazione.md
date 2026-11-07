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

## Conseguenze

- Duplicati, verifica backup e confronto per contenuto diventano istantanei una volta
  calcolati gli hash, anche con i dischi offline.
- Il primo calcolo completo su dischi grandi richiede ore: va reso visibile, interrompibile e
  poco invasivo.
