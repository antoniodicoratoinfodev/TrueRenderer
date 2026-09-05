# Dipendenze e stato di distribuzione

TrueRenderer è in fase di prototipo interno. La licenza finale del codice applicativo e il modello di distribuzione sono decisioni R0 ancora aperte; non è stata applicata automaticamente una licenza open source al codice dell'utente.

`Cargo.lock` blocca versioni e checksum. `reports/dependency-inventory.json` e `.csv` inventariano le dipendenze risolte per build/test macOS; i testi disponibili nei package sono raccolti in `reports/dependency-notices/`. È un inventario tecnico, non una distinta di rilascio approvata. Prima della distribuzione servono SBOM formale, verifica delle licenze effettive e degli obblighi per target/canale, firma e notarizzazione previste dal piano.

Il corpus è generato da formule in `scripts/generate-corpus.py`, senza fotografie o risorse di terzi. I valori dei campioni cromatici sono dati sintetici illustrativi: non rappresentano misure certificate di una carta fotografica commerciale.

Rigenerazione dell'inventario: `python3 scripts/dependency-inventory.py` dopo ogni modifica a Cargo.lock.
