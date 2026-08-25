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

    def test_status_line_on(self, monkeypatch):
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", True)
        state = _make_state()
        state.backpack.enabled = True
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "ON" in line

    def test_status_line_unsettled(self, monkeypatch):
        from escape_the_valley.backpack_models import SettlementRecord

        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", True)
        state = _make_state()
        state.backpack.enabled = True
        state.backpack.pending_settlements = [
            SettlementRecord(day=3, location="Test", status="pending"),
        ]
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "Unsettled: 1 checkpoint" in line
        assert "checkpoints" not in line  # singular

    def test_status_line_unsettled_plural(self, monkeypatch):
        from escape_the_valley.backpack_models import SettlementRecord

        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", True)
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

    def test_accept_negative_content_floored_not_applied(self):
        """ledger-CRIT-1 receive-path mirror of test_send_negative_amount.

        _decode_parcel_memo now rejects a non-positive amount before a
        ParcelRecord is ever created from the receive path, but contents can
        also be populated directly (tests, saves, a future non-XRPL channel).
        accept_parcel must floor a negative content at 0 independently, so it
        can never reduce supplies -- min(amount, cap) alone only bounds the
        top.
        """
        from escape_the_valley.backpack_models import ParcelRecord

        state = _make_state()
        before = state.supplies.food
        parcel = ParcelRecord(
            parcel_id="test:FOD:-999999",
            sender="rGriefer",
            contents={"food": -999999},
            day_received=3,
        )
        mgr = BackpackManager()
        result = mgr.accept_parcel(parcel, state)
        assert result is True
        assert parcel.accepted is True
        assert state.supplies.food == before  # floored at 0, never subtracted

    def test_accept_unrecognized_key_not_applied(self):
        """F-285e9fa6: an unrecognized resource key must never be credited.

        _decode_parcel_memo already rejects a supply key outside
        XRPL_TOKEN_MAP on the live on-chain path, but that guard lives at
        decode time -- accept_parcel itself had no equivalent check, so a
        ParcelRecord built via any other path (tests, saves, a future
        non-XRPL channel -- the same paths CRIT-1's amount floor exists to
        cover) could inject an arbitrary key as a permanent phantom entry in
        state.supplies that the rest of the game never recognizes.
        """
        from escape_the_valley.backpack_models import ParcelRecord

        state = _make_state()
        parcel = ParcelRecord(
            parcel_id="test:GLD:500",
            sender="rGriefer",
            contents={"gold": 500},
            day_received=3,
        )
        mgr = BackpackManager()
        result = mgr.accept_parcel(parcel, state)
        assert result is True
        assert parcel.accepted is True
        assert "gold" not in state.supplies.items

    def test_accept_mixed_keys_applies_valid_skips_unrecognized(self):
        """F-285e9fa6: a partially-valid parcel must apply only known keys."""
        from escape_the_valley.backpack_models import ParcelRecord

        state = _make_state()
        parcel = ParcelRecord(
            parcel_id="test:MIX",
            sender="rSender",
            contents={"food": 5, "gold": 500},
            day_received=3,
        )
        mgr = BackpackManager()
        result = mgr.accept_parcel(parcel, state)
        assert result is True
        assert state.supplies.food == 55  # 50 + 5, applied normally
        assert "gold" not in state.supplies.items  # unrecognized key skipped


class TestSettleNoXrpl:
    def test_settle_not_enabled(self, monkeypatch):
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", True)
        state = _make_state()
        mgr = BackpackManager()
        result = mgr.settle(state, "TestTown")
        assert result.success is False
        assert "not enabled" in result.message.lower()

    def test_enable_without_xrpl(self):
        """Enable should fail gracefully if xrpl-py not installed."""
        mgr = BackpackManager()
        if not mgr.available:
            state = _make_state()
            result = mgr.enable(state)
            assert result.success is False
            assert "xrpl" in result.message.lower()
            assert 'pip install "escape-the-valley[xrpl]"' in result.message


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

    def test_decode_parcel_memo_negative_amount(self):
        """ledger-CRIT-1: a forged negative amount must never decode."""
        memo_text = "PARCEL|RUN:abc|DAY:5|food:-999999"
        result = _decode_parcel_memo(_hex_encode(memo_text))
        assert result is None

    def test_decode_parcel_memo_zero_amount(self):
        """ledger-CRIT-1: zero is non-positive and must never decode."""
        memo_text = "PARCEL|RUN:abc|DAY:5|food:0"
        result = _decode_parcel_memo(_hex_encode(memo_text))
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
    bp.minted_initial = dict(bp.last_settled_supplies)
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
        # F-a6efdd6c: enable-time mint snapshot frozen independently of
        # later last_settled advances.
        from escape_the_valley.backpack_models import (
            XRPL_RESOURCES,
            minted_snapshot_of,
        )
        snap = minted_snapshot_of(state.backpack)
        assert XRPL_RESOURCES <= snap.keys()
        assert snap["food"] == 50
        assert snap == {
            k: state.backpack.last_settled_supplies[k] for k in XRPL_RESOURCES
        }

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
        # F-a6efdd6c: enable-time mint does not move with settlement.
        assert state.backpack.minted_initial["food"] == 50

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


class TestSettleMultiResourcePartialFailure:
    """ledger-CRIT-2: a settle() batch touching 2+ resources must never
    re-pay a resource that already cleared on-chain earlier in the SAME
    batch, when a LATER resource's Payment then fails. Prior tests only ever
    exercised a single-resource delta per settle() call."""

    @requires_xrpl
    def test_multi_resource_partial_failure_does_not_repay_confirmed(
        self, monkeypatch,
    ):
        from escape_the_valley.backpack_models import (
            XRPL_RESOURCES,
            XRPL_TOKEN_MAP,
        )
        from escape_the_valley.ledger_proof import reconcile

        # XRPL_RESOURCES is a set; its iteration order is not something this
        # test should assume. Compute the ACTUAL order settle() will process
        # these three keys in, and fail whichever is processed LAST -- so the
        # other two are guaranteed to have already cleared before it, no
        # matter what that order turns out to be.
        deltas_map = {"food": -12, "water": -6, "meds": -3}
        order = [k for k in XRPL_RESOURCES if k in deltas_map]
        assert len(order) == 3
        fail_key = order[-1]
        fail_code = XRPL_TOKEN_MAP[fail_key][0]
        confirmed_keys = order[:-1]

        state = _enabled_state()
        minted = dict(state.backpack.last_settled_supplies)
        state.supplies.set("food", 38)   # -12
        state.supplies.set("water", 44)  # -6
        state.supplies.set("meds", 2)    # -3
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        def fake_from_seed(seed, *a, **k):
            return _FakeWallet(
                "rPlayerAddr" if seed == "sPlayerSeed" else "rIssuerAddr",
            )

        submitted: list[str] = []

        def fake_submit(tx, client, signer):
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            submitted.append(code)
            if code == fail_code:
                raise RuntimeError(f"simulated {code} blip")
            return _FakeResp({"hash": f"HASH-{code}"})

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(fake_from_seed),
        )
        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit)

        res = mgr.settle(state, "TownA")
        assert res.success is False

        # The two resources that cleared before the failure are already
        # settled with real txids -- never left in limbo.
        assert len(state.backpack.settlements) == 1
        settled = state.backpack.settlements[0]
        assert settled.deltas == {k: deltas_map[k] for k in confirmed_keys}
        assert len(settled.txids) == 2
        for key in confirmed_keys:
            assert state.backpack.last_settled_supplies[key] == (
                minted[key] + deltas_map[key]
            )

        # Only the failed resource is pending.
        assert len(state.backpack.pending_settlements) == 1
        pending = state.backpack.pending_settlements[0]
        assert pending.deltas == {fail_key: deltas_map[fail_key]}
        assert state.backpack.last_settled_supplies[fail_key] == minted[fail_key]

        # Retry: the failed resource now clears. It must be the ONLY thing
        # resubmitted -- the two confirmed above must never be paid again.
        submitted.clear()

        def fake_submit_retry(tx, client, signer):
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            submitted.append(code)
            return _FakeResp({"hash": f"RETRY-{code}"})

        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit_retry)

        res2 = mgr.settle(state, "TownB")
        assert res2.success is True
        assert submitted == [fail_code]
        assert state.backpack.pending_settlements == []
        assert state.backpack.last_settled_supplies[fail_key] == (
            minted[fail_key] + deltas_map[fail_key]
        )

        # Conservation across the whole chain, for all three resources.
        ledger_balances = {
            XRPL_TOKEN_MAP[k][0]: state.backpack.last_settled_supplies.get(k, 0)
            for k in XRPL_RESOURCES
        }
        report = reconcile(
            run_id=state.run_id, seed=state.seed, minted_initial=minted,
            ledger_balances=ledger_balances,
            last_settled_supplies=state.backpack.last_settled_supplies,
            settlements=state.backpack.settlements,
            pending=state.backpack.pending_settlements,
        )
        assert report.passed is True, report.notes
        for key, delta in deltas_map.items():
            check = next(r for r in report.resources if r.resource == key)
            assert check.sum_deltas == delta
            assert check.minted + check.sum_deltas == check.engine_settled


