# LibRaw e vendita di TrueRenderer

Verifica delle fonti ufficiali: 8 settembre 2026.

**È possibile usare LibRaw nel software venduto senza acquistare una licenza commerciale**, rispettando una delle licenze open source offerte. La [pagina ufficiale](https://www.libraw.org/about) dichiara la libreria gratuita e disponibile a scelta sotto LGPL 2.1 oppure CDDL 1.0. La possibilità di vendere il prodotto non equivale all'assenza di obblighi.

Per il futuro componente RAW di TrueRenderer, la CDDL 1.0 è l'opzione candidata: permette di combinare la libreria con altro codice in un prodotto proprietario (§3.6), senza richiedere che tutto il codice indipendente dell'applicazione diventi open source. Il [testo distribuito da LibRaw](https://github.com/LibRaw/LibRaw/blob/master/LICENSE.CDDL) prevede, fra gli altri, questi obblighi:

- Rendere disponibile ai destinatari il sorgente del componente coperto dalla CDDL, comprese le eventuali modifiche, e indicare come ottenerlo (§3.1–3.2).
- Conservare licenza e attribuzioni; identificare le proprie modifiche (§3.3).
- Non limitare nell'EULA i diritti sul sorgente coperto dalla CDDL (§3.4–3.5).

L'alternativa LGPL 2.1 permette anch'essa l'uso commerciale, ma richiede di organizzare correttamente sostituzione/relinking della libreria, sorgenti e notice secondo il [testo LGPL incluso](https://github.com/LibRaw/LibRaw/blob/master/LICENSE.LGPL). Il solo linking dinamico non esaurisce tali obblighi.

**Situazione attuale:** TrueRenderer 0.1.5 non include né collega LibRaw. Su macOS lo sviluppo RAW passa da CIRAWFilter nei servizi XPC. LibRaw è previsto nell'architettura per la futura pipeline RAW multipiattaforma; non occorre acquistarlo per proseguire l'incremento attuale.

Prima di includerlo nel pacchetto si dovranno bloccare versione e opzioni, verificare i termini di tutti i componenti realmente compilati, scegliere formalmente l'opzione di licenza e preparare sorgenti/notice e condizioni di distribuzione. I termini di versioni future possono cambiare; questa verifica riguarda le fonti indicate alla data sopra, non certifica un futuro pacchetto commerciale.
