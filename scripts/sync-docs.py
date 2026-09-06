#!/usr/bin/env python3
"""Aggiorna solo il blocco di avanzamento, conservando il resto di ciascuna copia."""
from pathlib import Path

root = Path(__file__).resolve().parents[1]
original = root.parent / "TrueVision-Architettura.md"
canonical = root / "docs/TrueRenderer-Architettura.md"
begin, end = "<!-- TR_PROGRESS_START -->", "<!-- TR_PROGRESS_END -->"
progress = (root / "docs/avanzamento.md").read_text()
block = f"{begin}\n{progress}\n{end}\n"
for path in (original, canonical):
    body = path.read_text() if path.exists() else canonical.read_text()
    if begin in body:
        before, rest = body.split(begin, 1)
        _, after = rest.split(end, 1)
        body = before + block.rstrip() + after
    else:
        body = body.replace("TrueVision", "TrueRenderer").replace("truevision", "truerenderer")
        body = body.replace("tv-", "tr-")
        body = body.replace("**Stato: proposta pre-R0; le capability sono criteri di accettazione, non funzioni già disponibili.**", "**Stato: sviluppo R0 avviato; capacità implementate e verifiche effettive sono elencate nel registro §0. Il resto rimane specifica.**")
        first, rest = body.split("\n", 1)
        body = first + "\n\n" + block + rest
    path.write_text(body)
    print(path)
