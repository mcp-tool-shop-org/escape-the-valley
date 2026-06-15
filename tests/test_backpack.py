"""Tests for BackpackManager — mock XRPL, no network."""

import pytest

from escape_the_valley import backpack as backpack_mod
from escape_the_valley.backpack import (
    BackpackManager,
    _balance_to_int,
    _build_memo,
    _build_parcel_memo,
    _decode_parcel_memo,
    _hex_decode,
    _hex_encode,
    _settlement_memo_text,
    _shorten_address,
)
from escape_the_valley.backpack_models import ParcelRecord, SettlementRecord
from escape_the_valley.models import RunState, SuppliesState


def _make_state(**overrides) -> RunState:
    defaults = dict(
        run_id="test1234",
        seed=1,
        day=5,
        supplies=SuppliesState(items={
            "food": 50, "water": 50, "meds": 5,
            "ammo": 20, "parts": 3,
        }),
    )
    defaults.update(overrides)
    return RunState(**defaults)


class TestHelpers:
    def test_hex_encode(self):
        result = _hex_encode("hello")
        assert result == "68656C6C6F"

    def test_shorten_address_short(self):
        assert _shorten_address("rABC") == "rABC"

    def test_shorten_address_long(self):
        addr = "rN7qKvMzTdmhcjbw1234567890xKp"
        result = _shorten_address(addr)
        assert result.startswith("rN7q")
        assert result.endswith("xKp")  # last 4
        assert "..." in result

    def test_build_memo_format(self):
        deltas = {"food": -3, "water": 5}
        memos = _build_memo("run1", 5, deltas)
        assert len(memos) == 1
        # Memo data is hex-encoded
        assert memos[0].memo_data is not None


class TestBackpackManagerAvailability:
    def test_available_reflects_import(self):
        mgr = BackpackManager()
        # _HAS_XRPL may or may not be True depending on env
        assert isinstance(mgr.available, bool)

    def test_status_line_off(self):
        state = _make_state()
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "OFF" in line

    def test_status_line_on(self):
        state = _make_state()
        state.backpack.enabled = True
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "ON" in line

    def test_status_line_unsettled(self):
        from escape_the_valley.backpack_models import SettlementRecord

        state = _make_state()
        state.backpack.enabled = True
        state.backpack.pending_settlements = [
            SettlementRecord(day=3, location="Test", status="pending"),
        ]
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "Unsettled: 1 checkpoint" in line
        assert "checkpoints" not in line  # singular

    def test_status_line_unsettled_plural(self):
        from escape_the_valley.backpack_models import SettlementRecord

        state = _make_state()
        state.backpack.enabled = True
        state.backpack.pending_settlements = [
            SettlementRecord(day=3, location="A", status="pending"),
            SettlementRecord(day=5, location="B", status="pending"),
        ]
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "Unsettled: 2 checkpoints" in line

    def test_wallet_info_no_wallet(self):
        state = _make_state()
        mgr = BackpackManager()
        info = mgr.wallet_info(state)
        assert info["status"] == "No wallet"

    def test_wallet_info_with_wallet(self):
        state = _make_state()
        state.backpack.wallet_address = "rTestAddress12345678"
        state.backpack.issuer_address = "rIssuerAddress12345"
        state.backpack.trust_lines_ready = True
        mgr = BackpackManager()
        info = mgr.wallet_info(state)
        assert "address" in info
        assert info["trust_lines"] is True


class TestDisable:
    def test_disable_marks_off(self):
        state = _make_state()
        state.backpack.enabled = True
        mgr = BackpackManager()
        mgr.disable(state)
        assert state.backpack.enabled is False


