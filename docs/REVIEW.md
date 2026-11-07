# Prism — Revisione finale dell'utente

> Guida per l'unica revisione dell'utente, la sera del 2026-11-07 (CLAUDE.md, "Autonomia
> totale"). Lo sviluppo procede senza soste e registra qui ogni decisione presa senza l'utente,
> con le alternative e il modo di cambiarla. Chi aggiunge una voce la mette in fondo alla
> tabella giusta, con data e tappa.

## 1. Decisioni prese in autonomia

Decisioni già documentate prima dell'introduzione di questo file (dettagli nelle ADR):

| # | Tappa | Decisione | Alternative | Come cambiarla | Riferimento |
|---|---|---|---|---|---|
| D1 | T1 | Unicità dei nomi nel catalogo con confronto esatto (NTFS ammette cartelle sensibili alle maiuscole); il confronto senza maiuscole solo per percorsi digitati ed esclusioni | Unicità senza maiuscole (ADR-0005 originale) | Nuova migrazione dello schema | ADR-0015 |
| D2 | T2 | Le unità di rete mappate si leggono da `HKCU\Network` e si indicizzano via percorso UNC, perché un processo elevato non vede le mappature dell'utente | Chiedere all'utente il percorso UNC | Codice in `prism-win::volumes` | ADR-0016 |
| D3 | T2 | Lettore MFT adottato ("go") per i volumi NTFS locali; walker per gli altri e come ripiego | Solo walker | Scelta dell'enumeratore nel motore dei job | ADR-0017 |
| D4 | T2 | Prism non crea il journal USN sui dischi dell'utente che non lo hanno: per quei volumi si usa la sorveglianza delle cartelle o la riscansione | Creare il journal (modifica il volume) | Opzione nelle impostazioni | ADR-0018 |
| D5 | — | Annullata la sosta di revisione di fine T3: lo sviluppo arriva alla 1.0 e l'utente fa una sola revisione finale (decisione dell'utente) | Sosta a fine T3 | — | CLAUDE.md |

Decisioni successive (da compilare durante lo sviluppo):

