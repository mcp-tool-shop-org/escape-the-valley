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
    _setup_complete,
    _shorten_address,
)
from escape_the_valley.backpack_models import (
    MEMO_SCHEMA_VERSION,
    PARCEL_ACCEPT_CAP,
    ParcelRecord,
    SettlementRecord,
)
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

    @requires_xrpl
    def test_fail_after_retry_chain_stays_conservation_consistent(
        self, monkeypatch,
    ):
        """ENG-A-08 (adversarial, fail-AFTER-retry).

        A cross-family review argued that advancing the baseline inside
        _retry_pending corrupts state when the SAME settle() then fails. This
        locks that exact chain and proves conservation holds end to end.

        Town A: food 50->40 (-10); the FOD payment fails -> pending, baseline 50.
        Town B: food 40->35. _retry_pending settles A's -10 (baseline 50->40),
                then the FRESH FOD payment fails -> a new pending -5 (35-40),
                baseline 40. reconcile() must report NOT passed (pending remains).
        Town C: no consumption. _retry_pending settles the -5 (baseline 40->35),
                the fresh delta is 0 -> success. reconcile() must pass; the
                settled deltas telescope to -15 and no FOD is paid twice.
        Pre-fix the baseline would not advance on retry, so Town B's fresh delta
        spans the whole 50->35 interval and the -10 is paid/summed twice.
        """
        from escape_the_valley.backpack_models import (
            XRPL_RESOURCES,
            XRPL_TOKEN_MAP,
        )
        from escape_the_valley.ledger_proof import reconcile

        state = _enabled_state()
        minted = dict(state.backpack.last_settled_supplies)  # food=50
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        def fake_from_seed(seed, *a, **k):
            return _FakeWallet(
                "rPlayerAddr" if seed == "sPlayerSeed" else "rIssuerAddr",
            )

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(fake_from_seed),
        )

        food_moves: list[int] = []
        hashes = iter(f"HASH{i}" for i in range(100))
        fod_calls = {"n": 0}
        fail_on = {"call": None}  # which FOD payment (1-indexed) should raise

        def fake_submit(tx, client, signer):
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            value = getattr(amount, "value", None)
            if code == "FOD":
                fod_calls["n"] += 1
                if fail_on["call"] is not None and fod_calls["n"] == fail_on["call"]:
                    raise RuntimeError("simulated FOD blip")
                # Player->issuer means food leaving the pack (negative).
                signed = -int(value) if tx.account == "rPlayerAddr" else int(value)
                food_moves.append(signed)
            return _FakeResp({"hash": next(hashes)})

        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit)

        def _recon():
            ledger_balances = {
                XRPL_TOKEN_MAP[k][0]: state.backpack.last_settled_supplies.get(k, 0)
                for k in XRPL_RESOURCES
            }
            return reconcile(
                run_id=state.run_id, seed=state.seed, minted_initial=minted,
                ledger_balances=ledger_balances,
                last_settled_supplies=state.backpack.last_settled_supplies,
                settlements=state.backpack.settlements,
                pending=state.backpack.pending_settlements,
            )

        # ── Town A: the only (fresh) FOD payment fails.
        state.supplies.set("food", 40)
        fail_on["call"] = 1
        assert mgr.settle(state, "TownA").success is False
        assert len(state.backpack.pending_settlements) == 1
        assert state.backpack.last_settled_supplies["food"] == 50

        # ── Town B: retry of A's -10 succeeds (2nd FOD call), fresh -5 fails (3rd).
        state.supplies.set("food", 35)
        fail_on["call"] = 3
        assert mgr.settle(state, "TownB").success is False
        # Retry advanced the baseline by the settled -10; the fresh -5 is pending.
        assert state.backpack.last_settled_supplies["food"] == 40
        assert len(state.backpack.pending_settlements) == 1
        settled_food = sum(
            r.deltas.get("food", 0) for r in state.backpack.settlements
        )
        assert settled_food == -10  # only the retried record is settled so far
        assert _recon().passed is False  # a pending settlement remains

        # ── Town C: no consumption; retry settles the -5, fresh delta is 0.
        fail_on["call"] = None
        assert mgr.settle(state, "TownC").success is True
        assert state.backpack.pending_settlements == []
        assert state.backpack.last_settled_supplies["food"] == 35

        # No FOD paid twice; conservation holds across the whole chain.
        assert sum(food_moves) == -15
        report = _recon()
        assert report.passed is True, report.notes
        food = next(r for r in report.resources if r.resource == "food")
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