class TestAcceptParcel:
    def test_accept_applies_supplies(self):
        from escape_the_valley.backpack_models import ParcelRecord

        state = _make_state()
        parcel = ParcelRecord(
            parcel_id="test:FOD:5",
            sender="rSender",
            contents={"food": 5},
            day_received=3,
        )
        mgr = BackpackManager()
        result = mgr.accept_parcel(parcel, state)
        assert result is True
        assert parcel.accepted is True
        assert state.supplies.food == 55  # 50 + 5

    def test_accept_respects_cap(self):
        from escape_the_valley.backpack_models import ParcelRecord

        state = _make_state()
        parcel = ParcelRecord(
            parcel_id="test:FOD:100",
            sender="rSender",
            contents={"food": 100},
            day_received=3,
        )
        mgr = BackpackManager()
        mgr.accept_parcel(parcel, state, cap=20)
        assert state.supplies.food == 70  # 50 + 20 (capped)


class TestSettleNoXrpl:
    def test_settle_not_enabled(self):
        state = _make_state()
        mgr = BackpackManager()
        result = mgr.settle(state, "TestTown")
        assert result.success is False
        assert "not enabled" in result.message.lower() or "not available" in result.message.lower()

    def test_enable_without_xrpl(self):
        """Enable should fail gracefully if xrpl-py not installed."""
        mgr = BackpackManager()
        if not mgr.available:
            state = _make_state()
            result = mgr.enable(state)
            assert result.success is False
            assert "xrpl" in result.message.lower()


class TestParcelMemo:
    def test_hex_roundtrip(self):
        original = "PARCEL|RUN:abc|DAY:5|food:10"
        encoded = _hex_encode(original)
        decoded = _hex_decode(encoded)
        assert decoded == original

    def test_build_parcel_memo(self):
        memos = _build_parcel_memo("run1", 5, "food", 10)
        assert len(memos) == 1
        assert memos[0].memo_data is not None

    def test_decode_parcel_memo_valid(self):
        memo_text = "PARCEL|RUN:abc|DAY:5|food:10"
        hex_data = _hex_encode(memo_text)
        result = _decode_parcel_memo(hex_data)
        assert result is not None
        assert result["supply"] == "food"
        assert result["amount"] == 10

    def test_decode_parcel_memo_all_supplies(self):
        for supply in ("food", "water", "meds", "ammo", "parts"):
            memo_text = f"PARCEL|RUN:x|DAY:1|{supply}:5"
            result = _decode_parcel_memo(_hex_encode(memo_text))
            assert result is not None
            assert result["supply"] == supply

    def test_decode_parcel_memo_not_parcel(self):
        memo_text = "TRAIL|RUN:abc|DAY:5|DELTA:FOD+3"
        result = _decode_parcel_memo(_hex_encode(memo_text))
        assert result is None

    def test_decode_parcel_memo_invalid_supply(self):
        memo_text = "PARCEL|RUN:abc|DAY:5|gold:10"
        result = _decode_parcel_memo(_hex_encode(memo_text))
        assert result is None

    def test_decode_parcel_memo_invalid_hex(self):
        result = _decode_parcel_memo("ZZZZ")
        assert result is None

    def test_decode_parcel_memo_empty(self):
        result = _decode_parcel_memo("")
        assert result is None


