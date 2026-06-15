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

## Che cos’è questo?

«Escape the Valley» è un gioco di sopravvivenza ambientato in una regione selvaggia, simile alla storica Oregon Trail, che si svolge direttamente nel terminale del computer. Guida un gruppo di coloni attraverso un territorio generato proceduralmente. Gestisci le risorse alimentari e idriche, le condizioni del carro e il morale del gruppo, affrontando eventi imprevisti, pericoli e scelte difficili.

Un assistente di gioco basato sull’intelligenza artificiale (alimentato da Ollama) può essere attivato per narrare la tua avventura utilizzando tre diverse voci narranti. Inoltre, è possibile utilizzare un registro di test XRPL che tiene traccia delle variazioni nelle tue risorse, registrandole come ricevute sulla blockchain: una prova della tua sopravvivenza o del tuo tentativo.

## Novità nella versione 1.1.0

- **Narrazione in streaming:** il Game Master scrive un elemento alla volta, componendo ogni fase della storia in tempo reale anziché fornire un blocco completo dopo una pausa.
- **Finali differenziati:** le sessioni terminano con un epilogo che varia a seconda dell'esito (trionfale, difficile, di Pirro o fallimentare), raccontato da chi è sopravvissuto, indicando la durata e il costo della missione, anziché limitarsi a una semplice causa del decesso.
- **Conseguenze reali:** gli eventi possono ora ferire o uccidere i personaggi. Una scelta sbagliata può costare la vita e la morte viene attribuita alla sua vera causa.
- **Prova di riconciliazione on-ledger:** una modalità di controllo che riproduce le ricevute di regolamento di una sessione e le verifica rispetto alla XRPL Testnet, in modo da poter controllare indipendentemente la cronologia delle transazioni.
- **Oggetti commemorativi della sessione:** ogni sessione completata lascia un ricordo: una cartolina XRPL, le statistiche dei personaggi e un percorso per esportare o condividere i risultati.

## Guida rapida all’avvio

```bash
pip install escape-the-valley

# Or, zero-prerequisite (no Python setup) via the npm launcher — downloads a
# verified binary and runs it:
#   npx @mcptoolshop/escape-the-valley tui --seed 42

# Launch the full-screen TUI (recommended)
trail tui --seed 42

# Resume a saved game
trail tui --continue

# With AI narration (requires Ollama running locally)
trail tui --seed 42 --voice

# Spoken voice narration needs the voice extra:
#   pip install "escape-the-valley[voice]"

# With voice pacing control
trail tui --seed 42 --voice --voice-pace slow

# Without AI narration (deterministic mode)
trail tui --seed 42 --gm-off

# Use a specific Ollama model
trail tui --seed 42 --model mistral
```

## Come giocare

A ogni turno scegli un’azione da compiere nel campo:

| Azione | A cosa serve. |
|--------|-------------|
| **Travel** | Dirigetevi verso l’uscita della valle. L’operazione comporta un costo in termini di cibo e acqua. Esiste il rischio di guasti e imprevisti. |
| **Rest** | Cura il gruppo, risolleva il morale. Richiede risorse, ma non porta a nessun progresso. |
| **Hunt** | Usate le munizioni per cercare cibo. È più facile farlo nelle foreste e nelle pianure. |
| **Repair** | Utilizza un pezzo di ricambio per riparare il carro. È fondamentale per la sopravvivenza. |

**Eventi imprevisti** interrompono il viaggio, offrendo diverse opzioni (A/B/C). Le scelte più prudenti sono più sicure, ma richiedono più tempo. Le scelte audaci sono più rapide, ma comportano maggiori rischi. Non esiste una soluzione valida in ogni circostanza.

**Il vagone è fondamentale.** Se si rompe e non ci sono pezzi di ricambio, la corsa finisce. Mantenetelo in buone condizioni, almeno a metà della sua capacità, ed effettuate interventi di manutenzione periodici (pausa seguita da riparazione) per aumentarne temporaneamente la resistenza ai guasti.

**Ritmo:** determina il compromesso tra velocità e sicurezza. Il ritmo costante è l’impostazione predefinita. Un ritmo sostenuto consente di percorrere distanze maggiori, ma consuma più risorse e danneggia i carri più rapidamente.

Esistono delle **misure di emergenza** (razionamento rigoroso, riparazioni d’urgenza, abbandono del carico) da utilizzare in caso di necessità. Queste misure comportano degli effetti collaterali e richiedono un periodo di tempo prima di poter essere riutilizzate: sono soluzioni estreme, non strategie.

