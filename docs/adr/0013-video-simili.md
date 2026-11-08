# ADR-0013 — Decodifica video per i video simili

- **Stato**: **Accettata** (2026-11-07, T8)
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

## Prototipo e misure

Prototipo di A in `prism-win::video` (lettore di sorgente, primo flusso video, uscita NV12 con
ripiego su RGB32, salto ai punti richiesti e decodifica fino al tempo esatto) con il comando
`prism-cli video-frames`; dati in `docs/benchmarks/2026-11-07-t8-video.md`:

- **Copertura**: tutti i 123 + 60 video provati del disco D: (MP4 H.264 fino a 1920×1200, WebM
  VP8/VP9) decodificati con i codec di sistema, nessun errore.
- **Velocità**: 486 ms in media (858 ms al massimo) per 5 fotogrammi al tempo esatto con
  l'uscita NV12, contro 3,9 s con RGB32.
- **Robustezza**: la stessa clip ricodificata da Windows a un quarto della risoluzione e in WMV
  dà impronte a 0,2 bit per fotogramma; una clip diversa a 31,6.

B non è stato provato sul campo: ffmpeg non è presente sulla macchina e andrebbe scaricato e
distribuito con l'installer (licenza da chiarire, decine di MB); con A i formati della libreria
reale sono già coperti.

## Decisione

**A — Media Foundation.** Nessuna dipendenza né licenza da gestire, installer invariato,
formati della libreria reale coperti, velocità adatta a un job in background.

## Conseguenze

- I formati sono quelli per cui Windows ha un decodificatore: MP4/MOV H.264, WMV, MKV e WebM
  sui Windows recenti; **HEVC (H.265) solo con l'estensione "HEVC Video Extensions"** dello
  Store, che l'utente può non avere (REVIEW L15). I video che non si decodificano vengono
  contati e segnalati, non bloccano nulla.
- L'impronta di un video sono 5 *difference hash* di luminanza (10–90 % della durata) più la
  durata; due video sono simili se le durate sono vicine e la distanza media per fotogramma
  resta sotto la soglia scelta.
- Se un giorno servissero più formati, ffmpeg resta un'alternativa dietro la stessa funzione
  (`sample_frames`), senza cambiare il resto.