class TestRetryPendingMultiKeyNarrowing:
    """ledger-CRIT-2 extended to _retry_pending: a single pending record can
    itself hold 2+ resources (settle() enqueues everything from the first
    failing key onward). If an earlier key in that record clears on retry
    but a later one fails again, the cleared key must never be resubmitted
    by a subsequent retry pass."""

    @requires_xrpl
    def test_retry_narrows_multi_key_pending_on_partial_clear(self, monkeypatch):
        state = _enabled_state()
        state.backpack.pending_settlements = [
            SettlementRecord(
                day=4, location="Earlier",
                deltas={"food": -10, "water": -6},
                status="pending",
                memo=_settlement_memo_text(
                    state.run_id, 4, {"food": -10, "water": -6},
                ),
            ),
        ]
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        def fake_from_seed(seed, *a, **k):
            return _FakeWallet(
                "rPlayerAddr" if seed == "sPlayerSeed" else "rIssuerAddr",
            )

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(fake_from_seed),
        )

        submitted: list[str] = []

        def fake_submit_pass1(tx, client, signer):
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            submitted.append(code)
            if code == "WTR":
                raise RuntimeError("simulated WTR blip")
            return _FakeResp({"hash": f"HASH-{code}"})

        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit_pass1)

        mgr._retry_pending(state)

        # Food (inserted first in the dict) cleared and is already settled
        # with a real txid; water is the only thing still pending, in a
        # NARROWED record -- not the original 2-key one.
        assert submitted == ["FOD", "WTR"]
        assert len(state.backpack.settlements) == 1
        settled = state.backpack.settlements[0]
        assert settled.deltas == {"food": -10}
        assert settled.txids == ["HASH-FOD"]
        assert state.backpack.last_settled_supplies["food"] == 40  # 50 - 10

        assert len(state.backpack.pending_settlements) == 1
        pending = state.backpack.pending_settlements[0]
        assert pending.deltas == {"water": -6}
        assert state.backpack.last_settled_supplies["water"] == 50  # unadvanced

        # Second retry: water now clears. FOD must never be resubmitted.
        submitted.clear()

        def fake_submit_pass2(tx, client, signer):
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            submitted.append(code)
            return _FakeResp({"hash": f"HASH2-{code}"})

        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit_pass2)

        mgr._retry_pending(state)

        assert submitted == ["WTR"]
        assert state.backpack.pending_settlements == []
        assert len(state.backpack.settlements) == 2
        assert state.backpack.last_settled_supplies["water"] == 44  # 50 - 6


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

    @requires_xrpl
    def test_send_get_client_error_degrades(self, monkeypatch):
        """F-9517936e sweep: send_parcel already wraps _get_client; lock it."""
        state = _enabled_state()
        before = state.supplies.food
        mgr = BackpackManager()

        def boom_client():
            raise RuntimeError("client boom")

        monkeypatch.setattr(mgr, "_get_client", boom_client)
        _patch_signing(monkeypatch)

        res = mgr.send_parcel(state, "rRecipient", "food", 7)
        assert res.success is False
        assert isinstance(res, backpack_mod.SendResult)
        assert state.supplies.food == before
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


class TestCheckParcelsRejectsForgedAmounts:
    """ledger-CRIT-1: the receive path never turns a non-positive-amount
    memo into an acceptable parcel, end to end through check_parcels()."""

    @requires_xrpl
    def test_negative_amount_memo_never_becomes_a_parcel(self, monkeypatch):
        state = _enabled_state()
        forged_memo = "PARCEL|RUN:abc|DAY:5|food:-999999"

        class _TxClient(_FakeClient):
            def request(self, req):
                return _FakeResp({"transactions": [
                    {
                        "hash": "FORGEDHASH",
                        "meta": {"TransactionResult": "tesSUCCESS"},
                        "tx_json": {
                            "TransactionType": "Payment",
                            "Account": "rGriefer",
                            "Destination": "rPlayerAddr",
                            "Memos": [
                                {"Memo": {"MemoData": _hex_encode(forged_memo)}},
                            ],
                        },
                    },
                ]})

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _TxClient())

        parcels = mgr.check_parcels(state)
        assert parcels == []
        assert state.backpack.parcels == []

    @requires_xrpl
    def test_zero_amount_memo_never_becomes_a_parcel(self, monkeypatch):
        state = _enabled_state()
        forged_memo = "PARCEL|RUN:abc|DAY:5|food:0"

        class _TxClient(_FakeClient):
            def request(self, req):
                return _FakeResp({"transactions": [
                    {
                        "hash": "ZEROHASH",
                        "meta": {"TransactionResult": "tesSUCCESS"},
                        "tx_json": {
                            "TransactionType": "Payment",
                            "Account": "rSomeSender",
                            "Destination": "rPlayerAddr",
                            "Memos": [
                                {"Memo": {"MemoData": _hex_encode(forged_memo)}},
                            ],
                        },
                    },
                ]})

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _TxClient())

        parcels = mgr.check_parcels(state)
        assert parcels == []
        assert state.backpack.parcels == []


class TestCheckParcelsPagination:
    """ledger-CRIT-4 (mirrors fetch_onchain_memos' ledger-A03 fix):
    check_parcels() must not silently drop an incoming parcel that has
    scrolled past page 1."""

    @requires_xrpl
    def test_paginates_via_marker(self, monkeypatch):
        state = _enabled_state()
        parcel_memo_p2 = "PARCEL|RUN:abc|DAY:3|food:6"

        def page(tx_hash: str, memo: str) -> dict:
            return {
                "hash": tx_hash,
                "meta": {"TransactionResult": "tesSUCCESS"},
                "tx_json": {
                    "TransactionType": "Payment",
                    "Account": "rSomeSender",
                    "Destination": "rPlayerAddr",
                    "Memos": [{"Memo": {"MemoData": _hex_encode(memo)}}],
                },
            }

        class _PagedTxClient(_FakeClient):
            def __init__(self):
                self._page = 0

            def request(self, req):
                idx = self._page
                self._page += 1
                if idx == 0:
                    # Page 1: nothing relevant here, but a marker signals
                    # more transactions remain.
                    return _FakeResp({
                        "transactions": [],
                        "marker": {"ledger": 1, "seq": 9},
                    })
                # Page 2: the legitimate parcel, no marker -> done.
                return _FakeResp({
                    "transactions": [page("TX-PAGE2", parcel_memo_p2)],
                })

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _PagedTxClient())

        parcels = mgr.check_parcels(state)
        assert len(parcels) == 1
        assert parcels[0].txid == "TX-PAGE2"
        assert parcels[0].contents == {"food": 6}
        assert len(state.backpack.parcels) == 1

    @requires_xrpl
    def test_does_not_stop_after_first_page_when_marker_present(
        self, monkeypatch,
    ):
        """A parcel present on page 1 AND another on page 2 must both surface
        -- pagination must not stop early just because page 1 had a hit."""
        state = _enabled_state()
        memo_p1 = "PARCEL|RUN:abc|DAY:2|water:4"
        memo_p2 = "PARCEL|RUN:abc|DAY:3|food:6"

        def page(tx_hash: str, memo: str) -> dict:
            return {
                "hash": tx_hash,
                "meta": {"TransactionResult": "tesSUCCESS"},
                "tx_json": {
                    "TransactionType": "Payment",
                    "Account": "rSomeSender",
                    "Destination": "rPlayerAddr",
                    "Memos": [{"Memo": {"MemoData": _hex_encode(memo)}}],
                },
            }

        class _PagedTxClient(_FakeClient):
            def __init__(self):
                self._page = 0

            def request(self, req):
                idx = self._page
                self._page += 1
                if idx == 0:
                    return _FakeResp({
                        "transactions": [page("TX-PAGE1", memo_p1)],
                        "marker": {"ledger": 1, "seq": 9},
                    })
                return _FakeResp({
                    "transactions": [page("TX-PAGE2", memo_p2)],
                })

        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _PagedTxClient())

        parcels = mgr.check_parcels(state)
        txids = {p.txid for p in parcels}
        assert txids == {"TX-PAGE1", "TX-PAGE2"}


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


class TestSettleSetupDegrades:
    """F-86a4c19c: from_seed / _get_client / memo build must degrade to
    SettlementResult(success=False), never raise out of settle()/_retry_pending.
    """

    @requires_xrpl
    def test_settle_empty_secret_degrades_not_crash(self, monkeypatch):
        """Live Wallet.from_seed on a missing sidecar seed is ValueError, not
        an uncaught crash — queue remaining as pending, set last_settle_failed.
        """
        state = _enabled_state()
        state.backpack.wallet_secret = ""
        state.supplies.set("food", 40)  # -10, would have submitted FOD
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        # Intentionally do NOT mock from_seed: the real xrpl Wallet.from_seed
        # raises ValueError('Invalid checksum') on an empty seed.

        res = mgr.settle(state, "Town")
        assert res.success is False
        assert len(state.backpack.pending_settlements) == 1
        pending = state.backpack.pending_settlements[0]
        assert pending.status == "pending"
        assert pending.deltas == {"food": -10}
        assert state.backpack.last_settle_failed is True
        assert state.backpack.last_settled_supplies["food"] == 50
        assert res.record is pending

    @requires_xrpl
    def test_settle_from_seed_boom_degrades(self, monkeypatch):
        state = _enabled_state()
        state.supplies.set("food", 40)
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        def boom(_seed, *a, **k):
            raise ValueError("BoomSeed")

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(boom),
        )

        res = mgr.settle(state, "Town")
        assert res.success is False
        assert len(state.backpack.pending_settlements) == 1
        assert state.backpack.last_settle_failed is True
        assert state.backpack.last_settled_supplies["food"] == 50

    @requires_xrpl
    def test_retry_from_seed_boom_does_not_raise(self, monkeypatch):
        state = _enabled_state()
        state.backpack.pending_settlements = [
            SettlementRecord(
                day=4, location="Earlier", deltas={"food": -5},
                status="pending",
            ),
        ]
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        def boom(_seed, *a, **k):
            raise ValueError("BoomSeed")

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(boom),
        )

        mgr._retry_pending(state)
        assert len(state.backpack.pending_settlements) == 1
        assert state.backpack.pending_settlements[0].deltas == {"food": -5}
        assert state.backpack.last_settle_failed is True
        assert state.backpack.settlements == []

    @requires_xrpl
    def test_settle_get_client_error_degrades(self, monkeypatch):
        state = _enabled_state()
        state.supplies.set("food", 40)
        mgr = BackpackManager()

        def boom_client():
            raise RuntimeError("client boom")

        monkeypatch.setattr(mgr, "_get_client", boom_client)
        _patch_signing(monkeypatch)

        res = mgr.settle(state, "Town")
        assert res.success is False
        assert len(state.backpack.pending_settlements) == 1
        assert state.backpack.last_settle_failed is True

    @requires_xrpl
    def test_settle_memo_build_error_degrades(self, monkeypatch):
        state = _enabled_state()
        state.supplies.set("food", 40)
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch)

        def boom_memo(*a, **k):
            raise KeyError("gold")

        monkeypatch.setattr(backpack_mod, "_build_memo", boom_memo)

        res = mgr.settle(state, "Town")
        assert res.success is False
        assert len(state.backpack.pending_settlements) == 1
        assert state.backpack.last_settle_failed is True
        assert state.backpack.last_settled_supplies["food"] == 50

    @requires_xrpl
    def test_retry_unknown_pending_key_skipped_not_crash(self, monkeypatch):
        """A stale pending record with deltas={'gold': -10} used to KeyError
        in _settlement_memo_text; settle() always retries first, so even a
        no-delta checkpoint crashed. Unknown keys are skipped like
        accept_parcel, and the junk record is dropped.
        """
        state = _enabled_state()
        state.backpack.pending_settlements = [
            SettlementRecord(
                day=4, location="Earlier", deltas={"gold": -10},
                status="pending",
            ),
        ]
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch)

        res = mgr.settle(state, "Town")
        assert res.success is True
        assert res.message == "No changes to settle."
        assert state.backpack.pending_settlements == []
        assert state.backpack.last_settle_failed is False
        assert "gold" not in state.backpack.last_settled_supplies

    @requires_xrpl
    def test_retry_mixed_unknown_key_retries_known_only(self, monkeypatch):
        state = _enabled_state()
        state.backpack.pending_settlements = [
            SettlementRecord(
                day=4, location="Earlier",
                deltas={"gold": -10, "food": -5},
                status="pending",
            ),
        ]
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        calls = _patch_signing(monkeypatch, submit_hashes=["RETRY-FOD"])

        mgr._retry_pending(state)
        assert state.backpack.pending_settlements == []
        assert len(state.backpack.settlements) == 1
        settled = state.backpack.settlements[0]
        assert settled.deltas == {"food": -5}
        assert "gold" not in settled.deltas
        assert calls["submit"] == [("FOD", "RETRY-FOD")]
        assert state.backpack.last_settled_supplies["food"] == 45
        assert state.backpack.last_settle_failed is False


