<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.zh.md">中文</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (BR)</a>
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

## What Is This?

Escape the Valley is an Oregon Trail-style survival game that runs in your terminal. Lead a party of settlers through a procedurally generated wilderness. Manage food, water, wagon condition, and morale while navigating events, hazards, and hard choices.

An optional AI Game Master (powered by Ollama) narrates your journey with three distinct storytelling voices. An optional XRPL Testnet ledger backpack tracks your supply changes as on-chain receipts — proof that you survived, or proof that you tried.

## Quick Start

```bash
pip install escape-the-valley

# Launch the full-screen TUI (recommended)
trail tui --seed 42

# Resume a saved game
trail tui --continue

# With AI narration (requires Ollama running locally)
trail tui --seed 42 --voice

# With voice pacing control
trail tui --seed 42 --voice --voice-pace slow

# Without AI narration (deterministic mode)
trail tui --seed 42 --gm-off

# Use a specific Ollama model
trail tui --seed 42 --model mistral
```

## How to Play

Each turn you choose an action from camp:

| Action | What It Does |
|--------|-------------|
| **Travel** | Move toward the valley exit. Costs food and water. Risk of breakdown and events. |
| **Rest** | Heal the party, recover morale. Costs supplies but no progress. |
| **Hunt** | Spend ammo for a chance at food. Better in forests and plains. |
| **Repair** | Spend a spare part to fix the wagon. Critical for survival. |

**Events** interrupt travel with choices (A/B/C). Cautious choices are safer but cost time. Bold choices are faster but risky. There's no always-right answer.

**The wagon is everything.** If it breaks with no parts, the run is over. Keep it above half condition and do maintenance windows (rest then repair) for temporary breakdown resistance.

**Pace** controls speed vs. safety. Steady is the default. Hard pace covers more ground but burns more supplies and breaks wagons faster.

**Escape valves** (hard ration, desperate repair, abandon cargo) exist for emergencies. They have side effects and cooldowns — last resorts, not strategies.

For deeper tips, see the [Survival Guide](docs/survival-guide.md).

## GM Profiles

The AI narrator shapes the tone, not the mechanics. All three profiles play the same game.

- **Chronicler** — Grounded, practical, spare. Minimal folklore. Reports what happened.
- **Fireside** — Serious campfire narrator. Subtle uncanny moments. The default.
- **Lantern-Bearer** — Uncanny and liminal, but still grounded in consequences. The weird one.

Set with `--gm-profile`: `trail tui --gm-profile lantern`

## Supplies

The game tracks 12 resource types across two categories:

**Consumables:** food, water, firewood, meds, salt, ammo, lantern oil, cloth

**Gear:** parts, rope, tools, boots

The 5 core supplies (food, water, meds, ammo, parts) are the most critical. Extended supplies like firewood, salt, lantern oil, and cloth add depth: firewood fuels night camps, salt prevents food spoilage, lantern oil enables safer night travel, and cloth patches gear and wagon cover.

## Ledger Backpack (Optional)

The Ledger Backpack tracks your 5 core supplies (food, water, meds, ammo, parts) as tokens on the XRPL Testnet. Every town checkpoint records a settlement receipt on-chain. At the end of your run, your trail ledger includes transaction IDs anyone can verify.

Completely optional. The game plays identically with it off (the default). Enable it from the L menu in the TUI or via CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

Requires `pip install -e ".[xrpl]"` for the `xrpl-py` dependency.

## Commands

| Command | Description |
|---------|-------------|
| `trail tui` | Launch the full-screen Textual UI |
| `trail new` | Start a new run (classic CLI mode) |
| `trail play` | Continue a saved run (classic CLI mode) |
| `trail status` | Show party, wagon, and supplies |
| `trail journal` | Show recent journal entries |
| `trail self-check` | Check game environment health |
| `trail version` | Show version |
| `trail ledger status` | Show backpack status |
| `trail ledger enable` | Enable XRPL backpack |
| `trail ledger disable` | Disable XRPL backpack |
| `trail ledger settle` | Manually settle a checkpoint |
| `trail ledger reconcile` | Retry failed settlements |
| `trail ledger wallet` | Show wallet details |
| `trail stats` | Show run statistics (supports `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Send supplies to another traveler |
| `trail parcel list` | List received parcels |
| `trail parcel accept <id>` | Accept a pending parcel |
| `trail parcel sent` | List parcels you have sent |
| `trail wallet share` | Print your wallet address for trading |

## Warning Callouts

By default, the game shows verbose warnings to help new players spot danger early. Experienced players can switch to minimal mode, which only shows cliff-edge warnings (last-moment, critical threats):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Requirements

- Python 3.11+
- Ollama (optional, for AI narration)
- xrpl-py (optional, for ledger backpack)

## Security

No telemetry. No accounts. All network features (Ollama, XRPL) are opt-in and disabled by default. XRPL operations use Testnet only. See [SECURITY.md](SECURITY.md) for the full threat model.

## License

MIT

Built by <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
