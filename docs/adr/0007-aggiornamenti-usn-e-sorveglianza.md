# ADR-0007 — Aggiornamenti: journal USN, sorveglianza cartelle, riconciliazione

- **Stato**: Accettata
- **Data**: 2026-11-07

## Contesto

L'"incrementale" del vecchio Prism ripercorreva tutto il disco e risparmiava solo le scritture
sul database. NTFS mantiene un journal delle modifiche (USN) che permette di sapere cosa è
cambiato senza rileggere il disco.

## Decisione

- **NTFS locale**: per ogni volume si memorizzano `usn_journal_id` e `usn_next`. Con Prism
  attivo il journal si legge di continuo (aggiornamenti in pochi secondi, F05-AC1); al
  ricollegamento si legge dal punto salvato.
- **Journal non più valido** (ID cambiato, oppure `usn_next` precedente al primo USN
  disponibile): riconciliazione completa.
- **Altri file system locali** con Prism attivo: sorveglianza delle cartelle
  (`ReadDirectoryChangesW`) seguita da riconciliazione mirata delle cartelle toccate; se il
  buffer delle notifiche trabocca, riconciliazione completa.
- **Unità di rete e dischi ricollegati senza journal**: riconciliazione completa con il
  walker.

## Conseguenze

- Aggiornamenti quasi istantanei sui dischi NTFS, e catalogo "vivo" mentre Prism è aperto.
- La lettura del journal richiede amministratore, coerente con ADR-0003.
- Va gestito con cura l'ordine degli eventi USN (rinomina in due record, cancellazione e
  ricreazione con lo stesso nome).
