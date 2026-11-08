# Prism — Revisione finale dell'utente

> Guida per l'unica revisione dell'utente, la sera del 2026-11-07 (CLAUDE.md, "Autonomia
> totale"). Lo sviluppo procede senza soste e registra qui ogni decisione presa senza l'utente,
> con le alternative e il modo di cambiarla. Chi aggiunge una voce la mette in fondo alla
> tabella giusta, con data e tappa.

## 0. In breve

Tutte le funzioni della SPEC (F01–F22) sono implementate. Ogni criterio ha test automatici
(`scripts/check.ps1`, CI su GitHub, test elevati sul disco virtuale PRISMTEST) o una voce delle
checklist di §4 (NF8); molte sono state provate anche sull'app di sviluppo. L'installazione,
gli aggiornamenti reali e i dischi fisici restano da provare (§2). Le decisioni prese senza di
te sono in §1 (D1–D50), i limiti noti in §3. Restano le cose che solo tu puoi fare (§2), in
quest'ordine:

1. **Provare l'app** di sviluppo seguendo §4 (avvio, checklist T11 della nuova interfaccia,
   poi T3–T9): è il momento per cambiare una decisione di §1 o segnalare quello che non va
   (U4, U5). La nuova interfaccia (T11, D51, D52) è stata disegnata con te il 2026-11-08.
2. **Aggiornamenti e firma**: creare la coppia di chiavi degli aggiornamenti e scegliere dove
   pubblicare le versioni (U7); scegliere il certificato di firma del codice (U1). Entrambi si
   configurano senza toccare il codice.
