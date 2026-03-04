"""Skilled bot simulation — 1000 runs, doctrine breakdown, acceptance check.

Usage: python scripts/sim_skilled.py [num_runs]
"""

from __future__ import annotations

import sys
from collections import Counter

sys.path.insert(0, ".")
from agents.skilled import choose_intent, reset_step_counter  # noqa: E402
from escape_the_valley.gm import GMConfig  # noqa: E402
from escape_the_valley.intent import GamePhase  # noqa: E402
from escape_the_valley.physics import journey_pressure  # noqa: E402
from escape_the_valley.step_engine import StepEngine  # noqa: E402
from escape_the_valley.worldgen import create_new_run  # noqa: E402


def simulate_run(seed: int, max_steps: int = 600) -> dict:
    """Run one headless game with the skilled heuristic bot."""
    reset_step_counter()
    state = create_new_run(seed=seed)
    engine = StepEngine(state, gm_config=GMConfig(enabled=False))

    hunts = 0

    for _ in range(max_steps):
        if engine.phase == GamePhase.GAME_OVER:
            break

        intent = choose_intent(state, engine)
        if intent.action.value == "HUNT":
            hunts += 1
        engine.step(intent)

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
        "hunts": hunts,
    }


def main() -> None:
    num_runs = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    results = []

    for i in range(num_runs):
        seed = 2000 + i
        result = simulate_run(seed)
        results.append(result)
        if (i + 1) % 100 == 0:
            print(f"  ... {i + 1}/{num_runs} complete")

    # ── Summary ──

    victories = sum(1 for r in results if r["victory"])
    deaths = [r for r in results if not r["victory"]]
    causes = Counter(r["cause"] for r in deaths)
    days = [r["day"] for r in deaths]

    print(f"\n{'=' * 60}")
    print(f"SKILLED BOT SIMULATION: {num_runs} runs")
    print(f"{'=' * 60}")
    win_rate = 100 * victories / num_runs
    print(f"Overall win rate:     {win_rate:.1f}% ({victories}/{num_runs})"
          f"  [target: 20-40%]")

    # ── By doctrine ──

    print("\nBy Doctrine:")
    doctrine_groups: dict[str, list[dict]] = {}
    for r in results:
        doc = r["doctrine"] or "none"
        doctrine_groups.setdefault(doc, []).append(r)

    doctrine_rates: dict[str, float] = {}
    for doc in sorted(doctrine_groups.keys()):
        group = doctrine_groups[doc]
        wins = sum(1 for r in group if r["victory"])
        rate = 100 * wins / len(group) if group else 0
        doctrine_rates[doc] = rate
        print(f"  {doc:18s} {rate:5.1f}% ({wins}/{len(group)} runs)")

    # ── Death stats ──

    if days:
        avg_day = sum(days) / len(days)
        print(f"\nAvg death day:       {avg_day:.1f}")
    else:
        avg_day = 0
        print("\nNo deaths!")

    print("\nDeath causes:")
    total_deaths = len(deaths) or 1
    for cause, count in causes.most_common():
        pct = 100 * count / total_deaths
        print(f"  {cause:25s} {pct:5.1f}% ({count})")

    # ── Diagnostics ──

    avg_bd = sum(r["breakdowns"] for r in results) / num_runs
    avg_ca = sum(r["caches_found"] for r in results) / num_runs
    avg_ev = sum(r["escape_valves"] for r in results) / num_runs
    avg_mw = sum(r["maintenance_windows"] for r in results) / num_runs
    avg_hu = sum(r["hunts"] for r in results) / num_runs

    print("\nDiagnostics (per run averages):")
    print(f"  Breakdowns:     {avg_bd:.1f}")
    print(f"  Caches found:   {avg_ca:.1f}")
    print(f"  Escape valves:  {avg_ev:.1f}")
    print(f"  Maintenance:    {avg_mw:.1f}")
    print(f"  Hunts:          {avg_hu:.1f}")

    # ── Acceptance check ──

    print(f"\n{'-' * 60}")
    passed = True

    # Win rate band
    if 20 <= win_rate <= 40:
        print(f"[PASS] Win rate {win_rate:.1f}% is within 20-40% band")
    else:
        print(f"[FAIL] Win rate {win_rate:.1f}% is outside 20-40% band")
        passed = False

    # Doctrine balance
    for doc, rate in doctrine_rates.items():
        if rate > 50:
            print(f"[FAIL] {doc} win rate {rate:.1f}% exceeds 50%")
            passed = False
        elif rate < 10 and len(doctrine_groups.get(doc, [])) >= 50:
            print(f"[FAIL] {doc} win rate {rate:.1f}% is below 10%")
            passed = False
        else:
            print(f"[PASS] {doc} win rate {rate:.1f}% is balanced")

    # Resource death check
    starve_dehydrate = causes.get("starvation", 0) + causes.get("dehydration", 0)
    sd_pct = 100 * starve_dehydrate / total_deaths if deaths else 0
    if sd_pct > 70:
        print(f"[WARN] Starvation+dehydration = {sd_pct:.0f}% of deaths (>70%)")
    else:
        print(f"[PASS] Starvation+dehydration = {sd_pct:.0f}% of deaths")

    print(f"{'-' * 60}")
    print(f"{'[PASS] All acceptance criteria met' if passed else '[FAIL] Some criteria not met'}")

    # ── Per-run detail table ──

    if num_runs <= 50:
        print(
            f"\n{'Seed':>6} {'Day':>4} {'Prog':>5} {'Alive':>5} "
            f"{'Food':>5} {'Water':>5} {'Wagon':>5} {'BD':>3} "
            f"{'MW':>3} {'CA':>3} {'HU':>3} Result"
        )
        print("-" * 85)
        for r in results:
            result_str = "WIN" if r["victory"] else r["cause"][:20]
            print(
                f"{r['seed']:>6} {r['day']:>4} {r['progress']:>5} "
                f"{r['alive']:>5} {r['food']:>5} {r['water']:>5} "
                f"{r['wagon']:>5} {r['breakdowns']:>3} "
                f"{r['maintenance_windows']:>3} {r['caches_found']:>3} "
                f"{r['hunts']:>3} {result_str}"
            )


if __name__ == "__main__":
    main()
