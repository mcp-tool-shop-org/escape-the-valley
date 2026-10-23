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

Escape the Valley è un gioco di sopravvivenza in stile Oregon Trail, giocabile direttamente nel terminale. Guida un gruppo di coloni attraverso una natura selvaggia generata proceduralmente. Gestisci cibo, acqua, le condizioni del carro e il morale, affrontando eventi, pericoli e scelte difficili.

Un Game Master AI opzionale (basato su Ollama) narra il tuo viaggio con tre voci distinte. Un registro XRPL Testnet opzionale tiene traccia delle variazioni delle tue scorte, registrandole come ricevute sulla blockchain: una prova che sei sopravvissuto, o una prova che ci hai provato.

## Novità

**1.3.1** — Le esecuzioni CLI (`trail new` / `trail continue`) ora generano schede di memoria e la causa canonica del decesso, come nella TUI (`trail stats`, i giorni di decesso nel registro del percorso e i riepiloghi della memoria del GM funzionano in entrambe le modalità). La funzione "Abbandona carico" nella TUI segnala il carico effettivamente abbandonato. Le opzioni di percorso alternative non comportano più la perdita di un turno per la manutenzione del carro o il tempo di ricarica della valvola di sicurezza. Le partite ripetute con lo stesso seme generano `run_id` diversi per i memo XRPL. I file GitHub Release / `npx` ora includono `xrpl-py`. L'output piped di Windows (`cp1252`) non causa più crash con caratteri di disegno Rich box (#19).

**1.3.0** — Quattro finali diversi che puoi effettivamente raggiungere, incluso quello "indurito". Il carro è una minaccia, non l'intero gioco. L'inizio dell'inverno rallenta un anno specifico. I guadi nel deserto evitano la sabbia. La caccia indica chi è rimasto ferito. `npx @mcptoolshop/escape-the-valley` avvia il binario v1.3.0 (era rimasto bloccato sull'artefatto v1.1.1 GitHub).

**1.2.1** — PyPI wheel ora effettua l'upload (Metadata-Version 2.3). Stesso prodotto della versione 1.2.0.

**1.2.0** — I binari GitHub Release vengono avviati (libreria di eventi + foglio di stile inclusi; le verifiche di integrità affermano `--help` e ≥200 eventi). `trail ledger proof` controlla il salvataggio caricato. La TUI mostra seme/dottrina/imprevisti/morale; `c` regola il ritmo; il menu del registro `R` conferma questo salvataggio. L'HUD è leggibile a 80×24 e 120×30.

**1.1.1** — `pip install "escape-the-valley[voice]"` ora si installa correttamente (la versione precedente aveva bloccato un pacchetto non pubblicato). Il binario PyInstaller non include più il pacchetto aggiuntivo per la voce.

**1.1.0** — narrazione in streaming, finali graduati, eventi che possono causare ferite, verifica della riconciliazione sulla blockchain, artefatti di esecuzione.

I binari allegati alle v1.1.0 e v1.1.1 GitHub Release non vengono avviati (un import relativo nel punto di ingresso congelato). Utilizza `pip install escape-the-valley` o il binario v1.2.0+ GitHub / il launcher `npx`.

