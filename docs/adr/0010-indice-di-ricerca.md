# ADR-0010 — Indice di ricerca

- **Stato**: **Accettata** (2026-11-07, dopo i benchmark)
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

**Opzione A: indice in memoria** (decisa il 2026-11-07 con i benchmark di
`docs/benchmarks/2026-11-07-t5-search.md`, catalogo reale di C: con 1.374.926 voci e una
simulazione da 5,5 milioni).

| | A. In memoria | B. FTS5 trigram |
|---|---|---|
| Costruzione (C:) | 0,57 s, all'avvio e dopo i cambi | 17,5 s, e ogni scrittura del catalogo più lenta |
| Spazio | 60 B per voce in RAM (78 MiB per C:, ~300 MB per 5 milioni) | +127 B per voce su disco (oltre l'obiettivo NF2 di 250 B per voce in tutto) |
| Primi 1000 risultati | ≤ 1 ms su C:, ≤ 3 ms su 5,5 milioni (p95) | 0,1–0,7 ms |
| Tutti i risultati di una query comune | 0,5–2 ms tipico; 15 ms per una sola lettera su C:, 77 ms su 5,5 milioni | 19–33 ms per "windows"/"microsoft" su C: |
| 1–2 caratteri | sì | no (servirebbe un ripiego) |

L'interfaccia chiede i primi risultati (con un conteggio limitato), quindi F07-AC3 (50 ms p95
su 5 milioni di voci) è rispettato con ampio margine. La memoria rientra in NF3. FTS5 avrebbe
rallentato di circa 5 volte l'indicizzazione e superato il budget di spazio su disco.

Struttura (`prism-core::search::index::VolumeIndex`, uno per volume): nomi normalizzati
(`names::fold`) in un unico buffer separati da un byte nullo, cercati con `memchr::memmem`
(SIMD) anche su più thread; accanto, per posizione, id della voce, genitore, tipo, dimensione e
data di modifica per i filtri. I percorsi si ricostruiscono risalendo i genitori.

**Aggiornamento**: per ora l'indice di un volume si ricostruisce in background quando il
catalogo di quel volume cambia, al massimo ogni 30 s (i risultati possono essere indietro di
quel tanto; REVIEW L11). L'aggiornamento incrementale dalle operazioni di scrittura è un
miglioramento successivo.
