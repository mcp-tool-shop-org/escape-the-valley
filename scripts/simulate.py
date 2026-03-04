"""Headless simulation — run N games GM-off, collect survival statistics.

The bot plays a maintenance-aware survival strategy:
- Escape valves when desperate (hard ration, desperate repair, abandon cargo)
- Maintenance window: rest+repair combo for breakdown resistance
- Hunt when food is low and has ammo
- Rest when multiple party members are sick/injured
- Otherwise travel
- Always pick first choice for events/routes
"""

from __future__ import annotations

import sys
from collections import Counter

from escape_the_valley.gm import GMConfig
from escape_the_valley.intent import GamePhase, IntentAction, PlayerIntent
from escape_the_valley.models import Condition
from escape_the_valley.physics import (
    can_abandon_cargo,
    can_desperate_repair,
    can_hard_ration,
    journey_pressure,
)
from escape_the_valley.step_engine import StepEngine
from escape_the_valley.worldgen import create_new_run

TRAVEL = PlayerIntent(IntentAction.TRAVEL)
REST = PlayerIntent(IntentAction.REST)
HUNT = PlayerIntent(IntentAction.HUNT)
REPAIR = PlayerIntent(IntentAction.REPAIR)
ABANDON_CARGO = PlayerIntent(IntentAction.ABANDON_CARGO)
DESPERATE_REPAIR = PlayerIntent(IntentAction.DESPERATE_REPAIR)
HARD_RATION = PlayerIntent(IntentAction.HARD_RATION)
CHOOSE_A = PlayerIntent(IntentAction.CHOOSE, choice_id="A")


def _pick_action(state) -> PlayerIntent:
    """Maintenance-aware survival heuristic."""
    alive = state.party.alive_count
    if alive == 0:
        return TRAVEL

    # ── Escape valves first (desperate measures) ──

    if can_hard_ration(state):
        return HARD_RATION

    if can_desperate_repair(state):
        return DESPERATE_REPAIR

    if can_abandon_cargo(state):
        return ABANDON_CARGO

    # ── Maintenance window: rest+repair combo ──

    # If wagon needs attention and we have parts, try maintenance combo
    if state.wagon.condition < 60 and state.supplies.parts > 0:
        if state.last_action == "REST":
            return REPAIR  # Complete the maintenance window
        if state.last_action != "REPAIR":
            # Start maintenance: rest first, then repair next turn
            return REST

    # ── Standard actions ──

    # Repair if wagon is in bad shape and we have parts
    if state.wagon.condition < 50 and state.supplies.parts > 0:
        return REPAIR

    # Hunt if food is running low and we have ammo
    if state.supplies.food < alive * 4 and state.supplies.ammo > 0:
        return HUNT

    # Rest if multiple party members are hurt
    sick_or_hurt = sum(
        1 for m in state.party.members
        if m.is_alive() and m.condition in (Condition.SICK, Condition.INJURED)
    )
    if sick_or_hurt >= 2:
        return REST

    return TRAVEL


def simulate_run(seed: int, max_steps: int = 600) -> dict:
    """Run one headless game with simple survival heuristics."""
    state = create_new_run(seed=seed)
    engine = StepEngine(state, gm_config=GMConfig(enabled=False))

    for _ in range(max_steps):
        if engine.phase == GamePhase.GAME_OVER:
            break
        if engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
            engine.step(CHOOSE_A)
        else:
            engine.step(_pick_action(state))

    return {
        "seed": seed,
        "day": state.day,
        "victory": state.victory,
        "cause": state.cause_of_death or ("victory" if state.victory else "timeout"),
        "progress": round(journey_pressure(state), 2),
        "food": state.supplies.food,
        "water": state.supplies.water,
        "wagon": state.wagon.condition,
        "alive": state.party.alive_count,
        "doctrine": state.doctrine,
        # Diagnostics
        "breakdowns": engine.diagnostics["wagon_breakdowns"],
        "events_total": engine.diagnostics["events_total"],
        "events_high_sev": engine.diagnostics["events_high_sev"],
        "maintenance_windows": engine.diagnostics["maintenance_windows"],
        "caches_found": engine.diagnostics["caches_found"],
        "escape_valves": engine.diagnostics["escape_valves_used"],
    }


def main() -> None:
    num_runs = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    results = []

    for i in range(num_runs):
        seed = 1000 + i
        result = simulate_run(seed)
        results.append(result)

    # Summary
    victories = sum(1 for r in results if r["victory"])
    deaths = [r for r in results if not r["victory"]]
    causes = Counter(r["cause"] for r in deaths)
    days = [r["day"] for r in deaths]

    print(f"\n{'=' * 60}")
    print(f"SIMULATION: {num_runs} runs")
    print(f"{'=' * 60}")
    print(f"Victories: {victories}/{num_runs} ({100 * victories / num_runs:.0f}%)")
    print(f"Deaths: {len(deaths)}/{num_runs}")
    if days:
        print(
            f"Day of death: min={min(days)}, max={max(days)}, "
            f"avg={sum(days) / len(days):.1f}"
        )
    print("\nCauses of death:")
    for cause, count in causes.most_common():
        print(f"  {cause}: {count}")

    # Diagnostics aggregates
    avg_bd = sum(r["breakdowns"] for r in results) / num_runs
    avg_ev = sum(r["events_total"] for r in results) / num_runs
    avg_hi = sum(r["events_high_sev"] for r in results) / num_runs
    avg_mw = sum(r["maintenance_windows"] for r in results) / num_runs
    avg_ca = sum(r["caches_found"] for r in results) / num_runs
    avg_ev_u = sum(r["escape_valves"] for r in results) / num_runs

    print("\nDiagnostics (averages):")
    print(f"  Breakdowns/run:       {avg_bd:.1f}")
    print(f"  Events/run:           {avg_ev:.1f}")
    print(f"  High-sev events/run:  {avg_hi:.1f}")
    print(f"  Maintenance windows:  {avg_mw:.1f}")
    print(f"  Caches found:         {avg_ca:.1f}")
    print(f"  Escape valves used:   {avg_ev_u:.1f}")

    print(
        f"\n{'Seed':>6} {'Day':>4} {'Prog':>5} {'Alive':>5} "
        f"{'Food':>5} {'Water':>5} {'Wagon':>5} {'BD':>3} "
        f"{'MW':>3} {'CA':>3} Result"
    )
    print("-" * 80)
    for r in results:
        result = "WIN" if r["victory"] else r["cause"][:20]
        print(
            f"{r['seed']:>6} {r['day']:>4} {r['progress']:>5} "
            f"{r['alive']:>5} {r['food']:>5} {r['water']:>5} "
            f"{r['wagon']:>5} {r['breakdowns']:>3} "
            f"{r['maintenance_windows']:>3} {r['caches_found']:>3} "
            f"{result}"
        )


if __name__ == "__main__":
    main()
