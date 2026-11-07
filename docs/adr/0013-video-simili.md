# ADR-0013 — Decodifica video per i video simili

- **Stato**: **Proposta** (decisione in T8)
- **Data**: 2026-11-07

## Contesto

Per trovare video simili (F12) servono alcuni fotogrammi decodificati per ogni video, da cui
calcolare un'impronta percettiva.

## Opzioni

| | A. Media Foundation (Windows) | B. ffmpeg incluso |
|---|---|---|
| Dipendenze | Nessuna, è parte di Windows | Binario o librerie ffmpeg da distribuire |
| Licenza | Nessun problema | LGPL o GPL a seconda della build: da verificare |
| Dimensione installer | Invariata | Decine di MB in più |
| Formati | Quelli con codec installato (MP4/H.264, WMV, MKV su Windows recenti; HEVC richiede l'estensione dello Store) | Praticamente tutti |
| Sviluppo | API COM, più codice | API più comoda tramite binding Rust |

## Criteri

Copertura dei formati sulla libreria video reale dell'utente, velocità di estrazione dei
fotogrammi, licenza e dimensione dell'installer.

## Decisione

Da prendere in T8 con un prototipo su entrambe le opzioni.
