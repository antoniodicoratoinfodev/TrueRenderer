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
| [Sviluppo fotografico non distruttivo](progetto-sviluppo-fotografico.md) | Progetto dei controlli e ADR 0011: luce/WB/colore, dettaglio, ottica, maschere, ricette e parità vista/export; attività in STATO.md |
| [Color e livelli](color%20e%20livelli.md) | Ricerca funzionale e progetto esteso: strumenti colore, selezioni, maschere, livelli, composizione e interfaccia; dipendenze e criteri di accettazione, senza attribuire nuove capacità al bundle |
| [Navigatore filesystem commutabile](progetto-navigatore-filesystem.md) | Requisiti, decisioni, schede Libreria/Esplora, preferiti e matrice di accettazione; avanzamento in STATO.md |

## Decisioni integrate per argomento

Specifiche e decisioni risiedono tutte direttamente in questa cartella, senza sottocartella ADR. I numeri storici restano come ancore stabili:

- [Anteprime, cache e prestazioni](progetto-anteprime-cache-prestazioni.md): campionamento fisico (0003), cache per cartella (0005), residenza e compute (0006).
- [Motori RAW](progetto-motori-raw.md): ricette e limiti, licenza e integrazione LibRaw (0008).
- [Esportazione, precisione SDR e FITS](esportazione-precisione-fits.md): tre percorsi distinti (0010).
- [Sviluppo fotografico non distruttivo](progetto-sviluppo-fotografico.md): ricetta reversibile condivisa da vista ed export, controlli e correzioni ottiche della futura estensione (0011).
- [Isolamento decoder e formati](isolamento-decoder-e-formati.md): XPC macOS (0002), file esterni e pubblicazione (0004), LPAC Windows (0009).

## Lavori conclusi e verifiche

Sintesi, limiti e collegamenti ai rapporti JSON sono nel [registro unico](../STATO.md#registro-delle-verifiche-e-degli-incrementi). I resoconti Markdown assorbiti sono recuperabili dalla cronologia Git; i rapporti numerici conservano hash, esiti negativi e perimetro delle singole campagne.

## Copie e supporti da mantenere

- [TrueVision-Architettura nella radice](../TrueVision-Architettura.md): copia con nome storico, mantenuta dal sincronizzatore insieme alla versione in `docs/`.
- [Originale v1.2](TrueVision-Architettura.originale-v1.2.md): backup immutabile, da non allineare al codice corrente.
- [AGENTS](../AGENTS.md): istruzioni operative. Aggiornare normalmente solo STATO.md. Eseguire `python3 scripts/sync-docs.py` quando cambiano le fonti della specifica anteprime, senza modificare manualmente i blocchi generati.
- [Laboratorio XPC](../experiments/macos-xpc/README.md): esperimento iniziale, distinto dal bundle corrente.
- [Fixture decoder](../crates/tr-worker/tests/fixtures/README.md): input sintetici delle regressioni.
- [Modello di segnalazione](../.github/ISSUE_TEMPLATE/bug_report.md): supporto alle issue GitHub.
- [NOTICE](../NOTICE.md), [LICENSE](../LICENSE), [README LibRaw](../third_party/libraw/README-TrueRenderer.md) e notice delle dipendenze: attribuzioni e informazioni da conservare.
