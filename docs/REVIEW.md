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
| L1 | 2026-11-07 | Memoria massima durante l'indicizzazione ~315 B per voce (istantanea `ScanTree` completa in memoria) | Un volume da 5 milioni di voci richiederebbe ~1,5 GB durante l'indicizzazione (NF3 chiede ≤ 500 MB a regime) | Rimandato (dopo le funzioni della 1.0) |
| L2 | 2026-11-07 | Lettore MFT: una `$MFT` con `$ATTRIBUTE_LIST` nel record 0 (MFT enormi e frammentate) viene rifiutata; file compressi/sparsi non verificati su disco reale | Su quei volumi si usa il walker (più lento, stessi risultati logici) | Rimandato |
| L3 | 2026-11-07 | NTFS scrive in ritardo nei record MFT le date delle cartelle: due letture vicine possono differire di pochi ms | Solo estetico; si corregge alla modifica successiva della cartella | Accettato (ADR-0018) |
| L4 | 2026-11-07 | Aggiornamento dal journal: la mappa dei riferimenti (≈0,25 s per 1,3 milioni di voci) si ricarica a ogni aggiornamento | Aggiornamenti continui un po' più costosi del necessario | Da ottimizzare nel motore dei job (T3) |

## 4. Come provare l'app

Da compilare quando l'app è utilizzabile: come avviarla, quali schermate e flussi guardare, la
checklist manuale di ogni tappa. Le prove che cancellano, spostano o puliscono file si fanno
**solo** sul disco di test PRISMTEST o in cartelle di prova (TESTING S5).