# ──────────────────────────────────────────────────────────────────────
# Stage-C humanization fixes (ledger-B02..B09).
# ──────────────────────────────────────────────────────────────────────


class TestTestnetOnly:
    """ledger-B03 (SAFETY): the manager only connects to testnet hosts."""

    def test_default_url_is_accepted(self):
        # The default URL is a testnet host — constructs without error.
        mgr = BackpackManager()
        assert mgr is not None

    def test_explicit_testnet_url_accepted(self):
        mgr = BackpackManager("https://s.altnet.rippletest.net:51234/")
        assert mgr is not None

    def test_devnet_url_accepted(self):
        mgr = BackpackManager("https://s.devnet.rippletest.net:51234/")
        assert mgr is not None

    def test_mainnet_url_rejected(self):
        """A mainnet host must be refused — no real value at risk, in code."""
        with pytest.raises(ValueError) as exc:
            BackpackManager("https://s1.ripple.com:51234/")
        assert "testnet" in str(exc.value).lower()

    def test_arbitrary_host_rejected(self):
        with pytest.raises(ValueError):
            BackpackManager("https://evil.example.com:51234/")

    def test_non_testnet_allowed_with_explicit_flag(self):
        """The escape hatch is honored (e.g. a local standalone rippled)."""
        mgr = BackpackManager(
            "http://localhost:5005/", allow_non_testnet=True,
        )
        assert mgr is not None


class TestTimeoutClient:
    """ledger-B02: the XRPL client is bounded by an explicit timeout."""

    @requires_xrpl
    def test_get_client_uses_timeout_subclass(self):
        from escape_the_valley.backpack import (
            XRPL_REQUEST_TIMEOUT,
            _TimeoutJsonRpcClient,
        )

        mgr = BackpackManager()
        client = mgr._get_client()
        assert isinstance(client, _TimeoutJsonRpcClient)
        # The timeout is the ~30s ceiling consistent with the GM, not the
        # xrpl-py default of 10s.
        assert client._timeout == XRPL_REQUEST_TIMEOUT
        assert XRPL_REQUEST_TIMEOUT == 30.0