3. **Installer su un PC pulito** (U3): `scripts\build-installer.ps1`, poi la checklist T10.
4. **Merge di `rewrite` sul ramo principale e rilascio 1.0** (U2), e rimozione del runner
   amministratore da questa macchina (U6).

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
| D20 | 2026-11-07 | T4 | Treemap sulla dimensione **logica** dei file (non sullo spazio allocato), con le cartelle più grandi espanse per prime a qualunque profondità, soglia 0,002 % e al massimo 12.000 nodi; gli elementi più piccoli raggruppati in un blocco grigio | Dimensione allocata; livelli fissi (provato: le cartelle profonde restavano vuote) | `TreemapOptions` in `prism-core::space` | ARCHITECTURE §4.6 |
| D21 | 2026-11-07 | T4 | Nove categorie di file (immagini, video, audio, documenti, archivi, immagini disco, codice, programmi, altro) con la palette categoriale validata per daltonismo in entrambi i temi; `.ts` conta come video (i flussi video pesano più dei sorgenti TypeScript) | Colori per estensione; categorie configurabili | `category_of` in `prism-core::space`, `--cat-*` in `ui/src/app.css` | F08-AC3 |
| D22 | 2026-11-07 | T4 | La schermata *Spazio* si apre dall'icona sulla scheda di un disco indicizzato (nessuna voce nel menu laterale); le statistiche dell'intero catalogo sono una scelta dentro *Tipi di file* | Voce *Spazio* nel menu con scelta del disco | `ui/src/App.svelte` | F01-AC4 |
| D23 | 2026-11-07 | T4 | Eliminazione: Cestino solo sui volumi fissi locali; su chiavette e dischi rimovibili (Windows non ha il Cestino) e su unità di rete eliminazione definitiva solo con conferma esplicita. Se un elemento non entra nel Cestino (troppo grande) Windows chiede all'utente invece di eliminarlo in silenzio. Verifica prima di agire: dimensione e data di modifica identiche al catalogo | Cestino ovunque (su chiavette diventerebbe definitivo senza avviso); verifica con tolleranza sulle date | `prism-core::fileops`, `prism-win::trash` | ADR-0012 |
| D24 | 2026-11-07 | T4 | Regole prudenti della pulizia: temporanei solo se più vecchi di un giorno; cartelle vuote e file grandi e vecchi mai in Windows, programmi, `ProgramData`, `AppData`, `inetpub`, cartelle dei profili, elementi nascosti/di sistema, nomi con `.` o `$`, collegamenti; grandi e vecchi = almeno 500 MB e non modificati da un anno; cache solo di Chrome, Edge, Brave e Firefox, a browser chiuso | Temporanei senza limite d'età; soglie diverse; più browser e app | `DEFAULT_PROTECTED` e `CleanupRules` in `prism-core::cleanup`; soglie configurabili da aggiungere alle impostazioni | SPEC F09 |
| D25 | 2026-11-07 | T4 | Cartelle di programmi riconosciute anche fuori da `Program Files`: una cartella che contiene direttamente una `.dll` (giochi, Steam, app portatili) e tutto ciò che ha sotto non vengono proposti come cartelle vuote né come file grandi e vecchi. Trovato sui dati reali: Steam in `D:\Programmi` proponeva `steamapps\downloading` e simili | Basarsi solo sui nomi delle cartelle; considerare anche le `.exe` (escluderebbe `Download` con un programma d'installazione) | `ProgramFolders` in `prism-core::cleanup` | F09-AC2 |
| D26 | 2026-11-07 | T5 | Indice di ricerca **in memoria** (stile Everything) invece di FTS5 trigram: 60 B per voce in RAM, primi risultati in ≤ 3 ms su 5,5 milioni di voci, nessun costo su disco né in indicizzazione, ricerca anche con 1–2 caratteri | FTS5 trigram (17,5 s di costruzione su C:, +127 B per voce su disco, niente ricerche sotto i 3 caratteri) | `prism-core::search` | ADR-0010 |
| D27 | 2026-11-07 | T5 | Dettagli del linguaggio di ricerca: `size:1GB` senza segno = "almeno"; `>`/`<` su una data = dopo/prima di quel giorno locale, `2024` o `2024-03` = l'anno o il mese; `modified:<30d` = più recenti di 30 giorni, `>1y` = più vecchi; unità decimali (KB, MB, GB) e binarie (KiB…); `d:` = l'unità D:; alias italiani (`tipo`, `disco`, `percorso`, `modificato`, `documenti`…); ordinamento per pertinenza e conteggio fino a 10.000 | Semantica di Everything (`dm:`, `size:` con uguaglianza); solo chiavi inglesi | `prism-core::search::query` ed `engine` | SPEC F07-AC5 |
| D28 | 2026-11-07 | T5 | Ricerca in una finestra separata sempre in primo piano (nascosta con Esc o dopo aver aperto un risultato) e anche come voce del menu laterale; 200 risultati migliori; menu sotto la casella che scrivono i filtri nel testo; la scorciatoia rifiutata lascia attiva la precedente | Solo una pagina nella finestra principale; più risultati con lista virtualizzata | `prism-app/src/search_window.rs`, `ui/src/components/SearchView.svelte` | SPEC F07 |
| D29 | 2026-11-07 | T6 | Hash in tre passate: rapido (dimensione + 64 KiB iniziali e finali) dei file con dimensione condivisa, completo dei possibili duplicati, poi completo di tutto il resto dal file più piccolo; un file cambiato durante la lettura non riceve l'hash. Regole di selezione: le "tieni" restringono in ordine (una regola che non trova nulla si salta), a parità resta la prima copia; le copie su dischi scollegati non si selezionano mai | Solo hash completi (molte più letture); regole come filtri indipendenti | `prism-core::hashing`, `prism-core::duplicates` | ADR-0011, F10-AC3 |
| D30 | 2026-11-07 | T6 | **Superata da D58.** Le unità di rete non si analizzano in background: ogni byte attraverserebbe la rete per ore e il NAS potrebbe essere condiviso. Le copie sul NAS non entrano quindi nei gruppi di duplicati (L13) | Analizzarle a bassa priorità come i dischi locali | `prism-app/src/hash_jobs.rs` | F06-AC1 |
| D31 | 2026-11-07 | T6 | Un job di background cede il disco: ogni mezzo secondo chiede al motore se un job più urgente aspetta uno dei suoi dischi e, se sì, si ferma tra due file; il giro successivo (dopo almeno 2 minuti) riparte dai file senza hash. Gli aggiornamenti dal journal e dalla sorveglianza non aspettano gli hash; aggiornare o rimuovere un disco li ferma. L'aggiornamento dal journal (pochi record ogni 15 s) non riserva il disco: riservandolo, l'analisi si fermava a ogni giro (visto nell'app vera su D:, fermata dopo 293 file). Nessun record persistente per riprendere: l'avanzamento è la tabella degli hash | Priorità più bassa senza cessione (un'indicizzazione aspetterebbe la fine di un'analisi di ore); record dei job su disco | `JobEngine::should_yield`, `AppState::foreground_jobs` | ADR-0009, F06-AC2 |
| D32 | 2026-11-07 | T6 | Schermata *Duplicati*: i 300 gruppi con più spazio recuperabile e il totale di tutti; filtro predefinito ≥ 1 MB; regola predefinita "tieni la più vecchia"; le regole si applicano con un pulsante e poi ogni casella si cambia a mano entro i limiti di F10-AC4; una sola conferma, che dice se qualche copia sta su un disco senza Cestino (eliminazione definitiva); una seconda conferma se resterebbero solo copie su dischi scollegati. Nell'interfaccia si parla di "analisi del contenuto", non di hash | Lista completa paginata; nessuna regola predefinita; una conferma per disco | `duplicates_views.rs`, `DuplicatesView.svelte` | F10, F16-AC4 |
| D33 | 2026-11-07 | T6 | Avanzamento dell'analisi sulle schede come percentuale arrotondata per difetto (100% solo a lavoro finito), contato ogni 5 minuti (ogni minuto mentre gira); nel tray una riga con il numero di job in corso e la voce Sospendi/Riprendi, che salva l'impostazione; in *Impostazioni* pausa, batteria e dischi da analizzare (le unità di rete non compaiono) | Elenco dei job nel menu del tray; conteggio continuo | `hash_jobs.rs`, `tray.rs`, `SettingsView.svelte` | F06-AC5, F19-AC2, F21-AC3 |
| D34 | 2026-11-07 | T7 | Verifica dei backup: una copia vale per contenuto (stessa dimensione e hash) ovunque si trovi nelle destinazioni, anche rinominata; senza hash su uno dei due lati vale per nome, dimensione e data con 2 s di tolleranza (FAT), anche in un'altra cartella; con hash su entrambi i lati e contenuti diversi il file risulta mancante. Il confronto per contenuto abbina prima i percorsi uguali identici, poi lo stesso hash altrove, poi nome/dimensione/data | Abbinare solo per percorso relativo (un backup riorganizzato risulterebbe tutto mancante); fidarsi dei metadati anche quando gli hash dicono il contrario | `prism-core::backup` | F13-AC3, F14-AC2 |
| D35 | 2026-11-07 | T7 | Copia: file temporaneo `<nome>.prism-part`, riletto e confrontato con l'hash della sorgente prima di prendere il nome vero; data di modifica mantenuta; la ripresa riconosce i file già copiati dal contenuto; in conflitto *salta* o *rinomina* come `nome (2).ext`; un `.prism-part` rimasto da un'interruzione si sostituisce (è di Prism) | Scrivere direttamente con il nome finale; riprendere da un registro su disco | `prism-core::backup::copy` | F15-AC4/AC5 |
| D36 | 2026-11-07 | T7 | Schermata *Backup* con due schede (verifica, confronto); sorgente e destinazioni scelte come disco + percorso scritto (vuoto = disco intero), anche dischi scollegati; verifica e confronto girano subito nel catalogo (secondi anche su dischi grandi) e non come job annullabili; gli ultimi 4 risultati restano in memoria e si sfogliano a livelli; la copia propone come destinazione un disco di backup collegato o un altro disco collegato, sotto una cartella scritta dall'utente (niente selettore di cartelle di Windows); nel confronto gli identici sono nascosti all'inizio | Selettore di cartelle nativo (plugin in più); job annullabili anche per verifica e confronto; risultati salvati su disco | `BackupView.svelte`, `backup_state.rs` | F13, F14, F15-AC3 |
| D37 | 2026-11-07 | T8 | Immagini simili con *difference hash* a 64 bit (robusto a ridimensionamento e ricompressione, misurato nei test entro 6 bit) e BK-tree per la distanza di Hamming; RAW dall'anteprima JPEG incorporata, la più piccola con lato corto ≥ 400 px (decodificare l'anteprima a piena risoluzione costa molto più e l'impronta non ne ha bisogno); file oltre 512 MB non decodificati; un file non decodificabile non si riprova finché non cambia. Nuova dipendenza `image` (solo i formati della SPEC). HEIC/AVIF esclusi come da SPEC | pHash (DCT, più costoso), *multi-index hashing* (più veloce con soglie fisse ma più complesso); decodifica dei RAW | `prism-core::media` | F11 |
| D38 | 2026-11-07 | T8 | Schermata *Simili* nel menu laterale: soglia da 0 a 16 differenze su 64 (predefinita 6, entro cui stanno nei test ridimensionamenti, ricompressioni e piccoli ritocchi), filtro per disco, 200 gruppi con più immagini, 30 per volta; miniature caricate solo quando entrano nella vista e solo dei dischi collegati (per quelli scollegati restano nome, risoluzione, dimensione e posizione); nessuna eliminazione da questa schermata: le immagini simili non sono copie identiche e la regola dell'ultima copia non le protegge | Miniature salvate nel catalogo anche per i dischi scollegati (centinaia di MB); eliminazione anche dai simili | `SimilarView.svelte`, `similar_state.rs` | F11-AC1/AC3 |
| D39 | 2026-11-07 | T8 | Video: decodifica con **Media Foundation** (ADR-0013), senza ffmpeg: nessuna licenza né decine di MB nell'installer, tutti i video reali provati (MP4 H.264, WebM) decodificati; uscita NV12 e sola luminanza (8 volte più veloce di RGB32); 5 fotogrammi al tempo esatto (10–90 %) | ffmpeg incluso (più formati, HEVC senza estensione); fotogramma chiave soltanto (più veloce ma impronte meno stabili) | `prism-win::video`, ADR-0013 | F12-AC1/AC2 |
| D40 | 2026-11-07 | T8 | Video simili: impronta di 5 fotogrammi (10–90 %), simili se le durate differiscono al massimo di 2 s o del 3 % e la distanza media per fotogramma è entro la soglia (lo stesso cursore delle immagini, predefinito 6; le ricodifiche misurate stanno sotto 1); scheda *Video* nella schermata *Simili*; miniatura dal fotogramma a metà; i `.ts` non si considerano video (su un disco di sviluppo sono quasi tutti TypeScript); le impronte si calcolano dopo hash e immagini nello stesso job | Indice per la distanza anche sui video (pochi video, il filtro per durata basta); più fotogrammi; i `.ts` inclusi | `prism-core::media::video`, `SimilarView.svelte` | F12 |
| D41 | 2026-11-07 | T9 | Log: un file al giorno in `logs` nella cartella dati, 14 tenuti (`tracing-appender`, unica dipendenza nuova). Pacchetto diagnostico: `system.txt` più i log in uno ZIP non compresso scritto da Prism (niente libreria ZIP), mostrato prima di salvarlo, salvato in Download (in sviluppo nella cartella dati, sandbox) e poi indicato in Esplora risorse | Crate `zip` con compressione; scelta della cartella con un dialogo; log nel registro eventi di Windows | `prism-core::diagnostics`, `diagnostics_state.rs` | F20 |
| D42 | 2026-11-07 | T9 | Reset del catalogo: richiede una casella "ho capito" oltre al pulsante; ferma i job, toglie volumi, voci, hash e impronte; restano impostazioni e registro delle azioni (le esclusioni per disco si azzerano perché riguardano dischi tolti); i dischi collegati vengono riproposti come nuovi | Conservare il registro delle azioni? Sì: è la storia di ciò che Prism ha fatto ai file; scrivere il nome del catalogo per confermare | `store::reset_catalog`, `SettingsView.svelte` | F21-AC4 |
| D43 | 2026-11-07 | T9 | Identità ambigua: un riquadro per disco nella schermata *Dischi* con i dischi del catalogo che potrebbe essere (nome, posizione, capacità, ultima volta visto) e "È un altro disco"; la scelta si applica al rilevamento successivo (entro 2 s) e aggiorna il disco scelto con il GUID di quello collegato, così dopo viene riconosciuto da solo; una scelta per un disco già assegnato a un altro collegato si ripropone | Finestra modale; ricordare la scelta per sempre senza aggiornare il catalogo | `prism-app/src/identity.rs`, `monitor::apply_detection`, `CatalogView.svelte` | F02-AC3 |
| D44 | 2026-11-07 | T9 | Salute dei dischi senza librerie né WMI: IOCTL di Windows (pagina di log NVMe, attributi e soglie SMART ATA, stato SMART). Si mandano solo comandi di lettura; per i dischi ATA l'handle si apre con accesso in lettura e scrittura perché gli IOCTL SMART lo richiedono, senza mai scrivere. Stato: critico se il disco lo segnala (avviso critico NVMe, soglia superata, SMART "in guasto", vita esaurita, riserva sotto soglia), attenzione per settori riallocati o in attesa, errori sui dati, usura ≥ 90 %, temperatura ≥ 65 °C. Lettura ogni ora, storia solo quando cambia qualcosa (non la temperatura) o una volta al giorno, 400 giorni; notifica quando lo stato peggiora o crescono settori ed errori | WMI (`MSFT_StorageReliabilityCounter`, meno dati sugli HDD); smartmontools incluso (licenza GPL); passthrough SAT per i dischi USB (rischioso con alcuni adattatori) | `prism-core::health`, `prism-win::health`, `health_state.rs` | F17 |
| D45 | 2026-11-07 | T9 | Menu di Esplora risorse: `prism-shell.exe` non elevato manda la richiesta a Prism con una named pipe per utente (accesso esplicito all'utente stesso, integrità media, client remoti rifiutati); se Prism non è avviato lo avvia con l'attività pianificata `Prism`, che ora resta sempre registrata nelle build di rilascio e ha solo il trigger di accesso legato all'opzione "Avvia con Windows". Voci: *Analizza con Prism* e *Cerca duplicati qui* su cartelle, unità e sfondo di una cartella, registrate dall'installer per l'utente (`HKCU`) | Due attività (accesso e su richiesta); messaggi finestra o file di richiesta al posto della pipe; voci in `HKLM` per tutti gli utenti | `prism-win::pipe`, `shell_menu`, `crates/prism-shell`, `autostart::apply` | F18, ADR-0003 |
| D46 | 2026-11-07 | T10 | Installer NSIS di Tauri per tutta la macchina (serve l'elevazione per l'attività con i privilegi più alti, che comunque Prism chiede), italiano e inglese, WebView2 con il bootstrapper; `prism-shell.exe` aggiunto da un aggancio NSIS (non come risorsa di Tauri: renderebbe obbligatorio compilarlo prima di ogni `cargo build`) e chiamato con `--install`/`--uninstall`; la scelta di tenere il catalogo è la casella "Elimina i dati dell'applicazione" della disinstallazione di Tauri | MSI (WiX); installazione per utente (niente attività elevata); finestra di domanda propria per il catalogo | `tauri.conf.json`, `windows/installer-hooks.nsh`, `scripts/build-installer.ps1` | F22-AC1/AC2 |
| D47 | 2026-11-07 | T10 | **Per chiave e indirizzo, superata da D57.** Aggiornamenti con `tauri-plugin-updater`: chiave pubblica e indirizzo del manifest fissati alla compilazione (`PRISM_UPDATE_PUBKEY`, `PRISM_UPDATE_URL`) e non in `tauri.conf.json`, così finché l'utente non sceglie chiavi e luogo di pubblicazione nessuna build si collega; controllo un minuto dopo l'avvio e poi ogni 24 ore, download in sottofondo, installazione solo dopo conferma con riavvio; `latest.json` scritto e firmato da `build-installer.ps1`, pensato per GitHub Releases | Chiave nel file di configurazione (una chiave di prova finirebbe nel repository); installazione automatica alla chiusura senza chiedere; un servizio di aggiornamento proprio | `prism-app::updates`, `SettingsView.svelte`, `scripts/build-installer.ps1` | F22-AC3, NF6 |
| D48 | 2026-11-07 | T10 | L'analisi del contenuto tiene in memoria solo gli id dei file ancora da fare (ordinati in SQL nella prima passata; id, dimensione e "possibile duplicato" nella seconda) e rilegge ogni file dal catalogo quando tocca a lui; cache dei percorsi limitata a 4096 cartelle. Misurato: ~12 MiB invece di ~190 su D: (629.000 file), e con 5,7 milioni di voci l'app resta a ~325 MiB (NF3) | Struttura compatta per file (~72 B, comunque lineare nel numero di file); passate guidate da query a pagine (transazioni di lettura lunghe, il WAL non si svuoterebbe per ore) | `prism-core::hashing`, `store::hashes` | NF3, F06-AC4; `docs/benchmarks/2026-11-07-t10-memory.md` |
| D49 | 2026-11-07 | T10 | Istantanea `ScanTree` compatta: 64 byte per voce (metadati con valori riservati al posto degli `Option`, stato della cartella in un byte) e nomi in un unico buffer; i lettori vedono `ScanNode` per valore con gli stessi campi; il lettore MFT alloca l'istantanea una volta sola. Picco dell'indicizzazione di C: (1,5 milioni di voci) −22% con la MFT, −28% con il walker | Scrivere nel catalogo durante la lettura della MFT senza istantanea (cambia il riconciliatore e l'interfaccia degli enumeratori: dopo la 1.0) | `prism-core::scan`, `prism-win::mft` | REVIEW L1, `docs/benchmarks/2026-11-07-t10-memory.md` |
| D50 | 2026-11-07 | T10 | Aggiornamento dal journal con elenchi **parziali**: per ogni cartella toccata solo i file cambiati che ora sono lì, e il riconciliatore confronta solo le voci che quei file avevano nella cartella e i nomi occupati (nuovo `Listing::Partial`, `reconcile_changes`); la mappa dei riferimenti si carica una volta e passa dal pianificatore al riconciliatore. Su C: (cartella Temp con 115.792 file che cambia a ogni giro) un giro passa da ~0,7 s a ~0,32 s | Elenco completo della cartella (costo proporzionale alla cartella); ricerche per riferimento con un indice SQL (tolto in T2: inserimenti 5× più lenti, D6); mappa tenuta in memoria tra un giro e l'altro (~40 B per voce per sempre, contro NF3) | `prism-core::changes`, `reconcile`, `scan` | REVIEW L4, F05-AC3 |
| D51 | 2026-11-08 | T11 | **Decisa con l'utente.** Nuova interfaccia "B+C": niente menu laterale, barra in alto con la ricerca sempre visibile, tutto parte dal disco (schermata del disco con le schede Spazio, Duplicati, Simili, Backup, Pulizia, Salute, Tipi di file) o dal blocco "Tutto il catalogo"; base chiara con dischi come oggetti fisici (colore, posizione), dati in carattere a larghezza fissa, accento arancione; dischi nuovi in un riquadro a comparsa. Sostituisce l'aspetto e la navigazione di D14, D22, D28 | Stili A "Grafite" e B o C da soli; aspetto Windows 11 (escluso dall'utente) | ADR-0019 | ADR-0019 |
| D52 | 2026-11-08 | T11 | **Decisa con l'utente.** Icona "prisma a strati": riquadro nero, prisma con la punta bianca e strati arancione, blu e verde. La vecchia ricordava una copertina di album famosa | Le altre sette proposte sulla tela | `crates/prism-app/icons/` (generate da `scripts/make-icons.py`) | ADR-0019 |
| D53 | 2026-11-08 | T11 | **Su segnalazione dell'utente** ("dice sempre 99%, sembra rotto"). *Attività in corso*: per ogni job la fase che riporta (lettura del disco, lettura rapida o completa del contenuto, impronte di immagini e video, copia), quanti elementi su quanti, da quanto tempo e una stima del tempo che manca (misurata dalla velocità della fase in corso, dopo 15 s); i dischi con file ancora da analizzare e nessun job dicono "in attesa del prossimo giro". Prima mostrava il conteggio dei file analizzati, aggiornato ogni 5 minuti, mentre il job era già alle impronte | Una stima calcolata nel backend; nascondere i dischi in attesa | `ui/src/lib/activity.ts` | ADR-0019 |
| D54 | 2026-11-08 | T11 | Dopo un'indicizzazione o un aggiornamento l'avanzamento dell'analisi del contenuto si riconta al giro successivo (30 s) invece che fino a 5 minuti dopo: i file nuovi si analizzano subito (visto sull'app vera: un duplicato appena copiato compariva dopo 5 minuti) | Contare a ogni giro (costo su dischi grandi) | `hash_jobs::tick` | F06-AC1 |
| D55 | 2026-11-08 | T11 | Le condivisioni di rete del catalogo si cercano anche dal percorso di rete quando nessuna lettera le punta (mappatura non ripristinata all'accesso, o vista solo da un'altra sessione: Prism di rilascio gira elevato): una condivisione è collegata quando il server risponde, con o senza lettera; la scheda mostra il nome della condivisione. Il filtro *Rete* tiene le condivisioni anche quando il server non risponde ("Non raggiungibile", non "Nel cassetto"). Trovato con il NAS dell'utente acceso e le lettere non mappate | Solo le lettere mappate (ADR-0016 iniziale) | `monitor::known_shares`, `catalog.ts` (`driveLetter`, `offlineState`) | ADR-0016, F01-AC2 |
| D56 | 2026-11-08 | T11 | **Su segnalazione dell'utente.** Finestra minima 960 × 640 (prima 760 × 520, troppo stretta): barra in alto, barra di stato e barra d'azione restano su una riga (sotto 1120 px spariscono l'indicazione della scorciatoia e la riga dell'analisi nella barra di stato); i numeri delle intestazioni si rimpiccioliscono; le tabelle accanto alla mappa hanno colonne fisse e i nomi lunghi si accorciano | Layout a una colonna sotto una certa larghezza | `tauri.conf.json`, `app.css`, `TopBar.svelte`, `SpaceView.svelte`; test `ui/e2e/layout.spec.ts` | ADR-0019 |
| D57 | 2026-11-08 | Rilascio | **Scelta dell'utente: aggiornamenti con lo stesso schema di Orbit.** Chiave pubblica e indirizzo (`latest.json` dell'ultima release GitHub) in `tauri.conf.json`, controllo solo nelle build di release; chiave privata con password in `~\.tauri\prism-updater.key` e nei secret dell'environment `release` (solo `develop`, approvazione dell'utente), creata da `scripts/new-update-key.ps1`; release da GitHub Actions (`release.yml`, prova a secco o pubblicazione) o in locale con `build-installer.ps1`; note in `docs/releases/<versione>.md`. Niente firma del codice, come Orbit (U1) | Chiave e indirizzo dati alla compilazione con variabili d'ambiente (lo schema precedente); release solo in locale | `updates.rs`, `tauri.conf.json`, `scripts/build-installer.ps1`, `scripts/new-update-key.ps1`, `.github/workflows/release.yml`; test `updates::tests` | F22-AC3, ARCHITECTURE §8.1 |
| D58 | 2026-11-08 | Rilascio | **Su domanda dell'utente ("perché non analizziamo le cartelle di rete?").** Anche le unità di rete si analizzano in background, come i dischi locali: priorità più bassa, pausa e batteria uguali, escludibili una per una nelle impostazioni (ora compaiono nell'elenco). I job sulle unità di rete occupano la risorsa `net:<server>`: le cartelle di uno stesso NAS si leggono una alla volta. Costo accettato: la terza passata (hash completo di tutti i file, per i backup) fa passare dalla rete tutto il contenuto, circa 3 ore per TB via cavo | Lasciarle fuori (D30); analisi solo su richiesta | `hash_jobs.rs`, `state::job_disks`, `SettingsView.svelte`; test `state_tests::background_hashing_runs_pauses_and_counts`, e2e F06-AC5 | F06, F11, F21-AC3; risolve L13 |
| D59 | 2026-11-08 | Rilascio | **Su segnalazione dell'utente** (il NAS è passato da 192.168.1.65 a .57 dopo un riavvio e le sue cartelle risultavano non raggiungibili). Una cartella di rete a un nuovo indirizzo si ritrova in due modi. **Automatico**: Prism impara e salva il nome del server raggiunto per IP (`share_servers`, schema 9); se la cartella non risponde più cerca il nome e la prova ai nuovi indirizzi; una cartella con il nome di una del catalogo trovata a un altro server (così o mappata con una lettera) diventa ambigua e l'utente conferma. **A mano**: *Cambia indirizzo di rete…* nella scheda del disco, con il controllo che il percorso risponda e di quante voci principali del catalogo ci sono. Consigliata comunque una prenotazione DHCP sul router (fatta dall'utente per il suo NAS) | Riconoscere la cartella da sola senza chiedere; identificarla con un numero di serie del volume (Samba non ne dà uno stabile) | `netnames.rs`, `monitor::moved_shares`, `volumes::match_volumes`, `identity.rs`, `shares.rs`, `prism_win::net`, `ShareAddressDialog.svelte`; test `volumes::tests`, `netnames::tests`, `monitor::tests`, `state_tests::a_share_moves_to_a_new_path_only_when_it_answers`, e2e D59 | F02, ADR-0016 |
| D60 | 2026-11-08 | Rilascio | **Su domanda dell'utente sul multithreading ("misura, poi decido io").** Misurato (docs/benchmarks/2026-11-08-prints-threads.md): la decodifica delle foto scala quasi linearmente (×3,8 con 4 thread, ×7 con 8). Le impronte delle immagini usano un quarto dei core, da 1 a 4 thread, ciascuno in modalità background; gli hash (limitati dal disco) e le impronte dei video (Media Foundation) restano su un thread. Già paralleli prima: fino a 4 job su dischi diversi, il walker (fino a 8 thread), la ricerca | Lasciare tutto su un thread; usare tutti i core | `media::{Workers, print_workers, run_pass}`, `hash_jobs.rs`, `prism-cli print-bench`; test `tests/media.rs` (3 thread) | F11, NF1 |
| D61 | 2026-11-08 | Rilascio | **Su segnalazione dell'utente** ("che significa in attesa del prossimo giro?"). Il riquadro *Attività in corso* dice perché l'analisi di un disco con file ancora da leggere non sta girando: in pausa, un'altra attività sul disco, file in uso o illeggibili nell'ultimo giro ("6 file in uso o illeggibili · li riprovo più tardi"), oppure "riparte tra poco"; i dischi esclusi non compaiono | Un testo unico "in attesa del prossimo giro" | `hash_jobs::Waiting`, `dto::HashesView`, `activity.ts`, `CatalogView.svelte`; test `activity.test.ts`, `state_tests` | F06-AC5 |
| D63 | 2026-11-08 | Rilascio | **Scelta dell'utente** ("ha senso scrivere unità di rete? non è meglio un'icona?"). Una cartella di rete senza nome scelto dall'utente si chiama con il nome della cartella (*Big-Storage*, non *Unità di rete (Big-Storage)*); al posto della lettera c'è l'icona di rete, con la lettera accanto se la cartella è mappata; sopra il nome "NAS · server", con il nome imparato del server (D59) o il suo indirizzo. Senza mappatura una lettera non esiste: Prism raggiunge la cartella con il percorso di rete | "Nome + server in chiaro"; lasciare il testo | `catalog.ts` (`displayName`, `whereLabel`), `DiskCard.svelte`, `DiskView.svelte`, `CatalogView.svelte`; test `home.test.ts`, e2e D63/D64 | F01, ADR-0019 |
| D64 | 2026-11-08 | Rilascio | **Scelta dell'utente** ("esiste un'indicazione di quali unità appartengono allo stesso disco?", sul suo PC C:, D: ed E: sono tre partizioni di un Samsung 990 PRO 2TB). Le schede, e le righe della vista Elenco, si raggruppano sotto un'intestazione per disco fisico (modello · collegamento · numero di partizioni · salute del disco) o per server (NAS nome · indirizzo · numero di cartelle); un disco di cui non si conosce ancora il disco fisico sta da solo, sotto il suo tipo. I gruppi si affiancano sulla stessa riga quando ci stanno, ciascuno largo quanto le sue schede (aggiunto dopo gli screenshot del README: un gruppo per riga lasciava vuota metà dello schermo). Il disco fisico ("modello\|seriale", come per la salute) si salva nel catalogo (`volumes.disk_key`, schema 10), così i gruppi restano a disco scollegato; la salute si rilegge appena si collega un disco, non solo una volta all'ora | Una riga sulla scheda; solo un ordinamento | `health_state::tick`, `monitor.rs`, `store::{volume_disks, set_volume_disk}`, `dto::VolumeView` (`group`, `disk_model`, `bus`, `server`, `server_name`), `catalog.ts` (`diskGroups`, `groupTitle`), `CatalogView.svelte`; test `state_tests::volumes_are_grouped_by_physical_disk_and_by_server`, `home.test.ts`, e2e D63/D64 | F01, F17, ADR-0019 |
| D62 | 2026-11-08 | Rilascio | **Sicurezza, trovato con D58.** Lo stesso file può avere due percorsi: un hard link su NTFS, o un NAS che mostra un suo disco in due condivisioni (ZimaOS-HD dell'utente contiene in `.media` Big-Storage e Small-Storage). Il catalogo vede due copie identiche; eliminarne "una" cancellerebbe l'unica. Prima di eliminare un duplicato Prism confronta sul disco l'identità di ogni copia da eliminare con quella delle copie che restano (collegate): stesso volume e stesso ID, oppure, tra condivisioni dello stesso server, stesso numero di file nei 32 bit bassi dell'ID. Sul NAS dell'utente la stessa foto vista dai due percorsi ha ID `0x0000000000085cc2` e `0x0000004000085cc2`. Se coincidono l'eliminazione è rifiutata prima di toccare qualunque file | Escludere `.media` per nome (vale solo per ZimaOS); confrontare solo l'ID completo (sul NAS non basta) | `prism_win::files`, `duplicates::kept_copies`, `AppState::check_same_files`; test `files::tests` (hard link reali e ID del NAS), `state_tests::one_file_under_two_paths_is_not_deleted_as_a_duplicate` | F16-AC4 |

## 2. Cose che richiedono l'utente

| # | Cosa | Perché serve l'utente | Cosa è già pronto |
|---|---|---|---|
| U1 | Firma del codice (SPEC Q7) | Scelta e acquisto del certificato | **Scelta dell'utente il 2026-11-08: come Orbit, la 2.0.0 esce senza firma** (le note di rilascio spiegano l'avviso di SmartScreen). Per aggiungerla più avanti basta impostare `bundle.windows.signCommand` (o `certificateThumbprint`) in `crates/prism-app/tauri.conf.json` e rieseguire `scripts/build-installer.ps1` (ARCHITECTURE §8.1) |
| U2 | Merge di `rewrite` sul ramo principale e rilascio (Q11) | Approvazione | **Merge su `develop` chiesto dall'utente il 2026-11-08** (PR da `rewrite`, README nuovo, tolti gli screenshot della vecchia interfaccia). Resta il rilascio: su GitHub esistono già le release 1.0.0 e 1.0.1 del vecchio Prism, quindi la prima della riscrittura deve avere un numero più alto (consigliato **2.0.0**: è un'app nuova, con un altro identificativo e un catalogo separato); conviene pubblicarla dopo U3 e U7 (senza le chiavi le copie installate non riceverebbero aggiornamenti). Il vecchio Prism non ha un aggiornamento automatico, quindi le sue copie installate non passano da sole alla nuova. **Pubblicazione chiesta dall'utente il 2026-11-08**: note di rilascio in `docs/releases/2.0.0.md` (inglese come README e release precedenti, con un riassunto in italiano), installer 2.0.0 costruito; release `v2.0.0` su GitHub (repository pubblico) dopo la PR `rewrite` → `develop` |
| U3 | Prova su un PC Windows 11 pulito (installer, avvio con Windows, aggiornamenti) | Serve un'altra macchina; l'installazione non si prova sulla macchina di sviluppo | `target\release\bundle\nsis\Prism_2.0.0_x64-setup.exe` da `scripts/build-installer.ps1`; checklist T10 in §4. In alternativa a un'altra macchina, per i punti 1, 3 e 5 basta Windows Sandbox: attivata una volta (comando in testa a `scripts/sandbox-test.ps1`, da amministratore, poi riavvio), `scripts/sandbox-test.ps1` apre una sandbox con l'installer sul desktop; chiusa la finestra, tutto sparisce. Il riavvio (punto 2) e gli aggiornamenti (punto 6) restano da provare su un PC vero. **2026-11-08**: la 2.0.0 pubblicata è installata sul PC dell'utente (non pulito, ma senza il vecchio Prism): installazione elevata riuscita, attività `\Prism` registrata, voci del menu di Esplora risorse per cartelle, unità e sfondo delle cartelle registrate verso `prism-shell.exe`, app avviata con il catalogo nuovo |
| U4 | Collegare e scollegare un disco esterno (T2, F02) | Azione fisica | Test automatico del cambio di lettera sul disco di test già passato |
| U5 | Aspetto e usabilità dell'interfaccia | Giudizio estetico | **Fatta** il 2026-11-08: l'interfaccia di T3–T10 è stata bocciata; la nuova (B+C, D51, D52, ADR-0019) è stata disegnata e approvata con l'utente e si costruisce in T11. Resta da provarla nell'app |
| U6 | Rimuovere il runner amministratore a fine sviluppo | Riduce la superficie d'attacco | `scripts/admin/uninstall-admin-runner.ps1` |
| U7 | Chiavi e pubblicazione degli aggiornamenti (F22-AC3) | La chiave privata firma ogni versione: va creata e custodita dall'utente (persa la chiave, le copie installate non accettano più aggiornamenti); il luogo di pubblicazione è una scelta dell'utente | **Scelto il 2026-11-08: come Orbit (D57)**, release su GitHub. Environment `release` creato su GitHub (solo `develop`, approvazione dell'utente). **Chiave creata il 2026-11-08** su richiesta dell'utente ("puoi fare tutto te?") con `scripts/new-update-key.ps1 -GeneratePassword`: password casuale di 32 caratteri mai stampata, salvata in `~\.tauri\prism-updater.key.password.txt`; chiave e password nei secret dell'environment `release`; chiave pubblica in `tauri.conf.json`. **Copia fatta il 2026-11-08** su richiesta dell'utente: chiave e password nella sua cartella delle credenziali (la password non è più accanto alla chiave in `~\.tauri`); verificato che la chiave si apre con quella password. Quella cartella è sullo stesso disco fisico del PC: **resta consigliata una copia fuori dal PC** (persa la chiave, le copie installate non si aggiornano più) (ARCHITECTURE §8.1) |

## 3. Problemi aperti e limiti noti

| # | Data | Problema | Impatto | Stato |
|---|---|---|---|---|
| L1 | 2026-11-07 | Memoria massima durante l'indicizzazione completa: 223 B per voce con la MFT, 174 con il walker (prima 286 e 243; D49) | Un volume da 5 milioni di voci richiede ~1,1 GB per i secondi dell'indicizzazione completa; a regime NF3 è rispettato (~325 MiB con 5,7 milioni di voci) | **Ridotto** (2026-11-07, T10); il resto (scrittura senza istantanea) dopo la 1.0 |
| L2 | 2026-11-07 | Lettore MFT: una `$MFT` con `$ATTRIBUTE_LIST` nel record 0 (MFT enormi e frammentate) viene rifiutata | Su quei volumi si usa il walker (più lento, stessi risultati logici) | Rimandato: non si riproduce sul disco di test. File compressi e sparsi **verificati** sul disco PRISMTEST (2026-11-07, T10): dimensione e attributi uguali al walker, spazio occupato corretto |
| L3 | 2026-11-07 | NTFS scrive in ritardo nei record MFT le date delle cartelle: due letture vicine possono differire di pochi ms | Solo estetico; si corregge alla modifica successiva della cartella | Accettato (ADR-0018) |
| L4 | 2026-11-07 | Aggiornamento dal journal: la mappa dei riferimenti (~0,23 s per 1,5 milioni di voci) si carica a ogni giro con modifiche; fino a T10 anche ogni cartella toccata si rileggeva e si confrontava per intero | Su C: un giro costava ~0,7 s (la cartella Temp di 115.792 file cambia di continuo); ora ~0,32 s, quasi tutto la mappa | **Ridotto** (2026-11-07, T10, D50); resta il caricamento della mappa: si toglierebbe solo con un indice (D6) o tenendola in memoria |
| L5 | 2026-11-07 | Unità di rete: capacità e spazio libero non letti, e "collegata" significa "mappata" (non si verifica che il server risponda) | Schede di rete senza barra dello spazio; un NAS spento appare collegato | **Risolto** (2026-11-07): sonda in thread separati con tempo massimo di 5 s; una condivisione conta come collegata solo quando il server risponde (ARCHITECTURE §7) |
| L6 | 2026-11-07 | Nell'app, i volumi non NTFS (exFAT, FAT) e le unità di rete non si aggiornano da soli mentre sono collegati: solo alla riconnessione o con *Aggiorna* (la sorveglianza delle cartelle esiste in `prism-win::watch` ma non è ancora collegata all'app) | Catalogo di chiavette e NAS meno fresco | **Risolto** (2026-11-07): sorveglianza cartelle collegata nell'app per FAT/exFAT/ReFS e unità di rete; da provare con una chiavetta (checklist T3, punto 10) |
| L7 | 2026-11-07 | Identità ambigua di un volume (F02-AC3): nessuna finestra di scelta ancora (D13) | Caso raro (due dischi clonati); il disco non compare finché non si risolve | **Risolto** (2026-11-07, T9): riquadro "Quale disco è…?" nella schermata *Dischi* con i candidati e "È un altro disco" (D43) |
| L8 | 2026-11-07 | Disco nuovo collegato mentre la finestra è nascosta nel tray: nessuna notifica di Windows ancora, solo il riquadro alla riapertura | F02-AC1 parziale | **Risolto** (2026-11-07): notifica di Windows, una sola per più dischi trovati insieme (D19); il clic sulla notifica non apre ancora Prism (L9) |
| L9 | 2026-11-07 | Il clic su una notifica di Prism la chiude soltanto: aprire Prism dalla notifica richiede un attivatore COM registrato dall'installer | Si riapre Prism dall'icona nel tray | **Atteso risolto** con l'installer (T10): le scorciatoie hanno l'AppUserModelID delle notifiche, così Windows apre Prism al clic; da verificare nella prova U3 |
| S1 | 2026-11-07 | (Supervisione) L'app di debug elimina senza `SandboxGuard` (`prism-app/src/state.rs`, guardia `None`) mentre lo sviluppo la pilota con i cataloghi di C: e D:; nessuna eliminazione reale avvenuta finora | Rischio di eliminare file veri durante le prove dell'app | Istruzione in CLAUDE.md (Sicurezza dei dati): guardia obbligatoria nelle build di debug, con test |
| L10 | 2026-11-07 | Risposta a S1 | — | **Fatto** (2026-11-07): nelle build di debug `AppState::deletion_guard` limita ogni eliminazione al volume PRISMTEST (trovato dall'etichetta), a `%TEMP%` e a `%LOCALAPPDATA%\PrismDev`; la release non ha guardia. Test `state_tests::debug_build_refuses_deletions_outside_the_sandbox` (eliminazione in una cartella fuori sandbox → rifiutata, file intatto, registro vuoto). Nelle prove sull'app vera non è mai stata avviata un'eliminazione (solo schermate in lettura) |
| L11 | 2026-11-07 | L'indice di ricerca di un volume si ricostruisce per intero quando il suo catalogo cambia, al massimo ogni 30 s | Risultati in ritardo fino a ~30 s dopo un cambio; ~0,6 s di CPU per ricostruzione su un volume da 1,4 milioni di voci | Accettato per la 1.0; miglioramento: aggiornamento incrementale dalle operazioni di scrittura |
| L12 | 2026-11-07 | `Ctrl+Space` è usato da alcuni metodi di input (cinese, giapponese) per cambiare lingua | Con quei metodi la scorciatoia può non registrarsi o rubare il tasto | Prism lo segnala e la scorciatoia si cambia nelle impostazioni (F07-AC1); valutare un default diverso prima della 1.0 |
| L13 | 2026-11-07 | Le unità di rete non vengono analizzate in background (D30) | Un file presente sul NAS e su un disco locale non risulta duplicato; la verifica dei backup su NAS (T7) dovrà leggere i file al momento | **Risolto** il 2026-11-08 da D58: le unità di rete si analizzano in background |
| L14 | 2026-11-07 | Verifica e confronto tengono in memoria tutti i file dei due lati (~100 B per file) e non si annullano; i risultati si perdono alla chiusura di Prism; lasciando la schermata durante una copia il job continua ma l'esito non viene più mostrato | Su due dischi da un milione di file ~200 MB per qualche secondo; l'esito di una copia lasciata si vede solo nel registro delle azioni | Accettato per la 1.0; miglioramenti: risultati su disco, job annullabili, esito delle copie nella schermata dei job |
| L15 | 2026-11-07 | I video HEVC (H.265) si decodificano solo se Windows ha l'estensione "HEVC Video Extensions" dello Store (ADR-0013) | Senza estensione i video HEVC non entrano nei video simili; vengono contati come non leggibili | Accettato; scritto nel README (limiti noti); valutare ffmpeg solo se serve |
| L16 | 2026-11-07 | Uscendo con *Esci* Prism non registrava la chiusura pulita del catalogo: Tauri termina il processo senza rilasciare lo stato (trovato preparando gli aggiornamenti, che fanno lo stesso) | Ogni avvio dopo un'uscita normale rifaceva `PRAGMA quick_check` su tutto il catalogo (secondi su cataloghi grandi) e il WAL restava pieno | **Risolto** (2026-11-07, T10): `Store::close` all'uscita e prima di installare un aggiornamento, con test. Sulla macchina di sviluppo il tray non è automatizzabile (la barra delle applicazioni è sostituita da YasbBar): da confermare con la checklist T3, punto 7 |
| L17 | 2026-11-07 | Alla fine della prima passata dell'analisi del contenuto ogni hash calcolato cercava il proprio file in tutto il volume (costo proporzionale a file calcolati × file del volume) e lo stato di tutti i file restava in memoria (~300 B per file) | Con centinaia di migliaia di file, minuti di CPU a ogni pausa o fine passata e ~190 MiB in più per tutta l'analisi di D: | **Risolto** (2026-11-07, T10) con D48 |
| L18 | 2026-11-08 | Nella scheda *Spazio* la mappa tornava alla radice del disco a ogni aggiornamento del catalogo in sottofondo (job finiti, avanzamento dell'analisi): sul PC dell'utente ogni pochi secondi | Navigare nelle cartelle era quasi impossibile con un'analisi in corso | **Risolto** (2026-11-08, T11): si ricarica solo quando cambiano disco o cartella, e dopo un aggiornamento del disco rilegge la cartella in cui si è; test end-to-end che fallisce senza la correzione |
| L19 | 2026-11-08 | Una condivisione di sistema del NAS (ZimaOS-HD dell'utente) mostra in `.media` gli altri volumi del NAS: indicizzandola, Prism li conta due volte (771 GB su un disco da 105 GB) | Totali e duplicati gonfiati per quella condivisione | Accettato: Prism legge quello che il server mostra. Consiglio: indicizzare le condivisioni dei dati, non quella di sistema; eventuale esclusione di `.media` come cartella da ignorare. Con l'analisi del NAS (D58) quei file risultano anche "duplicati" di se stessi: eliminarne uno è rifiutato (D62) |
| L20 | 2026-11-08 | **Su segnalazione dell'utente** ("come mai non vedo le anteprime?"). Nell'app vera le anteprime della scheda *Simili* non comparivano: l'interfaccia le mostra con indirizzi `blob:`, che la CSP della finestra non ammetteva (solo `'self'` e `data:`); i test con il backend simulato non passano dalla CSP | Nessuna anteprima delle immagini simili | **Risolto**: `img-src 'self' data: blob:` in `tauri.conf.json` (gli indirizzi `blob:` li crea solo la pagina stessa); nuova prova dell'app vera con due immagini simili sul disco PRISMTEST che controlla che le anteprime siano disegnate |
| L21 | 2026-11-08 | Trovato nello screenshot della prova dell'app vera: nelle schede *Simili* e *Duplicati* di un disco già analizzato al 100% compariva "l'analisi deve ancora leggere 133.885 immagini", il numero di tutto il catalogo | Messaggio che contraddice il 100% della scheda | **Risolto**: nella schermata di un disco il conteggio è quello del disco (`HashingState::pending_*`); test in `state_tests` |
| L22 | 2026-11-08 | **Su segnalazione dell'utente** ("possibile che non abbiamo tolto PRISMTEST come unità?"). Il runner amministratore collegava il disco di test e non lo scollegava mai: restava in Esplora risorse fino al riavvio, e il Prism installato sul PC lo ha registrato come disco nuovo | Un disco di sviluppo nel catalogo vero dell'utente | **Risolto**: il runner scollega il disco alla fine se l'ha collegato lui; le richieste `attach`/`detach` (file `request.txt`) servono alla prova dell'app vera (TESTING §4, §4bis). Disco scollegato e verificato un giro completo (test passati, disco scollegato alla fine). Ripuliti anche gli scarti dello sviluppo (benchmark, esperimenti in `%TEMP%`, log: ~1,3 GB). La scheda PRISMTEST del catalogo installato va tolta dall'utente (menu della scheda › *Rimuovi dal catalogo*) |

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
- Da T6 l'app legge in sottofondo, a priorità minima, il contenuto dei file dei dischi locali
  collegati e indicizzati (solo lettura: serve a riconoscere i duplicati). Si sospende dal
  menu del tray o da *Impostazioni*, dove si possono anche escludere singoli dischi.

### Dove sono le cose nella nuova interfaccia (T11)

Le checklist T3–T9 sono state scritte per l'interfaccia precedente, con il menu laterale. Con
l'interfaccia di T11 (ADR-0019) gli stessi controlli si trovano così:

| Le checklist dicono | Ora |
|---|---|
| icona *Spazio* sulla scheda di un disco | clic sul **nome** del disco: si apre la sua schermata sulla scheda *Spazio* |
| *Duplicati*, *Simili*, *Backup* nel menu laterale | blocco nero **Tutto il catalogo** in cima al catalogo, oppure la scheda con lo stesso nome dentro un disco (limitata a quel disco) |
| *Tipi di file* per tutto il catalogo | **Catalogati →** nel blocco Tutto il catalogo; per un disco, la scheda *Tipi di file* |
| *Pulizia* nel menu laterale | scheda *Pulizia* dentro ogni disco |
| *Cerca* nel menu laterale | la casella in alto (scrivendo si apre la ricerca) o Ctrl+Space; dentro un disco, *Cerca in questo disco…* dal menu "⋯" |
| *Impostazioni* nel menu laterale | l'ingranaggio in alto a destra |
| *Nome e posizione*, *Ignora sempre*, *Rimuovi dal catalogo*, *Apri in Esplora risorse*, *Aggiorna* sulla scheda | il menu **"⋯"** della scheda (o i pulsanti in cima alla schermata del disco) |
| riquadro o pannello "Nuovo disco" / "N nuovi dischi trovati" | il pulsante arancione **"N nuovi dischi"** in alto: un riquadro con *Indicizza*, *Indicizza tutti*, *Non ora* e il menu "⋯" di ogni disco |
| la salute sulla scheda apre una finestra | la salute sulla scheda apre la scheda *Salute* del disco |
| "Collegati" / "Non collegati" | i filtri *Tutti*, *Collegati*, *Rete*, *Nel cassetto* sopra le schede |

### Checklist T11 (nuova interfaccia)

1. Confronto con i disegni approvati (tela di claude.ai, pagina "Versione B+C" e "Altre
   schermate"): catalogo, disco, ricerca, duplicati, simili, backup, pulizia, salute,
   impostazioni, menu e conferme. Tema chiaro e scuro (*Impostazioni → Tema*).
2. Catalogo: blocco "Tutto il catalogo" con i totali e i collegamenti; filtri con i conteggi;
   *Ordina* per posizione, nome o spazio usato; *Schede* / *Elenco* (la scelta resta al
   riavvio); le barre delle schede si colorano per tipo di file dopo qualche secondo per disco.
3. *Attività in corso* (indicizzazioni, copie, analisi del contenuto) e *Ultime azioni* (dal
   registro delle azioni: una riga per operazione, F16-AC6); barra di stato in fondo.
4. Disco nuovo: il pulsante arancione in alto apre il riquadro; *Indicizza* fa partire
   l'indicizzazione e la scheda mostra l'avanzamento (F02-AC1, F01-AC3).
5. Schermata del disco: colore, posizione, numeri (usato, libero, file, salute, contenuto,
   aggiornato) e le schede; *Cerca in questo disco…* dal menu "⋯" apre la ricerca con
   `disk:"nome"` (F01-AC4).
6. *Nome, posizione e colore*: le posizioni già usate sono suggerite; otto colori più
   "automatico"; il colore è il bollino della scheda.
7. Icona nuova (prisma a strati) nella barra delle applicazioni, nell'area di notifica e nella
   finestra; nella build di release anche nell'installer e nel menu Start.

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
   Dopo *Esci*, al nuovo avvio il log non riporta "previous session did not close cleanly"
   (NF4, L16).
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

### Checklist T4 (spazio)

1. Sulla scheda di un disco indicizzato, icona *Spazio*: si apre la mappa del disco con
   l'elenco del contenuto dal più grande (F08-AC1/AC2).
2. Clic su una cartella nella mappa o nell'elenco: si entra nella cartella; il percorso in alto
   (breadcrumb) e il pulsante *Cartella superiore* riportano indietro. Passando sopra la mappa
   compaiono nome, dimensione e tipo.
3. Le cartelle profonde (giochi, dischi virtuali, `node_modules`) mostrano i propri file grandi
   colorati per tipo; legenda sotto la mappa.
4. Scheda *File più grandi*: i 100 file più grandi della cartella corrente, con la cartella in
   cui si trovano; *Apri* e *Mostra in Esplora risorse* (disco collegato) aprono senza
   privilegi di amministratore.
5. Scheda *Tipi di file*: categorie, estensioni e fasce di dimensione, per il disco o per
   l'intero catalogo (F08-AC3).
6. Disco non collegato: mappa, elenco e statistiche funzionano con i dati dell'ultima
   indicizzazione; nessun pulsante di apertura (F08-AC4).
7. Giudizio sulla leggibilità della mappa in tema chiaro e scuro (U5).
8. *Pulizia* (menu laterale): per ogni disco collegato e indicizzato le categorie con
   spiegazione e totale (F09-AC1); nessuna proposta in Windows, programmi, `AppData`, cartelle
   di giochi o app installate altrove (F09-AC2); i file grandi e vecchi non sono preselezionati;
   ogni elemento si può deselezionare (F09-AC3); la cache di un browser aperto è bloccata con
   l'invito a chiuderlo (F09-AC4); in fondo lo spazio occupato dal Cestino.
9. **Prova di eliminazione solo sul disco di test**: creare su PRISMTEST una cartella vuota (o
   un file di prova), aspettare che compaia in *Pulizia*, spostarla nel Cestino con conferma:
   il risultato riporta spostati e spazio liberato; nel Cestino di PRISMTEST c'è l'elemento
   (F16-AC1). Sui dischi veri usare la pulizia solo quando lo si vuole davvero: la build di
   sviluppo rifiuta comunque ogni eliminazione fuori da PRISMTEST, `%TEMP%` e
   `%LOCALAPPDATA%\PrismDev` (S1).

### Checklist T5 (ricerca)

1. Da un altro programma premere **Ctrl+Space**: si apre la finestra di ricerca in primo piano;
   Esc la nasconde; *Cerca* nel menu del tray fa lo stesso (F07-AC1, F19-AC2). In
   *Impostazioni* la scorciatoia si cambia; se un'altra applicazione la usa già, Prism lo dice e
   tiene la precedente.
2. Scrivere una parte di un nome ("port" trova "report"): i risultati arrivano mentre si scrive,
   anche dai dischi non collegati, marcati con nome e posizione fisica (F07-AC2/AC4).
3. Filtri scritti o scelti dai menu sotto la casella (tipo, dimensione, data, disco,
   collegamento, file/cartelle) danno lo stesso testo (F07-AC5); un errore (es. `colour:red`,
   `size:>big`, `disk:Inesistente`) è spiegato e sottolineato nel punto giusto (F07-AC6).
4. Frecce per scegliere, Invio apre, Maiusc+Invio mostra in Esplora risorse, Ctrl+C copia il
   percorso; sui dischi non collegati apertura e Esplora risorse sono disattivati (F07-AC7).
5. Dopo aver creato o cancellato file, i risultati si aggiornano entro ~30 s (L11).

### Checklist T6 (analisi del contenuto e duplicati)

Le eliminazioni si provano **solo** sul disco PRISMTEST: per esempio copiare due volte una
cartella di foto su PRISMTEST, aggiornare il disco dalla sua scheda e attendere che l'analisi
del contenuto arrivi al 100%.

1. Sulle schede dei dischi collegati compare "Analisi del contenuto: N% · in corso", che sale
   nel tempo; a lavoro finito "Contenuto analizzato" (F06-AC1/AC5). Mentre gira il PC resta
   reattivo; in Gestione attività `prism.exe` ha priorità bassa (F06-AC2).
2. Menu del tray: una riga con le attività in corso e *Sospendi l'analisi del contenuto*; dopo
   il clic la voce diventa *Riprendi…* e l'opzione in *Impostazioni* risulta attiva (F06-AC5,
   F19-AC2). In *Impostazioni* → *Analisi del contenuto*: sospensione, sospensione a batteria
   (spenta per default), dischi da analizzare, unità di rete comprese (D58) (F21-AC3).
3. Aggiornare un disco mentre l'analisi gira: l'aggiornamento parte subito, l'analisi riprende
   dopo qualche minuto (D31).
4. *Duplicati*: gruppi con nome, tipo, dimensione, numero di copie e spazio recuperabile; per
   ogni copia il disco, la posizione fisica e la cartella; le copie su dischi non collegati
   sono marcate e non selezionabili (F10-AC1/AC2/AC4). Filtri per dimensione minima, tipo,
   disco e testo del percorso (F10-AC5).
5. Regole di selezione: cambiarle, aggiungerne (tieni sul disco X, più vecchia, più recente,
   percorso più corto, mai nelle cartelle con un testo) e premere *Applica le regole*; le
   caselle cambiano di conseguenza e restano modificabili a mano; in un gruppo l'ultima copia
   non selezionata non si può spuntare (F10-AC3/AC4).
6. **Solo su PRISMTEST**: selezionare una copia ed eliminarla; la conferma dice che va nel
   Cestino (o che l'eliminazione è definitiva su un disco senza Cestino) e che il contenuto
   viene riletto; poi il risultato e il gruppo sparisce (F16-AC1/AC3). Modificare il
   contenuto di una copia prima di confermare: resta al suo posto ("lasciati perché cambiati").
7. Con il disco PRISMTEST scollegato e una copia ancora su un disco collegato, eliminare quella
   copia porta a una seconda conferma ("resterebbero solo copie su dischi non collegati")
   (F16-AC4).

### Checklist T7 (backup)

Le copie si provano **solo verso** il disco PRISMTEST, partendo da file di PRISMTEST: per
esempio `F:\prism-t6\Documenti` come sorgente e `F:\prism-t7` come destinazione.

1. *Backup* → *Verifica un backup*: scegliere la cartella da proteggere e uno o più dischi di
   backup, anche scollegati; il risultato dice quanti file hanno una copia verificata dal
   contenuto, quanti per nome/dimensione/data e quanti no (F13-AC1/AC2/AC3).
2. I file senza copia sono raggruppati per cartella con numero e dimensione; si entra nelle
   cartelle, si cerca nei percorsi e si filtra per dimensione (F13-AC4).
3. *Copia i file mancanti* (solo con sorgente e destinazione collegate): anteprima con numero
   di file, dimensione, cartella di destinazione e spazio libero; scelta tra *salta* e *altro
   nome* in caso di conflitto (F13-AC5, F15-AC1/AC2/AC4).
4. Durante la copia la barra avanza e *Annulla* la ferma senza lasciare file a metà; *Riprendi
   la copia* riconosce i file già copiati; alla fine l'esito elenca gli errori (F15-AC5). Sulla
   destinazione le cartelle della sorgente sono ricreate sotto la cartella scelta (F15-AC3), e
   una nuova verifica non trova più mancanti.
5. *Confronta*: due cartelle, per percorso o per contenuto; i totali per stato si usano come
   filtri, l'albero mostra i totali per cartella, un file spostato o rinominato compare come
   identico con il percorso in B (F14-AC1/AC2/AC3).

### Checklist T8 (immagini simili)

1. Dopo l'analisi del contenuto (stessa riga sulle schede) *Simili* mostra i gruppi di immagini
   che si somigliano: copie ridimensionate, ricompresse, esportate da un RAW (CR2, NEF, ARW,
   DNG...), con anteprime affiancate, risoluzione, dimensione, disco e posizione (F11-AC1/AC3/
   AC4). Prova: salvare una foto in due dimensioni diverse sul disco PRISMTEST, aggiornarlo e
   attendere.
2. La soglia cambia i gruppi (0 = quasi identiche); il filtro per disco tiene i gruppi con una
   copia su quel disco (F11-AC1).
3. Con un disco scollegato le sue immagini restano nei gruppi, senza anteprima e marcate.
4. Scheda *Video*: lo stesso video in due risoluzioni o formati (per esempio un MP4 e la sua
   versione WMV) compare in un gruppo con durata, risoluzione, dimensione e miniatura (F12-AC1).
   I video HEVC si leggono solo con l'estensione "HEVC Video Extensions" di Windows (L15).

### Checklist T9 (integrazione con il sistema)

1. *Impostazioni* → *Diagnostica*: *Apri la cartella dei log* apre `logs` con un file per giorno
   (`prism.AAAA-MM-GG.log`); *Esporta un pacchetto diagnostico…* mostra i file e il testo di
   `system.txt` prima di salvare; dopo *Esporta* il file ZIP compare in Esplora risorse e si
   apre con Windows (F20-AC1/AC2).
2. *Impostazioni* → *Catalogo* → *Azzera il catalogo…*: il pulsante resta disattivato finché
   non si spunta la casella; dopo l'azzeramento la schermata *Dischi* è vuota e i dischi
   collegati vengono riproposti come nuovi; nessun file sui dischi è toccato (F21-AC4).
3. Salute (F17): sulle schede dei dischi interni collegati compare "Salute: buona · 42 °C" (o
   lo stato); un clic mostra modello, ore di funzionamento, usura, settori e la storia. Sulle
   chiavette e i dischi USB e sul disco virtuale PRISMTEST: "Salute: non disponibile" con il
   motivo (F17-AC1/AC4). La notifica di peggioramento (F17-AC3) si vede solo con un disco che
   peggiora davvero: coperta dai test automatici.
4. Menu di Esplora risorse (F18): le voci le registra l'installer (T10). Per provarlo prima, con
   l'app di sviluppo aperta, da un terminale **non** elevato:
   `target\debug\prism-shell.exe analyze "D:\Progetti"` porta Prism in primo piano sulla mappa
   di quella cartella; `... duplicates "D:\Progetti"` apre i duplicati filtrati su di essa.
5. Identità ambigua (F02-AC3): copiare il file VHDX del disco PRISMTEST e collegare la copia
   mentre l'originale è nel catalogo e scollegato; compare "Quale disco è PRISMTEST?" con il
   disco del catalogo e "È un altro disco". Scegliendo il disco, la scheda torna collegata con
   la nuova lettera; scegliendo "un altro disco", compare come disco nuovo da indicizzare.

### Checklist T10 (installazione, su un PC pulito)

1. `Prism_2.0.0_x64-setup.exe`: l'installazione chiede l'elevazione, installa WebView2 se manca,
   crea le scorciatoie (F22-AC1).
2. Al riavvio o al nuovo accesso Prism parte nascosto nel tray (F19-AC3); in *Impostazioni*
   l'opzione si spegne e al riavvio non parte più.
3. Clic destro su una cartella o un'unità: *Analizza con Prism* e *Cerca duplicati qui*, con
   Prism chiuso e aperto, senza richiesta UAC (F18).
4. Un disco nuovo collegato con la finestra nascosta mostra una notifica; il clic apre Prism
   (L9).
5. Disinstallazione: le voci del menu e l'attività spariscono; lasciando vuota la casella
   "Elimina i dati dell'applicazione" il catalogo resta in `%LOCALAPPDATA%\com.prism.catalog`
   e una nuova installazione lo ritrova (F22-AC2).
6. Aggiornamenti (dopo U7): installata una versione costruita con le chiavi e pubblicata una
   versione successiva, entro un minuto dall'avvio *Impostazioni › Aggiornamenti* dice "Prism
   x è pronto da installare"; *Installa e riavvia…* chiede conferma, poi Prism si chiude, si
   aggiorna e riparte con il catalogo intatto (F22-AC3). *Annulla* lascia tutto com'è.