class TestSendParcelValidation:
    """Test send_parcel input validation (no XRPL needed)."""

    def test_send_not_enabled(self):
        state = _make_state()
        mgr = BackpackManager()
        result = mgr.send_parcel(state, "rRecipient", "food", 5)
        assert result.success is False

    def test_send_invalid_supply(self):
        state = _make_state()
        state.backpack.enabled = True
        state.backpack.wallet_address = "rSender"
        mgr = BackpackManager()
        result = mgr.send_parcel(state, "rRecipient", "gold", 5)
        assert result.success is False
        assert "Unknown supply" in result.message

    def test_send_zero_amount(self):
        state = _make_state()
        state.backpack.enabled = True
        state.backpack.wallet_address = "rSender"
        mgr = BackpackManager()
        result = mgr.send_parcel(state, "rRecipient", "food", 0)
        assert result.success is False
        assert "positive" in result.message.lower()

    def test_send_negative_amount(self):
        state = _make_state()
        state.backpack.enabled = True
        state.backpack.wallet_address = "rSender"
        mgr = BackpackManager()
        result = mgr.send_parcel(state, "rRecipient", "food", -5)
        assert result.success is False

    def test_send_insufficient_supplies(self):
        state = _make_state()
        state.backpack.enabled = True
        state.backpack.wallet_address = "rSender"
        mgr = BackpackManager()
        result = mgr.send_parcel(state, "rRecipient", "food", 999)
        assert result.success is False
        assert "Not enough" in result.message

    def test_send_to_self(self):
        state = _make_state()
        state.backpack.enabled = True
        state.backpack.wallet_address = "rSelfAddress"
        mgr = BackpackManager()
        result = mgr.send_parcel(state, "rSelfAddress", "food", 5)
        assert result.success is False
        assert "yourself" in result.message.lower()

    def test_send_without_xrpl(self):
        """Send should fail gracefully if xrpl-py not installed."""
        mgr = BackpackManager()
        if not mgr.available:
            state = _make_state()
            state.backpack.enabled = True
            state.backpack.wallet_address = "rSender"
            result = mgr.send_parcel(state, "rRecipient", "food", 5)
            assert result.success is False
            assert "xrpl" in result.message.lower()


class TestRefuseParcel:
    def test_refuse_marks_parcel(self):
        from escape_the_valley.backpack_models import ParcelRecord

        parcel = ParcelRecord(
            parcel_id="tx123",
            sender="rSender",
            contents={"food": 5},
            day_received=3,
        )
        mgr = BackpackManager()
        result = mgr.refuse_parcel(parcel)
        assert result is True
        assert parcel.parcel_id.startswith("refused:")

    def test_refuse_already_accepted(self):
        from escape_the_valley.backpack_models import ParcelRecord

        parcel = ParcelRecord(
            parcel_id="tx123",
            sender="rSender",
            contents={"food": 5},
            accepted=True,
            day_received=3,
        )
        mgr = BackpackManager()
        result = mgr.refuse_parcel(parcel)
        assert result is False

    def test_refuse_does_not_apply_supplies(self):
        from escape_the_valley.backpack_models import ParcelRecord

        state = _make_state()
        original_food = state.supplies.food
        parcel = ParcelRecord(
            parcel_id="tx123",
            sender="rSender",
            contents={"food": 10},
            day_received=3,
        )
        mgr = BackpackManager()
        mgr.refuse_parcel(parcel)
        assert state.supplies.food == original_food


class TestSettleDeltaComputation:
    def test_no_changes_no_settlement(self):
        state = _make_state()
        state.backpack.enabled = True
        state.backpack.wallet_address = "rTest"
        state.backpack.wallet_secret = "sTest"
        state.backpack.issuer_address = "rIssuer"
        state.backpack.issuer_secret = "sIssuer"
        # Set last settled = current
        state.backpack.last_settled_supplies = {
            "food": 50, "water": 50, "meds": 5,
            "ammo": 20, "parts": 3,
        }

        mgr = BackpackManager()
        if not mgr.available:
            return  # Can't test XRPL ops without xrpl-py
        result = mgr.settle(state, "TestTown")
        # No deltas → success with "No changes"
        assert result.success is True
        assert "no changes" in result.message.lower()


# ──────────────────────────────────────────────────────────────────────
# Mock-based XRPL coverage (A-06 / ledger-008). The network is never
# touched: we monkeypatch the xrpl-py entry points that backpack.py
# imported at module load. These tests require the xrpl extra to be
# installed (so the real symbols exist to patch); they skip otherwise.
# ──────────────────────────────────────────────────────────────────────

requires_xrpl = pytest.mark.skipif(
    not backpack_mod._HAS_XRPL,
    reason="xrpl-py not installed; mock-based XRPL tests need the real symbols",
)


