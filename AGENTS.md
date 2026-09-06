# TrueRenderer — istruzioni per i prossimi incrementi

- Il nome corrente è **TrueRenderer**. TrueVision sopravvive soltanto nei nomi storici del documento e dei backup.
- Leggere `PLAN.md`, `docs/avanzamento.md` e le sezioni pertinenti di `docs/TrueRenderer-Architettura.md` prima di modificare un sottosistema.
- Aggiornare durante lo sviluppo cosa è implementato, verificato, aperto e mancante in `docs/avanzamento.md`; aggiornare le caselle del piano ed eseguire `python3 scripts/sync-docs.py` per riportarlo anche nel documento originale sul Desktop.
- Conservare immutata `docs/TrueVision-Architettura.originale-v1.2.md`. Non sostituire le modifiche dell'utente fuori dal blocco di avanzamento.
- Usare `scripts/cargo-local.sh` per la toolchain locale. Eseguire le verifiche appropriate; `scripts/verify.sh` raccoglie i controlli completi e `--gui` aggiunge la prova nativa.
- Il bundle macOS usa due servizi XPC; il binario fuori bundle usa pipe. Dopo modifiche a broker/decoder/bundle eseguire anche `scripts/test-xpc-integration.py` sul pacchetto. Decisioni e limiti in `docs/adr/0002-xpc-decoder-r0.md`; non trasformare la supervisione RSS o la firma ad hoc in garanzie di rilascio.
- Non dichiarare conclusi R0–R4 o disponibili Standard/Riferimento prima dei rispettivi gate. Dalla 0.1.3, su richiesta del titolare, i file esterni sono ammessi come Anteprima soltanto nel bundle macOS con servizi XPC/App Sandbox: vedere ADR 0004. Il worker su pipe mantiene la allowlist; nessun fallback per gli esterni fuori dall'isolamento OS.
- Il repository è pubblico con licenza proprietaria: conservare LICENSE e i diritti delle dipendenze. Non pubblicare var/, database, backup, toolchain, credenziali o fotografie dell'utente.
- Gli originali sono in sola lettura. `var/library.sqlite` e `var/backups` sono dati durevoli, non cache da pulire.
