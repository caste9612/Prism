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
| D30 | 2026-11-07 | T6 | Le unità di rete non si analizzano in background: ogni byte attraverserebbe la rete per ore e il NAS potrebbe essere condiviso. Le copie sul NAS non entrano quindi nei gruppi di duplicati (L13) | Analizzarle a bassa priorità come i dischi locali | `prism-app/src/hash_jobs.rs` | F06-AC1 |
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
| D47 | 2026-11-07 | T10 | Aggiornamenti con `tauri-plugin-updater`: chiave pubblica e indirizzo del manifest fissati alla compilazione (`PRISM_UPDATE_PUBKEY`, `PRISM_UPDATE_URL`) e non in `tauri.conf.json`, così finché l'utente non sceglie chiavi e luogo di pubblicazione nessuna build si collega; controllo un minuto dopo l'avvio e poi ogni 24 ore, download in sottofondo, installazione solo dopo conferma con riavvio; `latest.json` scritto e firmato da `build-installer.ps1`, pensato per GitHub Releases | Chiave nel file di configurazione (una chiave di prova finirebbe nel repository); installazione automatica alla chiusura senza chiedere; un servizio di aggiornamento proprio | `prism-app::updates`, `SettingsView.svelte`, `scripts/build-installer.ps1` | F22-AC3, NF6 |
| D48 | 2026-11-07 | T10 | L'analisi del contenuto tiene in memoria solo gli id dei file ancora da fare (ordinati in SQL nella prima passata; id, dimensione e "possibile duplicato" nella seconda) e rilegge ogni file dal catalogo quando tocca a lui; cache dei percorsi limitata a 4096 cartelle. Misurato: ~12 MiB invece di ~190 su D: (629.000 file), e con 5,7 milioni di voci l'app resta a ~325 MiB (NF3) | Struttura compatta per file (~72 B, comunque lineare nel numero di file); passate guidate da query a pagine (transazioni di lettura lunghe, il WAL non si svuoterebbe per ore) | `prism-core::hashing`, `store::hashes` | NF3, F06-AC4; `docs/benchmarks/2026-11-07-t10-memory.md` |

## 2. Cose che richiedono l'utente

