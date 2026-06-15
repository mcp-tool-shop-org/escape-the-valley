"""Offline tests for the ledger reconciliation verifier (no network)."""

from __future__ import annotations

from escape_the_valley.backpack_models import SettlementRecord
from escape_the_valley.ledger_proof import (
    reconcile,
    report_to_dict,
    report_to_markdown,
)

RUN_ID = "abc123"
SEED = 1015

MINTED = {"food": 50, "water": 50, "meds": 10, "ammo": 20, "parts": 10}


def _settlements() -> list[SettlementRecord]:
    return [
        SettlementRecord(
            day=5, location="Town A",
            deltas={"food": -12, "water": -10, "ammo": -2},
            txids=["AAA111"], status="settled",
            memo=f"TRAIL|RUN:{RUN_ID}|DAY:5",
        ),
        SettlementRecord(
            day=9, location="Town B",
            deltas={"food": 8, "water": -6, "parts": -1},
            txids=["BBB222"], status="settled",
            memo=f"TRAIL|RUN:{RUN_ID}|DAY:9",
        ),
    ]


# Engine truth after the deltas above: minted + sum(deltas).
SETTLED = {"food": 46, "water": 34, "meds": 10, "ammo": 18, "parts": 9}

# Ledger keyed by XRPL currency code (FOD/WTR/MED/AMO/PRT).
LEDGER_OK = {"FOD": 46, "WTR": 34, "MED": 10, "AMO": 18, "PRT": 9}


def _reconcile(**overrides):
    kwargs = dict(
        run_id=RUN_ID, seed=SEED, minted_initial=MINTED,
        ledger_balances=LEDGER_OK, last_settled_supplies=SETTLED,
        settlements=_settlements(), pending=[],
        player_address="rPlayer", issuer_address="rIssuer",
    )
    kwargs.update(overrides)
    return reconcile(**kwargs)


def test_clean_pass():
    report = _reconcile()
    assert report.passed is True
    assert report.memo_ok is True
    assert report.pending_count == 0
    assert report.settlements_count == 2
    assert set(report.txids) == {"AAA111", "BBB222"}
    assert all(r.ok for r in report.resources)
    # Conservation holds for every resource.
    for r in report.resources:
        assert r.minted + r.sum_deltas == r.engine_settled


def test_tampered_balance_fails():
    """Engine claims more food than the ledger holds — drift must be caught."""
    tampered = dict(LEDGER_OK, FOD=40)
    report = _reconcile(ledger_balances=tampered)
    assert report.passed is False
    food = next(r for r in report.resources if r.resource == "food")
    assert food.balance_ok is False
    assert any("food" in n and "ledger" in n for n in report.notes)


def test_broken_conservation_fails():
    """Settled snapshot inconsistent with minted + deltas."""
    bad_settled = dict(SETTLED, food=99)
    report = _reconcile(
        last_settled_supplies=bad_settled,
        ledger_balances=dict(LEDGER_OK, FOD=99),
    )
    assert report.passed is False
    food = next(r for r in report.resources if r.resource == "food")
    assert food.conservation_ok is False


def test_memo_mismatch_fails():
    recs = _settlements()
    recs[0].memo = "HACKED"
    report = _reconcile(settlements=recs)
    assert report.passed is False
    assert report.memo_ok is False


def test_pending_settlement_fails():
    pending = [SettlementRecord(day=12, location="Town C",
                                deltas={"water": -4}, status="pending")]
    report = _reconcile(pending=pending)
    assert report.passed is False
    assert report.pending_count == 1


def test_missing_ledger_balance_fails():
    partial = {k: v for k, v in LEDGER_OK.items() if k != "MED"}
    report = _reconcile(ledger_balances=partial)
    assert report.passed is False
    meds = next(r for r in report.resources if r.resource == "meds")
    assert meds.ledger is None
    assert meds.balance_ok is False


def test_serialization_roundtrip():
    report = _reconcile()
    data = report_to_dict(report)
    assert data["passed"] is True
    assert data["run_id"] == RUN_ID
    assert len(data["resources"]) == 5
    assert "wallet_secret" not in json_str(data)  # never serialize secrets

    md = report_to_markdown(report)
    assert "PASS" in md
    assert "Ledger Reconciliation Proof" in md


def json_str(obj) -> str:
    import json
    return json.dumps(obj)