## Avvio rapido

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (GitHub v1.3.1 binary; v1.1.0 and v1.1.1
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

# Without AI narration (deterministic mode)
trail tui --seed 42 --gm-off

# Use a specific Ollama model
trail tui --seed 42 --model mistral
```

## Come giocare

Ad ogni turno, scegli un'azione dal campo:

| Azione | Cosa fa |
|--------|-------------|
| **Travel** | Spostati verso l'uscita della valle. Richiede cibo e acqua. Rischio di guasto e di eventi. |
| **Rest** | Cura il gruppo, ripristina il morale. Richiede provviste, ma non fa progredire la storia. |
| **Hunt** | Consuma munizioni per avere una possibilità di procurarti cibo. Più efficace nelle foreste e nelle pianure. |
| **Repair** | Utilizza un pezzo di ricambio per riparare il carro. Fondamentale per la sopravvivenza. |

Gli **eventi** interrompono il viaggio con delle scelte (A/B/C). Le scelte prudenti sono più sicure, ma richiedono più tempo. Le scelte audaci sono più veloci, ma più rischiose. Non esiste una risposta sempre corretta.

**Il carro è tutto.** Se si rompe e non hai pezzi di ricambio, la partita è finita. Mantienilo in buone condizioni (almeno a metà) ed esegui la manutenzione durante le pause (riposa e poi ripara) per una resistenza temporanea ai guasti.

Il **ritmo** controlla la velocità rispetto alla sicurezza. Il ritmo normale è l'impostazione predefinita. Un ritmo sostenuto copre più terreno, ma consuma più provviste e danneggia più rapidamente i carri.

Le **valvole di sicurezza** (razioni ridotte, riparazioni disperate, abbandono del carico) sono disponibili per le emergenze. Hanno effetti collaterali e tempi di ricarica: sono l'ultima risorsa, non delle strategie.

Per suggerimenti più approfonditi, consulta la [Guida alla sopravvivenza](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Profili del GM

Il narratore AI definisce il tono, non la meccanica. Tutti e tre i profili giocano allo stesso gioco.

- **Cronista:** sobrio, pratico, essenziale. Folklore minimo. Riporta ciò che è successo.
- **Narratore attorno al fuoco:** narratore serio attorno al fuoco. Sottili momenti inquietanti. È l'impostazione predefinita.
- **Portatore di lanterna:** inquietante e liminale, ma comunque ancorato alle conseguenze. È il profilo più strano.

Imposta con `--gm-profile`: `trail tui --gm-profile lantern`

## Provviste

Il gioco tiene traccia di 12 tipi di risorse suddivisi in due categorie:

**Consumabili:** cibo, acqua, legna da ardere, medicinali, sale, munizioni, olio per lanterne, stoffa

**Equipaggiamento:** pezzi di ricambio, corda, attrezzi, stivali

Le 5 provviste principali (cibo, acqua, medicinali, munizioni, pezzi di ricambio) sono le più importanti. Le provviste aggiuntive come legna da ardere, sale, olio per lanterne e stoffa aggiungono profondità: la legna da ardere alimenta i campi notturni, il sale previene il deterioramento del cibo, l'olio per lanterne consente viaggi notturni più sicuri e la stoffa ripara l'equipaggiamento e la copertura del carro.

## Zaino del registro (opzionale)

Lo zaino del registro tiene traccia delle 5 provviste principali (cibo, acqua, medicinali, munizioni, pezzi di ricambio) come token sulla XRPL Testnet. Ogni punto di controllo registra una ricevuta di transazione sulla blockchain. Alla fine della partita, il tuo registro di percorso include gli ID delle transazioni che chiunque può verificare.

Completamente opzionale. Il gioco è identico anche se è disattivato (impostazione predefinita). Attivalo dal menu L nella TUI o tramite CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

Nella TUI, **L** apre il menu del registro; con lo zaino attivo, **R** conferma questo salvataggio (stessi risultati). Non consente di riposare.

Incluso nel binario `npx` / GitHub Release; per le installazioni `pip`, esegui `pip install "escape-the-valley[xrpl]"` (o `pip install -e ".[xrpl]"`) per la dipendenza `xrpl-py`.

## Comandi

| Comando | Descrizione |
|---------|-------------|
| `trail tui` | Avvia l'interfaccia utente testuale a schermo intero |
| `trail new` | Inizia una nuova partita (modalità CLI classica) |
| `trail play` | Continua una partita salvata (modalità CLI classica) |
| `trail status` | Mostra il gruppo, il carro e le provviste |
| `trail journal` | Mostra le voci del diario recenti |
| `trail self-check` | Verifica lo stato dell'ambiente di gioco |
| `trail version` | Mostra la versione |
| `trail licenses` | Mostra la licenza e le note sui software di terze parti (o `trail --licenses`) |
| `trail ledger status` | Mostra lo stato dello zaino |
| `trail ledger enable` | Abilita lo zaino XRPL |
| `trail ledger disable` | Disabilita lo zaino XRPL |
| `trail ledger settle` | Effettua il controllo di un punto di controllo manualmente |
| `trail ledger reconcile` | Riprova le transazioni non riuscite |
| `trail ledger proof` | Verifica il salvataggio caricato (ESITO POSITIVO/NEGATIVO/INCONCLUSIVO) |
| `trail ledger wallet` | Mostra i dettagli del portafoglio |
| `trail stats` | Mostra le statistiche di gioco e le schede di memoria (supporta `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Invia rifornimenti a un altro giocatore |
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

**Se qualcosa sembra non funzionare, esegui prima `trail self-check`.** Indica se Ollama è raggiungibile, se il salvataggio viene caricato e quale modello è installato. Le tre cose che possono causare problemi sono:

| Sintomo | Causa | Soluzione |
|---------|-------|-----|
| **Generic / no narration** | Ollama non è in esecuzione (il GM è opzionale e torna all'impostazione predefinita, non causa problemi) | Avvia Ollama (`ollama serve`) o gioca in modo deterministico con `--gm-off`. Esegui `trail self-check` per confermare. |
| **Transazione in sospeso / transazione non riuscita** | XRPL Testnet è una rete di test pubblica ed è a volte instabile | `trail ledger reconcile` riprova le transazioni non riuscite; eseguilo di nuovo quando la rete si riprende. In ogni caso, i rifornimenti sono corretti a livello locale. |
| **Save won't resume** | `run.json` è stato troncato o danneggiato durante la scrittura | Il motore lo mette in quarantena come `run.json.corrupt-<timestamp>` prima di rifiutarlo, in modo che il tuo prossimo salvataggio non possa compromettere le prove. Ripristina da quel backup o inizia una nuova partita da un seme. |

Il primo turno narrato carica il modello e può richiedere 10-30 secondi: è normale, non si tratta di un blocco. Dettagli completi: [Manuale per la risoluzione dei problemi](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisiti

- Python 3.11+
- Ollama (opzionale, per la narrazione AI)
- xrpl-py (opzionale, per il portafoglio nel registro su `pip`; incluso nel binario `npx` / GitHub)

## Sicurezza

Nessuna telemetria. Nessun account. La narrazione del GM è attiva per impostazione predefinita (HTTP verso Ollama locale); passa `--gm-off` per disabilitarla. XRPL è disattivato fino a `trail ledger enable` (solo Testnet). La voce è attivabile (`--voice`). Consulta [SECURITY.md](SECURITY.md) per il modello completo delle minacce.

## Licenza

MIT: esegui `trail licenses` (o `trail --licenses`) per visualizzare la licenza del gioco e le note sulle dipendenze di terze parti (anche allegate a ogni GitHub Release, un file per piattaforma: `THIRD_PARTY_NOTICES-<os>-<arch>.txt`).

Creato da <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
