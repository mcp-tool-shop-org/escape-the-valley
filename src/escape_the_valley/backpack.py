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
from decimal import Decimal, InvalidOperation
from typing import TYPE_CHECKING
from urllib.parse import urlparse

from .backpack_models import (
    MEMO_SCHEMA_VERSION,
    PARCEL_ACCEPT_CAP,
    TESTNET_HOSTS,
    TESTNET_URL,
    XRPL_RESOURCES,
    XRPL_TOKEN_MAP,
    ParcelRecord,
    SentParcelRecord,
    SettlementRecord,
)

if TYPE_CHECKING:
    from .models import RunState

log = logging.getLogger(__name__)

# Network timeout for every XRPL request (ledger-B02). Kept consistent with the
# GM's ~30s ceiling so a stalled testnet node degrades to the offline/unanchored
# path (a pending settlement, an unreachable wallet) instead of hanging the run.
# xrpl-py's default per-request timeout is only 10s and is not surfaced through
# JsonRpcClient(url); we pass it explicitly via _TimeoutJsonRpcClient below.
XRPL_REQUEST_TIMEOUT: float = 30.0

# ── Optional xrpl-py import ──────────────────────────────────────

_HAS_XRPL = False
try:
    from xrpl.clients import JsonRpcClient
    from xrpl.models.amounts import IssuedCurrencyAmount
    from xrpl.models.requests import AccountLines, AccountTx
    from xrpl.models.transactions import Memo, Payment, TrustSet
    from xrpl.transaction import submit_and_wait
    from xrpl.wallet import Wallet, generate_faucet_wallet

    _HAS_XRPL = True

    class _TimeoutJsonRpcClient(JsonRpcClient):
        """JsonRpcClient that bounds every request with an explicit timeout.

        xrpl-py's ``JsonRpcClient(url)`` constructor takes no timeout and the
        sync ``request()`` calls ``_request_impl`` with the library default
        (10s). We override ``request()`` to thread ``XRPL_REQUEST_TIMEOUT``
        through so a stalled node fails fast into the offline path (ledger-B02)
        rather than hanging the run, and on a consistent ceiling with the GM.
        """

        def __init__(self, url, timeout: float = XRPL_REQUEST_TIMEOUT):
            super().__init__(url)
            self._timeout = timeout

        def request(self, request):  # noqa: A003 - matches xrpl API name
            import asyncio

            return asyncio.run(
                self._request_impl(request, timeout=self._timeout)
            )

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


@dataclass
class SendResult:
    success: bool
    message: str
    txid: str = ""


# ── Helpers ───────────────────────────────────────────────────────


def _hex_encode(text: str) -> str:
    """Encode a string to hex for XRPL memo fields."""
    return text.encode("utf-8").hex().upper()


def _settlement_memo_text(run_id: str, day: int, deltas: dict[str, int]) -> str:
    """Canonical settlement memo text — the exact bytes written on-chain.

    Used by both ``_build_memo`` (the on-chain Memo) and the stored
    ``SettlementRecord.memo`` so the record matches the on-ledger memo
    byte-for-byte (ledger-003). Begins with ``TRAIL|RUN:<id>|DAY:<n>`` so the
    external verifier can confirm the prefix against the decoded on-chain memo.

    Schema version (ledger-B09): a trailing ``|V:<version>`` field stamps the
    memo grammar so a future format change is self-describing on-chain. It is
    appended AFTER the run/day/delta fields, never before, so the
    ``TRAIL|RUN:<id>|DAY:<n>`` prefix the verifier matches on stays intact.
    """
    delta_parts = []
    for key, diff in sorted(deltas.items()):
        code = XRPL_TOKEN_MAP[key][0]
        sign = "+" if diff > 0 else ""
        delta_parts.append(f"{code}{sign}{diff}")

    return (
        f"TRAIL|RUN:{run_id}|DAY:{day}|DELTA:{','.join(delta_parts)}"
        f"|V:{MEMO_SCHEMA_VERSION}"
    )


def _build_memo(run_id: str, day: int, deltas: dict[str, int]) -> list:
    """Build XRPL memo list for a settlement transaction."""
    memo_text = _settlement_memo_text(run_id, day, deltas)
    return [Memo(
        memo_data=_hex_encode(memo_text),
        memo_type=_hex_encode("text/plain"),
    )]


