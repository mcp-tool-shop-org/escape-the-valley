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

Escape the Valley è un gioco di sopravvivenza in stile Oregon Trail che si svolge nel terminale. Guida un gruppo di coloni attraverso una natura selvaggia generata proceduralmente. Gestisci cibo, acqua, le condizioni del carro e il morale mentre affronti eventi, pericoli e scelte difficili.

Un Game Master AI opzionale (basato su Ollama) narra il tuo viaggio con tre voci distinte. Un registro XRPL Testnet opzionale tiene traccia delle variazioni delle tue scorte come ricevute on-chain, una prova che sei sopravvissuto o che ci hai provato.

## Novità

**1.2.0** — Lancio delle versioni binarie su GitHub (libreria di eventi + foglio di stile inclusi; vengono eseguiti test preliminari `--help` e almeno 200 eventi). `trail ledger proof` verifica il file di salvataggio caricato. L’interfaccia utente testuale mostra seme/dottrina/eventi/morale; `c` regola il ritmo; il menu del registro `R` conferma questo salvataggio. L’HUD è leggibile a 80×24 e 120×30.

**1.1.1** — `pip install "escape-the-valley[voice]"` si installa effettivamente (la versione precedente aveva bloccato un pacchetto non pubblicato). Il file binario PyInstaller non include più la voce extra.

**1.1.0** — narrazione in streaming, finali graduati, eventi che possono ferire, prova di riconciliazione on-ledger, artefatti eseguiti.

Le versioni binarie allegate alle versioni v1.1.0 e v1.1.1 su GitHub non vengono eseguite (a causa di un import relativo nel punto di ingresso principale). Utilizzare `pip install escape-the-valley` o la versione binaria v1.2.0+ di GitHub / il programma di avvio `npx`.

