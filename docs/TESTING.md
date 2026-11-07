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
| Windows con admin | `crates/prism-win` (`#[ignore]` + feature `admin-tests`) | Lettura MFT e journal USN sul disco virtuale di test; SMART in sola lettura sui dischi fisici. | Runner amministratore (sezione 4) |
| Interfaccia | `ui/` | Componenti e store (Vitest); flussi end-to-end in browser con IPC simulato (Playwright). | `npm test`, `npm run e2e` |
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

## 4. Test che richiedono l'amministratore

La shell di sviluppo **non** è elevata. Per i test di MFT, USN e SMART si usa questo schema,
scelto per dare la massima autonomia allo sviluppo:

**Disco virtuale di test.** Un file VHDX dedicato (proposta: 4 GB, espansione dinamica) in
`%LOCALAPPDATA%\PrismDev\prism-test.vhdx`, formattato NTFS con etichetta `PRISMTEST`. I test lo
popolano con file sintetici, lo modificano e possono anche riformattarlo: contiene solo dati di
test. Anche le prove manuali distruttive dell'app (S5) si fanno qui.

**Runner amministratore.** Uno script di configurazione, da eseguire **una sola volta**
dall'utente in un PowerShell amministratore (`scripts/admin/install-admin-runner.ps1`, arriva in
T2), che:

1. crea e monta il disco virtuale di test;
2. registra un'attività pianificata `PrismDevAdminTests`, solo su richiesta e con i privilegi
   più elevati, che esegue `scripts/admin/run-admin-tests.ps1`.

Lo sviluppo può poi lanciare i test elevati con `schtasks /run /tn PrismDevAdminTests` e
leggerne l'esito in `%LOCALAPPDATA%\PrismDev\admin-tests\last.log`, senza intervento
dell'utente.

**Avvertenza di sicurezza (da accettare consapevolmente).** Finché l'attività esiste, qualunque
programma in esecuzione con l'utente può avviarla, e lo script e il codice compilato stanno in
cartelle scrivibili dall'utente: è di fatto un modo per eseguire codice come amministratore
senza UAC. Per questo:

- l'attività si rimuove con `scripts/admin/uninstall-admin-runner.ps1` a fine sviluppo, o prima
  se l'utente preferisce;
- l'alternativa è che l'utente esegua a mano `run-admin-tests.ps1` da un terminale
  amministratore quando richiesto (meno autonomia, nessun rischio aggiunto).

L'utente sceglie tra le due opzioni al momento della configurazione in T2.

## 5. Tracciabilità

- Nome dei test: descrittivo, con l'ID del criterio quando applicabile, ad esempio
  `f05_ac3_update_equals_full_reindex`.
- `docs/ROADMAP.md` elenca per ogni tappa i criteri coperti e quelli che restano manuali.
- I benchmark annotano macchina, disco, numero di voci e data in `docs/benchmarks/`.