class _FakeWallet:
    """Stand-in for an xrpl Wallet (faucet or from_seed)."""

    def __init__(self, address: str, seed: str = ""):
        self.address = address
        self.seed = seed


class _FakeResp:
    def __init__(self, result: dict):
        self.result = result


class _FakeClient:
    """Records nothing; request() is overridden per-test as needed."""

    def __init__(self, *a, **k):
        pass

    def request(self, _req):  # pragma: no cover - overridden in tests
        return _FakeResp({})

    def close(self):
        pass


def _enabled_state(**overrides) -> RunState:
    state = _make_state(**overrides)
    bp = state.backpack
    bp.enabled = True
    bp.wallet_address = "rPlayerAddr"
    bp.wallet_secret = "sPlayerSeed"
    bp.issuer_address = "rIssuerAddr"
    bp.issuer_secret = "sIssuerSeed"
    bp.trust_lines_ready = True
    bp.last_settled_supplies = {
        "food": 50, "water": 50, "meds": 5, "ammo": 20, "parts": 3,
    }
    return state


def _patch_signing(monkeypatch, *, submit_hashes=None, fail_keys=None):
    """Patch Wallet.from_seed + submit_and_wait. Returns the call log.

    ``submit_hashes`` — iterable of hashes to hand back (cycled).
    ``fail_keys`` — set of currency codes whose Payment should raise.
    """
    calls = {"submit": [], "memos": []}
    hashes = list(submit_hashes or ["HASH0", "HASH1", "HASH2", "HASH3", "HASH4"])
    fail_keys = fail_keys or set()

    def fake_from_seed(seed, *a, **k):
        return _FakeWallet("rPlayerAddr" if seed == "sPlayerSeed" else "rIssuerAddr")

    def fake_submit(tx, client, signer):
        # Capture memo bytes if present so tests can assert on-chain content.
        memos = getattr(tx, "memos", None)
        if memos:
            calls["memos"].append(_hex_decode(memos[0].memo_data))
        # Amount currency code, to decide failure injection.
        amount = getattr(tx, "amount", None)
        code = getattr(amount, "currency", None)
        if code in fail_keys:
            raise RuntimeError(f"submit failed for {code}")
        h = hashes[len(calls["submit"]) % len(hashes)]
        calls["submit"].append((code, h))
        return _FakeResp({"hash": h})

    monkeypatch.setattr(backpack_mod.Wallet, "from_seed", staticmethod(fake_from_seed))
    monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit)
    return calls


class TestEnableMocked:
    @requires_xrpl
    def test_first_enable_mints_and_records(self, monkeypatch):
        state = _make_state()
        wallets = iter([
            _FakeWallet("rIssuerAddr", "sIssuerSeed"),
            _FakeWallet("rPlayerAddr", "sPlayerSeed"),
        ])
        monkeypatch.setattr(
            backpack_mod, "generate_faucet_wallet",
            lambda *a, **k: next(wallets),
        )
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch)

        res = mgr.enable(state)
        assert res.success is True
        assert state.backpack.enabled is True
        assert state.backpack.wallet_address == "rPlayerAddr"
        assert state.backpack.issuer_address == "rIssuerAddr"
        assert state.backpack.trust_lines_ready is True
        # Snapshot captured from engine supplies.
        assert state.backpack.last_settled_supplies["food"] == 50

    @requires_xrpl
    def test_enable_idempotent_no_regen_no_remint(self, monkeypatch):
        """ledger-002: a second enable() must not regenerate or re-mint."""
        state = _make_state()
        wallets = iter([
            _FakeWallet("rIssuerAddr", "sIssuerSeed"),
            _FakeWallet("rPlayerAddr", "sPlayerSeed"),
        ])
        faucet_calls = {"n": 0}

        def fake_faucet(*a, **k):
            faucet_calls["n"] += 1
            return next(wallets)

        monkeypatch.setattr(backpack_mod, "generate_faucet_wallet", fake_faucet)
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch)

        mgr.enable(state)
        first_addr = state.backpack.wallet_address
        first_issuer_seed = state.backpack.issuer_secret
        assert faucet_calls["n"] == 2  # issuer + player on first enable

        # disable() keeps the wallet; re-enable must reuse it.
        mgr.disable(state)
        res2 = mgr.enable(state)

        assert res2.success is True
        assert state.backpack.enabled is True
        assert faucet_calls["n"] == 2  # NO new faucet wallets generated
        assert state.backpack.wallet_address == first_addr  # stable address
        assert state.backpack.issuer_secret == first_issuer_seed


