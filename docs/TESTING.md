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
| S6 | **Mai indebolire un test per farlo passare.** Se un test fallisce e la causa non è chiara, ci si ferma e si segnala. |

## 2. Livelli di test

| Livello | Dove | Cosa copre | Esecuzione |
|---|---|---|---|
| Unitari | `crates/*/src` (`#[cfg(test)]`) | Funzioni pure: esclusioni, riconciliazione, parser dei filtri, regole di selezione, validità degli hash, aggregati delle cartelle, ricostruzione dei percorsi. | `cargo test` |
| Proprietà | `crates/prism-core/tests` | Generazione casuale di alberi di file e sequenze di modifiche (proptest): l'aggiornamento incrementale deve produrre lo stesso catalogo di un'indicizzazione completa (F05-AC3). | `cargo test` |
| Integrazione | `crates/prism-core/tests` | Walker reale su cartelle temporanee → catalogo SQLite → modifiche sul disco → aggiornamento → verifiche. Copia verificata, Cestino e registro azioni in sandbox. | `cargo test` |
| Windows senza admin | `crates/prism-win` | Rilevamento volumi (lettura), identità dei volumi, avvio non elevato, Cestino su file temporanei. | `cargo test` |
| Windows con admin | `crates/prism-win/tests` (feature `admin-tests`) | Lettura MFT e journal USN sul disco virtuale di test; SMART in sola lettura sui dischi fisici; cambio della lettera del disco di test PRISMTEST con verifica dell'identità (`volume_identity.rs`, F02-AC3: tocca solo il volume con etichetta PRISMTEST e bus virtuale, e ripristina la lettera); lettore MFT confrontato con il walker su un albero creato sul disco di test (`mft.rs`: hard link, record di estensione, flussi alternativi, nomi Unicode e con punto finale; la cartella di prova viene creata ed eliminata tramite la guardia sandbox); journal USN (`usn.rs`: record di creazione, rinomina, cancellazione e hard link; recupero di singoli file con metadati identici alla lettura completa; il journal viene creato **solo** sul disco di test, se manca); aggiornamento dal journal confrontato con la reindicizzazione completa dopo modifiche casuali reali (`ntfs_update.rs`, F05-AC1/AC3). | Runner amministratore (sezione 4) |
| Interfaccia | `ui/` | Componenti e store (Vitest); flussi end-to-end in browser con IPC simulato (Playwright). | `npm test`, `npm run e2e` |
| Proprietà con riferimenti | `crates/prism-core/tests/reconcile_refs.rs` | File system in memoria con riferimenti stabili ai file (`tests/common/fakefs.rs`, come NTFS letto dalla MFT): rinomine, spostamenti, scambi di nomi, sostituzioni, aggiornamenti parziali; verifica F05-AC3 e che ogni file conservi il proprio id (F05-AC4). | `cargo test` |
| Proprietà del journal | `crates/prism-core/tests/journal_apply.rs` | Modifiche casuali sul file system simulato registrate come un journal (il file cambiato, mai la cartella madre), compreso Cestino escluso e ripristino: catalogo aggiornato = reindicizzazione, id conservati. Più uno stress di 3.000 semi da lanciare a mano (`-- --ignored`, release, ~6 minuti). | `cargo test` |
| Manuali | `docs/ROADMAP.md`, checklist di ogni tappa | Ciò che non si automatizza: aspetto grafico, UAC, collegamento fisico di dischi, installer su PC pulito. | Utente |
| Benchmark | `prism-cli bench` | Tempi di indicizzazione, dimensione del catalogo, latenza di ricerca su dischi reali (sola lettura). Risultati in `docs/benchmarks/`. | Manuale/periodico |

## 3. Controlli a ogni commit

`scripts/check.ps1` esegue, e deve passare prima di ogni commit:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. (da T3) `npm run check` e `npm test` in `ui/`

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
  estetico finale resta all'utente (sosta di revisione a fine T3).

## 5. Tracciabilità

- Nome dei test: descrittivo, con l'ID del criterio quando applicabile, ad esempio
  `f05_ac3_update_equals_full_reindex`.
- `docs/ROADMAP.md` elenca per ogni tappa i criteri coperti e quelli che restano manuali.
- I benchmark annotano macchina, disco, numero di voci e data in `docs/benchmarks/`.
