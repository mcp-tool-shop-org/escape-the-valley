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


def test_settled_record_empty_txids_fails_external():
    """ledger-A02: a settled record with no txids must NOT pass external memo
    integrity vacuously.

    When on-chain memos are supplied, the external loop iterates each record's
    txids. A settled record with an EMPTY txids list would skip the loop and
    leave onchain_memo_ok True with zero on-ledger evidence. The guard flags it
    as a miss so the proof cannot pass without any verifiable settlement tx.
    """
    recs = _settlements()
    # Strip the txids off the first settlement but keep it "settled".
    recs[0].txids = []
    # Supply on-chain memos for the OTHER record so the external block runs.
    onchain = {"BBB222": f"TRAIL|RUN:{RUN_ID}|DAY:9|DELTA:FOD+8,WTR-6,PRT-1"}
    report = _reconcile(settlements=recs, onchain_memos=onchain)
    assert report.onchain_memo_ok is False
    assert report.memo_ok is False
    assert report.passed is False
    assert any("no txids to verify on-chain" in n for n in report.notes)


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


# ── ledger-B06: INCONCLUSIVE vs FAIL banner ───────────────────────────


def test_markdown_inconclusive_when_only_pending():
    """ledger-B06: balances+memo all OK, sole blocker is unsettled checkpoints —
    render INCONCLUSIVE, not FAIL. This is a transient testnet outage, not drift.
    """
    pending = [SettlementRecord(day=12, location="Town C",
                                deltas={"water": -4}, status="pending")]
    report = _reconcile(pending=pending, onchain_memos=_onchain_ok())
    assert report.passed is False  # ANDON: not a PASS
    md = report_to_markdown(report)
    assert "INCONCLUSIVE" in md
    assert "FAIL" not in md.split("\n", 1)[0]  # title line is not FAIL
    assert "NOT a drift failure" in md
    assert "re-run" in md.lower()


def test_markdown_real_drift_still_fails_not_inconclusive():
    """A genuine balance mismatch must read FAIL even if a pending also exists —
    ANDON is preserved; drift is never relabeled INCONCLUSIVE."""
    pending = [SettlementRecord(day=12, location="Town C",
                                deltas={"water": -4}, status="pending")]
    tampered = dict(LEDGER_OK, FOD=40)  # real drift
    report = _reconcile(
        ledger_balances=tampered, pending=pending,
        onchain_memos=_onchain_ok(),
    )
    assert report.passed is False
    md = report_to_markdown(report)
    assert "INCONCLUSIVE" not in md
    assert "FAIL" in md.split("\n", 1)[0]


def test_clean_pass_markdown_is_pass_not_inconclusive():
    """No pending + all OK → PASS, never INCONCLUSIVE."""
    report = _reconcile(onchain_memos=_onchain_ok())
    assert report.passed is True
    md = report_to_markdown(report)
    assert "PASS" in md.split("\n", 1)[0]
    assert "INCONCLUSIVE" not in md


def test_dict_inconclusive_flag():
    """ledger-B06: the machine-readable dict carries the inconclusive flag."""
    pending = [SettlementRecord(day=12, location="Town C",
                                deltas={"water": -4}, status="pending")]
    report = _reconcile(pending=pending, onchain_memos=_onchain_ok())
    data = report_to_dict(report)
    assert data["inconclusive"] is True
    assert data["passed"] is False

    clean = report_to_dict(_reconcile(onchain_memos=_onchain_ok()))
    assert clean["inconclusive"] is False


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
        bp.minted_initial = dict(bp.last_settled_supplies)
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