class TestSettleMocked:
    @requires_xrpl
    def test_settle_with_deltas_records_and_memos(self, monkeypatch):
        state = _enabled_state()
        # Spend some food + water, gain nothing.
        state.supplies.set("food", 38)   # -12
        state.supplies.set("water", 44)  # -6
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        calls = _patch_signing(monkeypatch, submit_hashes=["TXF", "TXW"])

        res = mgr.settle(state, "Millford")
        assert res.success is True
        assert res.record is not None
        rec = state.backpack.settlements[-1]
        assert rec.status == "settled"
        assert rec.deltas == {"food": -12, "water": -6}
        assert len(rec.txids) == 2
        # ledger-003: stored memo equals the on-chain bytes (DELTA suffix).
        expected_memo = _settlement_memo_text(
            state.run_id, state.day, {"food": -12, "water": -6},
        )
        assert rec.memo == expected_memo
        assert all(m == expected_memo for m in calls["memos"])
        # Snapshot advanced to the new supplies.
        assert state.backpack.last_settled_supplies["food"] == 38

    @requires_xrpl
    def test_settle_failure_records_pending(self, monkeypatch):
        state = _enabled_state()
        state.supplies.set("food", 40)  # -10 → triggers a FOD payment that fails
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, fail_keys={"FOD"})

        res = mgr.settle(state, "BadTown")
        assert res.success is False
        assert len(state.backpack.pending_settlements) == 1
        pending = state.backpack.pending_settlements[0]
        assert pending.status == "pending"
        # Pending memo also carries the canonical header+delta (ledger-003).
        assert pending.memo.startswith(f"TRAIL|RUN:{state.run_id}|DAY:{state.day}")
        # Snapshot NOT advanced on failure.
        assert state.backpack.last_settled_supplies["food"] == 50


