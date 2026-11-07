# ADR-0010 — Indice di ricerca

- **Stato**: **Proposta** (decisione dopo i benchmark, entro T5)
- **Data**: 2026-11-07

## Contesto

Il vecchio Prism usava FTS5 con tokenizer `unicode61` su nome **e** percorso: trovava solo
l'inizio delle parole ("port" non trovava "report"), alcuni caratteri speciali rompevano la
query e restituiva zero risultati senza segnalare l'errore. Il nuovo requisito è la ricerca per
sottostringa su tutti i volumi, anche offline, entro 50 ms su 5 milioni di voci (F07-AC2/AC3).

## Opzioni

| | A. Indice in memoria (stile Everything) | B. FTS5 con tokenizer `trigram` |
|---|---|---|
| Sottostringa | Sì, scansione parallela dei nomi | Sì, da 3 caratteri in su |
| Latenza attesa | Molto bassa | Bassa, da misurare con molti risultati |
| Memoria | Nomi e genitori in RAM (stima 60–100 byte per voce) | Bassa |
| Spazio su disco | Nessun indice aggiuntivo | Indice di circa 3–4 volte il testo dei nomi |
| Avvio | Caricamento dal database all'avvio | Immediato |
| Filtri per percorso | Naturali (si risale l'albero in memoria) | Richiedono join o ricostruzione |

## Criteri di decisione

Benchmark su un catalogo reale (C:, D: e dischi di rete dell'utente) e uno sintetico da
5 milioni di voci: latenza p50/p95, memoria, dimensione su disco, tempo di avvio.

## Decisione

Da prendere. Il parser dei filtri e l'API di ricerca si progettano indipendenti dall'indice,
così la scelta non blocca il resto del lavoro.
