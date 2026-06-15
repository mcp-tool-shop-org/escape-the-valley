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
    """Local-consistency drift: a stored record memo with the wrong header."""
    recs = _settlements()
    recs[0].memo = "HACKED"
    report = _reconcile(settlements=recs)
    assert report.passed is False
    assert report.memo_ok is False
    assert report.memo_local_ok is False


def test_local_memo_accepts_delta_suffix():
    """The canonical on-chain memo carries a |DELTA:... suffix — prefix match."""
    recs = _settlements()
    recs[0].memo = f"TRAIL|RUN:{RUN_ID}|DAY:5|DELTA:FOD-12,WTR-10,AMO-2"
    report = _reconcile(settlements=recs)
    assert report.memo_local_ok is True
    # No on-chain memos supplied → external integrity stays unverified.
    assert report.onchain_memo_ok is None


# ── ledger-003: external (on-chain) memo verification ─────────────────


def _onchain_ok() -> dict[str, str]:
    """On-chain memos keyed by txid, matching the run + per-day header."""
    return {
        "AAA111": f"TRAIL|RUN:{RUN_ID}|DAY:5|DELTA:FOD-12,WTR-10,AMO-2",
        "BBB222": f"TRAIL|RUN:{RUN_ID}|DAY:9|DELTA:FOD+8,WTR-6,PRT-1",
    }


def test_onchain_memo_pass():
    """When the chain confirms each memo header, memo_ok is externally true."""
    report = _reconcile(onchain_memos=_onchain_ok())
    assert report.passed is True
    assert report.onchain_memo_ok is True
    assert report.memo_ok is True


def test_onchain_memo_drift_fails():
    """Injected on-chain memo with a foreign run id must be caught externally.

    This is the ledger-003 guarantee: the engine controls the stored record
    memo (memo_local_ok can pass), but it cannot fake the bytes already signed
    on-chain — a mismatch there fails the proof.
    """
    tampered = dict(_onchain_ok())
    tampered["AAA111"] = "TRAIL|RUN:WRONGRUN|DAY:5|DELTA:FOD-12"
    report = _reconcile(onchain_memos=tampered)
    assert report.passed is False
    assert report.onchain_memo_ok is False
    # The stored record memo is still locally consistent — proves the check is
    # genuinely external, not just re-reading the engine's own string.
    assert report.memo_local_ok is True
    assert any("on-chain memo" in n for n in report.notes)


def test_onchain_memo_missing_txid_fails():
    """A settlement txid with no on-chain memo is an external failure."""
    partial = {"AAA111": f"TRAIL|RUN:{RUN_ID}|DAY:5|DELTA:FOD-12"}  # BBB222 absent
    report = _reconcile(onchain_memos=partial)
    assert report.passed is False
    assert report.onchain_memo_ok is False


def test_no_onchain_memos_marks_unverified():
    """Offline reconcile reports external memo integrity as NOT verified."""
    report = _reconcile()
    assert report.onchain_memo_ok is None
    assert any("not verified" in n.lower() or "NOT verified" in n
               for n in report.notes)


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


# ── run_proof driver ↔ reconcile wiring (A-06 / ledger-008) ───────────
#
# Drive run_proof() with a fully mocked BackpackManager so no network is
# touched, proving the driver wires enable → settle → wallet_info →
# fetch_onchain_memos → reconcile() and that the on-chain memo it fetches
# is what reconcile() verifies (ledger-003 end to end).

from escape_the_valley.backpack import _settlement_memo_text  # noqa: E402
from escape_the_valley.backpack_models import (  # noqa: E402
    XRPL_RESOURCES,
    XRPL_TOKEN_MAP,
)