class TestRetryPendingMocked:
    @requires_xrpl
    def test_retry_success_moves_to_settled(self, monkeypatch):
        state = _enabled_state()
        state.backpack.pending_settlements = [
            SettlementRecord(
                day=4, location="Earlier", deltas={"water": -5},
                status="pending", memo=_settlement_memo_text(
                    state.run_id, 4, {"water": -5},
                ),
            ),
        ]
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, submit_hashes=["RETRYHASH"])

        mgr._retry_pending(state)
        assert state.backpack.pending_settlements == []
        assert len(state.backpack.settlements) == 1
        settled = state.backpack.settlements[0]
        assert settled.status == "settled"
        assert settled.txids == ["RETRYHASH"]
        # Memo refreshed to match on-chain bytes (ledger-003).
        assert settled.memo == _settlement_memo_text(
            state.run_id, 4, {"water": -5},
        )

    @requires_xrpl
    def test_retry_failure_stays_pending(self, monkeypatch):
        state = _enabled_state()
        state.backpack.pending_settlements = [
            SettlementRecord(
                day=4, location="Earlier", deltas={"food": -5},
                status="pending",
            ),
        ]
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, fail_keys={"FOD"})

        mgr._retry_pending(state)
        assert len(state.backpack.pending_settlements) == 1
        assert state.backpack.settlements == []

    @requires_xrpl
    def test_fail_then_retry_no_conservation_double_count(self, monkeypatch):
        """ENG-A-08: a failed settle, then a successful retry-plus-fresh settle,
        must NOT double-count the retried delta.

        Town A: consume food 50→40 (delta -10) but the FOD payment fails, so a
        pending record is enqueued and the baseline stays at 50.
        Town B: consume more food 40→35, then settle() succeeds. settle() runs
        _retry_pending() first (settles the -10 and, with the fix, advances the
        baseline 50→40), then computes a fresh delta of only -5 (35 - 40).

        Net on-chain food movement must equal the true net supply change
        (50→35 = -15), and reconcile() must pass (minted + Σdeltas == final).
        Pre-fix the baseline stayed at 50 after the retry, so the fresh settle()
        spanned the whole -15 interval — paying the -10 twice (once in retry,
        once folded into the fresh delta) and double-summing it in reconcile().
        """
        from escape_the_valley.backpack_models import (
            XRPL_RESOURCES,
            XRPL_TOKEN_MAP,
        )
        from escape_the_valley.ledger_proof import reconcile

        state = _enabled_state()
        minted = dict(state.backpack.last_settled_supplies)  # food=50, ...
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        # ── Town A: FOD payment fails → pending record, baseline NOT advanced.
        state.supplies.set("food", 40)  # delta -10
        _patch_signing(monkeypatch, fail_keys={"FOD"})
        res_a = mgr.settle(state, "TownA")
        assert res_a.success is False
        assert len(state.backpack.pending_settlements) == 1
        assert state.backpack.last_settled_supplies["food"] == 50  # un-advanced

        # ── Town B: more consumption, and now FOD succeeds.
        state.supplies.set("food", 35)  # further -5; true net since A-baseline = -15
        # Track every on-chain food movement (signed) so we can prove no double-pay.
        food_moves: list[int] = []

        def fake_from_seed(seed, *a, **k):
            return _FakeWallet(
                "rPlayerAddr" if seed == "sPlayerSeed" else "rIssuerAddr",
            )

        hashes = iter(f"HASH{i}" for i in range(100))

        def fake_submit(tx, client, signer):
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            value = getattr(amount, "value", None)
            if code == "FOD":
                # Player→issuer payment means food leaving the pack (negative).
                signed = -int(value) if tx.account == "rPlayerAddr" else int(value)
                food_moves.append(signed)
            return _FakeResp({"hash": next(hashes)})

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(fake_from_seed),
        )
        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit)

        res_b = mgr.settle(state, "TownB")
        assert res_b.success is True
        assert state.backpack.pending_settlements == []
        # Baseline advanced exactly to current supplies.
        assert state.backpack.last_settled_supplies["food"] == 35

        # Net on-chain food movement equals the true net supply change: 50→35.
        assert sum(food_moves) == -15

        # reconcile(): minted + Σ(all settled deltas) == final settled supplies.
        ledger_balances = {
            XRPL_TOKEN_MAP[k][0]: state.backpack.last_settled_supplies.get(k, 0)
            for k in XRPL_RESOURCES
        }
        report = reconcile(
            run_id=state.run_id,
            seed=state.seed,
            minted_initial=minted,
            ledger_balances=ledger_balances,
            last_settled_supplies=state.backpack.last_settled_supplies,
            settlements=state.backpack.settlements,
            pending=state.backpack.pending_settlements,
        )
        assert report.passed is True, report.notes
        food = next(r for r in report.resources if r.resource == "food")
        # Σ deltas across both settled records must telescope to -15, not -25.
        assert food.sum_deltas == -15
        assert food.minted + food.sum_deltas == food.engine_settled == 35