def _shorten_address(addr: str) -> str:
    """Shorten an XRPL address for display: rN7q...4xKp"""
    if len(addr) <= 10:
        return addr
    return f"{addr[:4]}...{addr[-4:]}"


def _hex_decode(hex_str: str) -> str:
    """Decode a hex string from XRPL memo fields."""
    return bytes.fromhex(hex_str).decode("utf-8")


def _build_parcel_memo(run_id: str, day: int, supply: str, amount: int) -> list:
    """Build XRPL memo list for a parcel send transaction."""
    memo_text = f"PARCEL|RUN:{run_id}|DAY:{day}|{supply}:{amount}"
    return [Memo(
        memo_data=_hex_encode(memo_text),
        memo_type=_hex_encode("text/plain"),
    )]


def _balance_to_int(balance: str) -> int:
    """Parse an IOU balance string to an exact integer.

    IOU balances are decimal strings; routing them through binary ``float``
    risks precision loss before they feed ``reconcile()`` (ledger-004). Parse
    with ``Decimal`` and require the fractional part to be zero — supplies are
    integers only, so a non-integer on-ledger balance is itself a drift signal
    and must not be silently truncated.
    """
    dec = Decimal(str(balance))
    if dec != dec.to_integral_value():
        raise ValueError(f"non-integer IOU balance: {balance!r}")
    return int(dec)


def _setup_complete(bp) -> bool:
    """True when enable() finished every step (ledger-B05).

    A pack is only "back online" if it has wallets, trust lines are ready, and
    the minted snapshot is populated. A half-built pack (faucet succeeded but
    trust lines or mint did not) returns False so enable() resumes the missing
    steps instead of falsely declaring it online.
    """
    return bool(
        bp.wallet_address
        and bp.issuer_secret
        and bp.trust_lines_ready
        and bp.last_settled_supplies
    )


def _decode_parcel_memo(memo_hex: str) -> dict | None:
    """Decode a PARCEL memo from hex. Returns {supply, amount} or None."""
    try:
        text = _hex_decode(memo_hex)
    except (ValueError, UnicodeDecodeError):
        return None

    if not text.startswith("PARCEL|"):
        return None

    parts = text.split("|")
    # Expected: PARCEL|RUN:<id>|DAY:<n>|<supply>:<amount>
    if len(parts) < 4:
        return None

    supply_part = parts[-1]
    if ":" not in supply_part:
        return None

    supply, _, amount_str = supply_part.partition(":")
    try:
        amount = int(amount_str)
    except ValueError:
        return None

    if supply not in XRPL_TOKEN_MAP:
        return None

    return {"supply": supply, "amount": amount}


# ── BackpackManager ──────────────────────────────────────────────


