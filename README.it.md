<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.zh.md">中文</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.md">English</a> | <a href="README.pt-BR.md">Português (BR)</a>
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/mcp-tool-shop-org/brand/main/logos/escape-the-valley/readme.png" width="400" alt="Ledger Trail: Escape the Valley">
</p>

<p align="center">
  <a href="https://github.com/mcp-tool-shop-org/escape-the-valley/actions"><img src="https://github.com/mcp-tool-shop-org/escape-the-valley/workflows/CI/badge.svg" alt="CI"></a>
  <a href="https://pypi.org/project/escape-the-valley/"><img src="https://img.shields.io/pypi/v/escape-the-valley" alt="PyPI"></a>
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT License">
  <a href="https://mcp-tool-shop-org.github.io/escape-the-valley/"><img src="https://img.shields.io/badge/Landing_Page-live-blue" alt="Landing Page"></a>
</p>

<p align="center">
  <em>A survival game where the trail is the teacher and the ledger keeps you honest.</em>
</p>

---

## Cos'è questo?

Escape the Valley è un gioco di sopravvivenza in stile Oregon Trail che funziona nel tuo terminale. Guida un gruppo di coloni attraverso una natura selvaggia generata proceduralmente. Gestisci cibo, acqua, le condizioni del carro e il morale mentre affronti eventi, pericoli e scelte difficili.

Un Game Master AI opzionale (basato su Ollama) narra il tuo viaggio con tre voci distinte. Un registro opzionale XRPL Testnet tiene traccia delle variazioni delle tue scorte come ricevute on-chain, una prova che sei sopravvissuto o che ci hai provato.

## Novità

**1.3.2** — Il cooldown per `event_id` (`N=4`) impedisce che lo stesso evento si ripeta in turni consecutivi o quasi consecutivi (#21; 1.3.1 → 1.3.2 sposta le estrazioni degli eventi dopo che si sarebbe verificata una ripetizione, quindi una partita salvata in 1.3.1 continua in modo diverso dopo l'aggiornamento a 1.3.2 perché le estrazioni degli eventi cambiano. All'interno di una versione, lo stesso seme e le stesse scelte danno lo stesso risultato). Il pannello CLI Outcome con GM disattivato visualizza il riepilogo di fallback una sola volta (#20), i suggerimenti sulle scelte eliminano la doppia punteggiatura finale e `1 mile remaining.` utilizza la grammatica singolare. I file `.trail/secrets.json` corrotti vengono messi in quarantena prima della riscrittura (#31.7). L'output di `THIRD_PARTY_NOTICES*.txt` e `trail licenses` viene normalizzato a LF su tutte le piattaforme e CI verifica l'integrità della licenza fornita rispetto all'archivio python-build-standalone scaricato (#31.1, #31.2, #31.3, #31.5).

**1.3.1** — Le esecuzioni CLI (`trail new` / `trail continue`) ora emettono schede di memoria e la causa canonica della morte come la TUI (`trail stats`, i giorni di morte del Trail Ledger e i riepiloghi della memoria del GM funzionano in entrambe le modalità). TUI Abandon Cargo segnala il carico effettivamente abbandonato. Le richieste di cambio percorso non consumano più un turno di manutenzione del carro o di cooldown della valvola di sicurezza. Le partite ripetute sullo stesso seme ottengono `run_id` distinti per XRPL memo. I binari GitHub Release / `npx` ora includono `xrpl-py`. L'output piped di Windows (`cp1252`) non causa più crash con i caratteri di disegno Rich (#19).

**1.3.0** — Quattro finali che puoi effettivamente raggiungere, incluso quello "indurito". Il carro è una minaccia, non l'intero gioco. L'inizio dell'inverno rallenta un anno contrassegnato. I guadi nel deserto evitano la sabbia. Hunt indica chi si è fatto male. `npx @mcptoolshop/escape-the-valley` avvia il binario v1.3.0 (era rimasto bloccato sull'artefatto v1.1.1 GitHub obsoleto).

**1.2.1** — PyPI wheel effettivamente carica (Metadata-Version 2.3). Stesso prodotto di 1.2.0.

**1.2.0** — I binari GitHub Release vengono avviati (libreria di eventi + foglio di stile inclusi; le asserzioni di test verificano `--help` e ≥200 eventi). `trail ledger proof` controlla il salvataggio caricato. La TUI mostra seme/dottrina/imprevisti/morale; `c` cicla il ritmo; il menu del registro `R` dimostra questo salvataggio. L'HUD è leggibile a 80×24 e 120×30.

