"""Ledger Backpack — optional XRPL token inventory.

Tracks the 5 core supplies (FOOD, WATR, MEDS, AMMO, PART) as issued
tokens on XRPL Testnet. Settlement batches at town checkpoints.

xrpl-py is an optional dependency. If not installed, BackpackManager
reports unavailable and all operations are no-ops.
"""

from __future__ import annotations

import logging
from collections.abc import Callable
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
    XRPL_EXTRA_MISSING_MSG,
    XRPL_EXTRA_PIP,
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

# Wall-clock ceiling for a single write (ledger-B02 WRITE-PATH). The READ path
# (AccountTx/AccountLines via client.request) is bounded per round-trip by
# _TimeoutJsonRpcClient, but the WRITE path goes through xrpl-py's
# submit_and_wait, which runs its OWN ledger-validation poll loop
# (_wait_for_final_transaction_outcome) — recursing with a 1s sleep between Tx
# lookups until the tx is validated OR the latest validated ledger passes the
# tx's LastLedgerSequence. submit_and_wait exposes NO timeout/deadline param
# (signature: transaction, client, wallet, *, check_fee, autofill, fail_hard),
# and the inner Tx polls call client._request_impl WITHOUT a timeout, so on a
# stalled node where ledgers stop closing the loop can spin well past
# XRPL_REQUEST_TIMEOUT. We therefore wrap submit_and_wait in a bounded worker
# (_submit_and_wait_bounded) whose wall-clock deadline caps the whole write,
# raising into the same offline/pending degradation path (settle → pending,
# send_parcel → supplies unchanged, enable → stays OFF) instead of freezing the
# run. The ceiling is a small multiple of the per-request timeout: a healthy
# write needs a handful of ledger closes (~4s each on testnet) plus signing, so
# 2x gives real submissions ample headroom while still bounding a stall.
XRPL_WRITE_DEADLINE: float = XRPL_REQUEST_TIMEOUT * 2  # 60s

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
        mapped = XRPL_TOKEN_MAP.get(key)
        if mapped is None:
            continue
        code = mapped[0]
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
    the minted snapshot is FULLY populated. A half-built pack (faucet
    succeeded but trust lines or mint did not) returns False so enable()
    resumes the missing steps instead of falsely declaring it online.

    "Fully populated" means every XRPL-tracked resource has a snapshot entry —
    not merely "the dict is non-empty" (ledger-CRIT-3). The mint step now
    records each resource into ``last_settled_supplies`` as its own Payment
    confirms (so a resumed enable() never re-mints one that already landed),
    which means a half-minted pack has a non-empty but INCOMPLETE dict; a bare
    truthiness check would wrongly call that "back online".
    """
    return bool(
        bp.wallet_address
        and bp.issuer_secret
        and bp.trust_lines_ready
        and XRPL_RESOURCES <= bp.last_settled_supplies.keys()
    )


def _decode_parcel_memo(memo_hex: str) -> dict | None:
    """Decode a PARCEL memo from hex. Returns {supply, amount} or None.

    Rejects a non-positive amount at decode time (ledger-CRIT-1): a parcel
    memo is attacker-controlled free text from any XRPL account (a throwaway
    testnet wallet costs nothing to create). A bare ``int()`` parse previously
    let a memo like ``food:-999999`` reach ``accept_parcel()``, which only
    bounded the upper side (``min(amount, cap)``) — so a single 12-drop
    payment could zero out or drive a victim's supply negative for free. An
    out-of-range amount is treated exactly like an unknown supply key: the
    parcel is never created.
    """
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

    if amount <= 0:
        return None

    if supply not in XRPL_TOKEN_MAP:
        return None

    return {"supply": supply, "amount": amount}


class WriteTimeoutError(Exception):
    """A write (submit_and_wait) exceeded its wall-clock deadline (ledger-B02).

    Raised by ``_submit_and_wait_bounded`` when the worker thread driving
    ``submit_and_wait`` does not finish within ``XRPL_WRITE_DEADLINE``. It is a
    plain ``Exception`` subclass so the existing ``except Exception`` blocks in
    ``settle``/``_retry_pending``/``send_parcel``/``enable`` catch it and route
    the write into the offline/pending degradation path — never a freeze.
    """


def _submit_and_wait_bounded(tx, client, signer, *, deadline: float | None = None):
    """Run ``submit_and_wait`` with a wall-clock deadline (ledger-B02 WRITE).

    xrpl-py's ``submit_and_wait`` polls the ledger for transaction validation in
    its own loop and exposes no timeout parameter, so a stalled testnet node
    (ledgers not closing) can keep it blocked far past ``XRPL_REQUEST_TIMEOUT``.
    Each individual round-trip is now capped by ``_TimeoutJsonRpcClient``, but
    the *number* of round-trips is unbounded; only an outer wall-clock deadline
    makes the WRITE path honor the "never hang, degrade to offline" guarantee.

    We drive the (synchronous) ``submit_and_wait`` on a daemon worker thread and
    wait at most ``deadline`` seconds for it. On timeout we raise
    ``WriteTimeoutError`` so the caller degrades exactly as it would for any other
    submit failure (settle → pending, send_parcel → supplies unchanged, enable
    → stays OFF). The worker is a daemon: if the underlying socket is wedged
    past the per-request timeout the thread cannot be force-killed, but it can
    never block the run or process exit, and the next attempt (manual or
    next-town retry) starts fresh.

    ``deadline`` defaults to ``None`` and is resolved to the module-level
    ``XRPL_WRITE_DEADLINE`` at call time (not bound at definition), so the
    ceiling stays tunable by monkeypatching the constant. ``submit_and_wait`` is
    likewise resolved through the module namespace so test monkeypatching of
    ``backpack.submit_and_wait`` continues to apply.
    """
    import threading

    if deadline is None:
        deadline = XRPL_WRITE_DEADLINE

    result: dict = {}

    def _worker() -> None:
        try:
            result["value"] = submit_and_wait(tx, client, signer)
        except BaseException as exc:  # noqa: BLE001 - re-raised on the caller thread
            result["error"] = exc

    worker = threading.Thread(target=_worker, daemon=True)
    worker.start()
    worker.join(deadline)

    if worker.is_alive():
        raise WriteTimeoutError(
            f"submit_and_wait exceeded {deadline:.0f}s deadline "
            f"(testnet stalled); degrading to offline/pending"
        )
    if "error" in result:
        raise result["error"]
    return result["value"]


# ── BackpackManager ──────────────────────────────────────────────


class BackpackManager:
    """Manages the optional XRPL Ledger Backpack.

    All XRPL operations are wrapped in try/except for graceful
    degradation. The game never blocks on network failures.
    """

    def __init__(
        self,
        url: str = TESTNET_URL,
        *,
        allow_non_testnet: bool = False,
        persist: Callable[[RunState], None] | None = None,
    ) -> None:
        """Create a manager bound to an XRPL endpoint.

        Testnet-only by default (ledger-B03 / SAFETY): the URL host must be on
        ``TESTNET_HOSTS`` (Ripple's public Testnet/Devnet). The "no real value
        at risk" guarantee is enforced here in code, not by convention — a
        mainnet URL is rejected with a clear error so a mis-set endpoint can
        never move value off a real account. ``allow_non_testnet=True`` is the
        single, explicit escape hatch (e.g. a local standalone rippled in CI);
        it is honored but logs a loud warning so the choice is never silent.

        ``persist`` (F-d178410b): an optional hook called with the full
        ``RunState`` immediately after each resource's Payment confirms
        inside ``settle()``/``_retry_pending()`` — see those methods'
        docstrings. Defaults to ``None``: both methods still mutate
        ``state.backpack`` purely in memory and rely on the caller to save
        afterward, exactly as before this fix, so passing nothing changes
        no existing behavior (and no existing test needs to account for
        surprise disk I/O). Every current call site constructs
        ``BackpackManager()`` with no arguments, so making the fix live for
        real players means passing e.g. ``persist=save_game`` (from
        ``.save``) at those call sites — a caller-side change outside this
        module's domain, not made here.
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
        self._persist = persist

    def _persist_state(self, state: RunState) -> None:
        """Invoke the ``persist`` hook, if any, tolerating its failure.

        A local disk error here must never undo or hide a Payment that has
        ALREADY confirmed on-chain, nor crash the caller mid-settlement — it
        only means this particular resource's fold stays exactly as durable
        as it was before this fix (in-memory only, saved whenever the caller
        next saves). That is strictly no worse than the pre-fix baseline, so
        degrading to it on a persist error is safe.

        Logs only the exception TYPE, never ``str(exc)`` or ``exc_info``
        (SAFETY): ``persist`` is caller-supplied and receives the full
        ``RunState``, including ``bp.wallet_secret``/``bp.issuer_secret`` —
        wallet secrets must never leave the gitignored sidecar into a log
        line (a hard product rule), and this module cannot verify a
        caller-supplied callable's exception messages never embed them.
        ``save_game`` (the intended hook — see ``__init__``) never does,
        but the wrapper stays defensive for any other callable passed here.
        """
        if self._persist is None:
            return
        try:
            self._persist(state)
        except Exception as exc:
            log.warning(
                "BackpackManager: persist hook raised %s; continuing with "
                "the in-memory fold only (F-d178410b)",
                type(exc).__name__,
            )

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
                message=XRPL_EXTRA_MISSING_MSG,
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

        resuming = bool(bp.wallet_address and bp.issuer_secret)

        # F-9517936e: _get_client / from_seed / faucet / mint all live inside
        # this try so a client-construct failure returns EnableResult
        # (success=False), matching send_parcel / settle. Never raise out.
        try:
            client = self._get_client()
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
                    _submit_and_wait_bounded(trust_tx, client, player)
                # Flag set ONLY after every trust line fully succeeds
                # (ledger-B05): a mid-loop failure leaves it False so the next
                # enable() resumes rather than skipping straight to mint.
                bp.trust_lines_ready = True

            # Step 4: Mint starting supplies (issuer sends to player), one
            # resource at a time. Unlike TrustSet (idempotent on XRPL — see
            # Step 3), a mint Payment is NOT idempotent: re-submitting an
            # already-successful mint on a resumed enable() would double the
            # player's starting balance. ledger-CRIT-3 fix: each resource is
            # recorded into last_settled_supplies the moment ITS OWN mint
            # confirms (or immediately if it needs no mint), rather than only
            # after the whole loop finishes — so a resource already present
            # here already landed on a prior attempt and is skipped, and a
            # resumed enable() mints only what is still missing.
            if XRPL_RESOURCES - bp.last_settled_supplies.keys():
                for key, (code, _display) in XRPL_TOKEN_MAP.items():
                    if key in bp.last_settled_supplies:
                        continue  # already minted (or needed none) previously
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
                        _submit_and_wait_bounded(mint_tx, client, issuer)
                    # Record ONLY after this resource's own mint confirms (or
                    # immediately when no mint was needed) — ledger-B05 /
                    # ledger-CRIT-3: a half-minted pack still keeps the
                    # resources that DID land, so the next enable() resumes
                    # exactly the ones that did not, instead of re-submitting
                    # every mint from scratch.
                    bp.last_settled_supplies[key] = amount

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
        """Settle a checkpoint: compute delta, batch Payment txs.

        Per-resource progress is tracked AS EACH PAYMENT CONFIRMS
        (ledger-CRIT-2): if an earlier resource's Payment clears on-chain and
        a LATER resource's Payment then raises (a transient network blip),
        the confirmed resource(s) are folded into ``bp.settlements`` and the
        baseline immediately, with their real txids — never discarded. Only
        the resource(s) that did NOT clear this pass are queued pending, so a
        retry can never re-submit a Payment for something already paid for on
        chain (the double-mint/double-burn this fix closes).

        That fold used to be purely in-memory: every real caller (tui_app.py,
        cli.py) saves separately, strictly AFTER this method returns. A crash
        anywhere in that gap replayed an already-confirmed Payment on the
        next run — a real duplicate on-chain settlement that local
        conservation math could not detect, because the crashed session's
        record was simply never persisted, so it was never summed either
        (F-d178410b). When this manager was constructed with a ``persist``
        hook, the baseline fold below calls it immediately after EACH
        resource confirms — but only AFTER that SAME resource's slice of the
        settlement receipt has also been folded into ``bp.settlements``
        in-memory (F-9d7eb977: the receipt is now built incrementally,
        one resource at a time, inside this same loop — never as a single
        pass after the loop ends). Ordering the persist call last within
        each iteration means every snapshot it writes is self-consistent: a
        resource is never seen advanced in the baseline without a matching
        settlement record, which matters the moment a crash lands between
        two resources of a multi-resource batch. Without a ``persist`` hook
        (the default), this method's observable behavior is byte-for-byte
        unchanged from before.
        """
        if not _HAS_XRPL:
            return SettlementResult(
                success=False, message=XRPL_EXTRA_MISSING_MSG,
            )

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

        confirmed: dict[str, int] = {}
        confirmed_txids: list[str] = []
        failure: Exception | None = None
        settled_record: SettlementRecord | None = None
        client = None
        player = None
        issuer = None
        memos = None

        # F-86a4c19c: wrap client/from_seed/memo into the same pending
        # split as a submit failure — ValueError/KeyError must not escape.
        try:
            client = self._get_client()
            player = Wallet.from_seed(bp.wallet_secret)
            issuer = Wallet.from_seed(bp.issuer_secret)
            # The on-chain memo for every Payment in this batch names the FULL set
            # of deltas being settled together (ledger-003) — unchanged even if a
            # later key in the loop fails, since it is the exact byte string
            # already signed and broadcast for whichever resources clear below.
            memos = _build_memo(state.run_id, state.day, deltas)
        except Exception as e:  # noqa: BLE001 - routed into the pending split below
            failure = e

        for key, diff in (deltas.items() if failure is None else ()):
            try:
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
                    resp = _submit_and_wait_bounded(tx, client, player)
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
                    resp = _submit_and_wait_bounded(tx, client, issuer)
            except Exception as e:  # noqa: BLE001 - routed into the pending split below
                failure = e
                break

            txid = resp.result.get("hash", "")
            if txid:
                confirmed_txids.append(txid)
            confirmed[key] = diff

            # Fold THIS resource's confirmation into the baseline —
            # before moving to the next resource (F-d178410b). Relocated
            # here from a single post-loop pass so a crash after this point
            # can lose at most the resource(s) not yet attempted, never
            # cause a later run to recompute and resubmit a Payment for one
            # that already cleared.
            bp.last_settled_supplies[key] = (
                bp.last_settled_supplies.get(key, 0) + diff
            )

            # Extend (or start) this batch's settlement receipt with THIS
            # key's confirmation — BEFORE persisting (F-9d7eb977). Building
            # the receipt once, in a single pass after the whole loop ends,
            # meant every per-key persist call above captured a baseline
            # already advanced for this key while ``bp.settlements`` still
            # had no record at all for it — a crash there durably persisted
            # "paid but not recorded," and the very next retry pass would
            # have no pending record to stop it from resubmitting. Building
            # the SAME settled_record incrementally — reusing the
            # ``confirmed``/``confirmed_txids`` objects by reference, so
            # later per-key appends to them are already visible here — means
            # this key's slice of the receipt is durable in the SAME
            # snapshot as its baseline advance, every time.
            if settled_record is None:
                settled_record = SettlementRecord(
                    day=state.day,
                    location=location,
                    deltas=confirmed,
                    txids=confirmed_txids,
                    status="settled",
                    # Matches the actual on-chain bytes (ledger-003): every
                    # Payment in this batch — confirmed or not — carries
                    # this same full-batch memo.
                    memo=_settlement_memo_text(state.run_id, state.day, deltas),
                    timestamp=datetime.now(UTC).isoformat(),
                )
                bp.settlements.append(settled_record)

            self._persist_state(state)

        if failure is None:
            bp.last_settlement_day = state.day

            # Settlement reached the ledger: clear the degraded signal
            # (ledger-B04) so the status line drops back to a healthy state.
            bp.last_settle_failed = False

            # Settlement-lifecycle log (ledger-B07): a successful settle records
            # the day, signed deltas, and txids so a later reconcile mismatch is
            # traceable from the log without re-running the chain.
            log.info(
                "settle ok: day=%d location=%s deltas=%s txids=%s",
                state.day, location, confirmed, confirmed_txids,
            )

            short_txid = (
                confirmed_txids[0][:12] + "..." if confirmed_txids else "none"
            )
            return SettlementResult(
                success=True,
                message=f"Checkpoint settled. Receipt: {short_txid}",
                txids=confirmed_txids,
                record=settled_record,
            )

        # Partial or total failure: only the resource(s) that did NOT clear
        # this pass are queued pending. The confirmed ones (if any) are
        # already settled above and must never be retried — that would
        # double-pay them on-chain.
        remaining = {k: v for k, v in deltas.items() if k not in confirmed}
        log.warning("Settlement failed at %s: %s", location, failure)
        pending_record = SettlementRecord(
            day=state.day,
            location=location,
            deltas=remaining,
            txids=[],
            status="pending",
            memo=_settlement_memo_text(state.run_id, state.day, remaining),
            timestamp=datetime.now(UTC).isoformat(),
        )
        bp.pending_settlements.append(pending_record)
        # Degraded signal (ledger-B04): the testnet was unreachable, this
        # checkpoint is unsettled. status_line + cli-tui render the offline
        # state from this so the pending count is not read as a healthy
        # backlog.
        bp.last_settle_failed = True
        log.info(
            "settle degraded: day=%d location=%s deltas=%s — "
            "queued as pending (testnet unreachable)",
            state.day, location, remaining,
        )

        return SettlementResult(
            success=False,
            message=(
                "The ledger is quiet. I couldn't settle this checkpoint. "
                "Your run continues offline for now. "
                "We'll retry at the next safe moment\u2014"
                "or you can settle manually from the Ledger menu."
            ),
            txids=confirmed_txids or None,
            record=pending_record,
        )

    def _retry_pending(self, state: RunState) -> None:
        """Retry all pending settlements. Move successful ones to settled.

        Per-resource progress WITHIN a single pending record is also tracked
        as each Payment confirms (ledger-CRIT-2): if a record holds 2+
        resources and one clears while a later one then fails on this same
        retry pass, only the resource(s) that did NOT clear stay pending (in
        a narrowed record) — the one(s) that did are folded into
        ``bp.settlements`` and the baseline immediately, so a later retry
        pass can never resubmit a Payment that already landed.

        That fold, and the queue narrowing itself, used to be purely
        in-memory, with the same caller-saves-later gap as ``settle()``
        (F-d178410b): a crash right after this method folded a confirmed key
        but before the caller's next save reverted BOTH the baseline and
        ``bp.pending_settlements`` back to their stale, wider shape — so the
        next retry pass resubmitted a Payment that had already cleared.

        With a ``persist`` hook, each key's fold, its removal from the live
        pending record, and its slice of the settlement receipt are all
        applied to memory BEFORE that key's persist call fires (F-9d7eb977) —
        not once per whole record after its inner loop ends. Sequencing
        persist any earlier let it write a snapshot with the baseline
        already advanced for a key while ``bp.pending_settlements`` still
        listed that same key as outstanding and ``bp.settlements`` had no
        record of it at all — exactly the ambiguous state a crash-recovery
        read cannot tell apart from "never attempted," so the next retry
        pass would resubmit a Payment that had already cleared on-chain.
        Without a ``persist`` hook (the default), behavior is unchanged from
        before.
        """
        bp = state.backpack
        if not bp.pending_settlements:
            return

        if not _HAS_XRPL:
            return

        # F-86a4c19c: skip unknown pending keys like accept_parcel so a
        # stale {gold: N} record cannot KeyError out of retry/settle.
        stripped = False
        for record in list(bp.pending_settlements):
            unknown = [k for k in record.deltas if k not in XRPL_TOKEN_MAP]
            if not unknown:
                continue
            for k in unknown:
                del record.deltas[k]
            stripped = True
            if not record.deltas:
                bp.pending_settlements = [
                    r for r in bp.pending_settlements if r is not record
                ]
        if stripped:
            self._persist_state(state)
        if not bp.pending_settlements:
            bp.last_settle_failed = False
            return

        try:
            client = self._get_client()
            player = Wallet.from_seed(bp.wallet_secret)
            issuer = Wallet.from_seed(bp.issuer_secret)
        except Exception as e:  # noqa: BLE001 - degrade; retry later
            log.warning("Retry settlement setup failed: %s", e)
            bp.last_settle_failed = True
            return

        # Snapshot to iterate; bp.pending_settlements itself is rebuilt
        # incrementally below (F-d178410b) as each record resolves, rather
        # than only once at the end of the whole pass.
        to_process = list(bp.pending_settlements)
        moved = 0  # settlements that cleared on this retry pass (ledger-B07)

        for record in to_process:
            # Snapshot the pre-narrowing deltas ONCE, before this record's
            # inner loop starts narrowing ``record.deltas`` in place below
            # (F-9d7eb977) — both the tx-level memo and the eventual
            # settled_record's memo must match the full set every Payment in
            # this pass actually carried on-chain, not whatever remains
            # after some keys have already been removed.
            original_deltas = dict(record.deltas)
            confirmed: dict[str, int] = {}
            confirmed_txids: list[str] = []
            failure: Exception | None = None
            settled_record: SettlementRecord | None = None
            try:
                memos = _build_memo(state.run_id, record.day, original_deltas)
            except Exception as e:  # noqa: BLE001 - abort this pass, keep pending
                log.warning(
                    "Retry settlement day %d failed: %s", record.day, e,
                )
                bp.last_settle_failed = True
                return

            for key, diff in original_deltas.items():
                try:
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
                        resp = _submit_and_wait_bounded(tx, client, player)
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
                        resp = _submit_and_wait_bounded(tx, client, issuer)
                except Exception as e:  # noqa: BLE001 - handled below, per key
                    failure = e
                    break

                txid = resp.result.get("hash", "")
                if txid:
                    confirmed_txids.append(txid)
                confirmed[key] = diff

                # Fold THIS key's confirmation into the baseline — same
                # rationale as settle() above (F-d178410b). Conservation fix
                # (ENG-A-08, extended by ledger-CRIT-2): a failed settle()
                # leaves the baseline un-advanced and enqueues this pending
                # record; now that this key is settled on-chain, fold its
                # signed delta into the baseline so the *next* fresh
                # settle() measures current against a baseline that already
                # accounts for it. Without this, settle() would recompute
                # (current - baseline) over the WHOLE interval — including
                # the just-retried delta — paying it on-chain twice and
                # double-summing it in reconcile(), breaking
                # 'minted + Σdeltas == final'.
                bp.last_settled_supplies[key] = (
                    bp.last_settled_supplies.get(key, 0) + diff
                )

                # Narrow the LIVE record — ``record`` is the SAME object
                # bp.pending_settlements still holds, so removing this key
                # here is visible there too, immediately — ledger-CRIT-2:
                # resubmitting an already-confirmed key on a later retry
                # would double-pay it on-chain.
                del record.deltas[key]

                # Extend (or start) this pass's settlement receipt with THIS
                # key — BEFORE persisting (F-9d7eb977), for the identical
                # reason settle() now does the same: a persist call that
                # fires before both of these updates can write a snapshot
                # with the baseline advanced, the key still listed pending,
                # and no settlement record to explain either — the exact
                # ambiguity a crash-recovery read cannot resolve.
                if settled_record is None:
                    settled_record = SettlementRecord(
                        day=record.day,
                        location=record.location,
                        deltas=confirmed,
                        txids=confirmed_txids,
                        status="settled",
                        # Match the on-chain memo bytes actually written
                        # above (ledger-003): every Payment resubmitted for
                        # this record carried the record's full
                        # (pre-narrowing) delta text.
                        memo=_settlement_memo_text(
                            state.run_id, record.day, original_deltas,
                        ),
                        timestamp=datetime.now(UTC).isoformat(),
                    )
                    bp.settlements.append(settled_record)

                self._persist_state(state)

            if settled_record is not None:
                moved += 1
                log.info(
                    "retry settled: day=%d deltas=%s txids=%s",
                    record.day, confirmed, confirmed_txids,
                )

            if failure is not None:
                log.warning(
                    "Retry settlement day %d failed: %s", record.day, failure,
                )
                # record.deltas was already narrowed incrementally above —
                # each confirmed key was removed from it the moment that key
                # cleared — so it already holds exactly what did NOT clear
                # this pass. Refresh the memo to match the narrowed shape.
                record.memo = _settlement_memo_text(
                    state.run_id, record.day, record.deltas,
                )
            else:
                # Fully confirmed: record.deltas is now empty (every key was
                # removed above as it cleared) — drop the emptied record
                # from the LIVE queue. This still runs strictly after every
                # per-key persist call above, so the persisted state right
                # after this record can never show it as still needing a
                # retry.
                bp.pending_settlements = [
                    r for r in bp.pending_settlements if r is not record
                ]

            self._persist_state(state)

        # Retry-pass lifecycle log + degraded signal (ledger-B04 / ledger-B07):
        # report how many cleared vs how many remain, and keep last_settle_failed
        # in sync — True while anything is still pending, cleared once the queue
        # drains (a fresh failing settle() re-sets it).
        log.info(
            "retry pending pass: moved=%d still_pending=%d",
            moved, len(bp.pending_settlements),
        )
        if bp.pending_settlements:
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

        # XRPL required for actual send (F-64e78470: same pip extra as enable)
        if not _HAS_XRPL:
            return SendResult(
                success=False,
                message=XRPL_EXTRA_MISSING_MSG,
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
            resp = _submit_and_wait_bounded(tx, client, player)
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

        Pagination (ledger-CRIT-4, mirrors ``fetch_onchain_memos``): AccountTx
        caps results per page, and a long run can exceed that on a single
        account (each town-checkpoint settlement can itself emit several
        Payment txs). Each response carries a ``marker`` when more
        transactions remain; we resubmit with that marker until the chain
        stops returning one (bounded by a page cap) so an older incoming
        parcel is never silently dropped off the end of page 1 — the same
        FALSE-NEGATIVE risk ``fetch_onchain_memos`` already guards against.
        """
        bp = state.backpack
        if not _HAS_XRPL or not bp.enabled or not bp.wallet_address:
            return []

        new_parcels: list[ParcelRecord] = []
        known_txids = {p.txid for p in bp.parcels if p.txid}
        max_pages = 100  # safety bound: mirrors fetch_onchain_memos

        try:
            client = self._get_client()
            marker = None
            pages = 0
            for _ in range(max_pages):
                resp = client.request(AccountTx(
                    account=bp.wallet_address,
                    limit=200,
                    marker=marker,
                ))
                pages += 1
                result = resp.result

                for tx_entry in result.get("transactions", []):
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

                    # tx-hash location varies by AccountTx api_version
                    # (ledger-007): api_version 2 puts `hash` on the wrapping
                    # entry alongside `tx_json`; the legacy shape nests it
                    # inside `tx`. Read the wrapper first, fall back to the
                    # inner object.
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
                    known_txids.add(tx_hash)

                # No marker → this account is fully paged.
                marker = result.get("marker")
                if not marker:
                    break

            log.info(
                "check_parcels: pages=%d new_parcels=%d", pages, len(new_parcels),
            )
            return new_parcels

        except Exception as e:
            # A page beyond the first may already have appended parcels to
            # bp.parcels before the request failed — report exactly what was
            # found so far rather than a misleading empty result.
            log.warning("Parcel check failed: %s", e)
            return new_parcels

    def accept_parcel(
        self, parcel: ParcelRecord, state: RunState,
        cap: int = PARCEL_ACCEPT_CAP,
    ) -> bool:
        """Accept a parcel: apply contents to supplies, capped.

        The per-supply cap defaults to ``PARCEL_ACCEPT_CAP`` (ledger-B09): a
        named design lever, not a bare literal, documenting why generosity is
        bounded — a parcel cannot trivialize the survival pressure.

        Clamped on BOTH ends (ledger-CRIT-1): ``_decode_parcel_memo`` now
        rejects a non-positive amount before a ParcelRecord is ever created
        from the receive path, but ``contents`` can also be populated
        directly (tests, saves, a future non-XRPL channel) — this is a
        second, independent floor so a negative content value can never
        reduce supplies, matching the cap's existing promise on the upper
        side.

        Resource key is validated here too (F-285e9fa6): ``_decode_parcel_memo``
        already rejects a supply key outside ``XRPL_TOKEN_MAP`` on the live
        on-chain path, but that guard lives at decode time, not at apply time
        — the same non-memo construction paths CRIT-1's fix above exists to
        cover (tests, saves, a future non-XRPL channel) could otherwise carry
        an arbitrary key straight into ``state.supplies``, injecting a
        permanent phantom resource the rest of the game (UI, XRPL mint/settle,
        save schema) never recognizes. An unrecognized key is silently
        skipped, exactly like a rejected memo — never partially applied.

        Idempotent (ledger-005): a second accept is a no-op so a double-trigger
        from any caller (the TUI path does not guard) cannot double the supplies
        and break conservation. Mirrors refuse_parcel's `accepted` guard.
        """
        if parcel.accepted:
            return False

        for key, amount in parcel.contents.items():
            if key not in XRPL_TOKEN_MAP:
                continue
            capped_amount = max(0, min(amount, cap))
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
        if not _HAS_XRPL:
            # F-64e78470: extra gone after an enabled save must not look
            # like an empty wallet or a network miss. Testnet extra, not
            # a wallet/mainnet issue. Overlay keys extra_missing so it
            # can name the pip extra instead of "Balances: unavailable".
            info["balances"] = {}
            info["balances_error"] = True
            info["extra_missing"] = True
        elif bp.enabled:
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
        if not _HAS_XRPL and bp.enabled:
            # F-64e78470: extra gone after an enabled save must not leave
            # "Ledger: ON". Name the pip extra; this is not a wallet miss
            # and not the B04 testnet-unreachable signal.
            return f"Ledger: extra missing — {XRPL_EXTRA_PIP}"
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