class BackpackManager:
    """Manages the optional XRPL Ledger Backpack.

    All XRPL operations are wrapped in try/except for graceful
    degradation. The game never blocks on network failures.
    """

    def __init__(self, url: str = TESTNET_URL, *, allow_non_testnet: bool = False):
        """Create a manager bound to an XRPL endpoint.

        Testnet-only by default (ledger-B03 / SAFETY): the URL host must be on
        ``TESTNET_HOSTS`` (Ripple's public Testnet/Devnet). The "no real value
        at risk" guarantee is enforced here in code, not by convention — a
        mainnet URL is rejected with a clear error so a mis-set endpoint can
        never move value off a real account. ``allow_non_testnet=True`` is the
        single, explicit escape hatch (e.g. a local standalone rippled in CI);
        it is honored but logs a loud warning so the choice is never silent.
        """
        host = (urlparse(url).hostname or "").lower()
        if host not in TESTNET_HOSTS and not allow_non_testnet:
            allowed = ", ".join(sorted(TESTNET_HOSTS))
            raise ValueError(
                f"Ledger Backpack refuses non-testnet host {host!r}: "
                f"the 'no real value at risk' guarantee is testnet-only. "
                f"Allowed hosts: {allowed}. "
                f"Pass allow_non_testnet=True only for a local test node."
            )
        if host not in TESTNET_HOSTS:
            log.warning(
                "Ledger Backpack connecting to NON-TESTNET host %r "
                "(allow_non_testnet=True) — real value may be at risk. "
                "This is not a supported configuration.",
                host,
            )
        self._url = url
        self._client: JsonRpcClient | None = None

    @property
    def available(self) -> bool:
        return _HAS_XRPL

    def _get_client(self) -> JsonRpcClient:
        if self._client is None:
            # Timeout-bounded client (ledger-B02): a stalled testnet node
            # degrades to the offline path instead of hanging the run.
            self._client = _TimeoutJsonRpcClient(self._url)
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

        # Idempotent re-enable (ledger-002 + ledger-B05): "back online" is only
        # honest when the setup actually COMPLETED — wallets exist, trust lines
        # are ready, AND the minted snapshot is populated. Checking just
        # wallet_address+issuer_secret (ledger-002's original guard) would
        # declare a half-built pack (faucet succeeded, trust lines or mint did
        # not) "back online" with no trust lines and no tokens, so the next
        # settle() fails silently. Now a complete pack flips back on in place
        # (no fresh faucet, no re-mint — that would orphan the old wallet's
        # tokens); a half-built pack falls through to RESUME the missing steps
        # below using the wallets it already has.
        if bp.wallet_address and bp.issuer_secret and _setup_complete(bp):
            bp.enabled = True
            return EnableResult(
                success=True,
                message="Ledger Backpack re-enabled. Your existing pack is back online.",
                wallet_address=bp.wallet_address,
            )

        client = self._get_client()
        resuming = bool(bp.wallet_address and bp.issuer_secret)

        try:
            if resuming:
                # Reuse the wallets a prior partial enable already created —
                # generating new faucet wallets would strand the old addresses
                # that `settlements`/`pending_settlements` may already reference.
                issuer = Wallet.from_seed(bp.issuer_secret)
                player = Wallet.from_seed(bp.wallet_secret)
                log.info(
                    "Backpack enable: resuming partial setup "
                    "(trust_lines_ready=%s, minted=%s)",
                    bp.trust_lines_ready, bool(bp.last_settled_supplies),
                )
            else:
                # Step 1: Create issuer wallet ("Trail Authority")
                issuer = generate_faucet_wallet(client, debug=False)
                bp.issuer_address = issuer.address
                bp.issuer_secret = issuer.seed

                # Step 2: Create player wallet
                player = generate_faucet_wallet(client, debug=False)
                bp.wallet_address = player.address
                bp.wallet_secret = player.seed

            # Step 3: Set trust lines (player trusts issuer for each token).
            # Skipped when already ready — TrustSet is idempotent on XRPL (a
            # repeat just re-sets the same limit) but we avoid the round-trips.
            if not bp.trust_lines_ready:
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
                # Flag set ONLY after every trust line fully succeeds
                # (ledger-B05): a mid-loop failure leaves it False so the next
                # enable() resumes rather than skipping straight to mint.
                bp.trust_lines_ready = True

            # Step 4: Mint starting supplies (issuer sends to player). Skipped
            # when the snapshot is already populated so a resume does not
            # double-mint.
            if not bp.last_settled_supplies:
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

                # Step 5: Record snapshot. Set ONLY after the full mint loop
                # completes (ledger-B05) so a half-minted pack stays "incomplete"
                # and resumes on the next enable().
                bp.last_settled_supplies = {
                    k: state.supplies.get(k) for k in XRPL_RESOURCES
                }
                bp.last_settlement_day = state.day

            bp.enabled = True

            message = (
                "Ledger Backpack setup resumed. Your pack is now receipted."
                if resuming
                else "Ledger Backpack enabled. Your pack is now receipted."
            )
            return EnableResult(
                success=True,
                message=message,
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

            # Store the exact on-chain memo text so the record matches the
            # on-ledger bytes (ledger-003), not a DELTA-less near-miss.
            memo_text = _settlement_memo_text(state.run_id, state.day, deltas)
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

            # Settlement reached the ledger: clear the degraded signal
            # (ledger-B04) so the status line drops back to a healthy state.
            bp.last_settle_failed = False

            # Settlement-lifecycle log (ledger-B07): a successful settle records
            # the day, signed deltas, and txids so a later reconcile mismatch is
            # traceable from the log without re-running the chain.
            log.info(
                "settle ok: day=%d location=%s deltas=%s txids=%s",
                state.day, location, deltas, txids,
            )

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
                memo=_settlement_memo_text(state.run_id, state.day, deltas),
                timestamp=datetime.now(UTC).isoformat(),
            )
            bp.pending_settlements.append(record)
            # Degraded signal (ledger-B04): the testnet was unreachable, this
            # checkpoint is unsettled. status_line + cli-tui render the offline
            # state from this so the pending count is not read as a healthy
            # backlog.
            bp.last_settle_failed = True
            log.info(
                "settle degraded: day=%d location=%s deltas=%s — "
                "queued as pending (testnet unreachable)",
                state.day, location, deltas,
            )

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
        moved = 0  # settlements that cleared on this retry pass (ledger-B07)

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
                # Match the on-chain memo bytes actually written above (ledger-003).
                record.memo = _settlement_memo_text(
                    state.run_id, record.day, record.deltas,
                )
                record.timestamp = datetime.now(UTC).isoformat()
                bp.settlements.append(record)

                # Conservation fix (ENG-A-08): a failed settle() leaves the
                # baseline un-advanced and enqueues this pending record. Now that
                # it is settled on-chain, fold its (signed) delta into the
                # baseline so the *next* fresh settle() measures current against a
                # baseline that already accounts for this retried portion.
                # Without this, settle() recomputes (current - baseline) over the
                # WHOLE interval — including the just-retried delta — paying it
                # on-chain twice and double-summing it in reconcile(), breaking
                # 'minted + Σdeltas == final'. Only advance on success; a record
                # that fails below stays pending with the baseline untouched.
                for key, val in record.deltas.items():
                    bp.last_settled_supplies[key] = (
                        bp.last_settled_supplies.get(key, 0) + val
                    )
                moved += 1
                log.info(
                    "retry settled: day=%d deltas=%s txids=%s",
                    record.day, record.deltas, txids,
                )

            except Exception as e:
                log.warning("Retry settlement day %d failed: %s", record.day, e)
                still_pending.append(record)

        bp.pending_settlements = still_pending

        # Retry-pass lifecycle log + degraded signal (ledger-B04 / ledger-B07):
        # report how many cleared vs how many remain, and keep last_settle_failed
        # in sync — True while anything is still pending, cleared once the queue
        # drains (a fresh failing settle() re-sets it).
        log.info(
            "retry pending pass: moved=%d still_pending=%d",
            moved, len(still_pending),
        )
        if still_pending:
            bp.last_settle_failed = True
        elif moved:
            bp.last_settle_failed = False

    def send_parcel(
        self,
        state: RunState,
        recipient: str,
        supply: str,
        amount: int,
    ) -> SendResult:
        """Send a parcel to another traveler via XRP micropayment + memo.

        Deducts supplies locally. The recipient discovers the parcel
        via check_parcels() at their next town.
        """
        bp = state.backpack
        if not bp.enabled or not bp.wallet_address:
            return SendResult(
                success=False,
                message="Ledger Backpack is not enabled.",
            )

        # Input validation (no xrpl-py needed)
        if supply not in XRPL_TOKEN_MAP:
            valid = ", ".join(sorted(XRPL_TOKEN_MAP.keys()))
            return SendResult(
                success=False,
                message=f"Unknown supply '{supply}'. Valid: {valid}",
            )

        if amount <= 0:
            return SendResult(success=False, message="Amount must be positive.")

        current = state.supplies.get(supply)
        if current < amount:
            return SendResult(
                success=False,
                message=f"Not enough {supply} (have {current}, need {amount}).",
            )

        if recipient == bp.wallet_address:
            return SendResult(success=False, message="Cannot send to yourself.")

        # XRPL required for actual send
        if not _HAS_XRPL:
            return SendResult(
                success=False,
                message="xrpl-py is not installed.",
            )

        # Build memo and send XRP micropayment (12 drops = minimum)
        memo_text = f"PARCEL|RUN:{state.run_id}|DAY:{state.day}|{supply}:{amount}"
        memos = _build_parcel_memo(state.run_id, state.day, supply, amount)

        try:
            client = self._get_client()
            player = Wallet.from_seed(bp.wallet_secret)

            tx = Payment(
                account=player.address,
                destination=recipient,
                amount="12",  # 12 drops — minimum XRP payment
                memos=memos,
            )
            resp = submit_and_wait(tx, client, player)
            txid = resp.result.get("hash", "")

            # Deduct supplies
            state.supplies.set(supply, current - amount)

            # Record outgoing parcel
            record = SentParcelRecord(
                recipient=recipient,
                supply=supply,
                amount=amount,
                txid=txid,
                day_sent=state.day,
                memo=memo_text,
            )
            bp.sent_parcels.append(record)

            short_addr = _shorten_address(recipient)
            return SendResult(
                success=True,
                message=(
                    f"Sent {amount} {supply} to {short_addr}. "
                    f"Receipt: {txid[:12]}..."
                ),
                txid=txid,
            )

        except Exception as e:
            log.warning("Parcel send to %s failed: %s", recipient, e)
            return SendResult(
                success=False,
                message=(
                    "The ledger is quiet. Couldn't send the parcel right now. "
                    "Your supplies are unchanged."
                ),
            )

    def check_parcels(self, state: RunState) -> list[ParcelRecord]:
        """Check for incoming parcels via account_tx memo scanning.

        Looks for XRP payments with PARCEL| memo prefix. Each unique
        tx hash becomes a parcel that can be accepted or refused.
        """
        bp = state.backpack
        if not _HAS_XRPL or not bp.enabled or not bp.wallet_address:
            return []

        try:
            client = self._get_client()
            resp = client.request(AccountTx(
                account=bp.wallet_address,
                limit=50,
            ))

            new_parcels: list[ParcelRecord] = []
            known_txids = {p.txid for p in bp.parcels if p.txid}

            for tx_entry in resp.result.get("transactions", []):
                tx = tx_entry.get("tx", tx_entry.get("tx_json", {}))
                meta = tx_entry.get("meta", {})

                # Only incoming XRP payments
                if tx.get("TransactionType") != "Payment":
                    continue
                if tx.get("Destination") != bp.wallet_address:
                    continue

                sender = tx.get("Account", "")
                if not sender or sender == bp.wallet_address:
                    continue

                # tx-hash location varies by AccountTx api_version (ledger-007):
                # api_version 2 puts `hash` on the wrapping entry alongside
                # `tx_json`; the legacy shape nests it inside `tx`. Read the
                # wrapper first, fall back to the inner object.
                tx_hash = tx_entry.get("hash") or tx.get("hash", "")
                if not tx_hash or tx_hash in known_txids:
                    continue

                # Check for PARCEL memo
                memos = tx.get("Memos", [])
                if not memos:
                    continue

                memo_data = memos[0].get("Memo", {}).get("MemoData", "")
                parsed = _decode_parcel_memo(memo_data)
                if parsed is None:
                    continue

                # Verify transaction succeeded
                result_code = meta.get("TransactionResult", "")
                if result_code != "tesSUCCESS":
                    continue

                parcel = ParcelRecord(
                    parcel_id=tx_hash,
                    sender=sender,
                    contents={parsed["supply"]: parsed["amount"]},
                    txid=tx_hash,
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
        self, parcel: ParcelRecord, state: RunState,
        cap: int = PARCEL_ACCEPT_CAP,
    ) -> bool:
        """Accept a parcel: apply contents to supplies, capped.

        The per-supply cap defaults to ``PARCEL_ACCEPT_CAP`` (ledger-B09): a
        named design lever, not a bare literal, documenting why generosity is
        bounded — a parcel cannot trivialize the survival pressure.

        Idempotent (ledger-005): a second accept is a no-op so a double-trigger
        from any caller (the TUI path does not guard) cannot double the supplies
        and break conservation. Mirrors refuse_parcel's `accepted` guard.
        """
        if parcel.accepted:
            return False

        for key, amount in parcel.contents.items():
            capped_amount = min(amount, cap)
            current = state.supplies.get(key)
            state.supplies.set(key, current + capped_amount)

        parcel.accepted = True
        return True

    def refuse_parcel(self, parcel: ParcelRecord) -> bool:
        """Refuse a parcel. Marks it refused without applying supplies."""
        if parcel.accepted:
            return False
        parcel.accepted = False
        parcel.parcel_id = f"refused:{parcel.parcel_id}"
        return True

    def fetch_onchain_memos(self, state: RunState) -> dict[str, str]:
        """Read settlement memos back OFF the chain, keyed by txid (ledger-003).

        The external-verifier half of the memo check: scans the issuer and
        player accounts via AccountTx and hex-decodes each transaction's
        ``MemoData`` so the proof driver can confirm the on-ledger memo matches
        the expected header — independent of whatever the engine stored locally.

        Pagination (ledger-A03): AccountTx caps results per page (we request
        200), and a long run can exceed that on a single account. Each response
        carries a ``marker`` when more transactions remain; we resubmit with
        that marker until the chain stops returning one, so older settlement
        memos are never dropped (which would otherwise produce a FALSE-NEGATIVE
        proof failure). A page cap bounds the loop against a server that keeps
        echoing a marker.

        Returns ``{txid: decoded_memo_text}``. Empty on any error or when xrpl
        is unavailable; the proof then reports memo integrity as unverified
        rather than crashing.
        """
        bp = state.backpack
        if not _HAS_XRPL or not bp.enabled or not bp.wallet_address:
            return {}

        memos: dict[str, str] = {}
        accounts = [a for a in (bp.wallet_address, bp.issuer_address) if a]
        max_pages = 100  # safety bound: 100 * 200 = 20k txns/account

        try:
            client = self._get_client()
            for account in accounts:
                marker = None
                pages = 0
                for _ in range(max_pages):
                    resp = client.request(AccountTx(
                        account=account, limit=200, marker=marker,
                    ))
                    pages += 1
                    result = resp.result
                    for tx_entry in result.get("transactions", []):
                        tx = tx_entry.get("tx", tx_entry.get("tx_json", {}))
                        tx_hash = tx_entry.get("hash") or tx.get("hash", "")
                        if not tx_hash or tx_hash in memos:
                            continue
                        tx_memos = tx.get("Memos", [])
                        if not tx_memos:
                            continue
                        memo_data = tx_memos[0].get("Memo", {}).get("MemoData", "")
                        try:
                            decoded = _hex_decode(memo_data)
                        except (ValueError, UnicodeDecodeError):
                            continue
                        memos[tx_hash] = decoded
                    # No marker → this account is fully paged.
                    marker = result.get("marker")
                    if not marker:
                        break
                # Pagination-lifecycle log (ledger-B07): how many AccountTx pages
                # were walked per account, so a missing memo can be traced to an
                # un-paged account rather than a verification bug.
                log.info(
                    "fetch_onchain_memos: account=%s pages=%d",
                    _shorten_address(account), pages,
                )
            return memos

        except Exception as e:
            log.warning("On-chain memo fetch failed: %s", e)
            return {}

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
                        # Integer/decimal-exact — no binary float (ledger-004).
                        try:
                            balances[currency] = _balance_to_int(balance)
                        except (ValueError, InvalidOperation):
                            log.warning(
                                "Non-integer IOU balance for %s: %r", currency, balance,
                            )
                info["balances"] = balances
                info["balances_error"] = False
            except Exception as e:
                # ledger-B08: distinguish "couldn't reach the ledger" from a
                # genuinely empty wallet. An empty {} could mean either; the
                # error flag lets the overlay say "balances unavailable
                # (offline)" instead of implying the pack is empty.
                log.warning("wallet_info balance query failed: %s", e)
                info["balances"] = {}
                info["balances_error"] = True

        return info

    def status_line(self, state: RunState) -> str:
        """One-line status for the TUI status panel.

        Degraded rendering (ledger-B04): when the last settle attempt failed
        (``last_settle_failed``) and checkpoints are unsettled, the line names
        the offline cause — "testnet unreachable" — instead of a bare pending
        count, so the player can tell a transient network outage from a healthy
        backlog. The cli-tui renders this string verbatim.
        """
        bp = state.backpack
        if bp.enabled:
            pending = len(bp.pending_settlements)
            if pending and bp.last_settle_failed:
                noun = "checkpoint" if pending == 1 else "checkpoints"
                return (
                    f"Ledger: ON (offline -- testnet unreachable, "
                    f"{pending} unsettled {noun})"
                )
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