class TestSendParcelMocked:
    @requires_xrpl
    def test_send_success_deducts_and_records(self, monkeypatch):
        state = _enabled_state()
        before = state.supplies.food
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, submit_hashes=["PARCELTX"])

        res = mgr.send_parcel(state, "rRecipient", "food", 7)
        assert res.success is True
        assert res.txid == "PARCELTX"
        assert state.supplies.food == before - 7  # deducted exactly once
        assert len(state.backpack.sent_parcels) == 1
        sent = state.backpack.sent_parcels[0]
        assert sent.recipient == "rRecipient"
        assert sent.amount == 7
        assert sent.supply == "food"

    @requires_xrpl
    def test_send_failure_keeps_supplies(self, monkeypatch):
        state = _enabled_state()
        before = state.supplies.food
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        def boom(*a, **k):
            raise RuntimeError("network down")

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed",
            staticmethod(lambda *a, **k: _FakeWallet("rPlayerAddr")),
        )
        monkeypatch.setattr(backpack_mod, "submit_and_wait", boom)

        res = mgr.send_parcel(state, "rRecipient", "food", 7)
        assert res.success is False
        assert state.supplies.food == before  # unchanged on failure
        assert state.backpack.sent_parcels == []


class TestAcceptParcelIdempotentMocked:
    def test_accept_twice_applies_once(self):
        """ledger-005: a second accept on the same parcel is a no-op."""
        state = _make_state()
        before = state.supplies.food
        parcel = ParcelRecord(
            parcel_id="tx:FOD:5", sender="rSender",
            contents={"food": 5}, day_received=3,
        )
        mgr = BackpackManager()

        first = mgr.accept_parcel(parcel, state)
        second = mgr.accept_parcel(parcel, state)

        assert first is True
        assert second is False
        assert parcel.accepted is True
        assert state.supplies.food == before + 5  # applied exactly once


class TestWalletInfoMocked:
    @requires_xrpl
    def test_integer_parsing_from_decimal_strings(self, monkeypatch):
        state = _enabled_state()

        class _LinesClient(_FakeClient):
            def request(self, _req):
                return _FakeResp({"lines": [
                    {"account": "rIssuerAddr", "currency": "FOD", "balance": "38"},
                    {"account": "rIssuerAddr", "currency": "WTR", "balance": "44.0"},
                    {"account": "rIssuerAddr", "currency": "MED", "balance": "5.000"},
                    # foreign issuer line — must be ignored
                    {"account": "rStranger", "currency": "XXX", "balance": "99"},
                ]})

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _LinesClient())

        info = mgr.wallet_info(state)
        assert info["balances"] == {"FOD": 38, "WTR": 44, "MED": 5}
        # Exact integers, no float drift.
        assert all(isinstance(v, int) for v in info["balances"].values())

    @requires_xrpl
    def test_non_integer_balance_dropped_not_truncated(self, monkeypatch):
        state = _enabled_state()

        class _LinesClient(_FakeClient):
            def request(self, _req):
                return _FakeResp({"lines": [
                    {"account": "rIssuerAddr", "currency": "FOD", "balance": "12.5"},
                    {"account": "rIssuerAddr", "currency": "WTR", "balance": "44"},
                ]})

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _LinesClient())

        info = mgr.wallet_info(state)
        # The fractional balance is a drift signal — dropped, never floored to 12.
        assert "FOD" not in info["balances"]
        assert info["balances"]["WTR"] == 44


class TestBalanceToInt:
    def test_decimal_exact(self):
        assert _balance_to_int("50") == 50
        assert _balance_to_int("50.0") == 50
        assert _balance_to_int("0") == 0

    def test_large_value_no_float_drift(self):
        # 999999999999999 is beyond exact float53 range; Decimal stays exact.
        assert _balance_to_int("999999999999999") == 999999999999999

    def test_non_integer_raises(self):
        with pytest.raises(ValueError):
            _balance_to_int("12.5")