| # | Cosa | Perché serve l'utente | Cosa è già pronto |
|---|---|---|---|
| U1 | Firma del codice (SPEC Q7) | Scelta e acquisto del certificato | Pronto: basta impostare `bundle.windows.signCommand` (o `certificateThumbprint`) in `crates/prism-app/tauri.conf.json` e rieseguire `scripts/build-installer.ps1` (ARCHITECTURE §8.1) |
| U2 | Merge di `rewrite` sul ramo principale e rilascio 1.0 (Q11) | Approvazione | — |
| U3 | Prova su un PC Windows 11 pulito (installer, avvio con Windows, aggiornamenti) | Serve un'altra macchina; l'installazione non si prova sulla macchina di sviluppo | `target\release\bundle\nsis\Prism_0.1.0_x64-setup.exe` da `scripts/build-installer.ps1`; checklist T10 in §4 |
| U4 | Collegare e scollegare un disco esterno (T2, F02) | Azione fisica | Test automatico del cambio di lettera sul disco di test già passato |
| U5 | Aspetto e usabilità dell'interfaccia | Giudizio estetico | — |
| U6 | Rimuovere il runner amministratore a fine sviluppo | Riduce la superficie d'attacco | `scripts/admin/uninstall-admin-runner.ps1` |
| U7 | Chiavi e pubblicazione degli aggiornamenti (F22-AC3) | La chiave privata firma ogni versione: va creata e custodita dall'utente (persa la chiave, le copie installate non accettano più aggiornamenti); il luogo di pubblicazione è una scelta dell'utente | Pronto: `npx tauri signer generate -w <file>` dalla cartella `ui`; poi `scripts/build-installer.ps1 -Notes "..."` con `PRISM_UPDATE_PUBKEY`, `PRISM_UPDATE_URL`, `TAURI_SIGNING_PRIVATE_KEY` (e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`); pubblicare installer, `.sig` e `latest.json` nella release `v<versione>` (ARCHITECTURE §8.1) |

## 3. Problemi aperti e limiti noti

| # | Data | Problema | Impatto | Stato |
|---|---|---|---|---|
| L1 | 2026-11-07 | Memoria massima durante l'indicizzazione ~315 B per voce (istantanea `ScanTree` completa in memoria) | Un volume da 5 milioni di voci richiederebbe ~1,5 GB durante l'indicizzazione (NF3 chiede ≤ 500 MB a regime) | Da fare in T10 (prestazioni finali). A regime NF3 è rispettato (~325 MiB con 5,7 milioni di voci, `docs/benchmarks/2026-11-07-t10-memory.md`) |
| L2 | 2026-11-07 | Lettore MFT: una `$MFT` con `$ATTRIBUTE_LIST` nel record 0 (MFT enormi e frammentate) viene rifiutata; file compressi/sparsi non verificati su disco reale | Su quei volumi si usa il walker (più lento, stessi risultati logici) | Rimandato |
| L3 | 2026-11-07 | NTFS scrive in ritardo nei record MFT le date delle cartelle: due letture vicine possono differire di pochi ms | Solo estetico; si corregge alla modifica successiva della cartella | Accettato (ADR-0018) |
| L4 | 2026-11-07 | Aggiornamento dal journal: la mappa dei riferimenti (≈0,25 s per 1,3 milioni di voci) si ricarica a ogni aggiornamento | Con il journal applicato ogni 15 s (D16), un volume grande con cambi continui (C:) costa ~0,25 s di CPU per giro | Aperto, da misurare nell'app e ottimizzare (indice parziale sulle cartelle o mappa mantenuta in memoria tra un giro e l'altro) |
| L5 | 2026-11-07 | Unità di rete: capacità e spazio libero non letti, e "collegata" significa "mappata" (non si verifica che il server risponda) | Schede di rete senza barra dello spazio; un NAS spento appare collegato | **Risolto** (2026-11-07): sonda in thread separati con tempo massimo di 5 s; una condivisione conta come collegata solo quando il server risponde (ARCHITECTURE §7) |
| L6 | 2026-11-07 | Nell'app, i volumi non NTFS (exFAT, FAT) e le unità di rete non si aggiornano da soli mentre sono collegati: solo alla riconnessione o con *Aggiorna* (la sorveglianza delle cartelle esiste in `prism-win::watch` ma non è ancora collegata all'app) | Catalogo di chiavette e NAS meno fresco | **Risolto** (2026-11-07): sorveglianza cartelle collegata nell'app per FAT/exFAT/ReFS e unità di rete; da provare con una chiavetta (checklist T3, punto 10) |
| L7 | 2026-11-07 | Identità ambigua di un volume (F02-AC3): nessuna finestra di scelta ancora (D13) | Caso raro (due dischi clonati); il disco non compare finché non si risolve | **Risolto** (2026-11-07, T9): riquadro "Quale disco è…?" nella schermata *Dischi* con i candidati e "È un altro disco" (D43) |
| L8 | 2026-11-07 | Disco nuovo collegato mentre la finestra è nascosta nel tray: nessuna notifica di Windows ancora, solo il riquadro alla riapertura | F02-AC1 parziale | **Risolto** (2026-11-07): notifica di Windows, una sola per più dischi trovati insieme (D19); il clic sulla notifica non apre ancora Prism (L9) |
| L9 | 2026-11-07 | Il clic su una notifica di Prism la chiude soltanto: aprire Prism dalla notifica richiede un attivatore COM registrato dall'installer | Si riapre Prism dall'icona nel tray | **Atteso risolto** con l'installer (T10): le scorciatoie hanno l'AppUserModelID delle notifiche, così Windows apre Prism al clic; da verificare nella prova U3 |
| S1 | 2026-11-07 | (Supervisione) L'app di debug elimina senza `SandboxGuard` (`prism-app/src/state.rs`, guardia `None`) mentre lo sviluppo la pilota con i cataloghi di C: e D:; nessuna eliminazione reale avvenuta finora | Rischio di eliminare file veri durante le prove dell'app | Istruzione in CLAUDE.md (Sicurezza dei dati): guardia obbligatoria nelle build di debug, con test |
| L10 | 2026-11-07 | Risposta a S1 | — | **Fatto** (2026-11-07): nelle build di debug `AppState::deletion_guard` limita ogni eliminazione al volume PRISMTEST (trovato dall'etichetta), a `%TEMP%` e a `%LOCALAPPDATA%\PrismDev`; la release non ha guardia. Test `state_tests::debug_build_refuses_deletions_outside_the_sandbox` (eliminazione in una cartella fuori sandbox → rifiutata, file intatto, registro vuoto). Nelle prove sull'app vera non è mai stata avviata un'eliminazione (solo schermate in lettura) |
| L11 | 2026-11-07 | L'indice di ricerca di un volume si ricostruisce per intero quando il suo catalogo cambia, al massimo ogni 30 s | Risultati in ritardo fino a ~30 s dopo un cambio; ~0,6 s di CPU per ricostruzione su un volume da 1,4 milioni di voci | Accettato per la 1.0; miglioramento: aggiornamento incrementale dalle operazioni di scrittura |
| L12 | 2026-11-07 | `Ctrl+Space` è usato da alcuni metodi di input (cinese, giapponese) per cambiare lingua | Con quei metodi la scorciatoia può non registrarsi o rubare il tasto | Prism lo segnala e la scorciatoia si cambia nelle impostazioni (F07-AC1); valutare un default diverso prima della 1.0 |
| L13 | 2026-11-07 | Le unità di rete non vengono analizzate in background (D30) | Un file presente sul NAS e su un disco locale non risulta duplicato; la verifica dei backup su NAS (T7) dovrà leggere i file al momento | Accettato; valutare in T7 un'analisi su richiesta per singola unità di rete |
| L14 | 2026-11-07 | Verifica e confronto tengono in memoria tutti i file dei due lati (~100 B per file) e non si annullano; i risultati si perdono alla chiusura di Prism; lasciando la schermata durante una copia il job continua ma l'esito non viene più mostrato | Su due dischi da un milione di file ~200 MB per qualche secondo; l'esito di una copia lasciata si vede solo nel registro delle azioni | Accettato per la 1.0; miglioramenti: risultati su disco, job annullabili, esito delle copie nella schermata dei job |
| L15 | 2026-11-07 | I video HEVC (H.265) si decodificano solo se Windows ha l'estensione "HEVC Video Extensions" dello Store (ADR-0013) | Senza estensione i video HEVC non entrano nei video simili; vengono contati come non leggibili | Accettato; da dire nella documentazione utente, valutare ffmpeg solo se serve |
| L16 | 2026-11-07 | Uscendo con *Esci* Prism non registrava la chiusura pulita del catalogo: Tauri termina il processo senza rilasciare lo stato (trovato preparando gli aggiornamenti, che fanno lo stesso) | Ogni avvio dopo un'uscita normale rifaceva `PRAGMA quick_check` su tutto il catalogo (secondi su cataloghi grandi) e il WAL restava pieno | **Risolto** (2026-11-07, T10): `Store::close` all'uscita e prima di installare un aggiornamento, con test. Sulla macchina di sviluppo il tray non è automatizzabile (la barra delle applicazioni è sostituita da YasbBar): da confermare con la checklist T3, punto 7 |
| L17 | 2026-11-07 | Alla fine della prima passata dell'analisi del contenuto ogni hash calcolato cercava il proprio file in tutto il volume (costo proporzionale a file calcolati × file del volume) e lo stato di tutti i file restava in memoria (~300 B per file) | Con centinaia di migliaia di file, minuti di CPU a ogni pausa o fine passata e ~190 MiB in più per tutta l'analisi di D: | **Risolto** (2026-11-07, T10) con D48 |

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
   (spenta per default), dischi da analizzare (le unità di rete non compaiono, D30) (F21-AC3).
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

1. `Prism_0.1.0_x64-setup.exe`: l'installazione chiede l'elevazione, installa WebView2 se manca,
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
