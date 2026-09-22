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
| [Esportazione, precisione SDR e FITS](esportazione-precisione-fits.md) | ADR 0010: formati di uscita, DNG distinti, superficie e dati scientifici |
| [Navigatore filesystem commutabile](progetto-navigatore-filesystem.md) | Requisiti, decisioni, schede Libreria/Esplora, preferiti e matrice di accettazione; avanzamento in STATO.md |
| [Verifica nativa navigatore](../reports/filesystem-browser-macos.json) | Layout IT/EN, finestra compatta e 200%; hash e limiti della campagna Mac |
| [Navigatore stile IDE](../reports/navigator-ide-macos.json) | Revisione estetica, schermate aggiornate, regressioni UI e bundle Mac |
| [Barra compatta e schede stabili](../reports/navigator-layout-macos.json) | Regressione cambio pannello, layout nativi, preferenze e XPC del bundle aggiornato |
| [Cache e suite Mac, 15 settembre](../reports/cache-integrity-macos.json) | Correzione hard link e verifica della base prima delle misure di navigazione |
| [Probe della superficie, 15 settembre](../reports/navigation-surface-macos.json) | Selezioni CPU/GPU e cache distinte; readback verificato, qualifica evento→display aperta |
| [Immagini grandi, pressione e RAW Mac](../STATO.md#campagne-raw-concluse) | 12/24/45 MP, D750 e correzione stack XPC; superamento memoria esplicito |
| [Correzione Fit, 15 settembre](../reports/fit-coverage-macos.json) | Riserva ampia entro quota; 240 azioni e copertura completa nella traccia, con limiti espliciti |
| [Transizioni del viewer, 15 settembre](../reports/navigation-transitions-macos.json) | Zoom, pan, qualità e 1:1; copertura provvisoria misurata e limite al ritorno a Fit |

## Decisioni integrate per argomento

Specifiche e decisioni risiedono tutte direttamente in questa cartella, senza sottocartella ADR. I numeri storici restano come ancore stabili:

- [Anteprime, cache e prestazioni](progetto-anteprime-cache-prestazioni.md): campionamento fisico (0003), cache per cartella (0005), residenza e compute (0006).
- [Motori RAW](progetto-motori-raw.md): ricette e limiti, licenza e integrazione LibRaw (0008).
- [Esportazione, precisione SDR e FITS](esportazione-precisione-fits.md): tre percorsi distinti (0010).
- [Isolamento decoder e formati](isolamento-decoder-e-formati.md): XPC macOS (0002), file esterni e pubblicazione (0004), LPAC Windows (0009).

## Lavori conclusi e verifiche

Sintesi, limiti e collegamenti ai rapporti JSON sono nel [registro unico](../STATO.md#registro-delle-verifiche-e-degli-incrementi). I resoconti Markdown assorbiti sono recuperabili dalla cronologia Git; i rapporti numerici conservano hash, esiti negativi e perimetro delle singole campagne.

## Copie e supporti da mantenere

- [TrueVision-Architettura nella radice](../TrueVision-Architettura.md): copia richiesta dal flusso del titolare e dal sincronizzatore; non eliminabile nel flusso attuale.
- [Originale v1.2](TrueVision-Architettura.originale-v1.2.md): backup immutabile, da non allineare al codice corrente.
- [AGENTS](../AGENTS.md): istruzioni operative. Aggiornare normalmente solo STATO.md. Eseguire `python3 scripts/sync-docs.py` quando cambiano le fonti della specifica anteprime, senza modificare manualmente i blocchi generati.
- [Laboratorio XPC](../experiments/macos-xpc/README.md): esperimento iniziale, distinto dal bundle corrente.
- [Fixture decoder](../crates/tr-worker/tests/fixtures/README.md): input sintetici delle regressioni.
- [Modello di segnalazione](../.github/ISSUE_TEMPLATE/bug_report.md): supporto alle issue GitHub.
- [NOTICE](../NOTICE.md), [LICENSE](../LICENSE), [README LibRaw](../third_party/libraw/README-TrueRenderer.md) e notice delle dipendenze: attribuzioni e informazioni da conservare.
