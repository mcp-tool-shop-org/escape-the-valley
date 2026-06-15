"""Ledger Reconciliation Proof — CLI runner.

Drives one deterministic run with the XRPL Ledger Backpack enabled on Testnet,
then reconciles on-ledger balances against the engine. Writes a JSON + Markdown
report and exits 0 on PASS, 1 on FAIL.

Usage:
    python scripts/ledger_proof.py [--seed N] [--out DIR] [--max-steps N]

Requires the xrpl extra:  pip install -e ".[xrpl]"
Testnet only — throwaway faucet wallets, no mainnet, no real value.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

from escape_the_valley.ledger_proof import (  # noqa: E402
    report_to_dict,
    report_to_markdown,
    run_proof,
)


def main() -> int:
    parser = argparse.ArgumentParser(description="XRPL ledger reconciliation proof")
    parser.add_argument("--seed", type=int, default=1015,
                        help="RNG seed (default 1015 — a known-winning run)")
    parser.add_argument("--out", type=str, default=".trail/proofs",
                        help="output directory for the report")
    parser.add_argument("--max-steps", type=int, default=600,
                        help="max engine steps before giving up")
    args = parser.parse_args()

    print(f"Ledger reconciliation proof — seed {args.seed} (Testnet)")
    print("Funding throwaway faucet wallets and driving a full run...")
    print("(this makes real testnet transactions; expect a few minutes)\n")

    started = time.perf_counter()

    def _progress(state) -> None:
        settled = len(state.backpack.settlements)
        pending = len(state.backpack.pending_settlements)
        print(
            f"  day {state.day:>3}  alive {state.party.alive_count}  "
            f"food {state.supplies.food:>3} water {state.supplies.water:>3}  "
            f"settled {settled} pending {pending}",
            end="\r",
        )

    try:
        report = run_proof(args.seed, max_steps=args.max_steps, on_progress=_progress)
    except RuntimeError as exc:
        print(f"\n[ERROR] {exc}")
        return 2

    elapsed = time.perf_counter() - started
    print("\n")
    print(report_to_markdown(report))
    print(f"\n(elapsed {elapsed:.0f}s)")

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    stem = f"ledger_proof_{report.run_id}"
    (out_dir / f"{stem}.json").write_text(
        json.dumps(report_to_dict(report), indent=2), encoding="utf-8"
    )
    (out_dir / f"{stem}.md").write_text(report_to_markdown(report), encoding="utf-8")
    print(f"\nReport written to {out_dir / stem}.json / .md")

    return 0 if report.passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
