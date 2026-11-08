# ADR-0012 — Eliminazione sicura

- **Stato**: Accettata
- **Data**: 2026-11-07

## Contesto

Nel vecchio Prism la cancellazione era definitiva (la documentazione parlava invece di
Cestino), e la cancellazione in blocco eliminava dal disco anche i file che non superavano la
verifica sul database.

## Decisione

Un solo percorso di eliminazione (`fileops::delete_entries`), usato da duplicati, simili e
pulizia:

1. L'interfaccia passa **identificativi del catalogo**; il backend risolve i percorsi.
2. Per ogni file: il volume deve essere online; dimensione e data sul disco devono coincidere
   con il catalogo; per le eliminazioni da duplicati, l'hash completo ricalcolato deve
   coincidere con quello del gruppo. Altrimenti il file viene saltato e segnalato.
3. Il backend rifiuta l'eliminazione di **tutte** le copie di un gruppo; se le uniche altre
   copie sono su volumi offline serve una conferma esplicita in più.
4. Destinazione: **Cestino** (`IFileOperation` con annullamento consentito). Sulle unità di
   rete il Cestino non esiste: eliminazione definitiva solo con conferma esplicita.
5. Ogni esito viene scritto in `actions_log`.
6. Nei test e nei comandi di sviluppo, la guardia `SandboxGuard` rifiuta qualsiasi percorso
   fuori dalle radici consentite (TESTING.md, regola S2).

## Conseguenze

- Più controlli prima di eliminare, quindi eliminazioni più lente, ma l'utente non perde mai
  l'unica copia di un file.
- Lo spazio non si libera subito: il Cestino va svuotato (la pulizia guidata lo segnala).
