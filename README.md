# Escape the Valley: Ledger Trail

Oregon Trail-style survival game powered by Ollama GM with XRPL ledger-backed inventory.

## Quick Start

```bash
pip install -e ".[dev]"
trail new --seed 42 --gm-profile fireside
trail play
```

## Commands

| Command | Description |
|---------|-------------|
| `trail new` | Start a new run |
| `trail play` | Continue a saved run |
| `trail status` | Show party, wagon, and supplies |
| `trail journal` | Show recent journal entries |
| `trail self-check` | Check game environment health |

## GM Profiles

- **Chronicler** — Grounded, practical, spare. Minimal folklore.
- **Fireside** — Serious campfire narrator. Subtle uncanny moments.
- **Lantern-Bearer** — Uncanny and liminal, but still grounded in consequences.

## Requirements

- Python 3.11+
- Ollama (optional, for AI-powered narration)

## License

MIT

Built by [MCP Tool Shop](https://mcp-tool-shop.github.io/)
