# Dipendenze e stato di distribuzione

TrueRenderer è un prototipo con **licenza proprietaria**, scelta dal titolare. Il testo in `LICENSE` si applica al codice, alla documentazione e agli asset originali del progetto. Il repository pubblico permette la consultazione e i diritti previsti dai termini GitHub; non concede una licenza open source. Le dipendenze mantengono le proprie licenze.

`Cargo.lock` blocca versioni e checksum. `reports/dependency-inventory.json` e `.csv` inventariano le dipendenze risolte per build/test macOS; i testi disponibili nei package sono raccolti in `reports/dependency-notices/`. È un inventario tecnico, non una distinta di rilascio approvata. Prima della distribuzione servono SBOM formale, verifica delle licenze effettive e degli obblighi per target/canale, firma e notarizzazione previste dal piano.

Il corpus è generato da formule in `scripts/generate-corpus.py`, senza fotografie o risorse di terzi. I valori dei campioni cromatici sono dati sintetici illustrativi: non rappresentano misure certificate di una carta fotografica commerciale.

Rigenerazione dell'inventario: `python3 scripts/dependency-inventory.py` dopo ogni modifica a Cargo.lock.

Dalla 0.1.3 il decoder macOS esterno usa ImageIO, Core Image/CIRAWFilter e ColorSync forniti dal sistema operativo; questi framework non sono copiati nel repository. La compatibilità RAW dipende dalla versione macOS e dalla fotocamera. Non viene incorporato LibRaw in questa versione. Anche i file di prova dei formati sono generati dal progetto, senza fotografie di terzi; `cwebp` serve esclusivamente a generare la prova WebP e non viene incorporato nell'app.