**1.1.1** — `pip install "escape-the-valley[voice]"` effettivamente si installa (l'extra aveva fissato un pacchetto non pubblicato). Il binario PyInstaller non include più l'extra vocale.

**1.1.0** — narrazione in streaming, finali graduati, eventi che possono ferire, riconciliazione on-ledger, artefatti di esecuzione.

I binari allegati alle v1.1.0 e v1.1.1 GitHub Release non vengono avviati (un import relativo nel punto di ingresso congelato). Utilizza `pip install escape-the-valley` o il binario v1.2.0+ GitHub / il launcher `npx`.

## Avvio rapido

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (GitHub v1.3.2 binary; v1.1.0 and v1.1.1
# artifacts do not start):
#   npx @mcptoolshop/escape-the-valley tui --seed 42

# Launch the full-screen TUI (recommended)
trail tui --seed 42

# Resume a saved game
trail tui --continue

# Spoken voice (opt-in; needs pip install "escape-the-valley[voice]").
# AI narration is already on by default when Ollama is running;
# pass --gm-off to disable the GM. --voice does not turn the GM on.
trail tui --seed 42 --voice

# With voice pacing control
trail tui --seed 42 --voice --voice-pace slow

# Without AI narration (deterministic mode; same seed and choices give the same run in this version)
trail tui --seed 42 --gm-off

# Use a specific Ollama model
trail tui --seed 42 --model mistral
```

## Come giocare

Ad ogni turno scegli un'azione dal campo:

| Azione | Cosa fa |
|--------|-------------|
| **Travel** | Muoviti verso l'uscita della valle. Costa cibo e acqua. Rischio di guasto ed eventi. |
| **Rest** | Cura il gruppo, ripristina il morale. Costa scorte ma non fa progressi. |
| **Hunt** | Spendi munizioni per avere una possibilità di ottenere cibo. Meglio nelle foreste e nelle pianure. |
| **Repair** | Spendi un pezzo di ricambio per riparare il carro. Fondamentale per la sopravvivenza. |

Gli **eventi** interrompono il viaggio con delle scelte (A/B/C). Le scelte prudenti sono più sicure ma costano tempo. Le scelte audaci sono più veloci ma rischiose. Non c'è una risposta sempre corretta.

**Il carro è tutto.** Se si rompe senza pezzi di ricambio, la partita è finita. Mantienilo a più della metà della sua capacità e fai delle pause per la manutenzione (riposa e poi ripara) per una resistenza temporanea ai guasti.

Il **ritmo** controlla la velocità rispetto alla sicurezza. Il ritmo normale è l'impostazione predefinita. Il ritmo sostenuto copre più terreno ma consuma più scorte e rompe i carri più velocemente.

Le **valvole di sicurezza** (razioni ridotte, riparazioni disperate, abbandono del carico) esistono per le emergenze. Hanno effetti collaterali e tempi di cooldown: sono l'ultima risorsa, non delle strategie.

Per suggerimenti più approfonditi, consulta la [Guida alla sopravvivenza](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Profili GM

Il narratore AI definisce il tono, non la meccanica. Tutti e tre i profili giocano allo stesso gioco.

- **Chronicler** — Pragmatico, pratico, sobrio. Folklore minimo. Riporta ciò che è successo.
- **Fireside** — Narratore di storie attorno al fuoco. Sottili momenti inquietanti. L'impostazione predefinita.
- **Lantern-Bearer** — Inquietante e liminale, ma comunque ancorato alle conseguenze. Quello strano.

Imposta con `--gm-profile`: `trail tui --gm-profile lantern`

## Scorte

Il gioco tiene traccia di 12 tipi di risorse suddivisi in due categorie:

**Consumabili:** cibo, acqua, legna da ardere, medicinali, sale, munizioni, olio per lanterne, stoffa

**Equipaggiamento:** pezzi di ricambio, corda, attrezzi, stivali

Le 5 scorte principali (cibo, acqua, medicinali, munizioni, pezzi di ricambio) sono le più importanti. Le scorte aggiuntive come legna da ardere, sale, olio per lanterne e stoffa aggiungono profondità: la legna da ardere alimenta i campi notturni, il sale previene il deterioramento del cibo, l'olio per lanterne consente viaggi notturni più sicuri e la stoffa ripara l'equipaggiamento e la copertura del carro.

## Ledger Backpack (Opzionale)

Il Ledger Backpack tiene traccia delle tue 5 scorte principali (cibo, acqua, medicinali, munizioni, pezzi di ricambio) come token sul XRPL Testnet. Ogni punto di controllo della città registra una ricevuta di insediamento on-chain. Alla fine della tua partita, il tuo registro trail include gli ID delle transazioni che chiunque può verificare.

Completamente opzionale. Il gioco funziona allo stesso modo anche se è disattivato (l'impostazione predefinita). Attivalo dal menu L nella TUI o tramite CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

Nella TUI, **L** apre il menu del registro; con lo zaino attivo, **R** convalida questo salvataggio (gli stessi risultati). Non riposa.

Incluso nel file binario della versione `npx` / GitHub; per le installazioni `pip`, eseguire `pip install "escape-the-valley[xrpl]"` (o `pip install -e ".[xrpl]"`) per la dipendenza `xrpl-py`.

## Comandi

| Comando | Descrizione |
|---------|-------------|
| `trail tui` | Avvia l'interfaccia utente Textual a schermo intero |
| `trail new` | Avvia una nuova partita (modalità CLI classica) |
| `trail play` | Continua una partita salvata (modalità CLI classica) |
| `trail status` | Mostra il gruppo, il carro e le provviste |
| `trail journal` | Mostra le voci recenti del diario |
| `trail self-check` | Verifica lo stato dell'ambiente di gioco |
| `trail version` | Mostra la versione |
| `trail licenses` | Mostra la licenza e le note sui componenti di terze parti (o `trail --licenses`) |
| `trail ledger status` | Mostra lo stato dello zaino |
| `trail ledger enable` | Abilita lo zaino XRPL |
| `trail ledger disable` | Disabilita lo zaino XRPL |
| `trail ledger settle` | Convalida manualmente un checkpoint |
| `trail ledger reconcile` | Riprova le convalide fallite |
| `trail ledger proof` | Verifica il salvataggio caricato (PASS/FAIL/INCONCLUSIVE) |
| `trail ledger wallet` | Mostra i dettagli del portafoglio |
| `trail stats` | Mostra le statistiche della partita e le schede di memoria (supporta `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Invia provviste a un altro viaggiatore |
| `trail parcel list` | Elenca i pacchi ricevuti |
| `trail parcel accept <id>` | Accetta un pacco in sospeso |
| `trail parcel sent` | Elenca i pacchi che hai inviato |
| `trail wallet share` | Stampa l'indirizzo del tuo portafoglio per lo scambio |

## Avvisi

Per impostazione predefinita, il gioco mostra avvisi dettagliati per aiutare i nuovi giocatori a individuare i pericoli in anticipo. I giocatori esperti possono passare alla modalità minima, che mostra solo gli avvisi di pericolo imminente (minacce critiche dell'ultimo momento):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Risoluzione dei problemi

**Se qualcosa sembra non andare, esegui prima `trail self-check`.** Segnala se Ollama è raggiungibile, se il tuo salvataggio viene caricato e quale modello è installato. Le tre cose che possono andare storte:

| Sintomo | Causa | Soluzione |
|---------|-------|-----|
| **Generic / no narration** | Ollama non è in esecuzione (il GM è opzionale e, in caso di problemi, non causa malfunzionamenti) | Avvia Ollama (`ollama serve`) o gioca in modo deterministico con `--gm-off`. Esegui `trail self-check` per confermare. |
| **Registro in sospeso / convalida fallita** | XRPL Testnet è una rete di test pubblica ed è a volte instabile | `trail ledger reconcile` riprova le convalide fallite; eseguilo di nuovo quando la rete si riprende. In entrambi i casi, le provviste sono corrette a livello locale. |
| **Save won't resume** | `run.json` è stato troncato o danneggiato durante la scrittura | Il motore lo mette in quarantena come `run.json.corrupt-<timestamp>` prima di rifiutarlo, in modo che il tuo prossimo salvataggio non possa compromettere le prove. Ripristina da quel backup o avvia una nuova partita da un seme. |

Il primo turno narrato carica il modello e può richiedere 10-30 secondi: è normale, non si tratta di un blocco. Dettagli completi: [Manuale per la risoluzione dei problemi](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisiti

- Python 3.11+
- Ollama (opzionale, per la narrazione AI)
- xrpl-py (opzionale, per lo zaino del registro nelle installazioni `pip`; incluso nel file binario della versione `npx` / GitHub)

## Sicurezza

Nessuna telemetria. Nessun account. La narrazione del GM è abilitata per impostazione predefinita (HTTP verso Ollama locale); passa `--gm-off` per disabilitarla. XRPL è disattivato fino a `trail ledger enable` (solo Testnet). La voce è facoltativa (`--voice`). Consulta [SECURITY.md](SECURITY.md) per il modello completo delle minacce.

## Licenza

MIT: esegui `trail licenses` (o `trail --licenses`) per visualizzare la licenza del gioco e le note sui componenti di terze parti (anche allegate a ogni GitHub Release, un file per piattaforma: `THIRD_PARTY_NOTICES-<os>-<arch>.txt`).

Creato da <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