class TestWritePathDeadline:
    """ledger-B02 WRITE-PATH: a stalled submit_and_wait degrades, never hangs.

    xrpl-py's submit_and_wait runs its own ledger-validation poll loop and
    exposes no timeout param, so a stalled testnet could block it past
    XRPL_REQUEST_TIMEOUT. _submit_and_wait_bounded caps the whole write with a
    wall-clock deadline; on timeout it raises WriteTimeout, which the existing
    except-Exception paths route into the same offline/pending degradation as
    any other submit failure (WriteTimeoutError is a plain Exception). These
    tests mock a submit that blocks until the
    test releases it (so no live thread leaks) and assert the write degrades
    within the (tiny, monkeypatched) deadline rather than freezing.
    """

    @staticmethod
    def _slow_submit(release_evt, *, fire_evt=None):
        """A submit_and_wait stand-in that blocks until ``release_evt`` is set.

        Simulates a stalled node: the call does not return on its own. The test
        sets ``release_evt`` in teardown so the daemon worker thread unwinds
        cleanly instead of leaking. ``fire_evt`` (if given) is set the moment
        the worker actually enters the call, so the test can prove the deadline
        fired against a genuinely in-flight submit.
        """

        def _submit(tx, client, signer):
            if fire_evt is not None:
                fire_evt.set()
            release_evt.wait(timeout=10)  # released by the test; safety cap
            return _FakeResp({"hash": "NEVER"})

        return _submit

    @requires_xrpl
    def test_bounded_wrapper_raises_writetimeout_on_stall(self, monkeypatch):
        """The wrapper itself raises WriteTimeoutError when submit blocks past
        the deadline — and does not block the calling thread."""
        import threading
        import time

        from escape_the_valley.backpack import (
            WriteTimeoutError,
            _submit_and_wait_bounded,
        )

        release = threading.Event()
        fired = threading.Event()
        monkeypatch.setattr(
            backpack_mod, "submit_and_wait",
            self._slow_submit(release, fire_evt=fired),
        )

        start = time.monotonic()
        try:
            with pytest.raises(WriteTimeoutError):
                _submit_and_wait_bounded(
                    object(), _FakeClient(), _FakeWallet("rX"), deadline=0.2,
                )
            elapsed = time.monotonic() - start
            # The caller was released ~at the deadline, not after the 10s stall.
            assert elapsed < 5.0
            assert fired.is_set()  # the slow submit was genuinely in flight
        finally:
            release.set()  # let the daemon worker unwind

    @requires_xrpl
    def test_stalled_settle_degrades_to_pending(self, monkeypatch):
        """A stalled write during settle() queues a pending record (degrade),
        does not hang, and flips the offline signal."""
        import threading

        release = threading.Event()
        monkeypatch.setattr(backpack_mod, "XRPL_WRITE_DEADLINE", 0.2)
        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed",
            staticmethod(lambda *a, **k: _FakeWallet("rPlayerAddr")),
        )
        monkeypatch.setattr(
            backpack_mod, "submit_and_wait", self._slow_submit(release),
        )

        state = _enabled_state()
        state.supplies.set("food", 40)  # -10 → triggers a write that stalls
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        try:
            res = mgr.settle(state, "StalledTown")
            assert res.success is False
            assert len(state.backpack.pending_settlements) == 1
            assert state.backpack.pending_settlements[0].status == "pending"
            assert state.backpack.last_settle_failed is True
            # Baseline NOT advanced — nothing reached the ledger.
            assert state.backpack.last_settled_supplies["food"] == 50
        finally:
            release.set()

    @requires_xrpl
    def test_stalled_send_parcel_degrades_supplies_unchanged(self, monkeypatch):
        """A stalled write during send_parcel() leaves supplies untouched and
        records no parcel — degrade, not freeze."""
        import threading

        release = threading.Event()
        monkeypatch.setattr(backpack_mod, "XRPL_WRITE_DEADLINE", 0.2)
        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed",
            staticmethod(lambda *a, **k: _FakeWallet("rPlayerAddr")),
        )
        monkeypatch.setattr(
            backpack_mod, "submit_and_wait", self._slow_submit(release),
        )

        state = _enabled_state()
        before = state.supplies.food
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        try:
            res = mgr.send_parcel(state, "rRecipient", "food", 7)
            assert res.success is False
            assert state.supplies.food == before  # unchanged on stall
            assert state.backpack.sent_parcels == []
        finally:
            release.set()

    @requires_xrpl
    def test_stalled_enable_degrades_to_off(self, monkeypatch):
        """A stalled write during enable() (the TrustSet step) leaves the pack
        OFF instead of hanging the faucet flow."""
        import threading

        release = threading.Event()
        monkeypatch.setattr(backpack_mod, "XRPL_WRITE_DEADLINE", 0.2)

        wallets = iter([
            _FakeWallet("rIssuerAddr", "sIssuerSeed"),
            _FakeWallet("rPlayerAddr", "sPlayerSeed"),
        ])
        monkeypatch.setattr(
            backpack_mod, "generate_faucet_wallet",
            lambda *a, **k: next(wallets),
        )
        monkeypatch.setattr(
            backpack_mod, "submit_and_wait", self._slow_submit(release),
        )

        state = _make_state()
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        try:
            res = mgr.enable(state)
            assert res.success is False
            assert state.backpack.enabled is False
            # Trust lines never completed — a later enable() resumes, not skips.
            assert state.backpack.trust_lines_ready is False
        finally:
            release.set()


