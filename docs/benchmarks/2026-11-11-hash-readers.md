# Lettura dei file dell'analisi: 2.1 contro 2.2 (REVIEW L31)

- **Data**: 2026-11-11
- **Macchina**: PC di sviluppo, Windows 11 Pro, SSD NVMe Samsung 990 PRO 2 TB
- **Comando**: `prism-cli hash-bench <cartella> --limit 20000 --readers ...` (solo lettura)

## Che cosa misura

`hash-bench` elenca fino a `--limit` file della cartella e li distribuisce a turno fra le
prove, così nessun file si legge due volte e nessuna cache aiuta una prova successiva:

- **2.1**: un file alla volta, hash completo, poi il file riaperto per nome per dimensione e
  data (come faceva la passata completa della 2.1);
- **2.2**: N file alla volta, ogni file aperto una volta, i file fino a 1 MiB letti interi,
  dimensione e data dal file aperto (`hashing::read_file`).

Ogni thread è in modalità background di Windows, come nell'app.

## Perché

Dal sesto pacchetto diagnostico del PC aziendale: sulla cartella d'archivio del file server
(3,3 milioni di voci) gli hash di 2,5 milioni di file hanno richiesto 21 ore e mezza, un file
alla volta (33 file al secondo); le impronte degli stessi file, con 4 thread, andavano a 100 file
al secondo pur decodificandoli. Sulla rete il tempo va nell'attesa del server per ogni file.

## SSD NVMe locale (`node_modules` di due progetti, file piccoli)

Prima cartella, prove nell'ordine 1, 2, 4, 8:

| Lettura | File | MB | Tempo | File/s | MB/s |
|---|---|---|---|---|---|
| 2.1: uno alla volta, aperto due volte | 4000 | 40,1 | 1,7 s | 2310 | 23,1 |
| 2.2: 1 alla volta, aperto una volta | 4000 | 34,6 | 1,4 s | 2904 | 25,1 |
| 2.2: 2 alla volta | 4000 | 28,5 | 1,8 s | 2207 | 15,7 |
| 2.2: 4 alla volta | 4000 | 39,8 | 2,2 s | 1831 | 18,2 |
| 2.2: 8 alla volta | 4000 | 35,8 | 2,1 s | 1929 | 17,2 |

Seconda cartella, prove nell'ordine 8, 4, 2, 1:

| Lettura | File | MB | Tempo | File/s | MB/s |
|---|---|---|---|---|---|
| 2.1: uno alla volta, aperto due volte | 4000 | 46,6 | 3,2 s | 1261 | 14,7 |
| 2.2: 8 alla volta | 4000 | 33,7 | 2,1 s | 1945 | 16,4 |
| 2.2: 4 alla volta | 4000 | 37,6 | 2,8 s | 1425 | 13,4 |
| 2.2: 2 alla volta | 4000 | 74,9 | 3,1 s | 1300 | 24,3 |
| 2.2: 1 alla volta | 4000 | 41,5 | 3,2 s | 1233 | 12,8 |

Su un SSD locale un file costa mezzo millisecondo o meno e le differenze fra 1 e 8 file alla
volta cambiano verso da una prova all'altra: sono dentro il rumore (cache di Windows, antivirus).
**Sui dischi locali l'app legge un file alla volta.** L'apertura unica dà +26 % nella prima
prova e niente nella seconda.

## Rete

Da misurare sul PC aziendale con la 2.2.0 di prova: le righe `hashing finished` del log
riportano file, byte, durata e thread di ogni giro, da confrontare con i 33 file al secondo
della 2.1.4 sulla stessa cartella.