| # | Data | Tappa | Decisione | Alternative | Come cambiarla | Riferimento |
|---|---|---|---|---|---|---|
| D6 | 2026-11-07 | T2 | Indice SQL su `file_ref` rimosso: le ricerche per riferimento usano una mappa in memoria (inserimenti MFT 5× più veloci) | Indice `(volume_id, file_ref)` | Migrazione che ricrea l'indice | ADR-0017 |
| D7 | 2026-11-07 | T2 | Sorveglianza cartelle con la libreria `notify` 8 (usa `ReadDirectoryChangesW`), invece di codice Win32 scritto a mano | Implementazione diretta con I/O sovrapposto | Sostituire `prism-win::watch` | ARCHITECTURE §4.2 |
| D8 | 2026-11-07 | T2 | Aggiornamento dai cambi di cartella: si rienumera il sottoalbero della cartella del catalogo più vicina a ogni percorso cambiato (solo le più in alto); un cambio nella radice rienumera tutto il volume | Elenchi superficiali come per il journal (più veloce, più complesso) | `prism-core::refresh` | ARCHITECTURE §4.2 |
| D9 | 2026-11-07 | T2 | T2 chiusa rimandando due ottimizzazioni (istantanea compatta, `$MFT` molto grandi) per arrivare prima all'app provabile | Completarle prima di T3 | Voci L1 e L2 in §3 | ROADMAP T2 |
| D10 | 2026-11-07 | T3 | Motore dei job in memoria (nessuna tabella `jobs` per ora): un'indicizzazione interrotta è già segnalata dallo stato del volume (`reindex_needed`) | Record persistente subito | Aggiungere la tabella quando arrivano i job di background riprendibili (T6) | ARCHITECTURE §6 |
| D11 | 2026-11-07 | T3 | Tipi TypeScript generati con `ts-rs` (stabile) e comandi Tauri scritti a mano in `ui/src/lib/api.ts`; strumenti dell'interfaccia: Svelte 5, Vite 8, TypeScript 5.9 (non la 7 in anteprima), Vitest | `tauri-specta` (genera anche i comandi, ma per Tauri 2 è ancora in release candidate) | Sostituire la generazione in `prism-app/src/dto.rs` | ARCHITECTURE §7 |
| D12 | 2026-11-07 | T3 | Identificativo dell'app `com.prism.catalog`: catalogo nuovo e separato dai dati del vecchio Prism (`com.prism.diskanalyzer`), che non vengono né letti né importati | Riprendere l'identificativo vecchio; importare il vecchio catalogo | `tauri.conf.json` (prima del rilascio) | ARCHITECTURE §7 |
| D13 | 2026-11-07 | T3 | Volume con identità ambigua (F02-AC3): per ora resta fuori dal catalogo e viene solo registrato nel log; la finestra di scelta arriva più avanti (§3 L7) | Associarlo al candidato più recente | `prism-app/src/monitor.rs` | SPEC F02-AC3 |
| D14 | 2026-11-07 | T3 | Dischi nuovi proposti con un riquadro nella schermata catalogo; da 3 dischi nuovi in su (tipico del primo avvio) un unico pannello compatto con *Indicizza tutti*; *Non ora* vale fino alla chiusura di Prism | Un riquadro per disco; *Non ora* permanente | `ui/src/lib/catalog.ts` (`GROUP_PROMPTS_FROM`) | SPEC F02-AC1 |
| D15 | 2026-11-07 | T3 | Disco senza etichetta mostrato come Esplora risorse: "Disco interno (C:)", "Unità di rete (Z:)" | Mostrare la lettera nuda | `displayName` in `ui/src/lib/catalog.ts` | F01 |
| D16 | 2026-11-07 | T3 | Rilevamento dei dischi per interrogazione ogni 2 s (rispetta F01-AC2, ≤ 5 s) invece delle notifiche di Windows sui dispositivi; journal applicato ogni 15 s ai volumi NTFS indicizzati | `RegisterDeviceNotification` / `WM_DEVICECHANGE` | `prism-app/src/monitor.rs` | ARCHITECTURE §7 |
| D17 | 2026-11-07 | T3 | Impostazioni salvate come un documento JSON nella tabella `settings` del catalogo; l'opzione "Avvia con Windows" registra o rimuove l'attività pianificata (D18) | File di configurazione separato | `prism-app/src/commands.rs` | F21-AC1 |
| D18 | 2026-11-07 | T3 | Avvio con Windows: attività pianificata `Prism` all'accesso dell'utente, con i privilegi più elevati, 10 s dopo l'accesso, finestra nascosta nel tray (`--autostart`); Prism la riallinea a ogni avvio (percorso del programma) e quando cambia l'impostazione. Le build di **sviluppo** non la creano mai (solo l'impostazione viene salvata) | Chiave `Run` del registro (non può partire elevato senza UAC); attività creata solo dall'installer | `prism-win::autostart`, `AppState::apply_autostart` | ADR-0003 |
| D19 | 2026-11-07 | T3 | Dischi nuovi a finestra nascosta: notifica di Windows (toast) nella lingua dell'interfaccia, una sola per i dischi trovati insieme; a finestra visibile basta il riquadro. Le build di release registrano l'identificativo `com.prism.catalog` (nome e icona) in `HKCU\Software\Classes\AppUserModelId`; quelle di sviluppo usano l'identificativo di Windows PowerShell per non scrivere nel registro | Plugin di notifica di Tauri; icona del tray con fumetto | `prism-win::notify`, `prism-app/src/notices.rs` | SPEC F02-AC1 |

## 2. Cose che richiedono l'utente

| # | Cosa | Perché serve l'utente | Cosa è già pronto |
|---|---|---|---|
| U1 | Firma del codice (SPEC Q7) | Scelta e acquisto del certificato | Da preparare in T10: firma configurabile senza modifiche al codice |
| U2 | Merge di `rewrite` sul ramo principale e rilascio 1.0 (Q11) | Approvazione | — |
| U3 | Prova su un PC Windows 11 pulito (installer, avvio con Windows, aggiornamenti) | Serve un'altra macchina | — |
| U4 | Collegare e scollegare un disco esterno (T2, F02) | Azione fisica | Test automatico del cambio di lettera sul disco di test già passato |
| U5 | Aspetto e usabilità dell'interfaccia | Giudizio estetico | — |
| U6 | Rimuovere il runner amministratore a fine sviluppo | Riduce la superficie d'attacco | `scripts/admin/uninstall-admin-runner.ps1` |

## 3. Problemi aperti e limiti noti

| # | Data | Problema | Impatto | Stato |
|---|---|---|---|---|
| L1 | 2026-11-07 | Memoria massima durante l'indicizzazione ~315 B per voce (istantanea `ScanTree` completa in memoria) | Un volume da 5 milioni di voci richiederebbe ~1,5 GB durante l'indicizzazione (NF3 chiede ≤ 500 MB a regime) | Rimandato (dopo le funzioni della 1.0) |
| L2 | 2026-11-07 | Lettore MFT: una `$MFT` con `$ATTRIBUTE_LIST` nel record 0 (MFT enormi e frammentate) viene rifiutata; file compressi/sparsi non verificati su disco reale | Su quei volumi si usa il walker (più lento, stessi risultati logici) | Rimandato |
| L3 | 2026-11-07 | NTFS scrive in ritardo nei record MFT le date delle cartelle: due letture vicine possono differire di pochi ms | Solo estetico; si corregge alla modifica successiva della cartella | Accettato (ADR-0018) |
| L4 | 2026-11-07 | Aggiornamento dal journal: la mappa dei riferimenti (≈0,25 s per 1,3 milioni di voci) si ricarica a ogni aggiornamento | Con il journal applicato ogni 15 s (D16), un volume grande con cambi continui (C:) costa ~0,25 s di CPU per giro | Aperto, da misurare nell'app e ottimizzare (indice parziale sulle cartelle o mappa mantenuta in memoria tra un giro e l'altro) |
| L5 | 2026-11-07 | Unità di rete: capacità e spazio libero non letti, e "collegata" significa "mappata" (non si verifica che il server risponda) | Schede di rete senza barra dello spazio; un NAS spento appare collegato | **Risolto** (2026-11-07): sonda in thread separati con tempo massimo di 5 s; una condivisione conta come collegata solo quando il server risponde (ARCHITECTURE §7) |
| L6 | 2026-11-07 | Nell'app, i volumi non NTFS (exFAT, FAT) e le unità di rete non si aggiornano da soli mentre sono collegati: solo alla riconnessione o con *Aggiorna* (la sorveglianza delle cartelle esiste in `prism-win::watch` ma non è ancora collegata all'app) | Catalogo di chiavette e NAS meno fresco | **Risolto** (2026-11-07): sorveglianza cartelle collegata nell'app per FAT/exFAT/ReFS e unità di rete; da provare con una chiavetta (checklist T3, punto 10) |
| L7 | 2026-11-07 | Identità ambigua di un volume (F02-AC3): nessuna finestra di scelta ancora (D13) | Caso raro (due dischi clonati); il disco non compare finché non si risolve | Da fare |
| L8 | 2026-11-07 | Disco nuovo collegato mentre la finestra è nascosta nel tray: nessuna notifica di Windows ancora, solo il riquadro alla riapertura | F02-AC1 parziale | **Risolto** (2026-11-07): notifica di Windows, una sola per più dischi trovati insieme (D19); il clic sulla notifica non apre ancora Prism (L9) |
| L9 | 2026-11-07 | Il clic su una notifica di Prism la chiude soltanto: aprire Prism dalla notifica richiede un attivatore COM registrato dall'installer | Si riapre Prism dall'icona nel tray | Da fare in T10 (installer) |