class TestEnableSetupDegrades:
    """F-9517936e: _get_client / from_seed on enable() must return
    EnableResult(success=False), never raise uncaught. Mirrors
    TestSettleSetupDegrades / test_settle_get_client_error_degrades.
    """

    @requires_xrpl
    def test_enable_get_client_error_degrades(self, monkeypatch):
        state = _make_state()
        mgr = BackpackManager()

        def boom_client():
            raise RuntimeError("client boom")

        monkeypatch.setattr(mgr, "_get_client", boom_client)
        _patch_signing(monkeypatch)

        res = mgr.enable(state)
        assert res.success is False
        assert isinstance(res, backpack_mod.EnableResult)
        assert state.backpack.enabled is False
        assert not state.backpack.wallet_address
        assert not state.backpack.issuer_secret
        assert state.backpack.last_settled_supplies == {}
        assert "Couldn't reach the faucet" in res.message

    @requires_xrpl
    def test_enable_resume_from_seed_boom_degrades(self, monkeypatch):
        """Resume path: Wallet.from_seed raising must not escape enable()."""
        state = _make_state()
        bp = state.backpack
        bp.wallet_address = "rPlayerAddr"
        bp.wallet_secret = "sPlayerSeed"
        bp.issuer_address = "rIssuerAddr"
        bp.issuer_secret = "sIssuerSeed"
        bp.trust_lines_ready = False  # incomplete so we don't short-circuit
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        def boom(_seed, *a, **k):
            raise ValueError("BoomSeed")

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(boom),
        )

        res = mgr.enable(state)
        assert res.success is False
        assert isinstance(res, backpack_mod.EnableResult)
        assert state.backpack.enabled is False
        assert "Couldn't reach the faucet" in res.message

    @requires_xrpl
    def test_enable_resume_empty_secret_degrades_not_crash(self, monkeypatch):
        """Live Wallet.from_seed on a missing sidecar seed is ValueError,
        returned as EnableResult, not an uncaught crash.
        """
        state = _make_state()
        bp = state.backpack
        bp.wallet_address = "rPlayerAddr"
        bp.wallet_secret = ""
        bp.issuer_address = "rIssuerAddr"
        bp.issuer_secret = "sIssuerSeed"
        bp.trust_lines_ready = False
        mgr = BackpackManager()
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        # Intentionally do NOT mock from_seed: the real xrpl Wallet.from_seed
        # raises ValueError('Invalid checksum') on an empty seed.

        res = mgr.enable(state)
        assert res.success is False
        assert isinstance(res, backpack_mod.EnableResult)
        assert state.backpack.enabled is False


class TestClientAndFromSeedSweep:
    """F-9517936e: every _get_client() / Wallet.from_seed call in
    backpack.py must sit inside a try that degrades to a result object.
    Leftover sites outside a try must be none.
    """

    def test_every_get_client_and_from_seed_is_inside_try(self):
        import ast
        from pathlib import Path

        src_path = Path(backpack_mod.__file__).resolve()
        tree = ast.parse(src_path.read_text(encoding="utf-8"))
        leftovers: list[tuple[str, str, int]] = []

        class Visitor(ast.NodeVisitor):
            def __init__(self) -> None:
                self.try_depth = 0
                self.fn = "<module>"

            def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
                prev = self.fn
                self.fn = node.name
                self.generic_visit(node)
                self.fn = prev

            def visit_Try(self, node: ast.Try) -> None:
                self.try_depth += 1
                self.generic_visit(node)
                self.try_depth -= 1

            def visit_Call(self, node: ast.Call) -> None:
                name = None
                func = node.func
                if isinstance(func, ast.Attribute) and func.attr in (
                    "_get_client", "from_seed",
                ):
                    name = func.attr
                elif isinstance(func, ast.Name) and func.id in (
                    "_get_client", "from_seed",
                ):
                    name = func.id
                if name is not None and self.try_depth == 0:
                    leftovers.append((self.fn, name, node.lineno))
                self.generic_visit(node)

        Visitor().visit(tree)
        assert leftovers == [], (
            "found leftover _get_client/from_seed call sites outside try: "
            f"{leftovers}"
        )


class TestStatusLineDegraded:
    """ledger-B04: status_line renders a distinct offline state."""

    def test_degraded_line_when_failed_and_pending(self, monkeypatch):
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", True)
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

    def test_singular_unsettled_checkpoint(self, monkeypatch):
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", True)
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

    def test_pending_without_failure_uses_plain_count(self, monkeypatch):
        """A backlog that did NOT fail (last_settle_failed False) reads plainly,
        not as 'testnet unreachable'."""
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", True)
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


class TestEnableMintResume:
    """ledger-CRIT-3: unlike TrustSet (idempotent on XRPL), a mint Payment is
    NOT idempotent. If a mint fails partway through enable(), a resumed
    enable() must not re-submit Payments for resources that already landed.
    """

    @requires_xrpl
    def test_partial_mint_failure_does_not_remint_on_resume(self, monkeypatch):
        state = _make_state()  # food=50, water=50, meds=5, ammo=20, parts=3
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

        def fake_from_seed(seed, *a, **k):
            return _FakeWallet(
                "rPlayerAddr" if seed == "sPlayerSeed" else "rIssuerAddr",
            )

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(fake_from_seed),
        )

        mint_calls: list[str] = []

        def fake_submit_first(tx, client, signer):
            # TrustSet has no `amount` attribute of this shape -> code is
            # None and it is never counted or failed here.
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            if code is not None:
                mint_calls.append(code)
                if code == "AMO":
                    raise RuntimeError("faucet blip on AMO mint")
            return _FakeResp({"hash": f"H{len(mint_calls)}"})

        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit_first)

        res1 = mgr.enable(state)
        assert res1.success is False
        # food/water/meds minted before ammo raised; parts never reached.
        assert mint_calls == ["FOD", "WTR", "MED", "AMO"]
        assert state.backpack.last_settled_supplies == {
            "food": 50, "water": 50, "meds": 5,
        }
        assert not state.backpack.enabled

        # Resume: AMO now succeeds, and PRT still needs minting. FOD/WTR/MED
        # must NOT be re-submitted.
        mint_calls.clear()

        def fake_submit_second(tx, client, signer):
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            if code is not None:
                mint_calls.append(code)
            return _FakeResp({"hash": f"H2-{len(mint_calls)}"})

        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit_second)

        res2 = mgr.enable(state)
        assert res2.success is True
        assert mint_calls == ["AMO", "PRT"]
        assert state.backpack.last_settled_supplies == {
            "food": 50, "water": 50, "meds": 5, "ammo": 20, "parts": 3,
        }
        assert state.backpack.enabled is True
        from escape_the_valley.backpack_models import minted_snapshot_of
        assert minted_snapshot_of(state.backpack) == {
            "food": 50, "water": 50, "meds": 5, "ammo": 20, "parts": 3,
        }

    @requires_xrpl
    def test_setup_complete_false_while_mint_partial(self, monkeypatch):
        """ledger-CRIT-3: _setup_complete must require ALL resources present,
        not merely a non-empty dict, or a half-minted pack would wrongly
        short-circuit to 'back online' on the next enable()."""
        from escape_the_valley.backpack import _setup_complete

        state = _enabled_state()
        # Simulate a partial mint: only 3 of 5 resources landed.
        state.backpack.last_settled_supplies = {
            "food": 50, "water": 50, "meds": 5,
        }
        assert _setup_complete(state.backpack) is False


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


class TestExtraMissingRecovery:
    """F-64e78470: extra gone after an already-enabled save must not
    claim Ledger: ON, and must name the pip extra (testnet, not wallet).
    """

    _PIP = 'pip install "escape-the-valley[xrpl]"'

    def test_status_line_does_not_claim_on(self, monkeypatch):
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", False)
        state = _enabled_state()
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "Ledger: ON" not in line
        assert "ON (Testnet)" not in line
        assert self._PIP in line
        assert "extra missing" in line.lower()
        assert "wallet" not in line.lower()
        assert "mainnet" not in line.lower()

    def test_settle_names_pip_extra_and_does_not_fold(self, monkeypatch):
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", False)
        state = _enabled_state()
        state.supplies.set("food", 40)  # delta vs last_settled 50
        mgr = BackpackManager()
        result = mgr.settle(state, "TestTown")
        assert result.success is False
        assert result.message == backpack_mod.XRPL_EXTRA_MISSING_MSG
        assert self._PIP in result.message
        assert not result.txids
        assert state.backpack.last_settled_supplies["food"] == 50
        assert state.backpack.last_settle_failed is False
        assert state.backpack.pending_settlements == []
        assert state.backpack.enabled is True  # save flag unchanged

    def test_send_parcel_names_pip_extra_supplies_unchanged(self, monkeypatch):
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", False)
        state = _enabled_state()
        before = state.supplies.food
        mgr = BackpackManager()
        result = mgr.send_parcel(state, "rRecipient", "food", 5)
        assert result.success is False
        assert result.message == backpack_mod.XRPL_EXTRA_MISSING_MSG
        assert self._PIP in result.message
        assert state.supplies.food == before

    def test_wallet_info_sets_extra_missing_not_empty_balances(self, monkeypatch):
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", False)
        state = _enabled_state()
        mgr = BackpackManager()
        info = mgr.wallet_info(state)
        assert info.get("balances") == {}
        assert info.get("balances_error") is True
        assert info.get("extra_missing") is True

    def test_wallet_overlay_names_pip_extra_not_ambiguous_unavailable(
        self, monkeypatch,
    ):
        from escape_the_valley.backpack_ui import WalletInfoOverlay

        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", False)
        state = _enabled_state()
        mgr = BackpackManager()
        overlay = WalletInfoOverlay()
        overlay.update_from_info(mgr.wallet_info(state))
        rendered = overlay.visual.plain
        assert "Couldn't reach the ledger" not in rendered
        assert "couldn't reach the ledger" not in rendered
        assert "xrpl extra missing" in rendered
        assert self._PIP in rendered
        assert "Wallet Info" in rendered
        # Ambiguous empty-wallet line is not used on its own.
        assert "Balances: unavailable\n" not in rendered + "\n" or (
            "xrpl extra missing" in rendered
        )

    def test_send_overlay_shows_pip_command(self, monkeypatch):
        from escape_the_valley.backpack_ui import SendParcelOverlay

        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", False)
        state = _enabled_state()
        mgr = BackpackManager()
        result = mgr.send_parcel(state, "rRecipient", "food", 5)
        overlay = SendParcelOverlay()
        overlay.show_failure(result.message)
        rendered = overlay.visual.plain
        assert "Send failed" in rendered
        assert self._PIP in rendered

    def test_enable_still_names_pip_extra_and_stays_off(self, monkeypatch):
        monkeypatch.setattr(backpack_mod, "_HAS_XRPL", False)
        state = _make_state()
        mgr = BackpackManager()
        result = mgr.enable(state)
        assert result.success is False
        assert state.backpack.enabled is False
        assert result.message == backpack_mod.XRPL_EXTRA_MISSING_MSG
        assert self._PIP in result.message
        assert mgr.status_line(state) == "Ledger: OFF"


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


