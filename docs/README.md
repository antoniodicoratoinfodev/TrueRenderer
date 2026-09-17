# Documentazione di TrueRenderer

Indice delle specifiche, delle decisioni e delle evidenze. Stato corrente e attività sono mantenuti soltanto in [STATO.md](../STATO.md).

## Documenti correnti

| Documento | Uso |
|---|---|
| [README del progetto](../README.md) | Funzioni, piattaforme, build e limiti |
| [Stato e piano](../STATO.md) | Punto di ripresa, attività, gate, matrice e registro degli incrementi |
| [Architettura](TrueRenderer-Architettura.md) | Specifica; distinguere proposta e stato implementato |
| [Anteprime, cache e prestazioni](progetto-anteprime-cache-prestazioni.md) | Fonte della specifica integrata, con requisiti ancora da qualificare |
| [Motori RAW](progetto-motori-raw.md) | Contratto, ricette e limiti sperimentali |
| [Restyling desktop](progetto-restyling.md) | Composizioni, stile, preferenze e verifica del layout bilingue |
| [Navigatore filesystem commutabile](progetto-navigatore-filesystem.md) | Piano da implementare: schede Libreria/Esplora nello stesso pannello sinistro, file/cartelle, preferiti, fasi e verifiche |
| [Verifiche](../reports/VERIFICA.md) | Cronologia delle campagne e rapporti con hash |
| [Cache e suite Mac, 15 settembre](../reports/cache-integrity-macos.json) | Correzione hard link e verifica della base prima delle misure di navigazione |
| [Probe della superficie, 15 settembre](../reports/navigation-surface-macos.json) | Selezioni CPU/GPU e cache distinte; readback verificato, qualifica evento→display aperta |
| [Immagini grandi, pressione e RAW Mac](verifica-grandi-raw-macos.md) | 12/24/45 MP, D750 e correzione stack XPC; superamento memoria esplicito |
| [Correzione Fit, 15 settembre](../reports/fit-coverage-macos.json) | Riserva ampia entro quota; 240 azioni e copertura completa nella traccia, con limiti espliciti |
| [Transizioni del viewer, 15 settembre](../reports/navigation-transitions-macos.json) | Zoom, pan, qualità e 1:1; copertura provvisoria misurata e limite al ritorno a Fit |

## Decisioni architetturali

Gli ADR vanno conservati: spiegano le decisioni nel loro contesto temporale.

- [0001 — prototipo controllato](adr/0001-prototipo-r0.md)
- [0002 — servizi XPC macOS](adr/0002-xpc-decoder-r0.md)
- [0003 — campionamento fisico](adr/0003-campionamento-fisico-r0.md)
- [0004 — formati esterni macOS e pubblicazione](adr/0004-formati-esterni-e-pubblicazione.md)
- [0005 — cache per cartella](adr/0005-cache-cartella.md)
- [0006 — anteprime, residenza e compute](adr/0006-anteprime-residenza-compute.md)
- [0007 — porta decoder e prima build Windows](adr/0007-porta-decoder-e-build-windows.md)
- [0008 — LibRaw e collegamento](adr/0008-libraw-licenza-e-collegamento.md)
- [0009 — confinamento Windows LPAC](adr/0009-isolamento-worker-windows.md), successivo al primo confine descritto nel piano Windows.

## Revisioni e prove storiche

Conservano problemi riprodotti, correzioni e limiti delle singole campagne. Per i difetti ancora aperti, leggere il registro e le note successive.

| Campagna | Correzioni / esito successivo |
|---|---|
| [Revisione prima di main](revisione-pre-main.md) | Correzioni nella seconda parte dello stesso documento |
| [Revisione serale del 13 settembre](revisione-continuata-2026-09-13.md) | [Correzioni della revisione serale](revisione-continuata-2026-09-13.md#correzioni) |
| [Ricontrollo generale del 14 settembre](revisione-generale-2026-09-14.md) | [Correzioni TIFF, RAW e cache](revisione-generale-2026-09-14.md#correzioni) |
| [Revisione aggiuntiva del 14 settembre](revisione-aggiuntiva-2026-09-14.md) | [Correzioni gamma e orientamento](revisione-aggiuntiva-2026-09-14.md#correzioni) |
| [Verifica dei motori sulla D40](verifica-motori-d40.md) | Supporto D40 e regressione D750 della campagna indicata |

## Accorpamenti eseguiti

| File eliminato | Contenuto conservato in |
|---|---|
| `piano-windows-raw.md` | ADR 0007 (storia e limiti del port), ADR 0005 (cache Windows), PLAN (attività aperte) |
| `licenza-libraw.md` | ADR 0008 (decisione e fonti della nota preliminare) |
| `correzioni-revisione-serale.md` | [Revisione serale, correzioni](revisione-continuata-2026-09-13.md#correzioni) |
| `correzioni-ricontrollo-generale.md` | [Revisione generale, correzioni](revisione-generale-2026-09-14.md#correzioni) |
| `correzioni-gamma-orientamento.md` | [Revisione aggiuntiva, correzioni](revisione-aggiuntiva-2026-09-14.md#correzioni) |

Il 17 settembre 2026 piano, avanzamento e ripresa sono stati accorpati in [STATO.md](../STATO.md), conservando attività, matrice dei requisiti, prove e limiti. I tre file precedenti sono rimossi; le versioni storiche restano in Git. Le architetture contengono un rimando stabile allo stato, senza copie del registro.

## Copie e supporti da mantenere

- [TrueVision-Architettura nella radice](../TrueVision-Architettura.md): copia richiesta dal flusso del titolare e dal sincronizzatore; non eliminabile nel flusso attuale.
- [Originale v1.2](TrueVision-Architettura.originale-v1.2.md): backup immutabile, da non allineare al codice corrente.
- [AGENTS](../AGENTS.md): istruzioni operative. Aggiornare normalmente solo STATO.md. Eseguire `python3 scripts/sync-docs.py` quando cambiano le fonti della specifica anteprime, senza modificare manualmente i blocchi generati.
- [Laboratorio XPC](../experiments/macos-xpc/README.md): esperimento iniziale, distinto dal bundle corrente.
- [Fixture decoder](../crates/tr-worker/tests/fixtures/README.md): input sintetici delle regressioni.
- [Modello di segnalazione](../.github/ISSUE_TEMPLATE/bug_report.md): supporto alle issue GitHub.
- [NOTICE](../NOTICE.md), [LICENSE](../LICENSE), [README LibRaw](../third_party/libraw/README-TrueRenderer.md) e notice delle dipendenze: attribuzioni e informazioni da conservare.