## 4. Come provare l'app

Le prove che cancellano, spostano o puliscono file si fanno **solo** sul disco di test
PRISMTEST o in cartelle di prova (TESTING S5).

### Avvio

Da un terminale nella cartella `D:\Progetti\Github\Prism-next`:

```powershell
npm --prefix ui ci                  # una volta: dipendenze dell'interfaccia
npm --prefix ui run build           # interfaccia → ui/dist
cargo run -p prism-app --features custom-protocol
```

- La build di debug **non** chiede l'amministratore (manifest `asInvoker`): avviata da un
  terminale normale indicizza leggendo le cartelle e non segue il journal; avviata da un
  terminale **elevato** usa la MFT e si aggiorna da sola (come il prodotto finale). Catalogo di
  prova in `%LOCALAPPDATA%\PrismDev\app\catalog.db`: si può cancellare per ripartire da zero.
- Build come il prodotto finale (chiede l'amministratore con UAC, catalogo in
  `%LOCALAPPDATA%\com.prism.catalog`):
  `cargo build -p prism-app --release --features custom-protocol`, poi
  `target\release\prism.exe`.
- Solo l'interfaccia, con dati d'esempio, nel browser: `npm --prefix ui run dev` e aprire
  `http://localhost:5173`.

### Checklist T3 (catalogo, disco nuovo, etichette, impostazioni, tray)

