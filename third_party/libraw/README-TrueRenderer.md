# LibRaw 0.22.2 — sorgenti upstream non modificati

Origine: tag ufficiale <https://github.com/LibRaw/LibRaw/releases/tag/0.22.2>. URL dell'archivio, SHA-256 e hash di ciascuno dei 106 file upstream sono in `manifest-truerenderer.json`; il manifest e questo README sono aggiunte TrueRenderer. La copia precedente 0.21.1 proveniva dal crate `libraw_rs_vendor` 1.0.0.

La regola `-text` in `.gitattributes` conserva anche i file upstream con CRLF: Git non deve normalizzarli, altrimenti gli hash e l'affermazione di copia non modificata diventerebbero falsi al primo checkout su un altro OS.

**Nessun sorgente upstream è stato modificato.** Le opzioni di build MSVC e `LIBRAW_CALLOC_RAWSTORE` vivono in `crates/tr-worker/build.rs`, lo shim in `native/libraw/`. Eventuali future modifiche ai sorgenti coperti vanno identificate come richiede la CDDL §3.3.

## Licenza scelta

TrueRenderer usa questi file sotto **CDDL-1.0**, una delle due opzioni che LibRaw offre. Il testo è in `LICENSE.CDDL`; `LICENSE.LGPL` è conservato perché fa parte della distribuzione originale, non perché sia l'opzione scelta. Le ragioni della scelta e gli obblighi che ne derivano sono in [ADR 0008](../../docs/adr/0008-libraw-licenza-e-collegamento.md).

## Cosa è incluso e cosa no

Inclusi: `src/`, `libraw/`, `internal/`, licenze, copyright, changelog e `Makefile.am` per confronto con l'elenco upstream. Le 79 unità compilate sono elencate nel manifest. Esclusi OpenMP, DNG SDK, RawSpeed, LCMS, Jasper e JPEG opzionali; i file `_ph.cpp` sono inclusi dalle rispettive unità principali.

**Non sono inclusi i demosaic pack GPL2 e GPL3** di LibRaw, distribuiti a parte. Non devono esserlo: la loro licenza è incompatibile con un prodotto proprietario. Il controllo va rifatto a ogni aggiornamento di versione, verificando che nessun sorgente compilato contenga il testo della GNU General Public License.
