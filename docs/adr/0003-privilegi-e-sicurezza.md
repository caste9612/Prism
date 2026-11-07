# ADR-0003 — Esecuzione sempre elevata, avvio senza UAC, superficie d'attacco ridotta

- **Stato**: Accettata (alcuni dettagli da verificare con prototipi, indicati sotto)
- **Data**: 2026-11-07

## Contesto

Leggere la MFT, il journal USN e lo SMART richiede i privilegi di amministratore. L'utente ha
scelto che Prism giri **sempre** come amministratore e parta con Windows senza prompt UAC.

## Decisione

1. Manifest dell'eseguibile con `requireAdministrator`.
2. Avvio all'accesso tramite un'**attività pianificata** "esegui con i privilegi più elevati",
   registrata dall'installer (che è già elevato). Le impostazioni la attivano o disattivano.
3. **Apertura di file e cartelle senza privilegi**: Prism non lancia programmi dell'utente
   come amministratore; usa la shell del desktop (tecnica `IShellDispatch2::ShellExecute`
   tramite la finestra di Esplora risorse) per aprire file e cartelle con il token normale.
4. **Menu contestuale** (F18): Esplora risorse non è elevato, quindi le voci di menu lanciano
   `prism-shell.exe` (non elevato), che inoltra la richiesta all'istanza attiva via named pipe
   oppure avvia l'attività pianificata (`schtasks /run`) se Prism non è in esecuzione.
5. **Superficie d'attacco minima della webview**, perché un'eventuale iniezione di codice
   girerebbe con privilegi di amministratore:
   - CSP restrittiva, nessun contenuto remoto, nessun `{@html}` con dati del catalogo;
   - nessun plugin Tauri `fs` o `shell` generico;
   - i comandi su file ricevono identificativi del catalogo, non percorsi; i percorsi scelti
     dall'utente arrivano da dialoghi nativi e sono validati nel backend;
   - la named pipe accetta solo messaggi di un formato chiuso e verifica il client.

## Conseguenze

- Le build di **debug** usano `asInvoker` (nessuna richiesta di elevazione): girano senza MFT,
  USN e SMART, ripiegando sul walker, così sviluppo e test dell'interfaccia funzionano da una
  shell non elevata. Solo le build di **release** hanno `requireAdministrator`.
- Il trascinamento di file da Esplora risorse a Prism non funziona (UIPI di Windows).
- Senza firma del codice il prompt UAC dell'installer mostra "Autore sconosciuto" e SmartScreen
  avvisa sul download: la firma diventa necessaria per la distribuzione pubblica (SPEC Q7).
- **Da verificare con prototipi**: WebView2 in processo elevato (cartella dati utente
  coerente), manifest personalizzato con Tauri 2, avvio dell'attività pianificata da un
  processo non elevato, apertura non elevata tramite la shell.

## Esiti dei prototipi (T3, 2026-11-07)

- **WebView2 elevata**: funziona. La finestra principale è creata dal codice
  (`create: false` in `tauri.conf.json`) con la cartella dati di WebView2 accanto al catalogo
  (`<cartella dati>\webview`); senza questo accorgimento anche le build di debug userebbero
  `%LOCALAPPDATA%\com.prism.catalog\EBWebView`.
- **Manifest personalizzato**: `crates/prism-app/build.rs` sceglie `windows/release.manifest`
  (`requireAdministrator`) o `windows/debug.manifest` (`asInvoker`) in base al profilo.
- **Attività pianificata**: `prism-win::autostart` registra con `schtasks.exe` e una
  definizione XML un'attività all'accesso dell'utente con i privilegi più elevati (ritardo
  10 s, nessun limite di durata, nessuna restrizione a batteria), argomento `--autostart` per
  partire nascosto nel tray. Test elevato su un'attività disattivata in `\PrismDevTests\`.
  L'avvio di un'attività di questo tipo da un processo non elevato senza UAC è lo stesso
  meccanismo del runner dei test amministratore, verificato in T2 (TESTING §4).
- **Apertura non elevata tramite la shell**: da prototipare quando serve aprire file (T4/T5).
