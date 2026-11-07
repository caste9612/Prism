# CLAUDE.md — Regole di lavoro per Prism

Prism è il catalogo di tutti i dischi di un utente Windows (anche scollegati), con ricerca,
analisi dello spazio, duplicati, verifica dei backup e salute dei dischi. È una **riscrittura
da zero**: questo branch (`rewrite`) sostituirà il ramo principale alla 1.0.

## All'inizio di ogni sessione

1. Leggi `docs/PROGRESS.md` (stato attuale e prossimo passo).
2. Leggi la sezione della tappa corrente in `docs/ROADMAP.md`.
3. Consulta `docs/SPEC.md`, `docs/ARCHITECTURE.md` e le ADR in `docs/adr/` per la parte che
   stai toccando. **I documenti del repository sono la fonte di verità**, non la memoria della
   conversazione.

## Regole di lavoro

- **Documentazione nello stesso commit del codice.** Se cambia un comportamento, aggiorna la
  SPEC; se cambia la struttura, ARCHITECTURE o una nuova ADR; a ogni passo significativo, una
  voce in `docs/PROGRESS.md` e le checkbox in `docs/ROADMAP.md`.
- **Passi piccoli, sempre verdi.** Prima di ogni commit `scripts/check.ps1` deve passare
  (fmt, clippy con `-D warnings`, test). Commit in autonomia.
- **Push**: consentito **solo** per il branch `rewrite` su `origin`, periodicamente (CI e
  backup). Mai force push, mai push di altri branch, mai merge sul ramo principale senza
  approvazione dell'utente.
- **Autonomia**: si procede tappa dopo tappa senza fermarsi, con un report in
  `docs/PROGRESS.md` a fine tappa. **Unica sosta obbligatoria**: a fine T3 (prima interfaccia
  vera) per la revisione dell'utente. Le scelte tecniche (ADR proposte, librerie) si prendono in
  autonomia sulla base di benchmark e prototipi, documentandole in un'ADR con i dati.
- **Test per ogni criterio.** Ogni criterio `Fxx-ACn` implementato ha un test che ne cita l'ID,
  oppure una voce nella checklist manuale della tappa.
- **Mai indebolire un test** per farlo passare. Se la causa di un fallimento non è chiara,
  fermati e segnalalo.
- **Fermati e chiedi** sulle decisioni di prodotto non già prese (SPEC, punti
  **[DA CONFERMARE]** e domande aperte del capitolo 8).
- **A fine tappa** consegna all'utente: cosa è stato fatto, l'output dei controlli, cosa deve
  verificare a mano.

## Sicurezza dei dati (vedi `docs/TESTING.md` §1)

- Operazioni distruttive (eliminare, spostare, sovrascrivere, Cestino, copiare *verso*) **solo**
  in cartelle temporanee create dai test o sul disco virtuale di test `PRISMTEST`.
- Le operazioni distruttive passano sempre dalla `SandboxGuard` nei test e nei comandi di
  sviluppo.
- Sui dischi reali solo letture. Dati di sviluppo in `%LOCALAPPDATA%\PrismDev\`.
- Non modificare il vecchio worktree `D:\Progetti\Github\Prism` né i dati del vecchio Prism in
  `%APPDATA%\com.prism.diskanalyzer`.

## Comandi

```powershell
.\scripts\check.ps1                 # fmt + clippy + test (obbligatorio prima di ogni commit)
cargo test --workspace              # solo test
cargo run -p prism-cli -- --help    # strumento di sviluppo
```

Non dare per scontato che la shell di sviluppo sia o non sia amministratore (dipende da come è
stata aperta la sessione; verificalo se conta). I test che richiedono l'amministratore si
eseguono sempre con il runner descritto in `docs/TESTING.md` §4 (`schtasks /run /tn
PrismDevAdminTests`). Con una shell elevata la `SandboxGuard` è ancora più importante: nessuna
operazione distruttiva fuori dalle cartelle di test.

## Convenzioni

- **Lingue**: documentazione di progetto in italiano; codice, identificatori, commenti nel
  codice e messaggi di commit in inglese; testi dell'interfaccia tramite i18n (EN/IT).
- **Commit**: stile Conventional Commits (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`,
  `build:`, `chore:`), con la riga di attribuzione richiesta dalla sessione.
- **Rust**: edition 2024, toolchain in `rust-toolchain.toml`. Niente `unwrap()`/`expect()`
  fuori dai test, salvo invarianti documentate. Errori con `thiserror` nei crate di libreria.
  Log con `tracing`. `unsafe` solo in `prism-win`, ogni blocco con `// SAFETY:`.
- **Struttura**: logica di dominio in `prism-core` (nessun accesso diretto a Win32);
  implementazioni Windows in `prism-win`; nessuna logica nei comandi Tauri.
