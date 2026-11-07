# Benchmark

Misure ripetibili delle prestazioni di Prism, in **sola lettura** sui dischi reali
(docs/TESTING.md, regola S3). Ogni file annota macchina, dischi, numero di voci e data.

| File | Tappa | Contenuto |
|---|---|---|
| [2026-11-07-t1-walker.md](2026-11-07-t1-walker.md) | T1 | Indicizzazione con il walker su C: e D:, dimensione del catalogo per voce, memoria, confronto con il vecchio Prism |
| [2026-11-07-t2-network.md](2026-11-07-t2-network.md) | T2 | Walker su una condivisione SMB letta via UNC da un processo elevato |
| [2026-11-07-t2-mft.md](2026-11-07-t2-mft.md) | T2 | Lettore MFT contro walker su C: e D:; indice sui riferimenti e memoria |

## Come ripetere

```powershell
cargo build --release -p prism-cli
.\target\release\prism-cli.exe bench C:\ --out risultati.md
```

`bench` indicizza in un catalogo nuovo in `%LOCALAPPDATA%\PrismDev\bench\`, ripete
l'aggiornamento senza modifiche, controlla la coerenza del catalogo e poi lo elimina (solo dentro
quella cartella, tramite la guardia sandbox). Opzioni utili: `--threads N`,
`--no-default-exclusions`, `--exclude-dir NOME`.