## Avvio rapido

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (v1.2.0+ GitHub binaries; v1.1.0 and v1.1.1
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
| **Travel** | Muoviti verso l'uscita della valle. Richiede cibo e acqua. Rischio di guasto ed eventi. |
| **Rest** | Cura il gruppo, ripristina il morale. Richiede provviste ma non comporta progressi. |
| **Hunt** | Spendi munizioni per avere una possibilità di trovare cibo. Meglio nelle foreste e nelle pianure. |
| **Repair** | Utilizza un pezzo di ricambio per riparare il carro. Fondamentale per la sopravvivenza. |

Gli **eventi** interrompono il viaggio con delle scelte (A/B/C). Le scelte prudenti sono più sicure ma richiedono tempo. Le scelte audaci sono più veloci ma rischiose. Non esiste una risposta sempre corretta.

**Il carro è tutto.** Se si rompe e non ci sono pezzi di ricambio, la partita finisce. Mantienilo in buone condizioni (almeno a metà) ed esegui interventi di manutenzione (riposo seguito da riparazione) per una temporanea resistenza ai guasti.

Il **ritmo** controlla la velocità rispetto alla sicurezza. Il ritmo normale è l'impostazione predefinita. Un ritmo sostenuto copre più terreno ma consuma più provviste e danneggia i carri più velocemente.

Le **valvole di emergenza** (razioni ridotte, riparazioni disperate, abbandono del carico) sono disponibili per le emergenze. Hanno effetti collaterali e tempi di ricarica: sono l'ultima risorsa, non delle strategie.

Per suggerimenti più approfonditi, consulta la [Guida alla sopravvivenza](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Profili del Game Master

Il narratore AI definisce il tono, non la meccanica. Tutti e tre i profili giocano allo stesso gioco.

- **Cronista:** sobrio, pratico, essenziale. Minimo folklore. Riporta ciò che è successo.
- **Narratore del focolare:** narratore serio attorno al fuoco. Sottili momenti inquietanti. È l'impostazione predefinita.
- **Portatore di lanterne:** inquietante e liminale, ma comunque ancorato alle conseguenze. È il più strano.

Imposta con `--gm-profile`: `trail tui --gm-profile lantern`

## Provviste

Il gioco tiene traccia di 12 tipi di risorse suddivisi in due categorie:

**Consumabili:** cibo, acqua, legna da ardere, medicinali, sale, munizioni, olio per lanterne, stoffa

**Equipaggiamento:** pezzi di ricambio, corda, attrezzi, stivali

Le 5 provviste principali (cibo, acqua, medicinali, munizioni, pezzi di ricambio) sono le più importanti. Le provviste aggiuntive come legna da ardere, sale, olio per lanterne e stoffa aggiungono profondità: la legna da ardere alimenta i campi notturni, il sale previene il deterioramento del cibo, l'olio per lanterne consente viaggi notturni più sicuri e la stoffa ripara l'equipaggiamento e la copertura del carro.

## Zaino del registro (opzionale)

Lo zaino del registro tiene traccia delle 5 provviste principali (cibo, acqua, medicinali, munizioni, pezzi di ricambio) come token sulla XRPL Testnet. Ogni punto di controllo registra una ricevuta di insediamento on-chain. Alla fine della tua partita, il tuo registro includerà gli ID delle transazioni che chiunque può verificare.

Completamente opzionale. Il gioco funziona allo stesso modo anche se è disattivato (impostazione predefinita). Attivalo dal menu L nella TUI o tramite CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

Nella TUI, **L** apre il menu del registro; con lo zaino attivo, **R** verifica questo salvataggio (stessi risultati). Non riposa.

Richiede `pip install -e ".[xrpl]"` per la dipendenza `xrpl-py`.

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
| `trail ledger status` | Mostra lo stato dello zaino |
| `trail ledger enable` | Abilita lo zaino XRPL |
| `trail ledger disable` | Disabilita lo zaino XRPL |
| `trail ledger settle` | Effettua il controllo di un punto di passaggio manualmente |
| `trail ledger reconcile` | Riprova i controlli non riusciti |
| `trail ledger proof` | Verifica il salvataggio caricato (PASS/FAIL/INCONCLUSIVE) |
| `trail ledger wallet` | Mostra i dettagli del portafoglio |
| `trail stats` | Mostra le statistiche della partita (supporta `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Invia provviste a un altro viaggiatore |
| `trail parcel list` | Elenca i pacchi ricevuti |
| `trail parcel accept <id>` | Accetta un pacco in sospeso |
| `trail parcel sent` | Elenca i pacchi che hai inviato |
| `trail wallet share` | Stampa l'indirizzo del tuo portafoglio per lo scambio |

## Avvisi

Per impostazione predefinita, il gioco mostra avvisi dettagliati per aiutare i nuovi giocatori a individuare rapidamente i pericoli. I giocatori esperti possono passare alla modalità minima, che mostra solo gli avvisi di pericolo imminente (ultimi momenti, minacce critiche):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Risoluzione dei problemi

**Se qualcosa sembra non andare bene, esegui prima `trail self-check`.** Segnala se Ollama è raggiungibile, se il salvataggio si carica e quale modello è installato. Le tre cose che possono andare storte:

| Sintomo | Causa | Soluzione |
|---------|-------|-----|
| **Generic / no narration** | Ollama non è in esecuzione (il Game Master è opzionale e si disattiva, ma non blocca il gioco) | Avvia Ollama (`ollama serve`) oppure esegui il programma in modo deterministico con `--gm-off`. Esegui `trail self-check` per confermare. |
| **Transazione in sospeso / transazione non riuscita** | XRPL Testnet è una rete di test pubblica e a volte può presentare instabilità. | `trail ledger reconcile` tenta nuovamente le transazioni non riuscite; eseguilo di nuovo quando la rete sarà stabile. In ogni caso, i dati locali sono corretti. |
| **Save won't resume** | `run.json` è stato troncato o danneggiato durante la scrittura. | Il motore lo mette in quarantena come `run.json.corrupt-<timestamp>` prima di rifiutarlo, quindi il tuo prossimo salvataggio non potrà sovrascrivere le prove. Ripristina da quel backup oppure avvia una nuova esecuzione partendo da un seme. |

La prima iterazione narrata carica il modello e può richiedere 10-30 secondi; è normale, non si tratta di un blocco del programma. Per maggiori dettagli: [Guida alla risoluzione dei problemi](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisiti

- Python 3.11+
- Ollama (opzionale, per la narrazione tramite AI)
- xrpl-py (opzionale, per il ledger backpack)

## Sicurezza

Nessuna telemetria. Nessun account. La narrazione GM è attiva per impostazione predefinita (HTTP verso Ollama locale); passa `--gm-off` per disattivarla. XRPL è disattivata fino a `trail ledger enable` (solo Testnet). La voce è attivabile (`--voice`). Consulta [SECURITY.md](SECURITY.md) per il modello completo delle minacce.

## Licenza

MIT

Realizzato da <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
