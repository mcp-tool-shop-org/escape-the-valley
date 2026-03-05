---
title: Commands
description: Full CLI reference for Escape the Valley.
sidebar:
  order: 6
---

## Game commands

| Command | Description |
|---------|-------------|
| `trail tui` | Launch the full-screen Textual UI |
| `trail tui --seed <n>` | Start with a specific seed (reproducible world) |
| `trail tui --continue` | Resume a saved game |
| `trail tui --voice` | Enable voice narration (requires Ollama) |
| `trail tui --gm-off` | Disable AI narration |
| `trail tui --gm-profile <name>` | Set GM profile: `chronicler`, `fireside`, `lantern` |
| `trail tui --callouts <mode>` | Warning mode: `verbose` (default) or `minimal` |
| `trail new` | Start a new run (classic CLI mode) |
| `trail play` | Continue a saved run (classic CLI mode) |

## Status commands

| Command | Description |
|---------|-------------|
| `trail status` | Show party, wagon, and supplies |
| `trail journal` | Show recent journal entries |
| `trail journal -n <count>` | Show last N journal entries |
| `trail version` | Show version |
| `trail self-check` | Check game environment health |

## Ledger commands

| Command | Description |
|---------|-------------|
| `trail ledger enable` | Enable XRPL backpack |
| `trail ledger disable` | Disable XRPL backpack |
| `trail ledger status` | Show backpack status |
| `trail ledger settle` | Manually settle a checkpoint |
| `trail ledger reconcile` | Retry failed settlements |
| `trail ledger wallet` | Show wallet details |

## Parcel commands

| Command | Description |
|---------|-------------|
| `trail parcel list` | List received parcels |
| `trail parcel accept <id>` | Accept a pending parcel |
