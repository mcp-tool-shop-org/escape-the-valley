<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.zh.md">中文</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.md">English</a> | <a href="README.pt-BR.md">Português (BR)</a>
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/mcp-tool-shop-org/brand/main/logos/escape-the-valley/readme.png" width="400" alt="Ledger Trail: Escape the Valley">
</p>

<p align="center">
  <a href="https://github.com/mcp-tool-shop-org/escape-the-valley/actions"><img src="https://github.com/mcp-tool-shop-org/escape-the-valley/workflows/CI/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT License">
  <img src="https://img.shields.io/badge/version-1.0.0-green" alt="Version">
  <a href="https://mcp-tool-shop-org.github.io/escape-the-valley/"><img src="https://img.shields.io/badge/Landing_Page-live-blue" alt="Landing Page"></a>
</p>

<p align="center">
  <em>A survival game where the trail is the teacher and the ledger keeps you honest.</em>
</p>

---

## Cosa è questo?

Escape the Valley è un gioco di sopravvivenza in stile Oregon Trail che viene eseguito nel tuo terminale. Guida un gruppo di coloni attraverso una regione selvaggia generata proceduralmente. Gestisci cibo, acqua, condizioni del carro e morale, affrontando eventi, pericoli e scelte difficili.

Un Game Master AI opzionale (alimentato da Ollama) narra il tuo viaggio con tre diverse voci narrative. Un ledger opzionale XRPL Testnet tiene traccia delle variazioni delle tue risorse come ricevute sulla blockchain, una prova della tua sopravvivenza, o una prova del tuo tentativo.

## Guida rapida

```bash
pip install -e ".[dev]"

# Launch the full-screen TUI (recommended)
trail tui --seed 42

# Resume a saved game
trail tui --continue

# With AI narration (requires Ollama running locally)
trail tui --seed 42 --voice

# Without AI narration (deterministic mode)
trail tui --seed 42 --gm-off
```

## Come giocare

Ad ogni turno, scegli un'azione dal campo:

| Azione | Cosa fa |
|--------|-------------|
| **Travel** | Avvicinati all'uscita della valle. Costa cibo e acqua. Rischio di guasto e di eventi. |
| **Rest** | Ripara il gruppo, recupera il morale. Costa risorse, ma non fa progredire. |
| **Hunt** | Usa le munizioni per avere una possibilità di trovare cibo. Più efficace nelle foreste e nelle pianure. |
| **Repair** | Usa un pezzo di ricambio per riparare il carro. Fondamentale per la sopravvivenza. |

Gli **eventi** interrompono il viaggio con delle scelte (A/B/C). Le scelte prudenti sono più sicure, ma richiedono tempo. Le scelte audaci sono più veloci, ma rischiose. Non esiste una risposta sempre giusta.

**Il carro è tutto.** Se si rompe senza pezzi di ricambio, la partita finisce. Mantienilo in buone condizioni (sopra la metà) e fai delle pause per la manutenzione (riposa e poi ripara) per aumentare temporaneamente la resistenza ai guasti.

Il **ritmo** controlla la velocità rispetto alla sicurezza. Il ritmo normale è l'impostazione predefinita. Un ritmo sostenuto copre più terreno, ma consuma più risorse e danneggia i carri più velocemente.

Esistono **valvole di sicurezza** (razione ridotta, riparazione disperata, abbandono del carico) per le emergenze. Hanno effetti collaterali e tempi di ricarica: sono un'ultima risorsa, non una strategia.

Per suggerimenti più approfonditi, consulta la [Guida alla sopravvivenza](docs/survival-guide.md).

## Profili del Game Master

Il narratore AI influenza il tono, non le meccaniche. Tutti e tre i profili giocano la stessa partita.

- **Chronicler** — Pragmatico, concreto, essenziale. Minimo folklore. Riporta ciò che è successo.
- **Fireside** — Narratore serio, come un racconto attorno al fuoco. Momenti sottilmente inquietanti. L'impostazione predefinita.
- **Lantern-Bearer** — Inquietante e al limite, ma comunque radicato nelle conseguenze. Il più strano.

Imposta con `--gm-profile`: `trail tui --gm-profile lantern`

## Ledger Backpack (Opzionale)

Il Ledger Backpack tiene traccia delle tue 5 risorse principali (cibo, acqua, medicinali, munizioni, pezzi di ricambio) come token sulla rete XRPL Testnet. Ogni punto di controllo della città registra una ricevuta di transazione sulla blockchain. Alla fine della tua partita, il tuo registro include gli ID delle transazioni che chiunque può verificare.

Completamente opzionale. Il gioco funziona esattamente allo stesso modo quando è disattivato (l'impostazione predefinita). Abilitalo dal menu L nell'interfaccia utente testuale o tramite riga di comando:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

Richiede `pip install -e ".[xrpl]"` per la dipendenza `xrpl-py`.

## Comandi

| Comando | Descrizione |
|---------|-------------|
| `trail tui` | Avvia l'interfaccia utente testuale a schermo intero |
| `trail new` | Inizia una nuova partita (modalità CLI classica) |
| `trail play` | Continua una partita salvata (modalità CLI classica) |
| `trail status` | Mostra il gruppo, il carro e le risorse |
| `trail journal` | Mostra le voci recenti del diario |
| `trail self-check` | Controlla lo stato dell'ambiente di gioco |
| `trail version` | Mostra la versione |
| `trail ledger status` | Mostra lo stato dello zaino |
| `trail ledger enable` | Abilita lo zaino XRPL |
| `trail ledger disable` | Disabilita lo zaino XRPL |
| `trail ledger settle` | Registra manualmente un punto di controllo |
| `trail ledger reconcile` | Riprova le registrazioni fallite |
| `trail ledger wallet` | Mostra i dettagli del portafoglio |
| `trail parcel list` | Elenca i pacchetti ricevuti |
| `trail parcel accept <id>` | Accetta un pacchetto in sospeso |

## Avvisi

Per impostazione predefinita, il gioco mostra avvisi dettagliati per aiutare i nuovi giocatori a individuare i pericoli in anticipo. I giocatori esperti possono passare alla modalità minima, che mostra solo gli avvisi critici (minacce dell'ultimo momento):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Requisiti

- Python 3.11 o superiore
- Ollama (opzionale, per la narrazione tramite intelligenza artificiale)
- xrpl-py (opzionale, per l'integrazione con il ledger)

## Sicurezza

Nessuna raccolta di dati telemetrici. Nessun account. Tutte le funzionalità di rete (Ollama, XRPL) sono attivabili su richiesta e disabilitate per impostazione predefinita. Le operazioni XRPL utilizzano solo la rete di test (Testnet). Consultare il file [SECURITY.md](SECURITY.md) per una descrizione completa del modello di rischio.

## Licenza

MIT

Creato da <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
