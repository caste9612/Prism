# ADR-0019 — Interfaccia: linguaggio visivo "B+C" e navigazione dal disco

- **Stato**: Accettata
- **Data**: 2026-11-08
- **Sostituisce in parte**: le scelte di interfaccia di REVIEW D14, D15 (solo l'aspetto), D22 e
  D28 (navigazione); ADR-0014 resta valida per tecnologia, tipi, i18n e tema.

## Contesto

Nella revisione finale (REVIEW U5) l'utente ha bocciato l'interfaccia costruita in T3–T10:
aspetto da dashboard web generica, colore indaco, menu laterale a icone, schede grandi con molto
spazio sprecato e poche informazioni, troppi pulsanti ripetuti, il riquadro dei dischi nuovi che
occupava mezza pagina. Ha chiesto esplicitamente di **non** imitare Windows 11 (Fluent) e di
avere un'interfaccia "sia funzionale che bella".

Abbiamo disegnato insieme dei mockup su una tela di claude.ai (privata dell'utente,
`https://claude.ai/artifact/RxU7WnmNgXxeLAYFheJyX8`): tre stili (A "Grafite" scuro essenziale,
B "Etichetta" chiaro e deciso, C "Strumento" denso e tecnico), poi una versione che unisce B e
C, poi tutte le schermate e quattro più quattro proposte d'icona. L'utente ha approvato la
versione **B+C**, tutte le schermate così come disegnate e l'icona **"prisma a strati"**.

## Decisione

**Navigazione: tutto parte dal disco.**

- Nessun menu laterale. In alto una barra fissa: logo, percorso (Catalogo › disco), casella di
  ricerca sempre visibile (con l'indicazione della scorciatoia), pulsante arancione dei dischi
  nuovi (un riquadro a comparsa, non più un pannello nella pagina), impostazioni.
- La schermata principale è il catalogo: in cima il blocco **Tutto il catalogo** (totali e
  accesso a Duplicati, Simili, Backup e Tipi di file su tutti i dischi), poi le schede compatte
  dei dischi con i filtri Tutti / Collegati / Rete / Nel cassetto e la scelta Schede / Elenco,
  poi le attività in corso e le ultime azioni, in fondo una barra di stato.
- Il clic su un disco apre la sua schermata: intestazione con colore, posizione e numeri, e le
  schede **Spazio, Duplicati, Simili, Backup, Pulizia, Salute, Tipi di file** limitate a quel
  disco.
- La ricerca della barra in alto mostra i risultati nella finestra principale; Ctrl+Space apre
  sempre la finestra di ricerca separata (D28), con lo stesso aspetto.

**Linguaggio visivo "B+C".**

- Da B: base chiara (`#ECEEF1`), schede bianche, inchiostro `#111418`; titoli e numeri grandi
  in **Space Grotesk**; i dischi come oggetti fisici, con il loro colore (un bollino) e la
  posizione fisica in maiuscoletto; il blocco del catalogo in nero (chiaro nel tema scuro);
  schede a pillola dentro un disco; barre d'azione nere fisse in basso per le operazioni sui
  file.
- Da C: dati in **IBM Plex Mono** (dimensioni, conteggi, date, percorsi); la barra dello spazio
  di ogni disco divisa per categorie di file; una griglia di dati su ogni scheda (usato,
  libero, file, salute); filtri al posto delle sezioni; pannelli delle attività e delle ultime
  azioni; barra di stato; tabelle con colonne.
- Testo dell'interfaccia in **Inter**. Accento **arancione** `#F04E23` (testo inchiostro sopra;
  `#C2410C` per testo arancione su fondo chiaro) per ciò che chiede attenzione: dischi nuovi,
  spazio recuperabile, l'azione principale delle operazioni sui file.
- Tema scuro con gli stessi ruoli (fondo `#0E1114`, schede `#171B20`, accento `#FF6A3D`); il
  tema segue Windows con scelta manuale come prima (ADR-0014).
- I caratteri sono **inclusi nell'app** (pacchetti `@fontsource`, licenza SIL OFL): la CSP non
  ammette contenuti remoti (ARCHITECTURE §8).

**Icona: prisma a strati.** Riquadro nero con angoli arrotondati; il prisma è fatto di strati:
la punta bianca, poi arancione, blu e verde. Sostituisce l'icona precedente (prisma su indaco
con un raggio che diventa arcobaleno), che usava i colori abbandonati e ricordava da vicino una
copertina di album famosa. La figura del logo nell'interfaccia è la stessa.

## Conseguenze

- I componenti Svelte si riscrivono; la logica di presentazione in `ui/src/lib/*.ts` e i
  comandi del backend restano, salvo due aggiunte: l'elenco delle ultime azioni del registro e
  un riepilogo per categorie di ogni volume per le barre delle schede.
- I test end-to-end seguono i nuovi percorsi (ogni criterio resta verificato, si cambia solo
  come ci si arriva).
- I disegni sulla tela sono il riferimento per l'aspetto; questa ADR e ARCHITECTURE §7 per le
  regole.
