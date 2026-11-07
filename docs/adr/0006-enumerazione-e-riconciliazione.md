# ADR-0006 — Enumerazione (MFT o walker) e riconciliazione unica

- **Stato**: Accettata (prestazioni MFT da misurare con il prototipo T2)
- **Data**: 2026-11-07

## Contesto

Nel vecchio Prism la scansione completa e quella incrementale erano due implementazioni
diverse, con esclusioni diverse (una per nome esatto, l'altra per **sottostringa** del
percorso): l'incrementale toglieva dal catalogo cartelle come `...\Templates\` e aggiungeva
`node_modules`. Inoltre l'enumerazione chiamava `fs::metadata` per ogni file, raddoppiando
l'I/O (pesante sulle unità di rete).

## Decisione

1. Un trait `Enumerator` produce un flusso di `RawEntry` per un volume. Implementazioni:
   - `MftEnumerator` (NTFS locale, amministratore): legge la `$MFT` dall'handle del volume;
     dimensioni e date dagli attributi `$STANDARD_INFORMATION`, `$FILE_NAME` e `$DATA`.
   - `WalkEnumerator` (tutti gli altri casi e fallback): enumerazione parallela delle cartelle
     che usa i metadati restituiti dall'enumerazione stessa, senza chiamate per file.
2. Una sola `ExclusionPolicy`, applicata a entrambi: confronto per nome esatto di cartella o per
   percorso esatto, mai per sottostringa (F04-AC5).
3. Un solo `Reconciler` trasforma il flusso in operazioni sul catalogo (inserisci, aggiorna,
   sposta, elimina). Indicizzazione completa e aggiornamento passano entrambi da qui.

## Conseguenze

- Il test di equivalenza (F05-AC3) diventa possibile: stesso stato del disco → stesso catalogo,
  qualunque sia il percorso seguito.
- Il lettore MFT è il pezzo più complesso del progetto: si parte da un prototipo misurato, e il
  `WalkEnumerator` resta sempre disponibile come ripiego.
