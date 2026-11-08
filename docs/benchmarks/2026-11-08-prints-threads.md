# Impronte delle immagini con più thread (2026-11-08)

Misura chiesta dall'utente prima di decidere se l'analisi del contenuto debba usare più core
(REVIEW D60).

**Macchina**: AMD Ryzen 9 7950X (16 core, 32 thread), Windows 11 Pro, SSD NVMe.
**File**: 300 foto JPEG di una cartella dell'utente su C: (718 MB, circa 2,4 MB l'una), lette in
sola lettura.
**Programma**: `prism-cli print-bench <cartella> --limit 300 --threads 1,2,4,8` in release. Ogni
thread entra in modalità background di Windows, come il job dell'app, e calcola l'impronta
(*difference hash*) con la stessa funzione (`media::print_image`). Una prima lettura di tutti i
file mette i dati in cache, così si misura la decodifica e non il disco (la prima lettura dei
718 MB ha richiesto 0,3 s: il disco non è il limite).

## Risultati (due ripetizioni)

| Thread | Tempo | Foto/s | Accelerazione |
|---|---|---|---|
| 1 | 17,07 s / 17,48 s | 17,6 / 17,2 | 1,00× |
| 2 | 8,84 s / 9,01 s | 33,9 / 33,3 | 1,93× / 1,94× |
| 4 | 4,55 s / 4,51 s | 66,0 / 66,4 | 3,75× / 3,87× |
| 8 | 2,43 s / 2,46 s | 123,5 / 122,1 | 7,03× / 7,11× |

## Conclusione

La decodifica delle foto è limitata dal processore e scala quasi linearmente con i thread. Le
impronte delle immagini usano quindi **un quarto dei core, da 1 a 4 thread**
(`media::print_workers`): 4 su questa macchina (circa 3,8 volte più veloce), 2 su un PC con 8
core, 1 su un portatile con 4 core. I thread sono in modalità background, e un quarto dei core
lascia il PC libero anche mentre l'analisi gira. Costo: ogni thread tiene in memoria una foto
decodificata (circa 70 MB per una foto da 24 megapixel), solo durante questa fase.

Restano su un solo thread:
- gli hash del contenuto (BLAKE3): li limita il disco, non il processore;
- le impronte dei video: le decodifica Media Foundation di Windows, un video alla volta.