# ──────────────────────────────────────────────────────────────────────
# F-78cd62e7: Static.update() parses markup EAGERLY and SYNCHRONOUSLY
# (textual/widgets/_static.py's update() calls visualize() ->
# Content.from_markup() inside update() itself, before refresh() ever
# runs) -- unlike notify()/Toast, which defer Content.from_markup() to
# their own render() call at paint time. An untrusted string embedded via
# ENABLE_FAILURE_TEXT / SEND_PARCEL_FAILURE_TEXT's {message} placeholder
# that happens to contain an orphan "[/tag]"-shaped substring (routine in
# XRPL/HTTP error text, or an address/amount echoed back from a rejected
# player command) used to raise textual.markup.MarkupError straight out
# of show_failure(), crashing the whole app. These call the REAL
# Static.update() / Content.from_markup() (no mocking of .update() itself
# -- this wave's standing rule), so the fix is proven against the actual
# renderer, not a stand-in for it.
# ──────────────────────────────────────────────────────────────────────


class TestOverlayFailureMarkupSafety:
    """The fix escapes only the DYNAMIC fragment via _escape_dynamic,
    never the surrounding template -- chrome markup like [b]Send failed[/b]
    must keep rendering bold. textual.markup.escape() is not enough: a
    leftover '[' (e.g. truncated 'rSender[...') still opens a tag into the
    chrome. A uniform markup=False flip on the widget would also pass a
    naive "does not crash" check while silently killing that bold heading,
    so each test asserts the heading's bold span survives, not just the
    absence of an exception.

    F-25704f7e / F-86f06d6a / F-5850c719: every overlay sink that splices
    a dynamic fragment into a markup template (show_parcel, show_success,
    show_form, show_failure, update_from_info) uses the same
    Static.update() path and must be covered here, still without mocking
    update/from_markup.
    """

    def _assert_heading_still_bold(self, overlay, heading: str) -> str:
        rendered = overlay.visual.plain
        # Markup was not disabled wholesale: a literal, unparsed "[b]"
        # would only appear in .plain if markup parsing were turned off
        # for the whole widget instead of just escaping the dynamic part.
        assert "[b]" not in rendered
        assert "[/b]" not in rendered
        assert heading in rendered
        start = rendered.index(heading)
        bold_spans = [s for s in overlay.visual.spans if s.style == "b"]
        assert any(
            s.start == start and s.end == start + len(heading)
            for s in bold_spans
        ), bold_spans
        return rendered

    def test_send_parcel_failure_survives_orphan_closing_tag(self):
        from escape_the_valley.backpack_ui import SendParcelOverlay

        overlay = SendParcelOverlay()
        # Shaped exactly like the real trigger: an address a player typed
        # or pasted, echoed back into the failure message by the caller's
        # f-string (tui_app.py's on_input_submitted), containing a
        # bracket-and-slash substring that reads as an orphan closing tag.
        malicious = "'r_looks_ok[/pwn]' is not a valid XRPL address."

        overlay.show_failure(malicious)  # pre-fix: raised MarkupError here

        rendered = overlay.visual.plain
        # The player's own text is preserved verbatim, not swallowed.
        assert malicious in rendered
        # Markup was not disabled wholesale: a literal, unparsed "[b]"
        # would only appear in .plain if markup parsing were turned off
        # for the whole widget instead of just escaping the dynamic part.
        assert "[b]" not in rendered
        assert "[/b]" not in rendered
        # The chrome heading is still real bold markup, not stripped text.
        heading = "Send failed"
        assert heading in rendered
        start = rendered.index(heading)
        bold_spans = [s for s in overlay.visual.spans if s.style == "b"]
        assert any(
            s.start == start and s.end == start + len(heading)
            for s in bold_spans
        ), bold_spans

    def test_enable_flow_failure_survives_orphan_closing_tag(self):
        """EnableFlowOverlay has the identical .update()/.format() pattern.
        It is not reachable with dynamic content today (enable()'s failure
        messages are hardcoded literals), but the sink is the same and the
        director's guidance is to fix it anyway rather than rely on that
        staying true."""
        from escape_the_valley.backpack_ui import EnableFlowOverlay

        overlay = EnableFlowOverlay()
        malicious = "Couldn't reach the faucet: unexpected '[/oops]' reply."

        overlay.show_failure(malicious)  # pre-fix: raised MarkupError here

        rendered = overlay.visual.plain
        assert malicious in rendered
        assert "[b]" not in rendered
        assert "[/b]" not in rendered
        heading = "Couldn't enable right now"
        assert heading in rendered
        start = rendered.index(heading)
        bold_spans = [s for s in overlay.visual.spans if s.style == "b"]
        assert any(
            s.start == start and s.end == start + len(heading)
            for s in bold_spans
        ), bold_spans

    def test_plain_message_unaffected(self):
        """A message with no bracket-shaped substring must render exactly
        as before -- the escape must be a no-op for ordinary text."""
        from escape_the_valley.backpack_ui import SendParcelOverlay

        overlay = SendParcelOverlay()
        overlay.show_failure("Not enough food (have 3, need 10).")
        rendered = overlay.visual.plain
        assert "Not enough food (have 3, need 10)." in rendered

    def test_parcel_survives_orphan_closing_tag_in_sender_and_contents(self):
        from escape_the_valley.backpack_ui import ParcelNotification

        overlay = ParcelNotification()
        # Canonical short form is first-4 + last-4. textual.markup.escape()
        # does not wrap leftover '[' in a truncated sender, so splicing it
        # used to unbalance the chrome [b] tags. Contents still carry the
        # orphan closing tag; sender short form is now rSen...pwn].
        overlay.show_parcel("rSender[/pwn]", "5 food [/pwn]")

        rendered = self._assert_heading_still_bold(overlay, "Parcel arrived!")
        assert "rSen...pwn]" in rendered
        assert "5 food [/pwn]" in rendered

    def test_send_parcel_success_survives_orphan_closing_tag(self):
        from escape_the_valley.backpack_ui import SendParcelOverlay

        overlay = SendParcelOverlay()
        malicious = "Sent 5 food to rN7q[/pwn]. Receipt: ABCDEF123456..."

        overlay.show_success(malicious)

        rendered = self._assert_heading_still_bold(overlay, "Parcel sent!")
        assert malicious in rendered

    def test_send_parcel_form_survives_orphan_closing_tag(self):
        from escape_the_valley.backpack_ui import SendParcelOverlay

        overlay = SendParcelOverlay()
        supplies = "food: 50 [/pwn]"

        overlay.show_form(supplies)

        rendered = self._assert_heading_still_bold(overlay, "Send Parcel")
        assert supplies in rendered

    def test_wallet_info_survives_orphan_closing_tag(self):
        from escape_the_valley.backpack_ui import WalletInfoOverlay

        overlay = WalletInfoOverlay()
        overlay.update_from_info({
            "address_short": "rABC[/pwn]",
            "issuer": "rISS[/pwn]",
            "trust_lines": True,
            "settlements": 3,
            "pending": 0,
            "balances": {"FOOD": "12[/pwn]"},
        })

        rendered = self._assert_heading_still_bold(overlay, "Wallet Info")
        assert "rABC[/pwn]" in rendered
        assert "rISS[/pwn]" in rendered
        assert "FOOD: 12[/pwn]" in rendered
        assert "Settlements: 3" in rendered
        assert "Pending: 0" in rendered

    def test_wallet_info_survives_hostile_settlements_and_pending(self):
        """Production wallet_info() passes ints, but the sink still
        interpolates dict values. A hostile settlements/pending string
        used to raise MarkupError on live Static.update (orphan [/pwn]
        or leftover '[' eating into Press [b]Esc[/b]). Escape the
        fragment; keep heading chrome as a real bold span.
        """
        from escape_the_valley.backpack_ui import WalletInfoOverlay

        overlay = WalletInfoOverlay()
        overlay.update_from_info({
            "address_short": "rABC",
            "issuer": "rISS",
            "trust_lines": True,
            "settlements": "[/pwn]",
            "pending": 0,
            "balances": {},
        })
        rendered = self._assert_heading_still_bold(overlay, "Wallet Info")
        assert "Settlements: [/pwn]" in rendered
        assert "Pending: 0" in rendered

        overlay.update_from_info({
            "address_short": "rABC",
            "issuer": "rISS",
            "trust_lines": True,
            "settlements": 0,
            "pending": "foo [ bar",
            "balances": {},
        })
        rendered = self._assert_heading_still_bold(overlay, "Wallet Info")
        assert "Settlements: 0" in rendered
        assert "Pending: foo [ bar" in rendered

        overlay.update_from_info({
            "address_short": "rSender[...",
            "issuer": "rISS",
            "trust_lines": True,
            "settlements": 3,
            "pending": 0,
            "balances": {},
        })
        rendered = self._assert_heading_still_bold(overlay, "Wallet Info")
        assert "rSender[..." in rendered
        assert "Settlements: 3" in rendered
        assert "Pending: 0" in rendered

    def test_proof_overlay_survives_orphan_closing_tag(self):
        from escape_the_valley.backpack_ui import ProofOverlay

        overlay = ProofOverlay()
        overlay.update_from_proof({
            "verdict": "FAIL",
            "run_id": "run[/pwn]",
            "settlements": "[/pwn]",
            "pending": 0,
            "memo": "ok [/pwn]",
            "resources": [{"resource": "food[/pwn]", "ok": False}],
            "notes": ["ledger 1 != engine [/pwn]"],
        })
        rendered = self._assert_heading_still_bold(overlay, "Ledger Proof: FAIL")
        assert "run[/pwn]" in rendered
        assert "food[/pwn]" in rendered

    def test_production_shaped_parcel_and_success_unaffected(self):
        """Classic r-address + catalog labels must still render as before."""
        from escape_the_valley.backpack_ui import (
            EnableFlowOverlay,
            ParcelNotification,
            SendParcelOverlay,
        )

        parcel = ParcelNotification()
        parcel.show_parcel("rN7qKvMzTdmhcjbw1234567890xKp", "5 food")
        rendered = self._assert_heading_still_bold(parcel, "Parcel arrived!")
        assert "rN7q...0xKp" in rendered
        assert "5 food" in rendered

        overlay = SendParcelOverlay()
        success = "Sent 5 food to rN7q...xKp. Receipt: ABCDEF123456..."
        overlay.show_success(success)
        rendered = self._assert_heading_still_bold(overlay, "Parcel sent!")
        assert success in rendered

        enable = EnableFlowOverlay()
        enable.show_success("rN7qKvMzTdmhcjbw1234567890xKp")
        rendered = self._assert_heading_still_bold(enable, "Ledger Backpack: Enabled")
        assert "rN7qKvMzTdmhcjbw1234567890xKp" in rendered

    def test_leftover_open_bracket_raises_under_escape_not_escape_dynamic(self):
        """Truncating a tag-shaped sender to 'rSender[...' leaves a raw '['.
        textual.markup.escape() does not wrap that leftover bracket, so
        splicing it into a chrome template raises MarkupError on the live
        renderer. _escape_dynamic does not.
        """
        from textual.markup import MarkupError, escape

        from escape_the_valley.backpack_ui import (
            ENABLE_FAILURE_TEXT,
            ENABLE_SUCCESS_TEXT,
            EnableFlowOverlay,
            _escape_dynamic,
        )

        leftover = "rSender[..."
        overlay = EnableFlowOverlay()
        # Failure now puts Esc above {message}, so a leftover '[' at the
        # end of the template does not unbalance later chrome. Success
        # still splices {address} before Press [b]Esc[/b] — that is the
        # live-renderer proof that escape() is not enough.
        with pytest.raises(MarkupError):
            overlay.update(ENABLE_SUCCESS_TEXT.format(address=escape(leftover)))

        overlay.update(ENABLE_FAILURE_TEXT.format(message=_escape_dynamic(leftover)))
        rendered = self._assert_heading_still_bold(
            overlay, "Couldn't enable right now",
        )
        assert leftover in rendered

    def test_send_parcel_failure_survives_leftover_open_bracket(self):
        from escape_the_valley.backpack_ui import SendParcelOverlay

        overlay = SendParcelOverlay()
        for payload in ("failed: rSender[...", "foo [ bar", "[/pwn]"):
            overlay.show_failure(payload)
            rendered = self._assert_heading_still_bold(overlay, "Send failed")
            assert payload in rendered

    def test_enable_flow_failure_survives_leftover_open_bracket(self):
        from escape_the_valley.backpack_ui import EnableFlowOverlay

        overlay = EnableFlowOverlay()
        for payload in ("failed: rSender[...", "foo [ bar", "[/pwn]"):
            overlay.show_failure(payload)
            rendered = self._assert_heading_still_bold(
                overlay, "Couldn't enable right now",
            )
            assert payload in rendered

    def test_enable_flow_success_survives_orphan_closing_tag(self):
        from escape_the_valley.backpack_ui import EnableFlowOverlay

        overlay = EnableFlowOverlay()
        overlay.show_success("[/pwn]")

        rendered = self._assert_heading_still_bold(
            overlay, "Ledger Backpack: Enabled",
        )
        assert "[/pwn]" in rendered

    def test_enable_flow_success_survives_leftover_open_bracket(self):
        """A leftover '[' in the full address spliced before Press [b]Esc[/b]
        must not unbalance chrome markup.
        """
        from escape_the_valley.backpack_ui import EnableFlowOverlay

        overlay = EnableFlowOverlay()

        overlay.show_success("r[/p]XXXXXXXXXX")
        rendered = self._assert_heading_still_bold(
            overlay, "Ledger Backpack: Enabled",
        )
        assert "r[/p]XXXXXXXXXX" in rendered

        overlay.show_success("rSender[...")
        rendered = self._assert_heading_still_bold(
            overlay, "Ledger Backpack: Enabled",
        )
        assert "rSender[..." in rendered


