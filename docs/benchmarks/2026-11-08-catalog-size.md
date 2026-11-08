# Dimensione del catalogo reale (2026-11-08)

Su domanda dell'utente ("quanto è grande il database?"). Misurato su una **copia** del catalogo
del Prism 2.0.0 installato sul PC dell'utente (`%LOCALAPPDATA%\com.prism.catalog\catalog.db`,
con Prism chiuso), mai sull'originale. Strumento: Python 3.12 con il suo SQLite 3.49 (senza
`dbstat`): il peso di ogni parte è la differenza di dimensione dopo averla tolta dalla copia e
aver fatto `VACUUM`.

## Il catalogo

| Volume | Voci |
|---|---:|
| Data (D:, NTFS) | 621.166 |
| C: (NTFS) | 1.645.548 |
| TMP (NTFS) | 59.893 |
| PRISMTEST (disco di test dello sviluppo, finito nel catalogo per L22) | 0 |
| NAS `Small-Storage` | 21.302 |
| NAS `Big-Storage` | 576.938 |
| NAS `ZimaOS-HD` | 604.091 |
| **Totale** | **3.528.938** (2.833.142 file, 2.804.725 con contenuto; 695.796 cartelle) |

Analisi del contenuto ancora in corso: 679.757 file con hash (il 24%), 14.336 impronte di
immagini, 4.600 di video.

## Peso su disco

| Parte | MB | Per unità |
|---|---:|---|
| `catalog.db` com'è | 580,1 | 164 B per voce |
| dopo `VACUUM` | 519,7 | 147 B per voce |
| — voci (`entries`, righe) | 241,8 | 69 B per voce |
| — indice nome per cartella (`entries_parent_name`) | 118,3 | 34 B per voce |
| — indice per volume (`entries_volume`) | 34,8 | 10 B per voce |
| — totali delle cartelle (`dir_stats`) | 11,7 | 17 B per cartella |
| — hash del contenuto (`content_hashes` con i due indici) | 112,3 | **165 B per file analizzato** |
| — impronte di immagini e video | 0,8 | ~40 B per impronta |

Accanto al catalogo: la cartella `webview` di WebView2 (69 MB) e i log.

## Lettura

- **Senza l'analisi del contenuto il catalogo resta sui 116 B per voce**, come misurato in T1 e
  T10 (108–129 B; 120 B con 5,7 milioni di voci).
- **L'analisi del contenuto costa 165 B per file**: due hash BLAKE3 da 32 byte (rapido e
  completo), dimensione e data per cui valgono, e due indici per cercare i duplicati. A analisi
  finita su questi 2,8 milioni di file il catalogo arriverebbe a ~870 MB (~250 B per voce),
  al limite dell'obiettivo NF2 (≤ 250 B).
- I ~60 MB tra il file e il `VACUUM` sono pagine mezze vuote lasciate da inserimenti e
  cancellazioni: SQLite non restituisce lo spazio da solo e Prism non fa `VACUUM` (nemmeno dopo
  *Rimuovi dal catalogo* o il reset).
- **Il 17% del catalogo è doppio**: la condivisione di sistema `ZimaOS-HD` contiene in `.media`
  gli altri due volumi del NAS (598.240 voci, esattamente Big-Storage + Small-Storage; REVIEW
  L19). L'analisi del contenuto rilegge quei file dalla rete una seconda volta.

## Come ripetere

Copiare `catalog.db` (a Prism chiuso) in una cartella di prova e lanciare gli script di misura
su quella copia: lo script elimina tabelle e indici dalla copia, mai dall'originale.
