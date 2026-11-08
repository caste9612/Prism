# CLAUDE.md — Regole di lavoro per Prism

Prism è il catalogo di tutti i dischi di un utente Windows (anche scollegati), con ricerca,
analisi dello spazio, duplicati, verifica dei backup e salute dei dischi. È una **riscrittura
da zero**, pubblicata come 2.0.0; dal 2026-11-08 si lavora direttamente su `develop`.

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
  (fmt, clippy con `-D warnings`, test). Commit in autonomia. Se `Cargo.lock` (o
  `ui/package-lock.json`) è modificato, va nello stesso commit: in locale `--locked` passa
  comunque, in CI no (CI rossa su `736db7c` per questo).
- **Branch e push** (decisione dell'utente, 2026-11-08: "smetti di aprire pull request,
  spostiamo tutto su develop e lavoriamo lì"): si lavora sul branch `develop` (ramo
  predefinito) nel worktree `D:\Progetti\Github\Prism-next`, con commit e push diretti su
  `origin/develop`, senza pull request. Mai force push. Il branch `rewrite` è chiuso (unito in
  `develop` con le PR #2 e #3). Le release partono dal workflow *Release* su `develop`, con
  l'approvazione dell'utente.
- **Autonomia** (decisioni dell'utente, 2026-11-07 e 2026-11-08: "procedi in totale
  autonomia"). Fino alla 2.0.0 si è proceduto tappa dopo tappa senza soste; dopo la 2.0.0 si
  lavora sulle richieste dell'utente (sezione "Dopo la 2.0.0" di `docs/ROADMAP.md`), sempre in
  autonomia, con un report in `docs/PROGRESS.md` a ogni passo significativo. Se una voce è
  bloccata, annotala in `docs/REVIEW.md` §3 e passa alla successiva indipendente.
- **Rilasci**: numero di versione uguale in `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`,
  `ui/package.json` e `ui/package-lock.json`, note in `docs/releases/<versione>.md`, poi il
  workflow *Release* con `publish=true` (l'utente approva l'environment `release`).
- **Decisioni: si prendono in autonomia e si registrano.** Sia quelle tecniche sia quelle di
  prodotto non ancora prese (punti **[DA CONFERMARE]**, domande aperte del capitolo 8 della SPEC,
  dettagli di interfaccia e di comportamento): scegli l'opzione più prudente e reversibile,
  coerente con SPEC e ADR, e **registrala in `docs/REVIEW.md` §1** (decisione, alternative, come
  cambiarla). Le scelte tecniche importanti restano anche in un'ADR con i dati.
- **Ciò che richiede davvero l'utente** (firma del codice se la vorrà, U1; prova su un altro
  PC; collegamento fisico di dischi; giudizio estetico; approvazione dei rilasci): prepara tutto il
  possibile, elencalo in `docs/REVIEW.md` §2 e prosegui con il resto.
- **Test per ogni criterio.** Ogni criterio `Fxx-ACn` implementato ha un test che ne cita l'ID,
  oppure una voce nella checklist manuale della tappa (riportata anche in `docs/REVIEW.md` §4).
- **Mai indebolire un test** per farlo passare. Se un fallimento non si spiega: nessun commit
  con controlli rossi; salva il lavoro incompiuto come patch in `%LOCALAPPDATA%\PrismDev\wip\`,
  torna all'ultimo stato verde, descrivi il problema in `docs/REVIEW.md` §3 e passa a una voce
  indipendente.

## Supervisione

Una seconda sessione di Claude Code, non elevata, sorveglia il loop per conto dell'utente:
controlla git, la CI, `docs/PROGRESS.md` e l'attività della sessione, e può aggiornare questo
file o `docs/REVIEW.md` con nuove istruzioni. Non può mandarti messaggi diretti (una sessione
elevata non li riceve), quindi **rileggi CLAUDE.md all'inizio di ogni iterazione** e segui
sempre la versione più recente. I suoi commit toccano solo documenti; non modificare a mano le
sue voci in `docs/REVIEW.md`, aggiungi le tue.

## Sicurezza dei dati (vedi `docs/TESTING.md` §1)

- Operazioni distruttive (eliminare, spostare, sovrascrivere, Cestino, copiare *verso*) **solo**
  in cartelle temporanee create dai test o sul disco virtuale di test `PRISMTEST`.
- Le operazioni distruttive passano sempre dalla `SandboxGuard` nei test e nei comandi di
  sviluppo.
- Sui dischi reali solo letture. Dati di sviluppo in `%LOCALAPPDATA%\PrismDev\`.
- Non modificare il vecchio worktree `D:\Progetti\Github\Prism`. Il vecchio Prism 1.0.1
  (portatile, sul Desktop) e i suoi dati (`%APPDATA%\com.prism.diskanalyzer`,
  `%LOCALAPPDATA%\com.prism.diskanalyzer`) sono stati spostati nel Cestino il 2026-11-08 su
  richiesta dell'utente, per installare la 2.0.0 su questo PC.
- **Prism 2.0 installato su questo PC** (dal 2026-11-08): l'app installata usa
  `%LOCALAPPDATA%\com.prism.catalog`, che lo sviluppo non tocca; prima di avviare l'app di
  sviluppo va chiusa quella installata (stesso identificativo: la seconda istanza passerebbe
  alla prima, e Ctrl+Spazio sarebbe di una sola delle due).
- **App di sviluppo protetta** (istruzione della supervisione, 2026-11-07; fatto, REVIEW L10):
  nelle build di **debug** ogni operazione distruttiva dell'app passa da
  `AppState::deletion_guard`, una `SandboxGuard` limitata al volume PRISMTEST (riconosciuto
  dall'etichetta, non dalla lettera), a `%TEMP%` e a `%LOCALAPPDATA%\PrismDev`; solo la
  **release** non ha guardia. Vale per ogni nuova operazione distruttiva (eliminazione,
  spostamento, copia verso, sovrascrittura), con un test che la prova rifiutata fuori sandbox
  (`state_tests::debug_build_refuses_deletions_outside_the_sandbox`).
- **Login e segreti**: le credenziali che l'utente dà (per esempio quelle del NAS) non vanno mai
  nel repository, nei documenti o nella memoria; i test reali le leggono da variabili d'ambiente
  (`PRISM_ZIMAOS*`) e il prodotto le tiene nel Gestore credenziali di Windows (REVIEW D66).
- **Mai azioni distruttive nell'app vera su dischi reali**: pilotando l'app (WebDriver, clic,
  IPC) con cataloghi di dischi reali non si avviano mai eliminazione, pulizia, spostamento o
  copia, nemmeno "per vedere il dialogo". I flussi distruttivi dell'app si provano solo su
  voci del disco PRISMTEST.

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

Igiene dei comandi shell: mai comandi che leggono lo standard input senza riceverlo (per
esempio `cat > file` senza heredoc o senza `< /dev/null`): restano bloccati fino al timeout (è
successo il 2026-11-07 alle 15:21, 16 minuti persi). Timeout dei comandi al massimo 10 minuti,
salvo build o benchmark lunghi noti; quelli lunghi in background.

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
