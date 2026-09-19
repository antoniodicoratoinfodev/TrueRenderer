# TrueRenderer — istruzioni per i prossimi incrementi

- Il nome corrente è **TrueRenderer**. TrueVision sopravvive soltanto nei nomi storici del documento e dei backup.
- Leggere `STATO.md` e le sezioni pertinenti di `docs/TrueRenderer-Architettura.md` prima di modificare un sottosistema.
- Aggiornare durante lo sviluppo soltanto `STATO.md` per implementato, verificato, aperto, mancante e caselle operative. Non duplicare cronologia o stato in altri Markdown. Aggiornare specifiche/ADR quando cambiano requisiti o decisioni, README quando cambiano le informazioni per l'utente e rapporti quando si eseguono nuove campagne.
- Eseguire `python3 scripts/sync-docs.py` quando cambiano le fonti della specifica anteprime: mantiene l'appendice E nelle due architetture e un rimando stabile a `STATO.md`, senza copiarne il registro. Non serve per i soli aggiornamenti di stato. Non ricreare la vecchia copia sul Desktop.
- Conservare immutata `docs/TrueVision-Architettura.originale-v1.2.md`. Non sostituire le modifiche dell'utente fuori dal blocco di avanzamento.
- Usare `scripts/cargo-local.sh` per la toolchain locale. Eseguire le verifiche appropriate; `scripts/verify.sh` raccoglie i controlli completi e `--gui` aggiunge la prova nativa.
- Il bundle macOS usa due servizi XPC; il binario fuori bundle usa pipe. Dopo modifiche a broker/decoder/bundle eseguire anche `scripts/test-xpc-integration.py` sul pacchetto. Decisioni e limiti in `docs/isolamento-decoder-e-formati.md#adr-0002`; non trasformare la supervisione RSS o la firma ad hoc in garanzie di rilascio.
- Non dichiarare conclusi R0–R4 o disponibili Standard/Riferimento prima dei rispettivi gate. Dalla 0.1.3, su richiesta del titolare, i file esterni sono ammessi come Anteprima soltanto nel bundle macOS con servizi XPC/App Sandbox: vedere ADR 0004. Il worker su pipe mantiene la allowlist; nessun fallback per gli esterni fuori dall'isolamento OS.
- Il repository è pubblico con licenza proprietaria: conservare LICENSE e i diritti delle dipendenze. Non pubblicare var/, database, backup, toolchain, credenziali o fotografie dell'utente.
- Gli originali sono in sola lettura. `var/library.sqlite` e `var/backups` sono dati durevoli, non cache da pulire.