class _FakeMgr:
    """Mock BackpackManager used by both run_proof and the engine's internal
    checkpoint settlement (engine constructs its own from .backpack)."""

    # Class-level knob so the engine-constructed instances share behavior.
    onchain_memos: dict[str, str] = {}

    available = True

    def __init__(self, *a, **k):
        pass

    def enable(self, state):
        from escape_the_valley.backpack import EnableResult
        bp = state.backpack
        bp.enabled = True
        bp.wallet_address = "rPlayerProof"
        bp.wallet_secret = "sPlayerProof"
        bp.issuer_address = "rIssuerProof"
        bp.issuer_secret = "sIssuerProof"
        bp.trust_lines_ready = True
        bp.last_settled_supplies = {k: state.supplies.get(k) for k in XRPL_RESOURCES}
        return EnableResult(success=True, message="ok", wallet_address=bp.wallet_address)

    def settle(self, state, location):
        from escape_the_valley.backpack import SettlementResult
        bp = state.backpack
        deltas = {}
        for key in XRPL_RESOURCES:
            diff = state.supplies.get(key) - bp.last_settled_supplies.get(key, 0)
            if diff:
                deltas[key] = diff
        bp.last_settled_supplies = {k: state.supplies.get(k) for k in XRPL_RESOURCES}
        if not deltas:
            return SettlementResult(success=True, message="No changes to settle.")
        memo = _settlement_memo_text(state.run_id, state.day, deltas)
        rec = SettlementRecord(
            day=state.day, location=location, deltas=deltas,
            txids=[f"TX{state.day}"], status="settled", memo=memo,
        )
        bp.settlements.append(rec)
        # Register the on-chain memo this txid carries (engine cannot fake it).
        type(self).onchain_memos[f"TX{state.day}"] = memo
        return SettlementResult(success=True, message="settled", txids=rec.txids, record=rec)

    def check_parcels(self, state):
        return []

    def wallet_info(self, state):
        bp = state.backpack
        balances = {
            XRPL_TOKEN_MAP[k][0]: bp.last_settled_supplies.get(k, 0)
            for k in XRPL_RESOURCES
        }
        return {"address": bp.wallet_address, "balances": balances}

    def fetch_onchain_memos(self, state):
        return dict(type(self).onchain_memos)

    def close(self):
        pass


def test_run_proof_wires_driver_to_reconcile(monkeypatch):
    import escape_the_valley.backpack as backpack_mod
    from escape_the_valley.ledger_proof import run_proof

    _FakeMgr.onchain_memos = {}
    monkeypatch.setattr(backpack_mod, "BackpackManager", _FakeMgr)

    report = run_proof(13, max_steps=80, isolate_save=True)

    # Driver produced a real report and the on-chain memo path was exercised.
    assert report.player_address == "rPlayerProof"
    assert report.issuer_address == "rIssuerProof"
    # Conservation + balance hold because the fake ledger mirrors the engine.
    assert all(r.conservation_ok for r in report.resources)
    assert all(r.balance_ok for r in report.resources)
    # External memo check ran against fetched on-chain memos (not None).
    assert report.onchain_memo_ok is True
    assert report.passed is True


def test_run_proof_external_memo_drift_fails(monkeypatch):
    """If the on-chain memo the driver fetches doesn't match, the proof FAILS —
    the wiring carries the external (ledger-003) check end to end."""
    import escape_the_valley.backpack as backpack_mod
    from escape_the_valley.ledger_proof import run_proof

    class _DriftMgr(_FakeMgr):
        def fetch_onchain_memos(self, state):
            # Tamper: every fetched on-chain memo names the wrong run.
            return {txid: "TRAIL|RUN:IMPOSTER|DAY:0|DELTA:FOD-1"
                    for txid in type(self).onchain_memos}

    _DriftMgr.onchain_memos = {}
    monkeypatch.setattr(backpack_mod, "BackpackManager", _DriftMgr)

    report = run_proof(13, max_steps=80, isolate_save=True)

    # Balances still reconcile, but the external memo check catches the drift.
    if report.settlements_count > 0:
        assert report.onchain_memo_ok is False
        assert report.passed is False