class TestLastSettleFailedSignal:
    """ledger-B04 (CONTRACT): settle()/_retry_pending track last_settle_failed."""

    @requires_xrpl
    def test_failed_settle_sets_flag(self, monkeypatch):
        state = _enabled_state()
        state.supplies.set("food", 40)  # -10 → FOD payment fails
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, fail_keys={"FOD"})

        res = mgr.settle(state, "BadTown")
        assert res.success is False
        assert state.backpack.last_settle_failed is True

    @requires_xrpl
    def test_successful_settle_clears_flag(self, monkeypatch):
        state = _enabled_state()
        state.backpack.last_settle_failed = True  # pretend a prior failure
        state.supplies.set("food", 38)  # -12, succeeds
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, submit_hashes=["TXF"])

        res = mgr.settle(state, "GoodTown")
        assert res.success is True
        assert state.backpack.last_settle_failed is False

    @requires_xrpl
    def test_retry_clears_flag_when_queue_drains(self, monkeypatch):
        state = _enabled_state()
        state.backpack.last_settle_failed = True
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
        assert state.backpack.last_settle_failed is False

    @requires_xrpl
    def test_retry_keeps_flag_when_still_pending(self, monkeypatch):
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
        assert state.backpack.last_settle_failed is True


class TestStatusLineDegraded:
    """ledger-B04: status_line renders a distinct offline state."""

    def test_degraded_line_when_failed_and_pending(self):
        state = _make_state()
        bp = state.backpack
        bp.enabled = True
        bp.last_settle_failed = True
        bp.pending_settlements = [
            SettlementRecord(day=3, location="A", status="pending"),
            SettlementRecord(day=5, location="B", status="pending"),
        ]
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "offline" in line.lower()
        assert "testnet unreachable" in line.lower()
        assert "2 unsettled checkpoints" in line

    def test_singular_unsettled_checkpoint(self):
        state = _make_state()
        bp = state.backpack
        bp.enabled = True
        bp.last_settle_failed = True
        bp.pending_settlements = [
            SettlementRecord(day=3, location="A", status="pending"),
        ]
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "1 unsettled checkpoint" in line
        assert "checkpoints" not in line  # singular

    def test_pending_without_failure_uses_plain_count(self):
        """A backlog that did NOT fail (last_settle_failed False) reads plainly,
        not as 'testnet unreachable'."""
        state = _make_state()
        bp = state.backpack
        bp.enabled = True
        bp.last_settle_failed = False
        bp.pending_settlements = [
            SettlementRecord(day=3, location="A", status="pending"),
        ]
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "Unsettled: 1 checkpoint" in line
        assert "offline" not in line.lower()


class TestSetupComplete:
    """ledger-B05: completion is the re-enable gate, not just wallet+secret."""

    def test_complete_when_all_set(self):
        state = _enabled_state()  # trust_lines_ready + last_settled_supplies set
        assert _setup_complete(state.backpack) is True

    def test_incomplete_without_trust_lines(self):
        state = _enabled_state()
        state.backpack.trust_lines_ready = False
        assert _setup_complete(state.backpack) is False

    def test_incomplete_without_minted_snapshot(self):
        state = _enabled_state()
        state.backpack.last_settled_supplies = {}
        assert _setup_complete(state.backpack) is False