# ──────────────────────────────────────────────────────────────────────
# F-83d0832c / F-b7eeb393 / F-a95177ae: identity overlays must show a
# usable classic r-address, a unique From stem, and 4-char token labels.
# Live renderer — no mock of Static.update / Content.from_markup.
# Visual proof at 80x24 and 120x30 (50% overlay width, matching tui.tcss).
# ──────────────────────────────────────────────────────────────────────

_CLASSIC_R = "rPT1Sjq2YGrBMTttX4gzHjKu9dyFZYYXrg"  # 34-char production r-address
_CLASSIC_R_SHORT = "rPT1...YXrg"


class TestOverlayIdentityReadability:
    """Full address + unique From + FOOD labels on the live renderer."""

    def _assert_heading_still_bold(self, overlay, heading: str) -> str:
        rendered = overlay.visual.plain
        assert "[b]" not in rendered
        assert "[/b]" not in rendered
        assert heading in rendered
        start = rendered.index(heading)
        bold_spans = [s for s in overlay.visual.spans if s.style == "b"]
        assert any(
            s.start == start and s.end == start + len(heading)
            for s in bold_spans
        ), bold_spans
        return rendered

    def test_wallet_and_enable_show_full_classic_address(self):
        from escape_the_valley.backpack_ui import (
            EnableFlowOverlay,
            WalletInfoOverlay,
        )

        wallet = WalletInfoOverlay()
        wallet.update_from_info({
            "address": _CLASSIC_R,
            "address_short": _CLASSIC_R_SHORT,
            "issuer": "rIss...XXYY",
            "trust_lines": True,
            "settlements": 1,
            "pending": 0,
            "balances": {},
        })
        rendered = self._assert_heading_still_bold(wallet, "Wallet Info")
        assert _CLASSIC_R in rendered
        assert _CLASSIC_R_SHORT in rendered
        assert f"Address: {_CLASSIC_R_SHORT}" in rendered

        enable = EnableFlowOverlay()
        enable.show_success(_CLASSIC_R)
        rendered = self._assert_heading_still_bold(
            enable, "Ledger Backpack: Enabled",
        )
        assert _CLASSIC_R in rendered
        assert f"Wallet: {_CLASSIC_R}" in rendered

    def test_wallet_balances_use_four_char_display_labels(self):
        from escape_the_valley.backpack_ui import WalletInfoOverlay

        overlay = WalletInfoOverlay()
        overlay.update_from_info({
            "address": _CLASSIC_R,
            "address_short": _CLASSIC_R_SHORT,
            "issuer": "rIss...XXYY",
            "trust_lines": True,
            "settlements": 0,
            "pending": 0,
            "balances": {
                "FOD": 40, "WTR": 50, "MED": 3, "AMO": 10, "PRT": 2,
            },
        })
        rendered = self._assert_heading_still_bold(overlay, "Wallet Info")
        assert "FOOD (FOD): 40" in rendered
        assert "WATR (WTR): 50" in rendered
        assert "MEDS (MED): 3" in rendered
        assert "AMMO (AMO): 10" in rendered
        assert "PART (PRT): 2" in rendered
        assert "FOD: 40" not in rendered

    def test_parcel_from_distinguishes_prefix8_colliding_senders(self):
        from escape_the_valley.backpack_ui import (
            ParcelNotification,
            WalletInfoOverlay,
        )

        a = "rN7qKvMzAAAAAAAAAAAAAAAAaaaa"
        b = "rN7qKvMzBBBBBBBBBBBBBBBBbbbb"
        parcel_a = ParcelNotification()
        parcel_a.show_parcel(a, "5 food")
        parcel_b = ParcelNotification()
        parcel_b.show_parcel(b, "5 food")
        from_a = self._assert_heading_still_bold(parcel_a, "Parcel arrived!")
        from_b = self._assert_heading_still_bold(parcel_b, "Parcel arrived!")
        assert "From: rN7q...aaaa" in from_a
        assert "From: rN7q...bbbb" in from_b
        assert "From: rN7q...aaaa" not in from_b
        assert "From: rN7qKvMz..." not in from_a
        assert "From: rN7qKvMz..." not in from_b

        parcel = ParcelNotification()
        parcel.show_parcel(_CLASSIC_R, "5 food")
        rendered = self._assert_heading_still_bold(parcel, "Parcel arrived!")
        assert f"From: {_CLASSIC_R_SHORT}" in rendered

        wallet = WalletInfoOverlay()
        wallet.update_from_info({
            "address": _CLASSIC_R,
            "address_short": _CLASSIC_R_SHORT,
            "issuer": "rIss...XXYY",
            "trust_lines": True,
            "settlements": 0,
            "pending": 0,
            "balances": {},
        })
        wallet_text = self._assert_heading_still_bold(wallet, "Wallet Info")
        assert _CLASSIC_R_SHORT in wallet_text
        assert _CLASSIC_R_SHORT in rendered


