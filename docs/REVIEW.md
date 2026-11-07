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

## 2. Cose che richiedono l'utente

| # | Cosa | Perché serve l'utente | Cosa è già pronto |
|---|---|---|---|
| U1 | Firma del codice (SPEC Q7) | Scelta e acquisto del certificato | Da preparare in T10: firma configurabile senza modifiche al codice |
| U2 | Merge di `rewrite` sul ramo principale e rilascio 1.0 (Q11) | Approvazione | — |
| U3 | Prova su un PC Windows 11 pulito (installer, avvio con Windows, aggiornamenti) | Serve un'altra macchina | — |
| U4 | Collegare e scollegare un disco esterno (T2, F02) | Azione fisica | Test automatico del cambio di lettera sul disco di test già passato |
| U5 | Aspetto e usabilità dell'interfaccia | Giudizio estetico | — |
| U6 | Rimuovere il runner amministratore a fine sviluppo | Riduce la superficie d'attacco | `scripts/admin/uninstall-admin-runner.ps1` |

## 3. Problemi aperti e limiti noti

| # | Data | Problema | Impatto | Stato |
|---|---|---|---|---|

## 4. Come provare l'app

Da compilare quando l'app è utilizzabile: come avviarla, quali schermate e flussi guardare, la
checklist manuale di ogni tappa. Le prove che cancellano, spostano o puliscono file si fanno
**solo** sul disco di test PRISMTEST o in cartelle di prova (TESTING S5).
