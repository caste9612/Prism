# ADR-0016 — Abbinamento dei volumi e unità di rete in un processo elevato

- **Stato**: Accettata
- **Data**: 2026-11-07
- **Rifinisce**: ADR-0004 (regole di abbinamento previste "da rifinire in T2")

## Contesto

L'ADR-0004 fissa le chiavi d'identità dei volumi (seriale del file system, capacità, percorso UNC,
GUID come informazione aggiuntiva). Implementando il rilevamento (T2) sono emersi casi concreti:

- un volume NTFS ridimensionato cambia capacità ma non seriale né GUID;
- due dischi clonati hanno stesso seriale e stessa capacità;
- sulla macchina di sviluppo esistono partizioni senza lettera (ripristino 940 MB, EFI 96 MB);
- Windows riporta i dischi esterni USB come "fissi": il tipo di unità non distingue interno ed
  esterno (F01);
- **un processo elevato non vede le unità di rete mappate dall'utente**: verificato il
  2026-11-07, `net use` in una shell elevata è vuoto mentre l'utente ha X:, Y:, Z: mappate su un
  NAS; il valore `EnableLinkedConnections` non è impostato. Prism gira sempre elevato
  (ADR-0003). Le stesse condivisioni sono invece raggiungibili dal processo elevato tramite il
  percorso UNC (verificato elencando `\\192.168.1.65\Small-Storage`).

## Decisione

**Rilevamento** (`prism-win::volumes::WinVolumeSource`, nessun privilegio richiesto):

- volumi locali da `FindFirstVolumeW`; si escludono quelli senza punto di montaggio (non c'è un
  percorso da cui leggerli) e le unità ottiche;
- seriale a 64 bit per NTFS e ReFS da `FILE_ID_INFO` sulla cartella radice (non serve
  amministratore; sui volumi NTFS i 32 bit bassi coincidono con il seriale a 32 bit di Windows,
  verificato da un test), a 32 bit per gli altri file system;
- **tipo di bus** del disco (`IOCTL_STORAGE_QUERY_PROPERTY`), memorizzato nel catalogo
  (migrazione 2, colonna `bus_type`): un volume "fisso" su USB, 1394, SD o MMC è **esterno**;
- numeri dei dischi fisici (`IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS`), solo in memoria: cambiano
  tra un riavvio e l'altro e servono per non far lavorare lo stesso disco su più job (ADR-0009);
- **unità di rete** da due fonti unite: le mappature persistenti dell'utente
  (`HKCU\Network\<lettera>\RemotePath`, visibili anche dal processo elevato perché il profilo è
  lo stesso) e quelle della sessione corrente (`WNetGetConnectionW`). Ogni condivisione è
  identificata **e letta** tramite il suo percorso UNC; la lettera resta solo come informazione
  per l'utente. Per le unità di rete non si fanno chiamate che vanno in rete durante il
  rilevamento (un server spento può bloccarle per decine di secondi).

**Abbinamento** (`prism-core::volumes::match_volumes`, funzione pura):

1. Unità di rete: percorso UNC normalizzato (`\\?\UNC\` accettato, senza maiuscole, senza
   separatore finale). Un solo candidato → noto.
2. Volumi locali con seriale: candidati = volumi del catalogo con stesso seriale e stesso file
   system.
   - Se esattamente un candidato ha anche lo **stesso GUID** → noto (stesso volume su questa
     macchina, anche se ridimensionato).
   - Altrimenti, se esattamente un candidato ha la **stessa capacità** → noto (stesso disco su
     un'altra macchina o con un'altra lettera).
   - Altrimenti → **ambiguo**: decide l'utente (F02-AC3).
3. Volumi senza seriale: solo il GUID.
4. Nessun candidato → **nuovo** (F02-AC1).
5. Un volume del catalogo si assegna a un solo volume collegato; prima le corrispondenze
   confermate dal GUID.

Un volume noto aggiorna nel catalogo informazioni di sistema, punto di montaggio e "visto
l'ultima volta"; uno nuovo viene aggiunto con stato `new`; uno ambiguo non viene registrato
finché l'utente non sceglie.

## Conseguenze

- La lettera non è mai una chiave: il test elevato `f02_ac3_letter_change_keeps_the_catalog_identity`
  cambia davvero la lettera del disco di test PRISMTEST e verifica che l'identità resti la stessa.
- Le unità di rete compaiono anche con Prism elevato. Limiti: le mappature **non persistenti**
  fatte nella sessione non elevata non sono visibili (si possono aggiungere a mano con il
  percorso UNC, F02-AC5); se l'elevazione avviene con un altro account amministratore, il
  registro `HKCU` è quello dell'altro account.
- L'accesso via UNC richiede che le credenziali della condivisione siano disponibili al processo
  elevato (Gestione credenziali di Windows è per utente, non per sessione: verificato sul NAS
  dell'utente).
- Capacità e spazio libero delle condivisioni non si leggono durante il rilevamento: si
  leggeranno in un job con un tempo massimo (T3).