class TestOverlayIdentityVisualSizes:
    """Coordinator: a layout that only works maximized is a failed fix."""

    _SIZES = ((80, 24), (120, 30))
    # CSS matches tui.tcss overlay rules (without display: none so Pilot
    # paints them). visual.plain is not proof — assert render_line strips.
    _OVERLAY_CSS = """
Screen {
  background: #0b0f14;
  color: #e7ecef;
}
#wallet_info {
  width: 50%;
  height: 60%;
  margin: 2 0 0 0;
  padding: 1 2;
  border: round #3a4b60;
  background: #0f1620;
  overflow-y: auto;
}
#enable_flow {
  width: 50%;
  height: auto;
  max-height: 50%;
  margin: 2 0 0 0;
  padding: 1 2;
  border: round #3a6040;
  background: #0f1620;
  overflow-y: auto;
}
#parcel_notify {
  width: 50%;
  height: auto;
  max-height: 40%;
  margin: 2 0 0 0;
  padding: 1 2;
  border: round #604030;
  background: #0f1620;
  overflow-y: auto;
}
#ledger_menu {
  width: 50%;
  height: 60%;
  margin: 2 0 0 0;
  padding: 1 2;
  border: round #3a6040;
  background: #0f1620;
  overflow-y: auto;
}
#nudge {
  width: 50%;
  height: auto;
  max-height: 40%;
  margin: 2 0 0 0;
  padding: 1 2;
  border: round #605a30;
  background: #0f1620;
  overflow-y: auto;
}
#learn_more {
  width: 60%;
  height: 70%;
  margin: 2 0 0 0;
  padding: 1 2;
  border: round #3a4b60;
  background: #0f1620;
  overflow-y: auto;
}
#send_parcel {
  width: 50%;
  height: auto;
  max-height: 60%;
  margin: 2 0 0 0;
  padding: 1 2;
  border: round #3a6040;
  background: #0f1620;
  overflow-y: auto;
}
#ledger_proof {
  width: 50%;
  height: 60%;
  margin: 2 0 0 0;
  padding: 1 2;
  border: round #3a4b60;
  background: #0f1620;
  overflow-y: auto;
}
"""

    def _painted(self, widget) -> str:
        # Concatenated render_line strips — the inner painted viewport,
        # not visual.plain (which still holds overflow below the fold).
        return "\n".join(
            widget.render_line(y).text for y in range(widget.size.height)
        )

    def _on_screen(self, widget, cols: int, rows: int):
        from textual.geometry import Region

        visible = widget.region.intersection(Region(0, 0, cols, rows))
        assert visible.width > 0 and visible.height > 0, (
            f"overlay region {widget.region} misses screen {cols}x{rows}"
        )
        return visible

    def test_identity_overlays_at_80x24_and_120x30(self):
        import asyncio

        from textual.app import App, ComposeResult

        from escape_the_valley.backpack_ui import (
            EnableFlowOverlay,
            ParcelNotification,
            WalletInfoOverlay,
        )

        css = self._OVERLAY_CSS
        classic = _CLASSIC_R
        short = _CLASSIC_R_SHORT
        painted_fn = self._painted
        on_screen = self._on_screen

        def _recover(widget) -> str:
            return "".join(painted_fn(widget).split())

        class _WalletApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield WalletInfoOverlay(id="wallet_info")

        class _EnableApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield EnableFlowOverlay(id="enable_flow")

        class _ParcelApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield ParcelNotification(id="parcel_notify")

        async def scenario(size: tuple[int, int]) -> None:
            cols, rows = size

            wallet_app = _WalletApp()
            async with wallet_app.run_test(size=size) as pilot:
                wallet = wallet_app.query_one("#wallet_info", WalletInfoOverlay)
                wallet.update_from_info({
                    "address": classic,
                    "address_short": short,
                    "issuer": "rIss...XXYY",
                    "trust_lines": True,
                    "settlements": 1,
                    "pending": 0,
                    "balances": {
                        "FOD": 40, "WTR": 50, "MED": 3, "AMO": 10, "PRT": 2,
                    },
                })
                await pilot.pause()
                assert wallet.size.width <= cols
                assert wallet.size.height <= rows
                on_screen(wallet, cols, rows)
                painted = painted_fn(wallet)
                # visual.plain still contains FOOD when the painted region
                # clips — that is a failed fix. Assert the strips.
                assert "FOOD (FOD)" in painted
                assert "WATR (WTR)" in painted
                assert "MEDS (MED)" in painted
                assert "AMMO (AMO)" in painted
                assert "PART (PRT)" in painted
                assert "Esc" in painted
                assert classic in wallet.visual.plain
                assert "[b]" not in wallet.visual.plain
                assert classic in _recover(wallet)

            enable_app = _EnableApp()
            async with enable_app.run_test(size=size) as pilot:
                enable = enable_app.query_one("#enable_flow", EnableFlowOverlay)
                enable.show_success(classic)
                await pilot.pause()
                assert enable.size.width <= cols
                assert enable.size.height <= rows
                on_screen(enable, cols, rows)
                painted = painted_fn(enable)
                assert "Esc" in painted
                assert classic in enable.visual.plain
                assert classic in _recover(enable)

            parcel_app = _ParcelApp()
            async with parcel_app.run_test(size=size) as pilot:
                parcel = parcel_app.query_one("#parcel_notify", ParcelNotification)
                parcel.show_parcel(classic, "5 food")
                await pilot.pause()
                assert parcel.size.width <= cols
                assert parcel.size.height <= rows
                on_screen(parcel, cols, rows)
                painted = painted_fn(parcel)
                assert "A) Accept" in painted
                assert "R) Refuse" in painted
                assert f"From: {short}" in parcel.visual.plain
                assert short in _recover(parcel)

        for size in self._SIZES:
            asyncio.run(scenario(size))

    def test_enable_failure_paints_esc_for_production_copy(self):
        """F-766d7cf5: faucet / extra-missing show_failure must paint Esc
        in render_line at 80x24, not only in visual.plain.
        """
        import asyncio

        from textual.app import App, ComposeResult

        from escape_the_valley.backpack_models import XRPL_EXTRA_MISSING_MSG
        from escape_the_valley.backpack_ui import EnableFlowOverlay

        css = self._OVERLAY_CSS
        painted_fn = self._painted
        on_screen = self._on_screen
        # Production copy from backpack.enable except-path (not a test stub).
        faucet = (
            "Couldn't reach the faucet right now. "
            "Ledger Backpack stays OFF. "
            "You can try again at the next town."
        )

        class _EnableApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield EnableFlowOverlay(id="enable_flow")

        async def scenario(size: tuple[int, int], message: str) -> None:
            cols, rows = size
            app = _EnableApp()
            async with app.run_test(size=size) as pilot:
                enable = app.query_one("#enable_flow", EnableFlowOverlay)
                enable.show_failure(message)
                await pilot.pause()
                assert enable.size.width <= cols
                assert enable.size.height <= rows
                on_screen(enable, cols, rows)
                painted = painted_fn(enable)
                assert "Esc" in painted, (
                    f"Esc missing from render_line at {size}; "
                    f"plain still has it={('Esc' in enable.visual.plain)!r}; "
                    f"painted={painted!r}"
                )

        for size in self._SIZES:
            asyncio.run(scenario(size, faucet))
            asyncio.run(scenario(size, XRPL_EXTRA_MISSING_MSG))

    def test_menu_nudge_learn_paints_offered_keys(self):
        """F-8b6e5842: menu / nudge / learn action chrome must paint at
        80x24 via render_line, not only visual.plain.
        """
        import asyncio

        from textual.app import App, ComposeResult

        from escape_the_valley.backpack_ui import (
            LearnMoreOverlay,
            LedgerMenuOverlay,
            NudgeOverlay,
        )

        css = self._OVERLAY_CSS
        painted_fn = self._painted
        on_screen = self._on_screen

        class _MenuApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield LedgerMenuOverlay(id="ledger_menu")

        class _NudgeApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield NudgeOverlay(id="nudge")

        class _LearnApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield LearnMoreOverlay(id="learn_more")

        async def scenario(size: tuple[int, int]) -> None:
            cols, rows = size

            menu_app = _MenuApp()
            async with menu_app.run_test(size=size) as pilot:
                menu = menu_app.query_one("#ledger_menu", LedgerMenuOverlay)
                menu.update_from_state(False)
                await pilot.pause()
                on_screen(menu, cols, rows)
                painted = painted_fn(menu)
                assert "E) Enable" in painted, painted
                assert "L) Learn" in painted, painted
                assert "Esc" in painted, painted

                menu.update_from_state(True)
                await pilot.pause()
                on_screen(menu, cols, rows)
                painted = painted_fn(menu)
                assert "W) Wallet" in painted, painted
                assert "R) Proof" in painted, painted
                assert "P) Send parcel" in painted, painted
                assert "S) Settle" in painted, painted
                assert "D) Disable" in painted, painted
                assert "Esc" in painted, painted

            nudge_app = _NudgeApp()
            async with nudge_app.run_test(size=size) as pilot:
                nudge = nudge_app.query_one("#nudge", NudgeOverlay)
                await pilot.pause()
                on_screen(nudge, cols, rows)
                painted = painted_fn(nudge)
                assert "E) Enable now" in painted, painted
                assert "N) Not now" in painted, painted
                assert "L) Learn" in painted, painted

            learn_app = _LearnApp()
            async with learn_app.run_test(size=size) as pilot:
                learn = learn_app.query_one("#learn_more", LearnMoreOverlay)
                await pilot.pause()
                on_screen(learn, cols, rows)
                painted = painted_fn(learn)
                assert "FOOD (FOD)" in painted, painted
                assert "Esc" in painted, painted

        for size in self._SIZES:
            asyncio.run(scenario(size))

    def test_class_sweep_remaining_overlays_paint_dismiss_row(self):
        """Every backpack_ui overlay paints its dismiss/action row at 80x24
        (render_line / region ∩ screen). visual.plain is not proof.
        """
        import asyncio

        from textual.app import App, ComposeResult

        from escape_the_valley.backpack_models import XRPL_EXTRA_MISSING_MSG
        from escape_the_valley.backpack_ui import (
            EnableFlowOverlay,
            ProofOverlay,
            SendParcelOverlay,
        )

        css = self._OVERLAY_CSS
        painted_fn = self._painted
        on_screen = self._on_screen
        quiet_ledger = (
            "The ledger is quiet. Couldn't send the parcel right now. "
            "Your supplies are unchanged."
        )
        invalid_address = (
            "'rPT1Sjq2YGrBMTttX4gzHjKu9dyFZYYXrg' is not a valid XRPL "
            "classic address (starts with 'r', 25-35 base58 chars)."
        )
        supplies = (
            "  FOOD: 50\n  WATR: 50\n  MEDS: 5\n  AMMO: 20\n  PART: 3"
        )

        class _EnableApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield EnableFlowOverlay(id="enable_flow")

        class _SendApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield SendParcelOverlay(id="send_parcel")

        class _ProofApp(App):
            CSS = css

            def compose(self) -> ComposeResult:
                yield ProofOverlay(id="ledger_proof")

        async def scenario(size: tuple[int, int]) -> None:
            cols, rows = size

            enable_app = _EnableApp()
            async with enable_app.run_test(size=size) as pilot:
                enable = enable_app.query_one("#enable_flow", EnableFlowOverlay)
                enable.show_progress()
                await pilot.pause()
                on_screen(enable, cols, rows)
                painted = painted_fn(enable)
                assert "Esc" in painted, painted

            send_app = _SendApp()
            async with send_app.run_test(size=size) as pilot:
                send = send_app.query_one("#send_parcel", SendParcelOverlay)
                send.show_failure(XRPL_EXTRA_MISSING_MSG)
                await pilot.pause()
                on_screen(send, cols, rows)
                assert "Esc" in painted_fn(send), painted_fn(send)

                send.show_failure(quiet_ledger)
                await pilot.pause()
                on_screen(send, cols, rows)
                assert "Esc" in painted_fn(send), painted_fn(send)

                send.show_failure(invalid_address)
                await pilot.pause()
                on_screen(send, cols, rows)
                assert "Esc" in painted_fn(send), painted_fn(send)

                send.show_success(
                    "Sent 5 food to rPT1...YXrg. Receipt: ABCDEF123456..."
                )
                await pilot.pause()
                on_screen(send, cols, rows)
                assert "Esc" in painted_fn(send), painted_fn(send)

                send.show_form(supplies)
                await pilot.pause()
                on_screen(send, cols, rows)
                painted = painted_fn(send)
                assert "cancel" in painted, painted

            proof_app = _ProofApp()
            async with proof_app.run_test(size=size) as pilot:
                proof = proof_app.query_one("#ledger_proof", ProofOverlay)
                proof.update_from_proof({
                    "verdict": "PASS",
                    "run_id": "abc123",
                    "settlements": 2,
                    "pending": 0,
                    "memo": "ok",
                    "resources": [
                        {"resource": "food", "ok": True},
                        {"resource": "water", "ok": True},
                        {"resource": "meds", "ok": True},
                        {"resource": "ammo", "ok": True},
                        {"resource": "parts", "ok": True},
                    ],
                    "notes": [],
                })
                await pilot.pause()
                on_screen(proof, cols, rows)
                painted = painted_fn(proof)
                assert "PASS" in painted, painted
                assert "Esc" in painted, painted

                proof.update_from_proof({
                    "verdict": "INCONCLUSIVE",
                    "run_id": "abc123",
                    "settlements": 2,
                    "pending": 1,
                    "memo": "ok",
                    "resources": [],
                    "notes": ["1 settlement(s) still pending"],
                    "summary": "pending checkpoints",
                })
                await pilot.pause()
                on_screen(proof, cols, rows)
                painted = painted_fn(proof)
                assert "INCONCLUSIVE" in painted, painted
                assert "Esc" in painted, painted

        for size in self._SIZES:
            asyncio.run(scenario(size))