class TestEnableResume:
    """ledger-B05: a half-built pack resumes instead of declaring 'back online'."""

    @requires_xrpl
    def test_half_built_no_trust_lines_resumes_without_refaucet(self, monkeypatch):
        """Wallets exist but trust lines never finished: enable() must NOT call
        the faucet again and must NOT short-circuit to 'back online'."""
        state = _make_state()
        bp = state.backpack
        # Simulate a prior enable() that created wallets but died before trust.
        bp.wallet_address = "rPlayerAddr"
        bp.wallet_secret = "sPlayerSeed"
        bp.issuer_address = "rIssuerAddr"
        bp.issuer_secret = "sIssuerSeed"
        bp.trust_lines_ready = False
        bp.last_settled_supplies = {}

        faucet_calls = {"n": 0}

        def fake_faucet(*a, **k):
            faucet_calls["n"] += 1
            return _FakeWallet("rUnexpected", "sUnexpected")

        monkeypatch.setattr(backpack_mod, "generate_faucet_wallet", fake_faucet)
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch)

        res = mgr.enable(state)

        assert res.success is True
        assert faucet_calls["n"] == 0  # reused existing wallets, no new faucet
        # Resume finished the missing steps.
        assert bp.trust_lines_ready is True
        assert bp.last_settled_supplies  # minted snapshot now populated
        assert bp.enabled is True
        # Address unchanged — old wallet not orphaned.
        assert bp.wallet_address == "rPlayerAddr"
        assert "resumed" in res.message.lower()

    @requires_xrpl
    def test_complete_pack_flips_back_online_without_remint(self, monkeypatch):
        """A fully-built pack short-circuits: no faucet, no mint, no trust set."""
        state = _enabled_state()
        state.backpack.enabled = False  # was disabled

        faucet_calls = {"n": 0}
        submit_calls = {"n": 0}

        monkeypatch.setattr(
            backpack_mod, "generate_faucet_wallet",
            lambda *a, **k: faucet_calls.__setitem__("n", faucet_calls["n"] + 1),
        )

        def fake_submit(*a, **k):
            submit_calls["n"] += 1
            return _FakeResp({"hash": "X"})

        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit)
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        res = mgr.enable(state)
        assert res.success is True
        assert state.backpack.enabled is True
        assert faucet_calls["n"] == 0
        assert submit_calls["n"] == 0  # no re-mint, no re-trust
        assert "back online" in res.message.lower()


class TestWalletInfoBalancesError:
    """ledger-B08: distinguish 'couldn't reach the ledger' from an empty wallet."""

    @requires_xrpl
    def test_balances_error_set_on_exception(self, monkeypatch):
        state = _enabled_state()

        class _BoomClient(_FakeClient):
            def request(self, _req):
                raise RuntimeError("ledger unreachable")

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _BoomClient())

        info = mgr.wallet_info(state)
        assert info["balances"] == {}
        assert info["balances_error"] is True

    @requires_xrpl
    def test_balances_error_false_on_success(self, monkeypatch):
        state = _enabled_state()

        class _LinesClient(_FakeClient):
            def request(self, _req):
                return _FakeResp({"lines": [
                    {"account": "rIssuerAddr", "currency": "FOD", "balance": "38"},
                ]})

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _LinesClient())

        info = mgr.wallet_info(state)
        assert info["balances_error"] is False
        assert info["balances"] == {"FOD": 38}


class TestParcelCapConstant:
    """ledger-B09: accept_parcel uses the named cap by default."""

    def test_default_cap_is_named_constant(self):
        state = _make_state()
        parcel = ParcelRecord(
            parcel_id="tx:FOD:100", sender="rSender",
            contents={"food": 100}, day_received=3,
        )
        mgr = BackpackManager()
        before = state.supplies.food
        mgr.accept_parcel(parcel, state)  # no explicit cap
        # Applied exactly PARCEL_ACCEPT_CAP, not the raw 100.
        assert state.supplies.food == before + PARCEL_ACCEPT_CAP


class TestMemoSchemaVersion:
    """ledger-B09: the settlement memo carries a schema-version token."""

    def test_memo_text_has_version_suffix(self):
        memo = _settlement_memo_text("run1", 5, {"food": -3, "water": 5})
        assert memo.endswith(f"|V:{MEMO_SCHEMA_VERSION}")

    def test_version_after_run_day_header_preserves_prefix(self):
        """The version is appended, so the TRAIL|RUN|DAY prefix the verifier
        matches on stays intact (ledger-B09 must not break ledger-003)."""
        memo = _settlement_memo_text("run1", 5, {"food": -3})
        assert memo.startswith("TRAIL|RUN:run1|DAY:5")
        # Version comes after DELTA, never before the header.
        assert memo.index("DELTA:") < memo.index("|V:")
