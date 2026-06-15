"""Ledger Reconciliation Proof — prove the on-ledger backpack matches the engine.

Drives one deterministic run (GM off) with the XRPL Ledger Backpack enabled on
Testnet, letting the engine settle supply deltas at each town checkpoint, then
independently reconciles the ledger against the engine truth:

  - on-ledger balances (FOD/WTR/MED/AMO/PRT) == engine's settled supplies
  - minted_initial + sum(settlement deltas) == final settled supplies (conservation)
  - every settlement memo matches TRAIL|RUN:<id>|DAY:<n>

The ledger is an EXTERNAL VERIFIER: a different system family than the engine, so
the engine cannot fake it. A mismatch means a settlement bug or state tampering.
This is the "audit mode" the design roadmap promised: verify balances evolved per
game deltas, detect drift.

Standards compliance (workflow-standards.md):
  EXTERNAL_VERIFIER=3      the ledger (xrpl) verifies the engine; the engine's logic
                          is hidden from the ledger, which only sees signed txns.
  PIN_PER_STEP=3           seed + rng_counter + GM-off => byte-replayable run.
  ANDON_AUTHORITY=2        any per-resource mismatch fails the whole proof; no
                          green-washing a partial pass.
  NAMED_COMPENSATORS       testnet-only, throwaway faucet wallets, no mainnet path =>
                          no irreversible real-value action. Undo = discard wallets.
  DECOMPOSE_BY_SECRETS=2   pure reconcile() (no network) is split from the driver.
  UNCERTAINTY_GATED_HUMANS=2  the testnet spend is gated on an explicit go.

The pure reconcile() function has no network or xrpl dependency and is unit-tested
offline (tests/test_ledger_proof.py). Only run_proof() touches the network.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .backpack_models import XRPL_RESOURCES, XRPL_TOKEN_MAP, SettlementRecord

# ── Reconciliation result types (pure, no network) ───────────────────


@dataclass
class ResourceCheck:
    """Per-resource reconciliation of ledger vs engine."""

    resource: str          # game key, e.g. "food"
    code: str              # XRPL currency code, e.g. "FOD"
    minted: int            # amount minted on-ledger at enable()
    sum_deltas: int        # sum of settlement deltas for this resource
    engine_settled: int    # engine's last-settled snapshot
    ledger: int | None     # live on-ledger balance (None if absent)
    balance_ok: bool       # ledger == engine_settled
    conservation_ok: bool  # minted + sum_deltas == engine_settled

    @property
    def ok(self) -> bool:
        return self.balance_ok and self.conservation_ok


@dataclass
class ReconcileReport:
    """Full reconciliation verdict for one proof run."""

    run_id: str
    seed: int
    player_address: str
    issuer_address: str
    settlements_count: int
    pending_count: int
    txids: list[str]
    resources: list[ResourceCheck]
    memo_ok: bool
    passed: bool
    notes: list[str] = field(default_factory=list)


def reconcile(
    *,
    run_id: str,
    seed: int,
    minted_initial: dict[str, int],
    ledger_balances: dict[str, int],
    last_settled_supplies: dict[str, int],
    settlements: list[SettlementRecord],
    pending: list[SettlementRecord],
    player_address: str = "",
    issuer_address: str = "",
) -> ReconcileReport:
    """Reconcile on-ledger balances against the engine's settled snapshot.

    Pure function — no network, no xrpl import. ``ledger_balances`` is keyed by
    XRPL currency code (FOD/WTR/...); engine dicts are keyed by game key
    (food/water/...).
    """
    notes: list[str] = []

    # Sum settlement deltas per resource (telescopes to final - minted).
    sum_deltas: dict[str, int] = dict.fromkeys(XRPL_RESOURCES, 0)
    for rec in settlements:
        for key, val in rec.deltas.items():
            if key in sum_deltas:
                sum_deltas[key] += val

    resources: list[ResourceCheck] = []
    for key in sorted(XRPL_RESOURCES):
        code = XRPL_TOKEN_MAP[key][0]
        minted = int(minted_initial.get(key, 0))
        settled = int(last_settled_supplies.get(key, 0))
        sdelta = sum_deltas[key]
        raw_ledger = ledger_balances.get(code)
        ledger = None if raw_ledger is None else int(raw_ledger)

        balance_ok = ledger is not None and ledger == settled
        conservation_ok = (minted + sdelta) == settled

        if ledger is None:
            notes.append(f"{key}: no on-ledger balance for {code}")
        elif not balance_ok:
            notes.append(
                f"{key}: ledger {ledger} != engine settled {settled} ({code})"
            )
        if not conservation_ok:
            notes.append(
                f"{key}: minted {minted} + deltas {sdelta} "
                f"({minted + sdelta}) != settled {settled}"
            )

        resources.append(ResourceCheck(
            resource=key, code=code, minted=minted, sum_deltas=sdelta,
            engine_settled=settled, ledger=ledger,
            balance_ok=balance_ok, conservation_ok=conservation_ok,
        ))

    # Memo integrity: each record memo must name this run and its own day.
    memo_ok = True
    for rec in settlements:
        expected = f"TRAIL|RUN:{run_id}|DAY:{rec.day}"
        if (rec.memo or "") != expected:
            memo_ok = False
            notes.append(f"settlement day {rec.day}: memo {rec.memo!r} != {expected!r}")

    if pending:
        notes.append(f"{len(pending)} settlement(s) still pending (unsettled on ledger)")

    txids: list[str] = []
    for rec in settlements:
        txids.extend(rec.txids)

    passed = all(r.ok for r in resources) and memo_ok and not pending

    return ReconcileReport(
        run_id=run_id,
        seed=seed,
        player_address=player_address,
        issuer_address=issuer_address,
        settlements_count=len(settlements),
        pending_count=len(pending),
        txids=txids,
        resources=resources,
        memo_ok=memo_ok,
        passed=passed,
        notes=notes,
    )


# ── Serialization ────────────────────────────────────────────────────


def report_to_dict(report: ReconcileReport) -> dict:
    """Serialize a report to a JSON-safe dict (no secrets)."""
    return {
        "run_id": report.run_id,
        "seed": report.seed,
        "player_address": report.player_address,
        "issuer_address": report.issuer_address,
        "settlements_count": report.settlements_count,
        "pending_count": report.pending_count,
        "txids": report.txids,
        "memo_ok": report.memo_ok,
        "passed": report.passed,
        "notes": report.notes,
        "resources": [
            {
                "resource": r.resource,
                "code": r.code,
                "minted": r.minted,
                "sum_deltas": r.sum_deltas,
                "engine_settled": r.engine_settled,
                "ledger": r.ledger,
                "balance_ok": r.balance_ok,
                "conservation_ok": r.conservation_ok,
            }
            for r in report.resources
        ],
    }


def report_to_markdown(report: ReconcileReport) -> str:
    """Render a human-readable reconciliation report."""
    verdict = "PASS" if report.passed else "FAIL"
    lines = [
        f"# Ledger Reconciliation Proof — {verdict}",
        "",
        f"- **Run:** `{report.run_id}`  (seed `{report.seed}`)",
        f"- **Player:** `{report.player_address}`",
        f"- **Issuer (Trail Authority):** `{report.issuer_address}`",
        f"- **Settlements:** {report.settlements_count}  "
        f"(**pending:** {report.pending_count})",
        f"- **Memo integrity:** {'ok' if report.memo_ok else 'FAILED'}",
        "",
        "| Resource | Code | Minted | Σ Deltas | Engine | Ledger | Balance | Conserv. |",
        "|----------|------|-------:|---------:|-------:|-------:|:-------:|:--------:|",
    ]
    for r in report.resources:
        ledger = "—" if r.ledger is None else str(r.ledger)
        lines.append(
            f"| {r.resource} | {r.code} | {r.minted} | {r.sum_deltas:+d} | "
            f"{r.engine_settled} | {ledger} | "
            f"{'✓' if r.balance_ok else '✗'} | "
            f"{'✓' if r.conservation_ok else '✗'} |"
        )
    lines.append("")
    if report.notes:
        lines.append("## Notes")
        for note in report.notes:
            lines.append(f"- {note}")
        lines.append("")
    if report.txids:
        lines.append("## Settlement receipts (txids)")
        for txid in report.txids[:12]:
            lines.append(f"- `{txid}`")
        if len(report.txids) > 12:
            lines.append(f"- … +{len(report.txids) - 12} more")
        lines.append("")
    lines.append(
        "> The ledger is an external verifier: a different system family than the "
        "engine. A PASS means on-ledger token balances independently confirm the "
        "engine's economy deltas — the engine cannot fake the ledger."
    )
    return "\n".join(lines)


# ── Network driver (only this part touches the chain) ────────────────


def _pick_action(state):
    """Maintenance-aware survival heuristic (mirrors scripts/simulate.py)."""
    from .intent import IntentAction, PlayerIntent
    from .models import Condition
    from .physics import can_abandon_cargo, can_desperate_repair, can_hard_ration

    travel = PlayerIntent(IntentAction.TRAVEL)
    if state.party.alive_count == 0:
        return travel

    if can_hard_ration(state):
        return PlayerIntent(IntentAction.HARD_RATION)
    if can_desperate_repair(state):
        return PlayerIntent(IntentAction.DESPERATE_REPAIR)
    if can_abandon_cargo(state):
        return PlayerIntent(IntentAction.ABANDON_CARGO)

    # Maintenance window: rest then repair.
    if state.wagon.condition < 60 and state.supplies.parts > 0:
        if state.last_action == "REST":
            return PlayerIntent(IntentAction.REPAIR)
        if state.last_action != "REPAIR":
            return PlayerIntent(IntentAction.REST)

    if state.wagon.condition < 50 and state.supplies.parts > 0:
        return PlayerIntent(IntentAction.REPAIR)
    if state.supplies.food < state.party.alive_count * 4 and state.supplies.ammo > 0:
        return PlayerIntent(IntentAction.HUNT)

    sick_or_hurt = sum(
        1 for m in state.party.members
        if m.is_alive() and m.condition in (Condition.SICK, Condition.INJURED)
    )
    if sick_or_hurt >= 2:
        return PlayerIntent(IntentAction.REST)

    return travel


def run_proof(
    seed: int,
    *,
    max_steps: int = 600,
    isolate_save: bool = True,
    enable_attempts: int = 3,
    on_progress=None,
) -> ReconcileReport:
    """Drive one testnet run with the backpack on, then reconcile.

    Requires xrpl-py (the ``xrpl`` extra) and Testnet reachability. Uses
    throwaway faucet wallets — testnet only, no mainnet, no real value.
    """
    from .backpack import BackpackManager
    from .gm import GMConfig
    from .intent import GamePhase, IntentAction, PlayerIntent
    from .step_engine import StepEngine
    from .worldgen import create_new_run

    # The proof does not need autosave; keep it from clobbering the user's save.
    if isolate_save:
        from . import step_engine as _se
        _se.save_game = lambda *a, **k: None

    mgr = BackpackManager()
    if not mgr.available:
        raise RuntimeError(
            "xrpl-py is not installed. Install with: pip install escape-the-valley[xrpl]"
        )

    state = create_new_run(seed=seed)

    enable_res = None
    for _ in range(max(1, enable_attempts)):
        enable_res = mgr.enable(state)
        if enable_res.success:
            break
    if not enable_res or not enable_res.success:
        msg = enable_res.message if enable_res else "unknown error"
        raise RuntimeError(f"could not enable Ledger Backpack: {msg}")

    minted_initial = dict(state.backpack.last_settled_supplies)

    choose_a = PlayerIntent(IntentAction.CHOOSE, choice_id="A")
    engine = StepEngine(state, gm_config=GMConfig(enabled=False))
    for _ in range(max_steps):
        if engine.phase == GamePhase.GAME_OVER:
            break
        if engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
            engine.step(choose_a)
        else:
            engine.step(_pick_action(state))
        if on_progress is not None:
            on_progress(state)

    # Flush any deltas accumulated since the last town checkpoint so the ledger
    # reflects the true final supplies. Retries pending settlements first.
    mgr.settle(state, "final-reconcile")

    info = mgr.wallet_info(state)
    ledger_balances = info.get("balances", {})
    mgr.close()

    return reconcile(
        run_id=state.run_id,
        seed=seed,
        minted_initial=minted_initial,
        ledger_balances=ledger_balances,
        last_settled_supplies=state.backpack.last_settled_supplies,
        settlements=state.backpack.settlements,
        pending=state.backpack.pending_settlements,
        player_address=state.backpack.wallet_address,
        issuer_address=state.backpack.issuer_address,
    )
