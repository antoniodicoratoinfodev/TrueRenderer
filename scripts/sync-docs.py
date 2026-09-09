#!/usr/bin/env python3
"""Sincronizza registro e specifica anteprime nelle due architetture del progetto.

Conserva il testo utente fuori dai blocchi gestiti; non ricrea file sul Desktop.
"""
from pathlib import Path
import os
import re

root = Path(__file__).resolve().parents[1]
original = root / "TrueVision-Architettura.md"
canonical = root / "docs/TrueRenderer-Architettura.md"
begin, end = "<!-- TR_PROGRESS_START -->", "<!-- TR_PROGRESS_END -->"
progress = (root / "docs/avanzamento.md").read_text()
block = f"{begin}\n{progress}\n{end}\n"
spec_begin, spec_end = "<!-- TR_PREVIEW_SPEC_START -->", "<!-- TR_PREVIEW_SPEC_END -->"


def relative_links(body, source, target):
    def replace(match):
        label, link = match.groups()
        if link.startswith(("http:", "https:", "#", "mailto:")):
            return match.group(0)
        name, separator, anchor = link.partition("#")
        rebased = os.path.relpath((source.parent / name).resolve(), target.parent)
        return f"[{label}]({rebased}{separator}{anchor})"
    return re.sub(r"\[([^\]\n]+)\]\(([^)\s]+)\)", replace, body)


def preview_spec(target):
    sections = [spec_begin, "### E. Specifica integrata — anteprime, cache e prestazioni\n",
        "Questa appendice integra per intero la specifica revisionata e ADR 0005–0006. "
        "Le sezioni numerate nei testi seguenti sono riferimenti interni ai rispettivi documenti. "
        "Stato applicato, prove e requisiti aperti restano distinti. "
        "Fonte modificabile: i tre documenti in `docs/`; aggiornare con `scripts/sync-docs.py`.\n"]
    for source in (root / "docs/progetto-anteprime-cache-prestazioni.md",
                   root / "docs/adr/0005-cache-cartella.md",
                   root / "docs/adr/0006-anteprime-residenza-compute.md"):
        body = relative_links(source.read_text(), source, target)
        # Shift headings outside fences; keep examples and diagrams intact.
        fenced = False
        lines = []
        for line in body.splitlines():
            if line.startswith("```"):
                fenced = not fenced
            if not fenced and line.startswith("#"):
                line = "##" + line
            lines.append(line)
        sections.append("\n".join(lines))
    return "\n\n".join(sections) + f"\n{spec_end}\n"


def replace_block(body, start, finish, content):
    if body.count(start) != body.count(finish) or body.count(start) > 1:
        raise ValueError(f"Blocco documentale non valido: {start}")
    if start in body:
        before, rest = body.split(start, 1)
        _, after = rest.split(finish, 1)
        return before + content.rstrip() + after
    return body.rstrip() + "\n\n" + content


for path in (original, canonical):
    body = path.read_text() if path.exists() else canonical.read_text()
    if begin in body or end in body:
        body = replace_block(body, begin, end, block)
    else:
        first, rest = body.split("\n", 1)
        body = first + "\n\n" + block + rest
    body = replace_block(body, spec_begin, spec_end, preview_spec(path))
    path.write_text(body)
    print(path)
