# Prism — Strategia di test e regole di sicurezza

> Stato: **v1** (2026-11-07). Obiettivo: ogni criterio di accettazione della SPEC (`Fxx-ACn`)
> è coperto da almeno un test automatico o da una voce della checklist manuale della tappa
> (NF8). I test citano l'ID del criterio che verificano (nel nome o in un commento).

## 1. Regole di sicurezza (non negoziabili)

Lo sviluppo avviene sul PC dell'utente, che contiene dati reali. Per questo:

| # | Regola |
|---|---|
| S1 | **Operazioni distruttive solo in sandbox.** Eliminazione, spostamento, sovrascrittura, Cestino e copia *verso* una destinazione si eseguono nei test esclusivamente: (a) in cartelle temporanee create dal test (`tempfile`, sotto `%TEMP%`), oppure (b) sul **disco virtuale di test** dedicato (sezione 4). |
| S2 | **Guardia nel codice.** Le operazioni distruttive di `fileops` passano da un'interfaccia che, nei test e nei comandi di sviluppo della CLI, è avvolta da una guardia (`SandboxGuard`): se il percorso non è dentro una radice consentita, l'operazione viene **rifiutata** e il test fallisce. Così un errore nel test non può raggiungere file reali. |
| S3 | **Dischi reali solo in lettura.** Scansioni, letture MFT/USN, SMART e benchmark sui dischi reali sono solo letture. |
| S4 | **Dati di sviluppo separati.** I cataloghi di sviluppo e di test stanno in `%LOCALAPPDATA%\PrismDev\`, mai nella cartella dati dell'app installata. I dati del vecchio Prism (`%APPDATA%\com.prism.diskanalyzer`) non si toccano. |
| S5 | **Niente prove manuali distruttive su dischi reali.** Quando si prova l'app a mano (cancellazioni, pulizia, copia), lo si fa su cartelle di prova o sul disco virtuale di test. |
| S6 | **Mai indebolire un test per farlo passare.** Se un test fallisce e la causa non è chiara, niente commit: si torna all'ultimo stato verde, si segnala il problema in `docs/REVIEW.md` §3 e si passa a un lavoro indipendente. |

## 2. Livelli di test

| Livello | Dove | Cosa copre | Esecuzione |
|---|---|---|---|
| Unitari | `crates/*/src` (`#[cfg(test)]`) | Funzioni pure: esclusioni, riconciliazione, parser dei filtri, regole di selezione, validità degli hash, aggregati delle cartelle, ricostruzione dei percorsi. | `cargo test` |
| Proprietà | `crates/prism-core/tests` | Generazione casuale di alberi di file e sequenze di modifiche (proptest): l'aggiornamento incrementale deve produrre lo stesso catalogo di un'indicizzazione completa (F05-AC3). | `cargo test` |
| Integrazione | `crates/prism-core/tests` | Walker reale su cartelle temporanee → catalogo SQLite → modifiche sul disco → aggiornamento → verifiche. Copia verificata, Cestino e registro azioni in sandbox. | `cargo test` |
| Windows senza admin | `crates/prism-win` | Rilevamento volumi (lettura), identità dei volumi, avvio non elevato, Cestino su file temporanei. | `cargo test` |
| Windows con admin | `crates/prism-win/tests` (feature `admin-tests`) | Lettura MFT e journal USN sul disco virtuale di test; SMART in sola lettura sui dischi fisici; cambio della lettera del disco di test PRISMTEST con verifica dell'identità (`volume_identity.rs`, F02-AC3: tocca solo il volume con etichetta PRISMTEST e bus virtuale, e ripristina la lettera); lettore MFT confrontato con il walker su un albero creato sul disco di test (`mft.rs`: hard link, record di estensione, flussi alternativi, nomi Unicode e con punto finale; la cartella di prova viene creata ed eliminata tramite la guardia sandbox); journal USN (`usn.rs`: record di creazione, rinomina, cancellazione e hard link; recupero di singoli file con metadati identici alla lettura completa; il journal viene creato **solo** sul disco di test, se manca); aggiornamento dal journal confrontato con la reindicizzazione completa dopo modifiche casuali reali (`ntfs_update.rs`, F05-AC1/AC3); attività di avvio con Windows con i privilegi più elevati, registrata **disattivata** in `\PrismDevTests\`, riletta e rimossa (`autostart.rs`, F19-AC3); apertura senza privilegi tramite la shell del desktop, verificata con il livello di integrità del processo avviato (`shell.rs`, ADR-0003); Cestino e eliminazione definitiva di file e cartelle creati sul disco di test, con svuotamento del solo Cestino di quel volume (`trash.rs`, F16-AC1/AC2); eliminazione sicura completa (catalogo, verifica, Cestino vero) su una cartella del disco di test (`fileops_real.rs`). | Runner amministratore (sezione 4) |
| Interfaccia | `ui/` | Logica di presentazione e layout della treemap (Vitest); flussi end-to-end in browser con IPC simulato: catalogo (`e2e/catalog.spec.ts`), spazio (`e2e/space.spec.ts`), pulizia (`e2e/cleanup.spec.ts`), ricerca, anche come finestra separata (`e2e/search.spec.ts`), duplicati e analisi del contenuto (`e2e/duplicates.spec.ts`), backup (`e2e/backup.spec.ts`) (Playwright). | `npm test`, `npm run e2e` |
| Proprietà con riferimenti | `crates/prism-core/tests/reconcile_refs.rs` | File system in memoria con riferimenti stabili ai file (`tests/common/fakefs.rs`, come NTFS letto dalla MFT): rinomine, spostamenti, scambi di nomi, sostituzioni, aggiornamenti parziali; verifica F05-AC3 e che ogni file conservi il proprio id (F05-AC4). | `cargo test` |
| Sicurezza dell'app | `crates/prism-app/src/state_tests.rs` | Nelle build di debug un'eliminazione fuori dalla sandbox (PRISMTEST, `%TEMP%`, `%LOCALAPPDATA%\PrismDev`) è rifiutata prima di toccare il disco; la release non ha guardia (istruzione S1). | `cargo test` |
| Ricerca | `crates/prism-core/src/search` | Indice (sottostringa senza maiuscole, F07-AC2), linguaggio dei filtri con errori e posizioni (F07-AC5/AC6, date locali e relative, unità di misura), motore su due volumi uno dei quali offline (F07-AC4), ordinamento, conteggio limitato, disco sconosciuto come errore, indici che seguono il catalogo. | `cargo test` |
| Backup | `crates/prism-core/tests/backup.rs`, `src/backup/` | Verifica contro destinazioni con e senza hash, file rinominati, stessi metadati ma contenuto diverso, destinazioni non più presenti sul disco (F13-AC1/AC2/AC3/AC4); confronto per percorso e per contenuto, file spostati, albero con totali (F14-AC1/AC2/AC3); copia con struttura ricreata, conflitti saltati o rinominati, data mantenuta, ripresa, annullamento a metà di un file da 48 MB senza file parziali, scritture fuori dalla sandbox rifiutate (F15-AC3/AC4/AC5). Nell'app (`state_tests.rs`): verifica, albero, anteprima, job di copia nella cartella temporanea, catalogo della destinazione aggiornato, nuova verifica senza mancanti, copia nella sorgente rifiutata, copia fuori sandbox rifiutata in debug. | `cargo test` |
| Installer | `scripts/build-installer.ps1`, `crates/prism-app/windows/installer-hooks.nsh` | L'installer si costruisce (NSIS di Tauri) e lo script generato contiene gli agganci (copia di `prism-shell`, `--install`, `--uninstall`) e la casella per tenere o cancellare il catalogo; `prism-shell --install/--uninstall` usa le funzioni già provate (voci del menu, attività). **Non** si esegue sulla macchina di sviluppo: installazione, avvio con Windows, menu e disinstallazione si provano su un PC pulito (REVIEW U3). | `scripts/build-installer.ps1` |
| Aggiornamenti | `crates/prism-app/src/updates.rs`, `ui/src/components/SettingsView.svelte`, `scripts/build-installer.ps1` | Unità: una build controlla solo con chiave e indirizzo `https`. e2e: la versione pronta si installa solo dopo la conferma, *Annulla* non fa nulla. App di sviluppo reale: senza chiavi le impostazioni lo dicono e nessun controllo parte. `build-installer.ps1` con una chiave di prova (nella cartella temporanea) produce la firma e un `latest.json` valido. La verifica della firma è del plugin di Tauri; l'aggiornamento completo si prova con le chiavi vere (REVIEW U7, checklist T10). | `cargo test -p prism-app`, `npx playwright test e2e/settings.spec.ts` |
| Menu di Esplora risorse | `crates/prism-win/src/pipe.rs`, `src/shell_menu.rs`, `crates/prism-shell`, `prism-app/src/shell_requests.rs` | Pipe: richiesta e risposta, secondo client, attesa limitata senza server; voci del menu scritte e tolte su una chiave di prova di `HKCU` (mai quella vera); riga di comando di `prism-shell`; percorsi trovati nel catalogo (maiuscole, cartella non ancora indicizzata, fuori catalogo). E2e: `e2e/shell.spec.ts` (F18-AC1). | `cargo test`, `npm run e2e` |
| Salute dei dischi | `crates/prism-core/src/health.rs`, `tests/health_history.rs`, `crates/prism-win/src/health.rs`, `prism-app/src/state_tests.rs` | Interpretazione di pagine NVMe e ATA sintetiche (buono, usurato, errori, guasto, settori riallocati e in attesa, usura degli SSD), peggioramenti (F17-AC1/AC3); storia salvata, letta dalla più recente, potata (F17-AC2); descrittore e comando SMART costruiti byte per byte; lettura in sola lettura dei dischi di questa macchina, con il motivo quando non c'è (F17-AC4); nell'app una lettura salvata una volta sola se nulla cambia; testi della notifica. E2e: salute in `e2e/catalog.spec.ts`. | `cargo test`, `npm run e2e` |
| Diagnostica e reset | `crates/prism-core/src/diagnostics.rs`, `tests/catalog_reset.rs`, `prism-app/src/state_tests.rs` | CRC-32 e date DOS, archivio ZIP riletto voce per voce, elenco dei log (F20-AC2); pacchetto mostrato prima di scriverlo, mai sopra un file esistente; reset: volumi, voci, hash spariti, impostazioni e registro delle azioni rimasti, eventi per gli indici, file intatti (F21-AC4). E2e: `e2e/settings.spec.ts`. Identità ambigue: due cloni nel catalogo, scelta di uno (riconosciuto da solo dopo) o di un disco nuovo, scelta non valida rifiutata (F02-AC3; e2e in `e2e/catalog.spec.ts`). | `cargo test`, `npm run e2e` |
| Video simili | `crates/prism-win/src/video.rs`, `crates/prism-core/tests/media.rs`, `src/media/` | Clip sintetiche codificate con gli encoder di Windows (H.264, WMV): la stessa clip ricodificata più piccola o in WMV resta vicina, un'altra no (F12-AC1/AC2); fotogramma a colori per le miniature; passata in background con un decodificatore finto (video non decodificabile segnato, `.ts` ignorati, pausa), gruppi per durata e distanza media (F12-AC1/AC3). Nell'app: impronte dei video dal job di background, gruppo con il video più grande per primo, miniatura. E2e: scheda *Video* in `e2e/similar.spec.ts`. | `cargo test`, `npm run e2e` |
| Immagini simili | `crates/prism-core/tests/media.rs`, `src/media/` | Impronte di immagini generate: ridimensionate, ricompresse, ritoccate restano vicine, un'immagine diversa no (F11-AC1); anteprime di un RAW in un contenitore TIFF e in uno CR3 (F11-AC4); passata in background su un volume con un file non decodificabile, gruppi, impronta invalidata quando il file cambia; BK-tree confrontato con il confronto di tutte le coppie (F11-AC2); miniature anche dei RAW. Nell'app: impronte calcolate dal job di background, gruppo con la più grande per prima, miniature solo dei dischi collegati (F11-AC3). E2e: `e2e/similar.spec.ts`. | `cargo test`, `npm run e2e` |
| Hash e duplicati | `crates/prism-core/tests/hashing.rs`, `tests/duplicates.rs`, `src/duplicates.rs` | Tre passate su file veri (stessa dimensione e stesso hash rapido ma contenuto diverso, file cambiati o spariti durante il calcolo, pausa), validità legata a dimensione e data (F06-AC3/AC4/AC5), gruppi e filtri (F10-AC1/AC2/AC5), regole di selezione (F10-AC3/AC4), regola dell'ultima copia e contenuto ricontrollato prima di eliminare (F16-AC3/AC4). Nell'app (`prism-app/src/state_tests.rs`): job di background in pausa, disco escluso, avanzamento sulla scheda, nessun blocco degli aggiornamenti; gruppi con la selezione delle regole, ultima copia rifiutata, eliminazione definitiva solo confermata (F06-AC5, F21-AC3, F16-AC2/AC4). Cessione del disco dei job di background (`jobs::tests`). | `cargo test` |
| Eliminazione sicura | `crates/prism-core/tests/fileops.rs` | Cartelle temporanee indicizzate, poi modificate: file cambiati o spariti saltati e segnalati, solo i percorsi verificati arrivano al `Remover` (che elimina tramite la guardia sandbox), catalogo e registro delle azioni aggiornati, guardia che rifiuta tutto prima di toccare qualcosa, voci di altri volumi rifiutate (F16-AC3/AC5/AC6). | `cargo test` |
| Proprietà del journal | `crates/prism-core/tests/journal_apply.rs` | Modifiche casuali sul file system simulato registrate come un journal (il file cambiato, mai la cartella madre), compreso Cestino escluso e ripristino: catalogo aggiornato = reindicizzazione, id conservati. Più uno stress di 3.000 semi da lanciare a mano (`-- --ignored`, release, ~6 minuti). | `cargo test` |
| Manuali | `docs/ROADMAP.md`, checklist di ogni tappa | Ciò che non si automatizza: aspetto grafico, UAC, collegamento fisico di dischi, installer su PC pulito. | Utente |
| Benchmark | `prism-cli bench`, `search-bench`, `mem-bench`, `replicate` | Tempi di indicizzazione, dimensione del catalogo, latenza di ricerca su dischi reali (sola lettura); memoria a regime con un catalogo grande (`replicate` aggiunge copie non collegate dei volumi, solo a cataloghi di sviluppo). Risultati in `docs/benchmarks/`. | Manuale/periodico |

## 3. Controlli a ogni commit

`scripts/check.ps1` esegue, e deve passare prima di ogni commit:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. (da T3) `npm run check`, `npm test` e `npm run e2e` in `ui/`. I test end-to-end
   (Playwright, `ui/e2e/`) aprono l'interfaccia con il backend simulato nel Chromium di
   Playwright, da scaricare una volta con `npx playwright install chromium --only-shell`
   (su questa macchina Edge non è installato; Chromium è anche il motore di WebView2).

La CI su GitHub Actions (`.github/workflows/ci.yml`, Windows) esegue gli stessi controlli. Si
attiva solo quando il branch viene pubblicato su GitHub.

### Note sui test d'integrazione (T1)

- Ogni modifica su disco degli helper (`tests/common/mod.rs`) passa da `GuardedFs`, con radice
  la cartella temporanea del test (regola S2); un test verifica che una giunzione verso
  l'esterno non permetta di uscire dalla sandbox.
- Negli helper dei test d'integrazione `unwrap`/`expect` sono consentiti con un `allow` a
  livello di crate di test (le regole li vietano solo nel codice di prodotto).
- **Cartelle illeggibili**: non si possono simulare con le ACL se il processo di test è
  elevato (qui un amministratore legge anche una cartella con DACL vuota: verificato il
  2026-11-07), né sulla CI. Il comportamento è coperto dal file system in memoria
  (`unreadable_folder_keeps_its_catalog_children`) e da un test unitario del walker sull'errore
  di lettura.

## 4. Test che richiedono l'amministratore

Non dare per scontato che la shell di sviluppo sia (o non sia) elevata: in T0 non lo era, nella
sessione T1 del 2026-11-07 sì (verificato aprendo `\\.\C:` con `runner_is_elevated`). I test che
richiedono l'amministratore si eseguono comunque sempre con il runner qui sotto, così il
risultato non dipende da come è stata aperta la shell. Per i test di MFT, USN e SMART si usa
questo schema, scelto per dare la massima autonomia allo sviluppo:

**Disco virtuale di test.** Un file VHDX dedicato (proposta: 4 GB, espansione dinamica) in
`%LOCALAPPDATA%\PrismDev\prism-test.vhdx`, formattato NTFS con etichetta `PRISMTEST`. I test lo
popolano con file sintetici, lo modificano e possono anche riformattarlo: contiene solo dati di
test. Anche le prove manuali distruttive dell'app (S5) si fanno qui.

**Runner amministratore.** Uno script di configurazione, da eseguire **una sola volta**
dall'utente in un PowerShell amministratore (`scripts/admin/install-admin-runner.ps1`), che:

1. crea e monta il disco virtuale di test (`diskpart` agisce solo sul file VHDX indicato per
   percorso, mai su un disco fisico);
2. registra un'attività pianificata `PrismDevAdminTests`, solo su richiesta e con i privilegi
   più elevati, che esegue `scripts/admin/run-admin-tests.ps1`.

Lo sviluppo può poi lanciare i test elevati con `schtasks /run /tn PrismDevAdminTests` e
leggerne l'esito senza intervento dell'utente:

- `%LOCALAPPDATA%\PrismDev\admin-tests\last.status` — `running`, `passed`, `failed <codice>`
  o `error`, con data e ora;
- `%LOCALAPPDATA%\PrismDev\admin-tests\last.log` — output completo di `cargo test`.

Il runner esegue `cargo test -p prism-win --features admin-tests`, passa ai test la radice del
volume di test in `PRISM_TEST_VOLUME` e compila in `target\admin`, separato dalla build normale.
I test di verifica del runner sono in `crates/prism-win/tests/admin_runner.rs`: da una shell non
elevata falliscono con "Accesso negato" (verificato), dal runner devono passare.

**Avvertenza di sicurezza (da accettare consapevolmente).** Finché l'attività esiste, qualunque
programma in esecuzione con l'utente può avviarla, e lo script e il codice compilato stanno in
cartelle scrivibili dall'utente: è di fatto un modo per eseguire codice come amministratore
senza UAC. Per questo:

- l'attività si rimuove con `scripts/admin/uninstall-admin-runner.ps1` a fine sviluppo, o prima
  se l'utente preferisce;
- l'alternativa è che l'utente esegua a mano `run-admin-tests.ps1` da un terminale
  amministratore quando richiesto (meno autonomia, nessun rischio aggiunto).

**Stato**: l'utente ha scelto l'attività pianificata; configurata e verificata il 2026-11-07
(volume PRISMTEST montato su F: al momento della configurazione). Il montaggio del VHDX non
sopravvive al riavvio: lo ricollega il runner, e la lettera di unità può cambiare, per questo i
test usano sempre `PRISM_TEST_VOLUME` e mai una lettera fissa.

## 4bis. Verifica dell'interfaccia sull'app vera

- Le build di **debug** usano il manifest `asInvoker`: partono dalla shell di sviluppo non
  elevata (senza MFT/USN, si usa il walker). Solo le build di **release** richiedono
  l'amministratore (ADR-0003).
- I flussi end-to-end si eseguono anche sull'app vera, pilotando WebView2 via WebDriver
  (`tauri-driver` + `msedgedriver`), oltre che nel browser con IPC simulato.
- Per i controlli visivi lo sviluppo cattura screenshot della finestra e li esamina; il giudizio
  estetico finale resta all'utente (revisione finale, `docs/REVIEW.md`).
- In T3 l'app vera è stata provata così (dalla shell elevata): screenshot della finestra
  (`PrintWindow`), clic sui pulsanti tramite UI Automation (WebView2 espone l'albero di
  accessibilità), indicizzazione di PRISMTEST con la MFT e aggiornamento automatico dal journal
  dopo la creazione di file sul disco di test; istanza singola e chiusura nel tray verificate
  con un secondo avvio e un `WM_CLOSE` (F19-AC1, F19-AC4).
- Interfaccia senza app: `npm run dev` in `ui/` apre l'interfaccia nel browser con il backend
  simulato; `npm test` esegue i test Vitest della logica di presentazione e del backend
  simulato (criteri F01/F02 citati nei nomi dei test).

## 5. Tracciabilità

- Nome dei test: descrittivo, con l'ID del criterio quando applicabile, ad esempio
  `f05_ac3_update_equals_full_reindex`.
- `docs/ROADMAP.md` elenca per ogni tappa i criteri coperti e quelli che restano manuali.
- I benchmark annotano macchina, disco, numero di voci e data in `docs/benchmarks/`.