class TestFetchOnchainMemos:
    @requires_xrpl
    def test_decodes_onchain_memos_by_txid(self, monkeypatch):
        """ledger-003 external half: read settlement memos back off-chain."""
        state = _enabled_state()
        memo_text = "TRAIL|RUN:test1234|DAY:5|DELTA:FOD-12"

        class _TxClient(_FakeClient):
            def request(self, req):
                # api_version-2 shape: hash on the wrapping entry, tx_json inner.
                return _FakeResp({"transactions": [
                    {
                        "hash": "TXHASH1",
                        "tx_json": {
                            "TransactionType": "Payment",
                            "Memos": [{"Memo": {"MemoData": _hex_encode(memo_text)}}],
                        },
                    },
                    # No-memo tx is skipped.
                    {"hash": "TXHASH2", "tx_json": {"TransactionType": "Payment"}},
                ]})

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _TxClient())

        memos = mgr.fetch_onchain_memos(state)
        assert memos["TXHASH1"] == memo_text
        assert "TXHASH2" not in memos

    @requires_xrpl
    def test_paginates_via_marker(self, monkeypatch):
        """ledger-A03: AccountTx is paged via the response marker until exhausted.

        A long run can exceed one page (limit=200). Without pagination, older
        memos beyond page 1 are dropped — a FALSE-NEGATIVE proof failure. Mock a
        2-page response per account and assert every memo across pages is
        gathered.
        """
        state = _enabled_state()
        memo_p1 = "TRAIL|RUN:test1234|DAY:1|DELTA:FOD-1"
        memo_p2 = "TRAIL|RUN:test1234|DAY:2|DELTA:FOD-2"

        def page(tx_hash: str, memo: str) -> dict:
            return {
                "hash": tx_hash,
                "tx_json": {
                    "TransactionType": "Payment",
                    "Memos": [{"Memo": {"MemoData": _hex_encode(memo)}}],
                },
            }

        class _PagedClient(_FakeClient):
            def __init__(self):
                # One independent page cursor per account so wallet + issuer
                # both walk their own 2-page sequence.
                self._cursor: dict[str, int] = {}

            def request(self, req):
                account = req.account
                page_idx = self._cursor.get(account, 0)
                self._cursor[account] = page_idx + 1
                if page_idx == 0:
                    # Page 1: a marker signals more remain.
                    return _FakeResp({
                        "transactions": [page(f"{account}-TX1", memo_p1)],
                        "marker": {"ledger": 1, "seq": 7},
                    })
                # Page 2: no marker → done.
                return _FakeResp({
                    "transactions": [page(f"{account}-TX2", memo_p2)],
                })

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _PagedClient())

        memos = mgr.fetch_onchain_memos(state)
        # Both pages of BOTH accounts were gathered (wallet + issuer).
        assert memos["rPlayerAddr-TX1"] == memo_p1
        assert memos["rPlayerAddr-TX2"] == memo_p2
        assert memos["rIssuerAddr-TX1"] == memo_p1
        assert memos["rIssuerAddr-TX2"] == memo_p2
        assert len(memos) == 4


class TestCheckParcelsTxHash:
    @requires_xrpl
    def test_hash_read_from_wrapping_entry(self, monkeypatch):
        """ledger-007: api_version-2 puts the tx hash on the wrapping entry."""
        state = _enabled_state()
        parcel_memo = "PARCEL|RUN:abc|DAY:5|food:6"

        class _TxClient(_FakeClient):
            def request(self, req):
                return _FakeResp({"transactions": [
                    {
                        "hash": "WRAPHASH",  # hash lives here, not in tx_json
                        "meta": {"TransactionResult": "tesSUCCESS"},
                        "tx_json": {
                            "TransactionType": "Payment",
                            "Account": "rSomeSender",
                            "Destination": "rPlayerAddr",
                            "Memos": [
                                {"Memo": {"MemoData": _hex_encode(parcel_memo)}},
                            ],
                        },
                    },
                ]})

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _TxClient())

        parcels = mgr.check_parcels(state)
        assert len(parcels) == 1
        assert parcels[0].parcel_id == "WRAPHASH"
        assert parcels[0].txid == "WRAPHASH"
        assert parcels[0].contents == {"food": 6}
