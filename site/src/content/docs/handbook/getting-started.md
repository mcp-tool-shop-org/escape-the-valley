---
title: Getting Started
description: Install Escape the Valley and play your first game.
sidebar:
  order: 1
---

## Requirements

- Python 3.11+
- Ollama (optional, for AI narration)
- xrpl-py (optional, for ledger backpack)

## Install

```bash
pip install escape-the-valley
```

For development or optional features:

```bash
# With dev tools
pip install -e ".[dev]"

# With XRPL ledger backpack
pip install -e ".[xrpl]"
```

## Play your first game

The full-screen TUI is the recommended way to play:

```bash
trail tui --seed 42
```

The `--seed` flag makes the run reproducible — same seed, same world, same events (given the same choices).

### Other launch options

```bash
# With AI narration (requires Ollama running locally)
trail tui --seed 42 --voice

# With a specific Ollama model
trail tui --seed 42 --model mistral

# Without AI narration (pure deterministic mode)
trail tui --seed 42 --gm-off

# Resume a saved game
trail tui --continue

# Classic CLI mode (no full-screen TUI)
trail new --seed 42
trail play
```

## What you'll see

The TUI shows your party status, supplies, wagon condition, and morale. Each turn you pick an action from camp: travel, rest, hunt, or repair. Events interrupt travel with choices (A/B/C). Towns offer safe harbor and trading.

Your goal: reach the valley exit before your supplies run out or your wagon breaks beyond repair.

## Next steps

Read the [Survival Guide](/escape-the-valley/handbook/survival-guide/) for tips that will keep your party alive longer.
