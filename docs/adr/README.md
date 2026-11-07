# Architecture Decision Records

Ogni decisione architetturale importante ha un file `NNNN-titolo.md` con contesto, decisione e
conseguenze. Una decisione non si modifica: se cambia, si scrive una nuova ADR che la sostituisce
e si aggiorna lo stato di quella vecchia ("Sostituita da ADR-NNNN").

| ADR | Titolo | Stato |
|---|---|---|
| [0001](0001-riscrittura-e-stack.md) | Riscrittura da zero, stesso stack | Accettata |
| [0002](0002-workspace-e-crate.md) | Workspace Cargo e separazione dei crate | Accettata |
| [0003](0003-privilegi-e-sicurezza.md) | Esecuzione elevata, avvio senza UAC, superficie d'attacco ridotta | Accettata |
| [0004](0004-identita-dei-volumi.md) | Identità stabile dei volumi | Accettata |
| [0005](0005-modello-dati-ad-albero.md) | Modello dati ad albero | Accettata (unicità dei nomi sostituita da ADR-0015) |
| [0006](0006-enumerazione-e-riconciliazione.md) | Enumerazione (MFT o walker) e riconciliazione unica | Accettata |
| [0007](0007-aggiornamenti-usn-e-sorveglianza.md) | Aggiornamenti: journal USN, sorveglianza, riconciliazione | Accettata |
| [0008](0008-accesso-al-database.md) | Accesso al database: uno scrittore, molti lettori | Accettata |
| [0009](0009-motore-dei-job.md) | Motore dei job ed eventi verso l'interfaccia | Accettata |
| [0010](0010-indice-di-ricerca.md) | Indice di ricerca | **Proposta** |
| [0011](0011-hash-e-invalidazione.md) | Hash del contenuto in background e invalidazione | Accettata |
| [0012](0012-eliminazione-sicura.md) | Eliminazione sicura | Accettata |
| [0013](0013-video-simili.md) | Decodifica video per i video simili | **Proposta** |
| [0014](0014-frontend.md) | Frontend: Svelte 5, IPC tipizzato, i18n, tema | Accettata |
| [0015](0015-dettagli-modello-dati-t1.md) | Dettagli del modello dati decisi in T1 | Accettata |