# ──────────────────────────────────────────────────────────────────────
# F-d178410b: settle()/_retry_pending() fold confirmed Payments into
# last_settled_supplies/bp.settlements/bp.pending_settlements PURELY in
# memory; every real caller (tui_app.py, cli.py) persists separately,
# strictly AFTER these methods return. A crash in that gap replays an
# already-confirmed Payment on the next run -- a real duplicate on-chain
# settlement local conservation math cannot detect, because the crashed
# session's record was never persisted, so it is never summed either.
# ──────────────────────────────────────────────────────────────────────


class TestSettleCrashWindow:
    """Chosen fix shape: persist per-resource as each Payment confirms
    (a ``persist`` hook on BackpackManager, called from inside settle()/
    _retry_pending() immediately after each resource's fold), rather than
    querying the chain to avoid resubmitting an already-broadcast txid.

    Chosen because the alternative's own source of truth for "did I
    already submit this" is exactly the in-memory SettlementRecord this
    bug loses in the crash -- after a crash, only the chain itself
    survives, so "don't resubmit" would need to re-derive intent from
    AccountTx memo text on EVERY settle() (not just a crash-recovery
    path), adding a network round-trip to the common case and depending
    on fragile memo-matching heuristics (batch membership, day, and
    pagination all have to line up) for correctness. Persisting the fold
    immediately is a same-process, no-I/O-in-between window instead of one
    spanning a network round-trip and a full return to the caller, and its
    correctness is a straightforward "write what you just confirmed,
    right after you confirmed it" rather than a heuristic chain query.

    The default (no hook) is UNCHANGED -- proven by test_backpack.py's
    existing ~774-test baseline still passing byte-for-byte -- so wiring
    persist=save_game at the real call sites (tui_app.py, cli.py,
    step_engine.py, adapter.py) to make this live for real players is a
    caller-side change outside this module's domain, not made here.
    """

    @requires_xrpl
    def test_crash_before_caller_save_replays_confirmed_payment_without_persist_hook(
        self, monkeypatch,
    ):
        """Documents the residual gap when no persist hook is wired -- which
        is every real call site today (BackpackManager() takes no
        arguments in tui_app.py/cli.py/step_engine.py/adapter.py). This
        must keep passing before AND after this fix: it is not what the
        fix closes by itself, only what wiring it in would close. Mirrors
        the finding's own repro: two independent settle() calls against
        the SAME engine truth, standing in for one process crashing before
        its caller could save and a second process reloading the stale
        (pre-settle) save.
        """
        state = _enabled_state()  # last_settled_supplies: food=50, ...
        mgr = BackpackManager()  # no persist hook -- matches every real call site
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, submit_hashes=["BURN1"])

        # "Session 1": consume food 50 -> 40 (delta -10). One real Payment
        # burns 10 FOD, confirmed.
        state.supplies.set("food", 40)
        res1 = mgr.settle(state, "TownA")
        assert res1.success is True
        assert res1.txids == ["BURN1"]
        assert state.backpack.last_settled_supplies["food"] == 40

        # Simulate the crash exactly as the finding did: the caller's own
        # save (tui_app.py's self._save() / cli.py's save_game(state) --
        # always a separate, later step) never ran, so nothing from
        # session 1 reached disk. A second process reloading now would
        # reconstruct a RunState from whatever WAS last durable: the
        # pre-settle baseline, with engine truth unchanged either.
        state2 = _enabled_state()  # last_settled_supplies still food=50
        state2.supplies.set("food", 40)  # SAME engine truth session 1 ended with
        mgr2 = BackpackManager()
        monkeypatch.setattr(mgr2, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, submit_hashes=["BURN2"])

        res2 = mgr2.settle(state2, "TownA-after-crash")

        # The bug: a SECOND real Payment burns another 10 FOD for a delta
        # the engine only ever produced once -- 20 total burned on-chain
        # for one true 10-unit consumption.
        assert res2.success is True
        assert res2.txids == ["BURN2"]
        assert state2.backpack.last_settled_supplies["food"] == 40

    @requires_xrpl
    def test_persist_hook_closes_crash_window_no_replay(self, monkeypatch, tmp_path):
        """With persist=save_game wired (what a real caller must do to get
        this fix live), the SAME crash -- nothing extra saved after
        settle() returns -- no longer loses the fold, because settle()
        itself already wrote it to disk the moment the Payment confirmed.
        Uses the REAL save_game()/load_game() round-trip (not a stand-in
        for it), redirected to tmp_path via the same
        escape_the_valley.save.SAVE_DIR monkeypatch
        test_step_engine.py's test_save_load_preserves_determinism already
        uses, so this test's disk I/O never touches the real working
        directory.
        """
        from escape_the_valley.save import load_game, save_game

        monkeypatch.setattr(
            "escape_the_valley.save.SAVE_DIR", tmp_path / ".trail",
        )

        state = _enabled_state()
        mgr = BackpackManager(persist=save_game)
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, submit_hashes=["BURN1"])

        state.supplies.set("food", 40)  # -10
        res1 = mgr.settle(state, "TownA")
        assert res1.success is True
        assert res1.txids == ["BURN1"]

        # Crash simulated identically to the test above: no explicit save
        # call after settle() returns. The only difference is that
        # settle() itself already persisted via the hook.
        reloaded = load_game()
        assert reloaded is not None
        assert reloaded.backpack.last_settled_supplies["food"] == 40
        assert len(reloaded.backpack.settlements) == 1
        assert reloaded.backpack.settlements[0].deltas == {"food": -10}

        # "Session 2": settle again on the RELOADED state (reconstructed
        # purely from what made it to disk, not the original `state`
        # object) against the SAME unchanged engine truth. Any Payment
        # submission at all would mean the crash window reopened.
        def _must_not_submit(tx, client, signer):
            raise AssertionError(
                "must not submit a Payment -- nothing changed since the "
                "persisted baseline"
            )

        def _fake_from_seed(seed, *a, **k):
            return _FakeWallet(
                "rPlayerAddr" if seed == "sPlayerSeed" else "rIssuerAddr",
            )

        mgr2 = BackpackManager(persist=save_game)
        monkeypatch.setattr(mgr2, "_get_client", lambda: _FakeClient())
        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(_fake_from_seed),
        )
        monkeypatch.setattr(backpack_mod, "submit_and_wait", _must_not_submit)

        reloaded.supplies.set("food", 40)  # unchanged since session 1
        res2 = mgr2.settle(reloaded, "TownA-again")

        assert res2.success is True
        assert res2.message == "No changes to settle."
        assert reloaded.backpack.last_settled_supplies["food"] == 40
        # Still exactly one settlement on record -- no duplicate receipt.
        assert len(reloaded.backpack.settlements) == 1

    @requires_xrpl
    def test_persist_hook_narrows_pending_queue_durably(
        self, monkeypatch, tmp_path,
    ):
        """The SAME crash-window gap exists in _retry_pending()'s queue
        bookkeeping specifically: a multi-key pending record that partially
        clears must have its narrowing (the cleared key dropped) persisted
        immediately, not just the baseline -- otherwise a crash right after
        the partial clear reverts bp.pending_settlements to its wider,
        stale shape and a later retry resubmits the key that already
        cleared. Uses the real save_game()/load_game() round-trip, same
        SAVE_DIR isolation as the test above.
        """
        from escape_the_valley.save import load_game, save_game

        monkeypatch.setattr(
            "escape_the_valley.save.SAVE_DIR", tmp_path / ".trail",
        )

        state = _enabled_state()
        state.backpack.pending_settlements = [
            SettlementRecord(
                day=4, location="Earlier",
                deltas={"food": -10, "water": -6},
                status="pending",
                memo=_settlement_memo_text(
                    state.run_id, 4, {"food": -10, "water": -6},
                ),
            ),
        ]
        mgr = BackpackManager(persist=save_game)
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())

        def _fake_from_seed(seed, *a, **k):
            return _FakeWallet(
                "rPlayerAddr" if seed == "sPlayerSeed" else "rIssuerAddr",
            )

        monkeypatch.setattr(
            backpack_mod.Wallet, "from_seed", staticmethod(_fake_from_seed),
        )

        def fake_submit_pass1(tx, client, signer):
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            if code == "WTR":
                raise RuntimeError("simulated WTR blip")
            return _FakeResp({"hash": f"HASH-{code}"})

        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit_pass1)

        mgr._retry_pending(state)  # food clears, water fails and stays pending

        # Crash simulated: nothing else saves after this. Reload from disk.
        reloaded = load_game()
        assert reloaded is not None
        assert reloaded.backpack.last_settled_supplies["food"] == 40
        assert len(reloaded.backpack.pending_settlements) == 1
        # The persisted queue is already NARROWED to just water -- not the
        # original 2-key record -- so food can never be replayed.
        assert reloaded.backpack.pending_settlements[0].deltas == {"water": -6}

        # A second retry pass, on the reloaded state, must submit ONLY
        # water. Resubmitting food would mean the narrowing was lost.
        submitted: list[str] = []

        def fake_submit_pass2(tx, client, signer):
            amount = getattr(tx, "amount", None)
            code = getattr(amount, "currency", None)
            submitted.append(code)
            return _FakeResp({"hash": f"HASH2-{code}"})

        mgr2 = BackpackManager(persist=save_game)
        monkeypatch.setattr(mgr2, "_get_client", lambda: _FakeClient())
        monkeypatch.setattr(backpack_mod, "submit_and_wait", fake_submit_pass2)

        mgr2._retry_pending(reloaded)

        assert submitted == ["WTR"]
        assert reloaded.backpack.pending_settlements == []


