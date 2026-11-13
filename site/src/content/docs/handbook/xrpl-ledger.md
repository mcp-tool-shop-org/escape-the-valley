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

From the TUI, press `L` to open the ledger menu. With the backpack on, `R` proofs this save (**PASS** / **FAIL** / **INCONCLUSIVE**) and does not Rest. Or via CLI:

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

## Reconciliation Proof (`trail ledger proof`)

The settlement receipts make a claim: this is what your supplies did, recorded on a public ledger. `trail ledger proof` checks that claim against the ledger itself, on the **loaded save** (the run you actually played), and reports **PASS**, **FAIL**, or **INCONCLUSIVE**.

```bash
trail ledger proof
```

The TUI **R** key on the ledger menu is the same proof: it calls `proof_player_save` on the loaded save and paints the verdict. Camp **R** without that menu still Rests. Parcel **R** still refuses.

The proof reads the ledger itself, not only the save. It makes three checks: the token balances on the XRPL Testnet equal the supplies the save says were settled; the starting mint plus every saved settlement adds up to that final total; and each settlement's on-chain memo, read back through `AccountTx`, begins with this run's id and day.

**What it does not check.** The proof does not compare the amounts written in each on-chain memo with the per-checkpoint amounts in the save, so it shows that the save and the ledger agree in total, not that every saved entry is untouched. A save edited so that its totals still add up can pass. Treat a PASS as a consistency check between your save and the ledger, not as tamper-proofing. Anyone with the transaction IDs can still read the memos on the Testnet and compare them by hand.

If a settlement failed and left the chain incomplete, `trail ledger reconcile` retries the pending settlements first; the proof is only as complete as the receipts that actually landed. **INCONCLUSIVE** means pending checkpoints, not supply drift — re-run `trail ledger proof` after reconcile when the Testnet recovers. Do not confuse the two: `reconcile` retries pending Payments; `proof` audits the loaded save.

## Run Artifacts

Every finished run leaves a keepsake. The end-of-run screen surfaces an **XRPL postcard** — a small on-chain memento of the journey — alongside the run's stats and an export/share path, so a finished run leaves something you can keep and pass along. The postcard is drawn from the same ledger that recorded your supplies, which is why it lives here rather than in the engine alone.

## Requirements

- Bundled in the `npx` / GitHub Release binary; for `pip` installs, run `pip install "escape-the-valley[xrpl]"` (or `pip install -e ".[xrpl]"`) for the `xrpl-py` dependency
- XRPL Testnet only — no real value, no mainnet support
- Internet connection for settlement transactions

## Security

- All XRPL operations use Testnet exclusively
- Wallet keys are stored locally, never transmitted except for signing
- No telemetry, no accounts, no cloud state
- See [SECURITY.md](https://github.com/mcp-tool-shop-org/escape-the-valley/blob/main/SECURITY.md) for the full threat model
