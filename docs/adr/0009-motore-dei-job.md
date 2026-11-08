# ADR-0009 — Motore dei job ed eventi verso l'interfaccia

- **Stato**: Accettata
- **Data**: 2026-11-07

## Contesto

Nel vecchio Prism i lavori lunghi partivano da tre punti diversi (comandi di scansione, coda,
monitor dei dischi), con flag di stato duplicati tra backend e frontend e circa 9 eventi che si
sovrapponevano.

## Decisione

- Un unico motore di job in `prism-core`. Ogni job ha: tipo, volume, priorità, stato
  (`queued`, `running`, `paused`, `done`, `failed`, `cancelled`), avanzamento, token di
  annullamento, record persistente.
- Priorità: interattiva > indicizzazione > background.
- Concorrenza limitata per **disco fisico**: al massimo un job pesante sullo stesso disco.
- I job di background (hash, impronte) girano sempre con la priorità minima di I/O e CPU
  (modalità background di Windows); la sospensione a batteria è un'opzione (SPEC F06-AC2).
- Eventi verso l'interfaccia, con frequenza limitata (circa 4 al secondo per job):
  `job-updated`, `catalog-changed`, `volume-status`, `notification`. Nessun altro canale.

## Conseguenze

- L'interfaccia non deve indovinare lo stato: lo legge dal motore e reagisce agli eventi.
- Ripresa dopo crash: i job di background ripartono dal punto salvato, gli altri vengono
  marcati come interrotti (NF4).