def test_run_proof_does_not_leak_save_isolation_into_the_process(monkeypatch):
    """isolate_save must not disable autosave for anyone but the proof's engine.

    The isolation used to be ``step_engine.save_game = lambda *a, **k: None``
    with no restore, which silently no-op'd autosave for the whole process.
    Under pytest that meant every test collected after this file ran against a
    dead autosave, so any test asserting a save happened passed vacuously.
    """
    import escape_the_valley.backpack as backpack_mod
    import escape_the_valley.step_engine as step_engine_mod
    from escape_the_valley.ledger_proof import run_proof

    _FakeMgr.onchain_memos = {}
    monkeypatch.setattr(backpack_mod, "BackpackManager", _FakeMgr)

    # Compare against whatever is bound now rather than against save.save_game
    # itself: the invariant is "run_proof leaves this alone", which must hold
    # even when a plugin or fixture has legitimately wrapped it.
    before = step_engine_mod.save_game

    run_proof(13, max_steps=80, isolate_save=True)

    # The module global is untouched — a later StepEngine still autosaves.
    assert step_engine_mod.save_game is before


def test_run_proof_isolated_writes_no_save(tmp_path, monkeypatch):
    """The isolation still works: an isolated proof writes no .trail/ at all."""
    import escape_the_valley.backpack as backpack_mod
    from escape_the_valley.ledger_proof import run_proof

    _FakeMgr.onchain_memos = {}
    monkeypatch.setattr(backpack_mod, "BackpackManager", _FakeMgr)
    monkeypatch.chdir(tmp_path)

    run_proof(13, max_steps=80, isolate_save=True)

    assert not (tmp_path / ".trail").exists()


# ── proof_player_save / proof_loaded_save (F-a6efdd6c) ─────────────
#
# The player command proves the LOADED save, not a throwaway faucet seed.
# Network is mocked; run_proof() is not called.


def _player_state(*, pending=False, minted=True):
    from escape_the_valley.models import RunState, SuppliesState
    from escape_the_valley.backpack_models import stamp_minted_snapshot

    state = RunState(
        run_id=RUN_ID,
        seed=SEED,
        supplies=SuppliesState(items=dict(SETTLED)),
    )
    bp = state.backpack
    bp.enabled = True
    bp.wallet_address = "rPlayer"
    bp.issuer_address = "rIssuer"
    bp.trust_lines_ready = True
    bp.last_settled_supplies = dict(SETTLED)
    bp.settlements = _settlements()
    if pending:
        bp.pending_settlements = [
            SettlementRecord(
                day=12, location="Town C",
                deltas={"water": -4}, status="pending",
            ),
        ]
    if minted:
        stamp_minted_snapshot(bp, dict(MINTED))
    return state


class _ProofMgr:
    """Read-only fake manager for proof_player_save — no faucet, no settle."""

    available = True
    balances: dict = LEDGER_OK
    balances_error = False
    extra_missing = False

    def wallet_info(self, state):
        return {
            "address": state.backpack.wallet_address,
            "balances": dict(type(self).balances),
            "balances_error": type(self).balances_error,
            "extra_missing": type(self).extra_missing,
        }

    def fetch_onchain_memos(self, state):
        return dict(_onchain_ok())

    def close(self):
        pass


def test_proof_player_save_pass_on_loaded_save():
    """Loaded save with mint snapshot + live receipts → PASS.

    Must not call run_proof / faucet a seed.
    """
    from escape_the_valley.ledger_proof import (
        proof_loaded_save,
        proof_player_save,
    )

    state = _player_state()
    result = proof_player_save(state, manager=_ProofMgr())
    assert result.verdict == "PASS"
    assert result.passed is True
    assert result.report is not None
    assert result.report.passed is True
    assert "PASS" in result.markdown.split("\n", 1)[0]
    # Alias is the same API.
    alias = proof_loaded_save(state, manager=_ProofMgr())
    assert alias.verdict == "PASS"


def test_proof_player_save_fail_on_balance_drift():
    from escape_the_valley.ledger_proof import proof_player_save

    class _Drift(_ProofMgr):
        balances = dict(LEDGER_OK, FOD=1)

    result = proof_player_save(_player_state(), manager=_Drift())
    assert result.verdict == "FAIL"
    assert "FAIL" in result.markdown.split("\n", 1)[0]
    assert "INCONCLUSIVE" not in result.markdown.split("\n", 1)[0]