# ──────────────────────────────────────────────────────────────────────
# F-9d7eb977: the per-key ``self._persist_state(state)`` call inside BOTH
# settle()'s and _retry_pending()'s inner per-key loop used to fire BEFORE
# the bookkeeping that makes that snapshot internally consistent -- the
# settlement receipt (bp.settlements.append) and the pending-queue
# narrowing/removal both used to happen only once, in a single pass AFTER
# the whole inner loop finished, not atomically with the key that
# triggered them. A crash between a per-key persist call and that later
# bookkeeping durably persists a key that is simultaneously "already
# folded into the baseline" and "still owed" (per the pending queue) or
# "unreceipted" (per bp.settlements) -- the next retry/settle resubmits a
# real duplicate on-chain Payment. TestSettleCrashWindow above proves the
# END-TO-END, post-return behavior is correct; these tests use the
# finding's own repro method -- a snapshotting persist hook that records
# state at EVERY call, not just the last one -- because the bug is
# invisible to any assertion that only looks at the method's final
# return value.
# ──────────────────────────────────────────────────────────────────────


def _snapshotting_persist(snapshots: list[dict]):
    """Build a ``persist`` callable that appends a deep-ish copy of every
    ledger-relevant field to ``snapshots`` on each call, so a test can
    inspect exactly what would have hit disk at EACH crash point during
    settle()/_retry_pending(), not only after the method returns.
    """

    def _persist(state) -> None:
        bp = state.backpack
        snapshots.append({
            "last_settled_supplies": dict(bp.last_settled_supplies),
            "settlement_deltas": [dict(r.deltas) for r in bp.settlements],
            "pending_deltas": [dict(r.deltas) for r in bp.pending_settlements],
        })

    return _persist


def _assert_every_snapshot_self_consistent(
    snapshots: list[dict], pre_baseline: dict[str, int],
):
    """F-9d7eb977's acceptance bar, checked against EVERY persisted
    snapshot (not just the final one): a key that has advanced past its
    pre-call baseline in ``last_settled_supplies`` must, in that SAME
    snapshot, (a) already have a matching settlement record, and (b)
    never still appear in any pending record's deltas. Either violation
    means a crash immediately after that snapshot leaves the key durably
    in a state a later retry pass cannot tell apart from "never
    attempted" -- a genuine duplicate on-chain settlement.
    """
    for i, snap in enumerate(snapshots):
        advanced = {
            key for key, baseline in pre_baseline.items()
            if snap["last_settled_supplies"].get(key, baseline) != baseline
        }
        if not advanced:
            continue

        recorded: set[str] = set()
        for deltas in snap["settlement_deltas"]:
            recorded.update(deltas.keys())
        missing_receipt = advanced - recorded
        assert not missing_receipt, (
            f"snapshot #{i}: {sorted(missing_receipt)} advanced in the "
            f"baseline with NO matching settlement record -- "
            f"F-9d7eb977 duplicate-payment window (last_settled_supplies="
            f"{snap['last_settled_supplies']!r}, settlements="
            f"{snap['settlement_deltas']!r})"
        )

        still_pending: set[str] = set()
        for deltas in snap["pending_deltas"]:
            still_pending.update(deltas.keys())
        overlap = advanced & still_pending
        assert not overlap, (
            f"snapshot #{i}: {sorted(overlap)} advanced in the baseline "
            f"while STILL listed as pending -- F-9d7eb977 "
            f"duplicate-payment window (last_settled_supplies="
            f"{snap['last_settled_supplies']!r}, pending="
            f"{snap['pending_deltas']!r})"
        )


_PRE_BASELINE = {"food": 50, "water": 50, "meds": 5, "ammo": 20, "parts": 3}


class TestPersistOrderingAtomicity:
    """Reproduces F-9d7eb977's three repro scenarios with a snapshotting
    persist hook, proving the reorder actually closes the window rather
    than merely rearranging comments. Each test fails against the
    pre-fix ordering (persist called before the receipt/narrowing
    bookkeeping) and passes against the fix (persist called after it).
    """

    @requires_xrpl
    def test_settle_multi_key_batch_never_snapshots_advanced_without_receipt(
        self, monkeypatch,
    ):
        """Repro 3: a fresh 2-key batch where BOTH Payments confirm.
        Pre-fix, the combined SettlementRecord was only built and
        appended to bp.settlements in a single pass AFTER the whole
        per-key loop -- so 2 of the pass's persist calls showed
        last_settled_supplies fully advanced for one or both keys while
        bp.settlements was still completely empty.
        """
        state = _enabled_state()
        state.supplies.set("food", 40)   # -10
        state.supplies.set("water", 44)  # -6

        snapshots: list[dict] = []
        mgr = BackpackManager(persist=_snapshotting_persist(snapshots))
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, submit_hashes=["BURN-A", "BURN-B"])

        res = mgr.settle(state, "TownA")

        assert res.success is True
        # One persist call per confirmed key -- not one extra, batched
        # call after the loop on top of the per-key ones.
        assert len(snapshots) == 2
        _assert_every_snapshot_self_consistent(snapshots, _PRE_BASELINE)

        # End-to-end sanity: the final snapshot is fully resolved.
        final = snapshots[-1]
        assert final["last_settled_supplies"]["food"] == 40
        assert final["last_settled_supplies"]["water"] == 44
        assert final["settlement_deltas"] == [{"food": -10, "water": -6}]

    @requires_xrpl
    def test_retry_pending_single_key_never_snapshots_advanced_while_pending(
        self, monkeypatch,
    ):
        """Repro 1 ("the common case"): a single-key pending record
        {food: -10}. Pre-fix, folding food's confirmation into the
        baseline and persisting happened strictly BEFORE the post-loop
        queue narrowing/removal -- so the FIRST of two persist calls
        showed food already advanced in last_settled_supplies while
        bp.pending_settlements STILL listed the unchanged, un-narrowed
        record for it.
        """
        state = _enabled_state()
        state.backpack.pending_settlements = [
            SettlementRecord(
                day=4, location="Earlier",
                deltas={"food": -10},
                status="pending",
                memo=_settlement_memo_text(state.run_id, 4, {"food": -10}),
            ),
        ]

        snapshots: list[dict] = []
        mgr = BackpackManager(persist=_snapshotting_persist(snapshots))
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(monkeypatch, submit_hashes=["BURN-RETRY-FOOD"])

        mgr._retry_pending(state)

        # One persist call for the fold+narrow+receipt, one more for the
        # now-empty record's removal from the live queue.
        assert len(snapshots) == 2
        _assert_every_snapshot_self_consistent(snapshots, _PRE_BASELINE)

        final = snapshots[-1]
        assert final["last_settled_supplies"]["food"] == 40
        assert final["pending_deltas"] == []
        assert final["settlement_deltas"] == [{"food": -10}]

    @requires_xrpl
    def test_retry_pending_partial_failure_never_snapshots_inconsistent_state(
        self, monkeypatch,
    ):
        """Repro 2: a 2-key pending record where food clears and water
        then fails on the SAME retry pass. Pre-fix, the mid-pass persist
        (right after food confirms) showed food folded into the baseline
        while the record still listed BOTH food and water un-narrowed,
        and bp.settlements was still empty -- a crash there loses both
        the eventual settled_record for food and the narrowing, so the
        next pass would replay food.
        """
        state = _enabled_state()
        state.backpack.pending_settlements = [
            SettlementRecord(
                day=4, location="Earlier",
                deltas={"food": -10, "water": -6},
                status="pending",
                memo=_settlement_memo_text(
                    state.run_id, 4, {"food": -10, "water": -6},
                ),
            ),
        ]

        snapshots: list[dict] = []
        mgr = BackpackManager(persist=_snapshotting_persist(snapshots))
        monkeypatch.setattr(mgr, "_get_client", lambda: _FakeClient())
        _patch_signing(
            monkeypatch, submit_hashes=["BURN-RETRY-FOOD2"], fail_keys={"WTR"},
        )

        mgr._retry_pending(state)

        # One persist for food's fold+narrow+receipt, one more for the
        # post-loop memo refresh on the still-narrowed (water-only) record.
        assert len(snapshots) == 2
        _assert_every_snapshot_self_consistent(snapshots, _PRE_BASELINE)

        final = snapshots[-1]
        assert final["last_settled_supplies"]["food"] == 40
        assert "water" not in final["last_settled_supplies"] or (
            final["last_settled_supplies"]["water"] == 50
        )
        assert final["pending_deltas"] == [{"water": -6}]
        assert final["settlement_deltas"] == [{"food": -10}]
