---
title: XRPL Ledger
description: On-chain supply tracking with the Ledger Backpack.
sidebar:
  order: 5
---

The Ledger Backpack tracks your 5 core supplies as tokens on the XRPL Testnet. Every town checkpoint records a settlement receipt on-chain. At the end of your run, your trail ledger includes transaction IDs anyone can look up on the Testnet, until the Testnet is next wiped (see [When the Testnet is reset](#when-the-testnet-is-reset)).

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

Audit mode replays a run's settlement receipts and reads the memos back off the XRPL Testnet — pulling the transactions through `AccountTx` and verifying the on-chain memos directly, rather than trusting whatever the local save happens to say. The check is genuinely external: the engine reports against the chain, not against its own copy. The supply history of a run can be independently audited, and the engine cannot fake the ledger — until the Testnet is next reset, anyone with the transaction IDs can walk the same trail of memos and confirm or refute it.

This sits on top of the optional backpack — itself optional. It is for runs where the on-chain history is the point: proof that the supplies moved the way the journal says they did, settled and checkable by a stranger.

If a settlement failed and left the chain incomplete, `trail ledger reconcile` retries the pending settlements first; the proof is only as complete as the receipts that actually landed. **INCONCLUSIVE** means the ledger couldn't confirm the run, not supply drift: pending checkpoints, a read that failed, or accounts the Testnet no longer has (see below). Re-run `trail ledger proof` after reconcile when the Testnet recovers. Do not confuse the two: `reconcile` retries pending Payments; `proof` audits the loaded save.

## When the Testnet is reset

The XRPL Testnet is a test network, and it is wiped from time to time. A reset deletes every account, balance and transaction on it; the network's address stays the same. Resets are announced in advance on the [xrpl.org blog](https://xrpl.org/blog/); the one on 2026-11-25 was announced on 2026-11-09.

After a reset, a save made before it names an issuer and a player account that no longer exist:

- **`trail ledger proof`** reports **INCONCLUSIVE** and names the missing account. It never reports FAIL for this: your save's own record of supplies and settlement transaction IDs is unchanged, but the Testnet can no longer confirm it.
- **`trail ledger reconcile`** stops before sending anything and says why. Pending checkpoints can't be settled on accounts that are gone, so retrying later won't help.
- **`trail ledger wallet`** and the backpack screen say the account is not on the ledger, instead of showing an empty pack.
- **The transaction IDs** in your trail ledger no longer resolve on the Testnet or in an explorer.

Your run itself is untouched: supplies are kept locally, and the game plays the same with the backpack's ledger gone.

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