def test_proof_player_save_inconclusive_when_pending():
    from escape_the_valley.ledger_proof import proof_player_save

    result = proof_player_save(_player_state(pending=True), manager=_ProofMgr())
    assert result.verdict == "INCONCLUSIVE"
    assert "INCONCLUSIVE" in result.markdown.split("\n", 1)[0]


def test_proof_player_save_inconclusive_when_network_down():
    from escape_the_valley.ledger_proof import proof_player_save

    class _Down(_ProofMgr):
        balances = {}
        balances_error = True

    result = proof_player_save(_player_state(), manager=_Down())
    assert result.verdict == "INCONCLUSIVE"
    assert any("could not reach" in n.lower() for n in result.notes)


def test_proof_player_save_inconclusive_when_backpack_off():
    from escape_the_valley.ledger_proof import proof_player_save
    from escape_the_valley.models import RunState

    state = RunState(run_id="x", seed=1)
    result = proof_player_save(state, manager=_ProofMgr())
    assert result.verdict == "INCONCLUSIVE"
    assert result.report is None
    assert "not enabled" in result.markdown.lower()


def test_proof_player_save_inconclusive_when_mint_snapshot_missing():
    """Legacy save: last_settled already includes deltas, no mint snapshot.

    Reconstructing minted from last_settled is tautological, so even a
    clean live match is INCONCLUSIVE — not PASS.
    """
    from escape_the_valley.ledger_proof import proof_player_save

    state = _player_state(minted=False)
    result = proof_player_save(state, manager=_ProofMgr())
    assert result.verdict == "INCONCLUSIVE"
    assert any("mint snapshot missing" in n for n in result.notes)


def test_proof_player_save_still_fails_drift_without_mint_snapshot():
    """Missing snapshot does not green-wash a live balance mismatch."""
    from escape_the_valley.ledger_proof import proof_player_save

    class _Drift(_ProofMgr):
        balances = dict(LEDGER_OK, FOD=1)

    result = proof_player_save(_player_state(minted=False), manager=_Drift())
    assert result.verdict == "FAIL"


def test_proof_player_save_does_not_call_run_proof(monkeypatch):
    """The player path must not drive the throwaway faucet harness."""
    import escape_the_valley.ledger_proof as proof_mod
    from escape_the_valley.ledger_proof import proof_player_save

    def _boom(*a, **k):
        raise AssertionError("run_proof must not be called")

    monkeypatch.setattr(proof_mod, "run_proof", _boom)
    result = proof_player_save(_player_state(), manager=_ProofMgr())
    assert result.verdict == "PASS"


def test_manager_proof_loaded_save_delegates(monkeypatch):
    """BackpackManager.proof_loaded_save is the CLI-shaped hook."""
    from escape_the_valley.backpack import BackpackManager
    from escape_the_valley.ledger_proof import PlayerProofResult

    captured = {}

    def fake_proof(state, *, manager=None):
        captured["mgr"] = manager
        return PlayerProofResult(
            verdict="PASS", report=None, markdown="# PASS", notes=[],
        )

    monkeypatch.setattr(
        "escape_the_valley.ledger_proof.proof_player_save", fake_proof,
    )
    mgr = BackpackManager()
    result = mgr.proof_loaded_save(_player_state())
    assert result.verdict == "PASS"
    assert captured["mgr"] is mgr


def test_overlay_dict_has_no_secrets():
    from escape_the_valley.ledger_proof import proof_player_save

    result = proof_player_save(_player_state(), manager=_ProofMgr())
    data = result.to_overlay_dict()
    blob = json_str(data)
    assert "wallet_secret" not in blob
    assert "issuer_secret" not in blob
    assert data["verdict"] == "PASS"
