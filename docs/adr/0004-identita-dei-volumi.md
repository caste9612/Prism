# ADR-0004 — Identità stabile dei volumi

- **Stato**: Accettata; regole di abbinamento rifinite in ADR-0016 (T2)
- **Data**: 2026-11-07

## Contesto

Il vecchio Prism identificava i dischi con la lettera di unità (`C:\`). Le lettere cambiano
(chiavette e dischi esterni prendono la prima lettera libera) e lo stesso disco poteva
diventare un "disco nuovo", mentre due dischi diversi potevano essere confusi. Per un catalogo
offline l'identità del disco è il dato più importante.

## Decisione

Ogni volume ha un `id` interno. Al collegamento, un volume viene abbinato al catalogo con
queste chiavi, in ordine:

1. **NTFS**: seriale del file system a 64 bit (`FSCTL_GET_NTFS_VOLUME_DATA`) più capacità.
2. **Altri file system locali**: seriale del volume a 32 bit (`GetVolumeInformationW`) più
   tipo di file system e capacità.
3. **Unità di rete**: percorso UNC normalizzato (`\\server\share`), ricavato anche dalle
   lettere mappate (`WNetGetConnection`).
4. Il GUID del volume (`\\?\Volume{...}`) è un'informazione aggiuntiva: vale solo su questa
   macchina.

Se le chiavi non bastano a decidere (es. due dischi clonati con lo stesso seriale), Prism
chiede all'utente (F02-AC3). La lettera o il percorso attuale è solo un attributo
(`last_mount`), mai una chiave.

## Conseguenze

- Etichette, hash e storia SMART seguono il disco anche se cambia lettera.
- Riformattare un disco ne cambia il seriale: per Prism diventa un volume nuovo, ed è corretto
  così (il contenuto è comunque diverso).
