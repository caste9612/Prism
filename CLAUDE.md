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
- **Autonomia totale fino alla 1.0, senza soste** (decisione dell'utente, 2026-11-07
  pomeriggio). Si procede tappa dopo tappa fino all'ultima voce della roadmap che non richiede
  l'utente. La sosta di revisione di fine T3 è **annullata**: l'utente fa **una sola revisione
  finale** (sera del 2026-11-07), guidata da `docs/REVIEW.md`. A fine tappa, report in
  `docs/PROGRESS.md` come sempre, poi si continua subito.
- **Non terminare il loop** (niente `ScheduleWakeup` con `stop: true`, niente "mi fermo e
  aspetto l'utente") finché in roadmap resta lavoro fattibile. Se una voce è bloccata, annotala
  in `docs/REVIEW.md` §3 e passa alla successiva indipendente. Il loop si chiude solo quando tutto
  ciò che resta richiede l'utente, con `docs/REVIEW.md` completo.
- **Decisioni: si prendono in autonomia e si registrano.** Sia quelle tecniche sia quelle di
  prodotto non ancora prese (punti **[DA CONFERMARE]**, domande aperte del capitolo 8 della SPEC,
  dettagli di interfaccia e di comportamento): scegli l'opzione più prudente e reversibile,
  coerente con SPEC e ADR, e **registrala in `docs/REVIEW.md` §1** (decisione, alternative, come
  cambiarla). Le scelte tecniche importanti restano anche in un'ADR con i dati.
- **Ciò che richiede davvero l'utente** (certificato di firma Q7, merge sul ramo principale
  Q11, test su PC pulito, collegamento fisico di dischi, giudizio estetico): prepara tutto il
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
