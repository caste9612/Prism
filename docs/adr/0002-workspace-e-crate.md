# ADR-0002 — Workspace Cargo e separazione dei crate

- **Stato**: Accettata
- **Data**: 2026-11-07

## Contesto

Nel vecchio Prism logica di dominio, accesso a Win32, comandi Tauri e strumenti di debug
(egui, benchmark) stavano nello stesso crate: la logica non era testabile senza Tauri, e
l'app includeva dipendenze pesanti che non usava.

## Decisione

Workspace con crate separati:

- `prism-core`: dominio, archivio, job, ricerca, duplicati, backup. **Nessun `unsafe`**,
  nessuna API Win32 diretta: la piattaforma è raggiunta tramite trait, con implementazioni
  finte nei test.
- `prism-win`: implementazioni Windows dei trait. Unico crate dove `unsafe` è permesso; ogni
  blocco ha un commento `// SAFETY:` (lint `clippy::undocumented_unsafe_blocks`).
- `prism-cli`: strumento di sviluppo (scansioni, benchmark, diagnostica).
- `prism-app`: Tauri, solo comandi sottili ed eventi (da T3).
- `prism-shell`: helper non elevato per il menu contestuale (da T9).

Lint comuni definiti una volta nel `Cargo.toml` del workspace (`unsafe_code = "deny"`,
clippy con `-D warnings` in CI). Toolchain fissata in `rust-toolchain.toml`.

## Conseguenze

- La logica principale si testa con `cargo test` senza interfaccia né privilegi.
- Aggiungere un trait per ogni accesso alla piattaforma costa un po' di codice in più, ma
  rende possibili i test di riconciliazione e dei job con dati controllati.
