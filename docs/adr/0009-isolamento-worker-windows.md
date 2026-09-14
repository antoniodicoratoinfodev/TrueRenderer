# ADR 0009 — confinamento del worker Windows dopo l'audit

Data: 13 settembre 2026. Stato: implementato e verificato localmente su Windows, [rapporto](../../reports/pre-main-fixes-windows.json); nessuna qualifica di rilascio o dei gate R0–R4.

## Problema e decisione

L'audit pre-main ha dimostrato che rimuovere i privilegi dal token e applicare un Job Object non impediva al decoder di leggere e scrivere file privati non concessi. Il nuovo avvio Windows richiede un **Less Privileged AppContainer (LPAC) senza capacità**, oltre al token ristretto, al Job Object e alla lista esplicita degli handle ereditati. Il worker interroga il proprio token per LPAC/AppContainer e zero capacità, e verifica i limiti effettivi del Job prima di abilitare input esterni. Un fallimento dell'avvio o della verifica chiude il percorso esterno.

Il Job ammette un solo processo, limita memoria impegnata per processo e complessiva e termina i processi alla chiusura. Il broker ricrea il worker quando cambia la fiducia della sorgente: corpus a 384 MiB, esterni a 2 GiB; la quota non resta quella del primo lavoro.

## Autorità concesse e durata

Ogni avvio crea un profilo AppContainer univoco con l'API Windows e una copia privata dell'eseguibile. Le ACL aggiungono soltanto lettura/esecuzione sul codice e sulla sua cartella. Un handle aperto ne impedisce modifica e cancellazione durante l'uso. Nessuna ACL viene aggiunta alle fotografie, alla cartella d'installazione o ai dati del catalogo. Gli input arrivano come byte sulle pipe; il codice non riceve un handle al file originale.

Il percorso dell'applicazione è passato esplicitamente a Windows in UTF-16. Il runtime C/C++ è collegato staticamente per x86-64 MSVC; rimangono le DLL di sistema. L'ambiente proviene da `CreateEnvironmentBlock` senza ereditarietà ed è filtrato alle sole variabili di percorso di sistema/profilo. PATH, opzioni shell e variabili arbitrarie del processo host sono escluse. Windows reindirizza le cartelle temporanee al profilo isolato, che può contenere dati temporanei propri.

Alla terminazione vengono chiusi processo/handle, rimossa la copia privata e richiesto a Windows di cancellare il profilo creato da quell'istanza. Un arresto brusco dell'host può lasciare residui temporanei: questa non è una garanzia di cancellazione sicura. Non vengono riutilizzati o cancellati profili preesistenti, né puliti dati durevoli dell'utente.

## Prove e limiti

La verifica LPAC legge `WIN://NOALLAPPPKG` tramite `TokenSecurityAttributes`: buffer limitato a 64 KiB, versione/tipo/numero di valori controllati e ogni puntatore verificato entro l'allocazione restituita dal kernel. La query scalare `TokenIsLessPrivilegedAppContainer` restituisce parametro/classe non validi su questo host; il controllo alternativo di membership non distingue le due modalità e non viene usato come gate. Il confronto negativo con AppContainer ordinario restituisce attributo falso; LPAC restituisce vero. Riferimento primario per questa semantica: [Google Project Zero, proprietà LowPrivilegeAppContainer](https://github.com/googleprojectzero/sandbox-attacksurface-analysis-tools/blob/main/NtCoreLib/NtToken.cs).

La regressione riproducibile è `scripts/cargo-local.sh run --offline -p tr-platform --example windows-isolation-probe`. Su Windows va eseguita fuori dalla sandbox del terminale. Usa una sentinella temporanea con percorso assoluto, controlli positivi nell'host, pipe, lettura del codice, tentativo di scrittura nella cartella del codice e tentativo di creare un secondo processo.

Nella prova locale lettura e apertura in scrittura della sentinella, e creazione di un file accanto al codice, restituiscono Windows 5 (accesso negato); lettura del codice e pipe funzionano; il secondo processo è rifiutato. Il controllo host accede alla sentinella e al listener loopback. Nel worker LPAC Winsock non si inizializza (10107 nella prova osservata): **non è una misura di WSAEACCES sulla connessione**, né una prova esaustiva delle interfacce di rete. La prima variante AppContainer ordinaria permetteva il loopback su questo PC, motivo per cui non è stata mantenuta. Non sono state cambiate regole firewall o esenzioni del PC.

LPAC esclude l'autorità generica di All Application Packages. Le risorse di sistema con ACL compatibili, il profilo temporaneo proprio e gli handle concessi rimangono accessibili. Queste prove non qualificano attacchi al kernel, tutti gli oggetti IPC, tutti i driver, altri Windows o un ambiente di installazione pulito. Il nome `kernel_enforced` nell'API continua a indicare il tetto di memoria del Job, non una garanzia universale di sandbox.

macOS mantiene il proprio percorso XPC/App Sandbox. Il titolare ha rinviato esplicitamente la nuova verifica nativa al Mac; le prove Windows non la sostituiscono. ADR 0007 resta il documento storico del primo port, questa decisione descrive il nuovo confine dell'esperimento autorizzato.

Fonti primarie: [Microsoft: avvio AppContainer e LPAC](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer), [attributi di creazione](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute), [ambiente del token](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-createenvironmentblock).
