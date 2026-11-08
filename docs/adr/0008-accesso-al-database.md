# ADR-0008 — Accesso al database: uno scrittore, molti lettori

- **Stato**: Accettata
- **Data**: 2026-11-07

## Contesto

Nel vecchio Prism un `tokio::sync::Mutex<Database>` con una sola connessione serializzava
tutto: durante una scansione o una ricerca di duplicati ogni comando dell'interfaccia restava in
attesa per minuti.

## Decisione

- SQLite in modalità WAL, `synchronous = NORMAL`.
- **Un thread scrittore** possiede l'unica connessione in scrittura. Riceve operazioni da un
  canale, le raggruppa in transazioni (per numero di operazioni o per intervallo di tempo) e,
  dopo il commit, pubblica notifiche di modifica.
- **Un pool di connessioni in sola lettura** per i comandi e i job che leggono.
- Schema versionato: migrazioni SQL incorporate nel binario, `PRAGMA user_version`, test che
  applica tutte le migrazioni a un database vuoto e a uno della versione precedente.
- Checkpoint del WAL periodico e alla chiusura; controllo di integrità dopo una chiusura non
  pulita.

## Conseguenze

- I comandi dell'interfaccia non aspettano mai i job (NF1).
- I lettori vedono lo stato all'ultimo commit: le transazioni dello scrittore devono lasciare
  sempre il catalogo coerente (mai un volume a metà senza un flag di stato).
