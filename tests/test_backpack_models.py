"""Tests for backpack_models — pure data models + token map."""

from escape_the_valley.backpack_models import (
    MEMO_SCHEMA_VERSION,
    MINTED_SNAPSHOT_PERMIT_ID,
    PARCEL_ACCEPT_CAP,
    TESTNET_HOSTS,
    TESTNET_URL,
    XRPL_EXTRA_MISSING_MSG,
    XRPL_EXTRA_PIP,
    XRPL_RESOURCES,
    XRPL_TOKEN_MAP,
    BackpackState,
    ParcelRecord,
    PermitRecord,
    SettlementRecord,
    minted_snapshot_of,
    stamp_minted_snapshot,
)


class TestBackpackState:
    def test_defaults(self):
        bp = BackpackState()
        assert bp.enabled is False
        assert bp.wallet_address == ""
        assert bp.wallet_secret == ""
        assert bp.issuer_address == ""
        assert bp.issuer_secret == ""
        assert bp.trust_lines_ready is False
        assert bp.settlements == []
        assert bp.pending_settlements == []
        assert bp.parcels == []
        assert bp.permits == []
        assert bp.nudge_shown is False
        assert bp.nudge_dismissed is False
        # ledger-B04 (CONTRACT): degraded-network signal defaults to False.
        assert bp.last_settle_failed is False
        # F-a6efdd6c: enable-time mint snapshot starts empty.
        assert bp.minted_initial == {}

    def test_settlement_record_defaults(self):
        rec = SettlementRecord()
        assert rec.day == 0
        assert rec.status == "pending"
        assert rec.deltas == {}
        assert rec.txids == []

    def test_parcel_record(self):
        parcel = ParcelRecord(
            parcel_id="abc:FOD:5",
            sender="rSender123",
            contents={"food": 5},
            day_received=3,
        )
        assert parcel.accepted is False
        assert parcel.contents["food"] == 5

    def test_permit_record(self):
        permit = PermitRecord(permit_id="p1", day_earned=5)
        assert permit.used is False
        assert permit.day_used == 0


class TestTokenMap:
    def test_five_tokens_mapped(self):
        assert len(XRPL_TOKEN_MAP) == 5

    def test_all_codes_three_chars(self):
        for _key, (code, _display) in XRPL_TOKEN_MAP.items():
            assert len(code) == 3

    def test_keys_match_resources(self):
        assert set(XRPL_TOKEN_MAP.keys()) == XRPL_RESOURCES

    def test_expected_mappings(self):
        assert XRPL_TOKEN_MAP["food"] == ("FOD", "FOOD")
        assert XRPL_TOKEN_MAP["water"] == ("WTR", "WATR")
        assert XRPL_TOKEN_MAP["meds"] == ("MED", "MEDS")
        assert XRPL_TOKEN_MAP["ammo"] == ("AMO", "AMMO")
        assert XRPL_TOKEN_MAP["parts"] == ("PRT", "PART")

    def test_testnet_url(self):
        assert "rippletest" in TESTNET_URL


class TestSafetyConstants:
    def test_testnet_url_host_is_in_allowlist(self):
        """ledger-B03: the default URL's host must be a known testnet host."""
        from urllib.parse import urlparse

        host = urlparse(TESTNET_URL).hostname
        assert host in TESTNET_HOSTS

    def test_testnet_hosts_are_ripple_test_networks(self):
        assert "s.altnet.rippletest.net" in TESTNET_HOSTS
        assert "s.devnet.rippletest.net" in TESTNET_HOSTS
        # No mainnet host is on the allowlist.
        assert not any("rippletest" not in h for h in TESTNET_HOSTS)

    def test_parcel_accept_cap_is_named_constant(self):
        """ledger-B09: the parcel cap is a documented lever, not a bare literal."""
        assert isinstance(PARCEL_ACCEPT_CAP, int)
        assert PARCEL_ACCEPT_CAP == 20

    def test_memo_schema_version_token(self):
        """ledger-B09: a schema-version token exists for the memo header."""
        assert isinstance(MEMO_SCHEMA_VERSION, str)
        assert MEMO_SCHEMA_VERSION

    def test_extra_missing_copy_names_quoted_pip_extra(self):
        """F-64e78470: extra-missing copy names the quoted pip extra."""
        assert XRPL_EXTRA_PIP == 'pip install "escape-the-valley[xrpl]"'
        assert XRPL_EXTRA_MISSING_MSG.endswith(XRPL_EXTRA_PIP)
        assert "wallet" not in XRPL_EXTRA_MISSING_MSG.lower()
        assert "mainnet" not in XRPL_EXTRA_MISSING_MSG.lower()


class TestMintedSnapshot:
    """F-a6efdd6c: enable-time mint snapshot + save shim."""

    _SNAP = {"food": 50, "water": 50, "meds": 10, "ammo": 20, "parts": 10}

    def test_stamp_writes_field_and_reserved_permit(self):
        bp = BackpackState()
        stamp_minted_snapshot(bp, self._SNAP)
        assert bp.minted_initial == self._SNAP
        assert any(
            p.permit_id == MINTED_SNAPSHOT_PERMIT_ID and p.used
            for p in bp.permits
        )

    def test_stamp_ignores_partial_snapshot(self):
        bp = BackpackState()
        stamp_minted_snapshot(bp, {"food": 50, "water": 40})
        assert bp.minted_initial == {}
        assert bp.permits == []

    def test_snapshot_prefers_in_memory_field(self):
        bp = BackpackState(minted_initial=dict(self._SNAP))
        assert minted_snapshot_of(bp) == self._SNAP

    def test_snapshot_hydrates_from_permit_shim(self):
        bp = BackpackState()
        stamp_minted_snapshot(bp, self._SNAP)
        bp.minted_initial = {}  # simulate save.py dropping the dedicated field
        restored = minted_snapshot_of(bp)
        assert restored == self._SNAP
        assert bp.minted_initial == self._SNAP  # hydrated in place

    def test_missing_snapshot_is_empty(self):
        bp = BackpackState(
            last_settled_supplies=dict(self._SNAP),
            settlements=[],
        )
        assert minted_snapshot_of(bp) == {}
