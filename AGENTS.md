# TrueRenderer — istruzioni per i prossimi incrementi

- Il nome corrente è **TrueRenderer**. TrueVision sopravvive soltanto nei nomi storici del documento e dei backup.
- Leggere `PLAN.md`, `docs/avanzamento.md` e le sezioni pertinenti di `docs/TrueRenderer-Architettura.md` prima di modificare un sottosistema.
- Aggiornare durante lo sviluppo cosa è implementato, verificato, aperto e mancante in `docs/avanzamento.md`; aggiornare le caselle del piano ed eseguire `python3 scripts/sync-docs.py` per riportarlo anche nel documento originale sul Desktop.
- Conservare immutata `docs/TrueVision-Architettura.originale-v1.2.md`. Non sostituire le modifiche dell'utente fuori dal blocco di avanzamento.
- Usare `scripts/cargo-local.sh` per la toolchain locale. Eseguire le verifiche appropriate; `scripts/verify.sh` raccoglie i controlli completi e `--gui` aggiunge la prova nativa.
- Non dichiarare conclusi R0–R4 o disponibili Standard/Riferimento prima dei rispettivi gate. Non rimuovere la allowlist per abilitare gli archivi esterni senza l'isolamento OS richiesto.
- Gli originali sono in sola lettura. `var/library.sqlite` e `var/backups` sono dati durevoli, non cache da pulire.
