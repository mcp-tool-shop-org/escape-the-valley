---
title: XRPL Ledger
description: On-chain supply tracking with the Ledger Backpack.
sidebar:
  order: 5
---

The Ledger Backpack tracks your 5 core supplies as tokens on the XRPL Testnet. Every town checkpoint records a settlement receipt on-chain. At the end of your run, your trail ledger includes transaction IDs anyone can verify.

Completely optional. The game plays identically with it off (the default).

## Token types

| Token | Resource | What it tracks |
|-------|----------|----------------|
| FOOD | Food supply | Consumption, hunting gains, trading |
| WATR | Water supply | Auto-refill at towns, terrain drain |
| MEDS | Medicine | Party health recovery |
| AMMO | Ammunition | Hunting costs, event rewards |
| PART | Spare parts | Repair action costs, trading |

## Enabling the backpack

From the TUI, press `L` to open the ledger menu. Or via CLI:

```bash
# Enable the backpack
trail ledger enable

# Check status
trail ledger status

# View wallet details
trail ledger wallet
```

## Settlements

At each town checkpoint, the game batches all supply changes since the last settlement into a single XRPL transaction. This keeps costs low and the ledger clean.

```bash
# Manually trigger a settlement
trail ledger settle

# Retry failed settlements
trail ledger reconcile
```

## Parcels

Players can trade supplies with each other on the XRPL Testnet:

```bash
# Share your address so others can send you supplies
trail wallet share

# Send supplies to another traveler
trail parcel send <address> food 10

# List received parcels
trail parcel list

# Accept a pending parcel
trail parcel accept <id>

# See what you've sent
trail parcel sent
```

Parcels arrive as pending and must be explicitly accepted before supplies are added to your inventory. You can also refuse parcels.

## Requirements

- `pip install -e ".[xrpl]"` for the `xrpl-py` dependency
- XRPL Testnet only — no real value, no mainnet support
- Internet connection for settlement transactions

## Security

- All XRPL operations use Testnet exclusively
- Wallet keys are stored locally, never transmitted except for signing
- No telemetry, no accounts, no cloud state
- See [SECURITY.md](https://github.com/mcp-tool-shop-org/escape-the-valley/blob/main/SECURITY.md) for the full threat model