Per consigli più approfonditi, consultare la [Guida alla sopravvivenza](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Profili dei responsabili delle attività commerciali

L’intelligenza artificiale che fa da narratore influenza lo stile, non gli aspetti tecnici. Tutti e tre i personaggi giocano allo stesso gioco.

- **Cronista:** sobrio, pragmatico, essenziale. Pochi elementi folkloristici. Riporta semplicemente ciò che è successo.
- **Narratore attorno al fuoco:** narratore serio e riflessivo. Momenti inquietanti ma sottili. L’opzione più comune.
- **Portatore di lanterna:** misterioso e ambiguo, ma comunque ancorato alle conseguenze. Il personaggio più particolare.

Utilizzare l’opzione `--gm-profile`: `trail tui --gm-profile lantern`

## Forniture / Materiali

Il gioco tiene traccia di 12 tipi di risorse suddivisi in due categorie:

**Materiale di consumo:** cibo, acqua, legna da ardere, medicinali, sale, munizioni, olio per lanterne, stoffa.

**Attrezzatura:** pezzi di ricambio, corda, utensili, stivali

I cinque elementi essenziali (cibo, acqua, medicinali, munizioni, pezzi di ricambio) sono fondamentali. Risorse aggiuntive come legna da ardere, sale, olio per lanterne e tessuto aumentano le possibilità: la legna da ardere serve per alimentare i fuochi notturni, il sale previene il deterioramento del cibo, l’olio per lanterne consente di viaggiare in sicurezza di notte e il tessuto viene utilizzato per riparare l’equipaggiamento e coprire il carro.

## Zaino Ledger (opzionale)

Lo zaino Ledger tiene traccia delle tue cinque risorse principali (cibo, acqua, medicinali, munizioni, pezzi di ricambio) sotto forma di token sulla XRPL Testnet. Ogni punto di controllo della città registra una ricevuta di rifornimento sulla blockchain. Alla fine della tua avventura, il registro dei tuoi spostamenti include gli ID delle transazioni che chiunque può verificare.

È una funzione completamente facoltativa. Il gioco funziona esattamente allo stesso modo anche se è disattivata (impostazione predefinita). Per attivarla, utilizzate il menu «L» nell’interfaccia utente testuale (TUI) oppure tramite l’interfaccia a riga di comando (CLI):

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

È necessario eseguire il comando `pip install -e ".[xrpl]"` per installare la dipendenza `xrpl-py`.

## Comandi

| Comando | Descrizione |
|---------|-------------|
| `trail tui` | Avvia l’interfaccia utente testuale a schermo intero. |
| `trail new` | Avvia una nuova esecuzione (modalità classica da riga di comando). |
| `trail play` | Riprendi un’esecuzione salvata (modalità classica da riga di comando). |
| `trail status` | Mostra la tenda, il carro e le provviste. |
| `trail journal` | Mostra le voci del diario più recenti. |
| `trail self-check` | Verifica lo stato dell’ambiente di gioco. |
| `trail version` | Mostra la versione. |
| `trail ledger status` | Mostra lo stato dello zaino. |
| `trail ledger enable` | Attiva la funzione «XRPL backpack». |
| `trail ledger disable` | Disattiva lo zaino XRPL. |
| `trail ledger settle` | Risolvere manualmente un punto di controllo. |
| `trail ledger reconcile` | Riprova le transazioni non completate. |
| `trail ledger wallet` | Mostra i dettagli del portafoglio. |
| `trail stats` | Mostra le statistiche sull’esecuzione del programma (supporta l’opzione `--json`). |
| `trail parcel send <addr> <supply> <amount>` | Invia provviste a un altro viaggiatore. |
| `trail parcel list` | Elenco dei pacchi ricevuti |
| `trail parcel accept <id>` | Accetta il pacco in attesa di consegna. |
| `trail parcel sent` | Elenco dei pacchi che hai spedito. |
| `trail wallet share` | Stampa l’indirizzo del tuo portafoglio per effettuare operazioni di scambio. |

## Avvisi di pericolo

Per impostazione predefinita, il gioco visualizza avvisi dettagliati per aiutare i nuovi giocatori a individuare tempestivamente i pericoli. I giocatori esperti possono passare alla modalità ridotta, che mostra solo gli avvisi relativi ai punti critici (minacce imminenti):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Risoluzione dei problemi

**Se qualcosa sembra non funzionare correttamente, esegui prima il comando `trail self-check`.** Questo comando verifica se è possibile accedere a Ollama, se i dati salvati vengono caricati correttamente e quale modello è installato. Ecco le tre possibili cause di errore:

| Sintomo | Causa | Risolvere/Correggere |
|---------|-------|-----|
| **Generic / no narration** | Ollama non è in esecuzione (il GM è opzionale e viene utilizzato come fallback, non causa problemi irreparabili). | Avvia Ollama (`ollama serve`) oppure utilizza l'opzione `--gm-off` per un funzionamento deterministico. Esegui `trail self-check` per confermare. |
| **Transazioni in sospeso / transazioni non completate correttamente.** | XRPL Testnet è una rete di test pubblica e a volte può presentare instabilità. | `trail ledger reconcile` tenta nuovamente le transazioni non completate; eseguilo di nuovo quando la rete sarà stabile. I dati locali sono corretti in entrambi i casi. |
| **Save won't resume** | Il file `run.json` è stato troncato o danneggiato durante la scrittura. | Il sistema lo mette in quarantena come `run.json.corrupt-<timestamp>` prima di rifiutarlo, in modo che il salvataggio successivo non possa sovrascrivere le prove. Ripristina da quel backup oppure avvia una nuova esecuzione partendo da un seme. |

La prima iterazione narrata carica il modello e può richiedere 10-30 secondi: è normale, non indica un blocco del sistema. Per maggiori dettagli, consulta il [manuale di risoluzione dei problemi](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisiti

- Python 3.11+
- Ollama (opzionale, per la narrazione tramite AI)
- xrpl-py (opzionale, per il ledger backpack)

## Sicurezza

Nessuna telemetria. Nessun account. Tutte le funzionalità di rete (Ollama, XRPL) sono opzionali e disabilitate per impostazione predefinita. Le operazioni XRPL utilizzano solo la Testnet. Consulta il file [SECURITY.md](SECURITY.md) per l'analisi completa delle minacce.

## Licenza

MIT

Realizzato da <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
