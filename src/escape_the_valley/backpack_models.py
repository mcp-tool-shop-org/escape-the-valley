"""Ledger Backpack data models — pure dataclasses, no xrpl imports.

Safe to import from models.py, save.py, adapter.py without pulling
in the optional xrpl-py dependency.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class SettlementRecord:
    """One checkpoint settlement on XRPL Testnet."""

    day: int = 0
    location: str = ""
    deltas: dict[str, int] = field(default_factory=dict)
    txids: list[str] = field(default_factory=list)
    status: str = "pending"  # "settled" | "pending" | "failed"
    memo: str = ""
    timestamp: str = ""


@dataclass
class ParcelRecord:
    """An incoming token parcel from another traveler."""

    parcel_id: str = ""
    sender: str = ""
    contents: dict[str, int] = field(default_factory=dict)
    txid: str = ""
    accepted: bool = False
    day_received: int = 0


@dataclass
class SentParcelRecord:
    """An outgoing parcel sent to another traveler via XRP + memo."""

    recipient: str = ""
    supply: str = ""  # game key (e.g. "food")
    amount: int = 0
    txid: str = ""
    day_sent: int = 0
    memo: str = ""


@dataclass
class PermitRecord:
    """A one-time clutch tool earned from a hard choice.

    Future seam (ledger-B09): permits are modeled and persisted but not yet
    issued or spent by any engine path. When the "earn a one-time tool from a
    hard choice" mechanic lands, it mints a permit token on the testnet and
    appends a record here; spending flips ``used`` and stamps ``day_used``.
    Kept in the data model now so saves written today round-trip forward without
    a schema migration once the mechanic ships.
    """

    permit_id: str = ""
    txid: str = ""
    used: bool = False
    day_earned: int = 0
    day_used: int = 0


@dataclass
class BackpackState:
    """Full state for the optional XRPL Ledger Backpack."""

    enabled: bool = False
    wallet_address: str = ""
    wallet_secret: str = ""  # testnet only — acceptable for play money
    issuer_address: str = ""
    issuer_secret: str = ""  # testnet only — per-run "Trail Authority"
    trust_lines_ready: bool = False

    # Snapshot of the 5 XRPL-tracked resources at last settlement
    last_settled_supplies: dict[str, int] = field(default_factory=dict)
    last_settlement_day: int = 0

    # Settlement history
    settlements: list[SettlementRecord] = field(default_factory=list)
    pending_settlements: list[SettlementRecord] = field(default_factory=list)

    # Parcels from other travelers
    parcels: list[ParcelRecord] = field(default_factory=list)

    # Outgoing parcels sent to other travelers
    sent_parcels: list[SentParcelRecord] = field(default_factory=list)

    # Permits (clutch tools)
    permits: list[PermitRecord] = field(default_factory=list)

    # UX state
    nudge_shown: bool = False
    nudge_dismissed: bool = False  # "Not now" was clicked — never nag again

    # Degraded-network signal (ledger-B04 / CROSS-DOMAIN CONTRACT). True after a
    # settle()/_retry_pending attempt could not reach the testnet and left a
    # checkpoint unsettled; cleared once a settlement (or retry) succeeds. The
    # status_line + cli-tui render a distinct "offline -- testnet unreachable"
    # state from this so a pending count is not mistaken for a healthy backlog.
    last_settle_failed: bool = False


# ── Currency mapping ──────────────────────────────────────────────
# Game key → (XRPL 3-char standard code, 4-char display label)
# Only these 5 resources are tracked on-chain.

XRPL_TOKEN_MAP: dict[str, tuple[str, str]] = {
    "food": ("FOD", "FOOD"),
    "water": ("WTR", "WATR"),
    "meds": ("MED", "MEDS"),
    "ammo": ("AMO", "AMMO"),
    "parts": ("PRT", "PART"),
}

XRPL_RESOURCES: set[str] = set(XRPL_TOKEN_MAP.keys())

TESTNET_URL = "https://s.altnet.rippletest.net:51234/"

# ── Testnet-only safety (ledger-B03) ──────────────────────────────
# The "no real value at risk" guarantee is enforced in code, not by
# convention: BackpackManager only connects to a host on this allowlist
# unless explicitly overridden with allow_non_testnet=True (which also
# logs a loud warning). These are Ripple's public test networks; tokens
# minted here have no monetary value, so a lost/leaked seed costs nothing.
TESTNET_HOSTS: frozenset[str] = frozenset({
    "s.altnet.rippletest.net",   # XRPL Testnet
    "s.devnet.rippletest.net",   # XRPL Devnet
})

# ── Parcel accept cap (ledger-B09) ────────────────────────────────
# A single accepted parcel cannot inject more than this many units of any
# one supply. Multiplayer parcels are a generosity channel, not an economy
# break: capping the accept keeps a benefactor (or a griefer) from trivializing
# the survival pressure that the whole game is about. Named here so the cap is
# a documented design lever rather than a bare literal at the call site.
PARCEL_ACCEPT_CAP: int = 20

# Memo schema-version token (ledger-B09). Stamped into the settlement memo
# header so a future memo-format change is self-describing on-chain and an
# external verifier can branch on the version it reads back. Bump when the
# TRAIL| memo grammar changes in a non-backward-compatible way.
MEMO_SCHEMA_VERSION: str = "v1"