1. Primo avvio: pannello "N nuovi dischi trovati"; i dischi senza etichetta si chiamano
   "Disco interno (C:)" ecc. (D14, D15).
2. *Indicizza* su PRISMTEST o su una chiavetta: avanzamento sulla scheda, poi numero di file e
   cartelle (F01-AC3). *Indicizza tutti* accoda tutto; i dischi fisici diversi lavorano in
   parallelo.
3. (App elevata) creare o cancellare file sul disco indicizzato: entro ~15 s la scheda si
   aggiorna da sola (F05-AC1).
4. *Nome e posizione*: nome, posizione fisica, note, colore; restano dopo il riavvio (F03).
5. *Ignora sempre*: il disco sparisce; *Impostazioni → Dischi ignorati → Non ignorare più* lo
   riporta (F02-AC4).
6. *Rimuovi dal catalogo* (cestino sulla scheda): chiede conferma, i file non vengono toccati
   (F01-AC5).
7. Chiudere la finestra: Prism resta nell'area di notifica; clic sull'icona per riaprire, menu
   *Esci* per uscire (F19-AC1). Avviarlo di nuovo porta la finestra in primo piano (F19-AC4).
8. *Impostazioni*: lingua (Italiano/English/come Windows) e tema (chiaro/scuro/come Windows)
   cambiano subito, anche le voci del tray.
9. Scollegare un disco esterno: entro 5 s passa tra i "Non collegati" con i dati dell'ultima
   indicizzazione; ricollegandolo torna "Collegato" e si aggiorna (F01-AC1/AC2, F05-AC2; U4).
10. Chiavetta USB (exFAT/FAT) indicizzata: copiare o cancellare file mentre Prism è aperto; in
    pochi secondi la scheda si aggiorna da sola (sorveglianza cartelle, REVIEW L6).
11. Unità di rete: con il server acceso la scheda mostra lo spazio usato e libero; spegnendo il
    server (o scollegando la rete) entro ~15 s passa tra i "Non collegati" (REVIEW L5).
12. (Build di release) *Impostazioni → Avvia Prism con Windows*: attivandola compare
    l'attività `Prism` nell'Utilità di pianificazione; al successivo accesso a Windows Prism
    parte da solo nell'area di notifica, senza richiesta UAC (F19-AC3; prova completa sul PC
    pulito, U3). Disattivandola l'attività sparisce.
13. Con Prism nascosto nel tray, collegare una chiavetta mai vista: compare una notifica di
    Windows "Nuovo disco: …" (F02-AC1, D19). In sviluppo la notifica appare a nome di Windows
    PowerShell; nella build di release a nome di Prism.
14. *Apri in Esplora risorse* (icona sulla scheda di un disco collegato): si apre la finestra di
    Esplora risorse sul disco, senza privilegi di amministratore (ADR-0003).
15. Giudizio su aspetto e usabilità (U5).

Note sulle prove di T3:

- è rimasta sul disco di test la cartella `F:\prism-demo` (file di prova creati per verificare
  l'aggiornamento dal journal); la sua rimozione automatica è stata bloccata dal controllo di
  sicurezza della sessione. Si può cancellare a mano;
- nel Centro notifiche possono esserci due notifiche di prova a nome di Windows PowerShell
  ("Prism (test)" e "7 nuovi dischi trovati"), generate verificando le notifiche.
