"""Ledger Backpack — optional XRPL token inventory.

Tracks the 5 core supplies (FOOD, WATR, MEDS, AMMO, PART) as issued
tokens on XRPL Testnet. Settlement batches at town checkpoints.

xrpl-py is an optional dependency. If not installed, BackpackManager
reports unavailable and all operations are no-ops.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import TYPE_CHECKING

from .backpack_models import (
    TESTNET_URL,
    XRPL_RESOURCES,
    XRPL_TOKEN_MAP,
    ParcelRecord,
    SettlementRecord,
)

if TYPE_CHECKING:
    from .models import RunState

log = logging.getLogger(__name__)

# ── Optional xrpl-py import ──────────────────────────────────────

_HAS_XRPL = False
try:
    from xrpl.clients import JsonRpcClient
    from xrpl.models.amounts import IssuedCurrencyAmount
    from xrpl.models.requests import AccountLines
    from xrpl.models.transactions import Memo, Payment, TrustSet
    from xrpl.transaction import submit_and_wait
    from xrpl.wallet import Wallet, generate_faucet_wallet

    _HAS_XRPL = True
except ImportError:

    class Memo:  # type: ignore[no-redef]
        """Stub for when xrpl-py is not installed."""

        def __init__(self, **kwargs):
            self.__dict__.update(kwargs)


# ── Result types ──────────────────────────────────────────────────


@dataclass
class EnableResult:
    success: bool
    message: str
    wallet_address: str = ""


@dataclass
class SettlementResult:
    success: bool
    message: str
    txids: list[str] | None = None
    record: SettlementRecord | None = None


# ── Helpers ───────────────────────────────────────────────────────


def _hex_encode(text: str) -> str:
    """Encode a string to hex for XRPL memo fields."""
    return text.encode("utf-8").hex().upper()


def _build_memo(run_id: str, day: int, deltas: dict[str, int]) -> list:
    """Build XRPL memo list for a settlement transaction."""
    delta_parts = []
    for key, diff in sorted(deltas.items()):
        code = XRPL_TOKEN_MAP[key][0]
        sign = "+" if diff > 0 else ""
        delta_parts.append(f"{code}{sign}{diff}")

    memo_text = f"TRAIL|RUN:{run_id}|DAY:{day}|DELTA:{','.join(delta_parts)}"
    return [Memo(
        memo_data=_hex_encode(memo_text),
        memo_type=_hex_encode("text/plain"),
    )]


def _shorten_address(addr: str) -> str:
    """Shorten an XRPL address for display: rN7q...4xKp"""
    if len(addr) <= 10:
        return addr
    return f"{addr[:4]}...{addr[-4:]}"


# ── BackpackManager ──────────────────────────────────────────────


class BackpackManager:
    """Manages the optional XRPL Ledger Backpack.

    All XRPL operations are wrapped in try/except for graceful
    degradation. The game never blocks on network failures.
    """

    def __init__(self, url: str = TESTNET_URL):
        self._url = url
        self._client: JsonRpcClient | None = None

    @property
    def available(self) -> bool:
        return _HAS_XRPL

    def _get_client(self) -> JsonRpcClient:
        if self._client is None:
            self._client = JsonRpcClient(self._url)
        return self._client

    def enable(self, state: RunState) -> EnableResult:
        """Enable the Ledger Backpack: create wallets, trust lines, mint."""
        if not _HAS_XRPL:
            return EnableResult(
                success=False,
                message=(
                    "xrpl-py is not installed. "
                    "Install with: pip install escape-the-valley[xrpl]"
                ),
            )

        bp = state.backpack
        client = self._get_client()

        try:
            # Step 1: Create issuer wallet ("Trail Authority")
            issuer = generate_faucet_wallet(client, debug=False)
            bp.issuer_address = issuer.address
            bp.issuer_secret = issuer.seed

            # Step 2: Create player wallet
            player = generate_faucet_wallet(client, debug=False)
            bp.wallet_address = player.address
            bp.wallet_secret = player.seed

            # Step 3: Set trust lines (player trusts issuer for each token)
            for _key, (code, _display) in XRPL_TOKEN_MAP.items():
                trust_tx = TrustSet(
                    account=player.address,
                    limit_amount=IssuedCurrencyAmount(
                        currency=code,
                        issuer=issuer.address,
                        value="999999",
                    ),
                )
                submit_and_wait(trust_tx, client, player)

            bp.trust_lines_ready = True

            # Step 4: Mint starting supplies (issuer sends to player)
            for key, (code, _display) in XRPL_TOKEN_MAP.items():
                amount = state.supplies.get(key)
                if amount > 0:
                    mint_tx = Payment(
                        account=issuer.address,
                        destination=player.address,
                        amount=IssuedCurrencyAmount(
                            currency=code,
                            issuer=issuer.address,
                            value=str(amount),
                        ),
                    )
                    submit_and_wait(mint_tx, client, issuer)

            # Step 5: Record snapshot
            bp.last_settled_supplies = {
                k: state.supplies.get(k) for k in XRPL_RESOURCES
            }
            bp.last_settlement_day = state.day
            bp.enabled = True

            return EnableResult(
                success=True,
                message="Ledger Backpack enabled. Your pack is now receipted.",
                wallet_address=player.address,
            )

        except Exception as e:
            log.warning("Backpack enable failed: %s", e)
            return EnableResult(
                success=False,
                message=(
                    "Couldn't reach the faucet right now. "
                    "Ledger Backpack stays OFF. "
                    "You can try again at the next town."
                ),
            )

    def settle(self, state: RunState, location: str) -> SettlementResult:
        """Settle a checkpoint: compute delta, batch Payment txs."""
        if not _HAS_XRPL:
            return SettlementResult(success=False, message="xrpl-py not available")

        bp = state.backpack
        if not bp.enabled or not bp.wallet_address:
            return SettlementResult(success=False, message="Backpack not enabled")

        # Retry any pending settlements first
        self._retry_pending(state)

        # Compute deltas since last settlement
        deltas: dict[str, int] = {}
        for key in XRPL_RESOURCES:
            current = state.supplies.get(key)
            previous = bp.last_settled_supplies.get(key, 0)
            diff = current - previous
            if diff != 0:
                deltas[key] = diff

        if not deltas:
            return SettlementResult(
                success=True,
                message="No changes to settle.",
            )

        client = self._get_client()
        player = Wallet.from_seed(bp.wallet_secret)
        issuer = Wallet.from_seed(bp.issuer_secret)
        memos = _build_memo(state.run_id, state.day, deltas)
        txids: list[str] = []

        try:
            for key, diff in deltas.items():
                code = XRPL_TOKEN_MAP[key][0]

                if diff < 0:
                    # Player lost supplies → send back to issuer
                    tx = Payment(
                        account=player.address,
                        destination=issuer.address,
                        amount=IssuedCurrencyAmount(
                            currency=code,
                            issuer=issuer.address,
                            value=str(abs(diff)),
                        ),
                        memos=memos,
                    )
                    resp = submit_and_wait(tx, client, player)
                else:
                    # Player gained supplies → issuer sends to player
                    tx = Payment(
                        account=issuer.address,
                        destination=player.address,
                        amount=IssuedCurrencyAmount(
                            currency=code,
                            issuer=issuer.address,
                            value=str(diff),
                        ),
                        memos=memos,
                    )
                    resp = submit_and_wait(tx, client, issuer)

                txid = resp.result.get("hash", "")
                if txid:
                    txids.append(txid)

            # Update snapshot
            bp.last_settled_supplies = {
                k: state.supplies.get(k) for k in XRPL_RESOURCES
            }
            bp.last_settlement_day = state.day

            memo_text = f"TRAIL|RUN:{state.run_id}|DAY:{state.day}"
            record = SettlementRecord(
                day=state.day,
                location=location,
                deltas=deltas,
                txids=txids,
                status="settled",
                memo=memo_text,
                timestamp=datetime.now(UTC).isoformat(),
            )
            bp.settlements.append(record)

            short_txid = txids[0][:12] + "..." if txids else "none"
            return SettlementResult(
                success=True,
                message=f"Checkpoint settled. Receipt: {short_txid}",
                txids=txids,
                record=record,
            )

        except Exception as e:
            log.warning("Settlement failed at %s: %s", location, e)
            record = SettlementRecord(
                day=state.day,
                location=location,
                deltas=deltas,
                txids=[],
                status="pending",
                memo=f"TRAIL|RUN:{state.run_id}|DAY:{state.day}",
                timestamp=datetime.now(UTC).isoformat(),
            )
            bp.pending_settlements.append(record)

            return SettlementResult(
                success=False,
                message=(
                    "The ledger is quiet. I couldn't settle this checkpoint. "
                    "Your run continues offline for now. "
                    "We'll retry at the next safe moment\u2014"
                    "or you can settle manually from the Ledger menu."
                ),
                record=record,
            )

    def _retry_pending(self, state: RunState) -> None:
        """Retry all pending settlements. Move successful ones to settled."""
        bp = state.backpack
        if not bp.pending_settlements:
            return

        if not _HAS_XRPL:
            return

        client = self._get_client()
        player = Wallet.from_seed(bp.wallet_secret)
        issuer = Wallet.from_seed(bp.issuer_secret)

        still_pending: list[SettlementRecord] = []

        for record in bp.pending_settlements:
            try:
                txids: list[str] = []
                memos = _build_memo(state.run_id, record.day, record.deltas)

                for key, diff in record.deltas.items():
                    code = XRPL_TOKEN_MAP[key][0]

                    if diff < 0:
                        tx = Payment(
                            account=player.address,
                            destination=issuer.address,
                            amount=IssuedCurrencyAmount(
                                currency=code,
                                issuer=issuer.address,
                                value=str(abs(diff)),
                            ),
                            memos=memos,
                        )
                        resp = submit_and_wait(tx, client, player)
                    else:
                        tx = Payment(
                            account=issuer.address,
                            destination=player.address,
                            amount=IssuedCurrencyAmount(
                                currency=code,
                                issuer=issuer.address,
                                value=str(diff),
                            ),
                            memos=memos,
                        )
                        resp = submit_and_wait(tx, client, issuer)

                    txid = resp.result.get("hash", "")
                    if txid:
                        txids.append(txid)

                record.txids = txids
                record.status = "settled"
                record.timestamp = datetime.now(UTC).isoformat()
                bp.settlements.append(record)

            except Exception:
                still_pending.append(record)

        bp.pending_settlements = still_pending

    def check_parcels(self, state: RunState) -> list[ParcelRecord]:
        """Check for incoming token parcels from other travelers."""
        bp = state.backpack
        if not _HAS_XRPL or not bp.enabled or not bp.wallet_address:
            return []

        try:
            client = self._get_client()
            resp = client.request(AccountLines(
                account=bp.wallet_address,
            ))

            # Look for balances from non-issuer addresses
            new_parcels: list[ParcelRecord] = []
            known_ids = {p.parcel_id for p in bp.parcels}

            for line in resp.result.get("lines", []):
                peer = line.get("account", "")
                if peer == bp.issuer_address:
                    continue  # Skip issuer lines — those are game inventory

                balance = float(line.get("balance", "0"))
                currency = line.get("currency", "")

                if balance <= 0:
                    continue

                # Map XRPL code back to game key
                game_key = None
                for key, (code, _) in XRPL_TOKEN_MAP.items():
                    if code == currency:
                        game_key = key
                        break

                if game_key is None:
                    continue

                parcel_id = f"{peer}:{currency}:{int(balance)}"
                if parcel_id in known_ids:
                    continue

                parcel = ParcelRecord(
                    parcel_id=parcel_id,
                    sender=peer,
                    contents={game_key: int(balance)},
                    txid="",
                    accepted=False,
                    day_received=state.day,
                )
                new_parcels.append(parcel)
                bp.parcels.append(parcel)

            return new_parcels

        except Exception as e:
            log.warning("Parcel check failed: %s", e)
            return []

    def accept_parcel(
        self, parcel: ParcelRecord, state: RunState, cap: int = 20,
    ) -> bool:
        """Accept a parcel: apply contents to supplies, capped."""
        for key, amount in parcel.contents.items():
            capped_amount = min(amount, cap)
            current = state.supplies.get(key)
            state.supplies.set(key, current + capped_amount)

        parcel.accepted = True
        return True

    def disable(self, state: RunState) -> None:
        """Disable the Ledger Backpack. Keep wallet for potential re-enable."""
        state.backpack.enabled = False

    def wallet_info(self, state: RunState) -> dict:
        """Return wallet details for display."""
        bp = state.backpack
        if not bp.wallet_address:
            return {"status": "No wallet"}

        info = {
            "address": bp.wallet_address,
            "address_short": _shorten_address(bp.wallet_address),
            "issuer": _shorten_address(bp.issuer_address),
            "trust_lines": bp.trust_lines_ready,
            "settlements": len(bp.settlements),
            "pending": len(bp.pending_settlements),
        }

        # Query live balances if available
        if _HAS_XRPL and bp.enabled:
            try:
                client = self._get_client()
                resp = client.request(AccountLines(
                    account=bp.wallet_address,
                ))
                balances = {}
                for line in resp.result.get("lines", []):
                    if line.get("account") == bp.issuer_address:
                        currency = line.get("currency", "")
                        balance = line.get("balance", "0")
                        balances[currency] = int(float(balance))
                info["balances"] = balances
            except Exception:
                info["balances"] = {}

        return info

    def status_line(self, state: RunState) -> str:
        """One-line status for the TUI status panel."""
        bp = state.backpack
        if bp.enabled:
            pending = len(bp.pending_settlements)
            line = "Ledger: ON (Testnet)"
            if pending:
                line += f"  Unsettled: {pending} checkpoint"
                if pending > 1:
                    line += "s"
            return line
        return "Ledger: OFF"

    def close(self) -> None:
        """Close the XRPL client connection."""
        if self._client is not None:
            try:
                self._client.close()
            except Exception:
                pass
            self._client = None
