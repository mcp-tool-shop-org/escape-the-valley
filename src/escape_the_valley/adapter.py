"""Adapter: RunState + StepEngine -> FrameState for the TUI.

The UI never sees RunState directly. This module pre-renders all
strings so the TUI just displays them.
"""

from __future__ import annotations

from .intent import GamePhase
from .ledger import build_trail_ledger, build_xrpl_postcard
from .physics import can_abandon_cargo, can_desperate_repair, can_hard_ration
from .resources import RESOURCE_CATALOG, ResourceCategory
from .step_engine import StepEngine
from .tui_app import Choice, FrameState


def state_to_frame(engine: StepEngine) -> FrameState:
    """Convert engine state + last step messages into a FrameState."""
    s = engine.state
    msgs = engine.msgs

    # Find current + destination nodes
    cur_node = _node_by_id(s, s.location_id)
    dest_node = _node_by_id(s, s.destination_id)

    location = cur_node.name if cur_node else "Unknown"
    next_stop = dest_node.name if dest_node else "Unknown"
    biome = cur_node.biome.value.title() if cur_node else "?"
    weather = s.time_of_day.value.title()

    # Wagon summary
    wagon = (
        f"Wagon: {s.wagon.condition}% \u2022 "
        f"Animals: {s.wagon.animals_health}%"
    )

    # Party summary
    alive = s.party.alive_count
    sick = sum(
        1 for m in s.party.members
        if m.is_alive() and m.condition.value == "sick"
    )
    injured = sum(
        1 for m in s.party.members
        if m.is_alive() and m.condition.value == "injured"
    )
    party_summary = (
        f"Party: {alive} \u2022 Sick: {sick} \u2022 Injured: {injured}"
    )

    # Supplies — grouped by category from catalog
    supplies = _build_supplies(s)

    # Route ASCII
    route_ascii = _build_route_ascii(s, cur_node, dest_node)

    # Narration — combine step messages
    narration = "\n".join(msgs.lines) if msgs.lines else ""
    if msgs.event_narration:
        if narration:
            narration += "\n\n"
        narration += msgs.event_narration
    if msgs.outcome_narration and engine.phase != GamePhase.EVENT:
        if narration:
            narration += "\n\n"
        narration += f"**{msgs.outcome_title}:** {msgs.outcome_narration}"

    if not narration:
        narration = _idle_narration(s, cur_node)

    # Party detail
    party_detail = []
    for m in s.party.members:
        if m.is_alive():
            traits = ", ".join(t.value for t in m.traits)
            cond = m.condition.value
            party_detail.append(
                f"{m.name} \u2014 {m.health}% ({cond})"
                + (f" [{traits}]" if traits else "")
            )
        else:
            party_detail.append(f"{m.name} \u2014 dead")

    # Warnings — driven by ResourceDef.warning_low
    warnings = _build_warnings(s)

    # Choices — depends on phase
    prompt_title, prompt_text, choices = _build_prompt(engine)

    # Journal
    journal = []
    for j in s.journal[-20:]:
        journal.append(
            f"Day {j.day} \u2014 {j.scene_title}: {j.choice_made}"
        )

    # Backpack status line
    from .backpack import BackpackManager

    bp_mgr = BackpackManager()
    backpack_status = bp_mgr.status_line(s)

    return FrameState(
        day=s.day,
        location=location,
        next_stop=next_stop,
        weather=weather,
        biome=biome,
        pace=s.wagon.pace.value.title(),
        wagon=wagon,
        party_summary=party_summary,
        supplies=supplies,
        route_ascii=route_ascii,
        narration=narration,
        party_detail=party_detail,
        warnings=warnings,
        prompt_title=prompt_title,
        prompt_text=prompt_text,
        choices=choices,
        journal=journal,
        backpack_status=backpack_status,
    )


def _build_prompt(engine: StepEngine):
    """Build prompt title, text, and choices based on game phase."""
    if engine.phase == GamePhase.EVENT:
        choices = [
            Choice(
                id=c.id,
                label=c.label,
                risk_hint=c.risk_hint,
                cost_hint=c.cost_hint,
            )
            for c in engine.msgs.event_choices
        ]
        return (
            engine.msgs.event_title or "Event",
            "Choose wisely.",
            choices,
        )

    if engine.phase == GamePhase.ROUTE:
        choices = [
            Choice(
                id=chr(65 + i),  # A, B, C, D
                label=f"{r.name} ({r.distance} mi)",
                risk_hint="",
                cost_hint="",
            )
            for i, r in enumerate(engine.msgs.route_options)
        ]
        return ("Fork in the road", "Which way?", choices)

    if engine.phase == GamePhase.GAME_OVER:
        if engine.state.backpack.enabled and engine.state.backpack.settlements:
            ledger = build_xrpl_postcard(engine.state)
        else:
            ledger = build_trail_ledger(engine.state)
        title = "Victory!" if engine.state.victory else "Game Over"
        return (title, "\n".join(ledger), [])

    # CAMP — standard actions + conditional escape valves
    choices = [
        Choice("A", "Travel", "Consumes supplies", "Food/Water"),
        Choice("B", "Rest", "Heals party", "Food/Water"),
        Choice("C", "Hunt", "Needs ammo", "Ammo"),
        Choice("D", "Repair", "Needs parts", "Parts"),
    ]

    s = engine.state
    valve_key = ord("E")  # E, F, G
    if can_abandon_cargo(s):
        choices.append(Choice(
            chr(valve_key), "Abandon Cargo",
            "Wagon badly damaged", "Drops cargo, +25 wagon, -morale",
        ))
        valve_key += 1
    if can_desperate_repair(s):
        choices.append(Choice(
            chr(valve_key), "Desperate Repair",
            "No parts, wagon failing", "50% success, risk injury",
        ))
        valve_key += 1
    if can_hard_ration(s):
        choices.append(Choice(
            chr(valve_key), "Hard Ration",
            "Food critical", "Half rations 2 days, -morale, -health",
        ))

    return ("Camp", "What will you do?", choices)


def _build_route_ascii(s, cur_node, dest_node) -> str:
    """Simple ASCII route showing current position + next stops."""
    lines = ["  [You]", "    |"]

    if cur_node:
        lines.append(f" {cur_node.name}")
    else:
        lines.append(" ???")

    if dest_node and s.distance_remaining > 0:
        lines.append("    |")
        lines.append(
            f"  {dest_node.name} ({s.distance_remaining} mi)"
        )

        # Show next node after destination
        if dest_node.connections:
            next_id = dest_node.connections[0]
            next_node = _node_by_id(s, next_id)
            if next_node:
                lines.append("    |")
                lines.append(f"  {next_node.name}")

            # Show fork if multiple connections
            if len(dest_node.connections) > 1:
                fork_names = []
                for cid in dest_node.connections[1:]:
                    fn = _node_by_id(s, cid)
                    if fn:
                        fork_names.append(fn.name)
                if fork_names:
                    lines.append("   / \\")
                    lines.append(
                        "  "
                        + "   ".join(fork_names[:2])
                    )
    elif cur_node and cur_node.connections:
        # At a node, show connections
        for cid in cur_node.connections:
            cn = _node_by_id(s, cid)
            dist = cur_node.distance_to.get(cid, "?")
            if cn:
                lines.append("    |")
                lines.append(f"  {cn.name} ({dist} mi)")

    return "\n".join(lines)


def _idle_narration(s, cur_node) -> str:
    """Default narration when nothing just happened."""
    location = cur_node.name if cur_node else "the trail"
    biome = cur_node.biome.value if cur_node else "wilderness"
    return (
        f"Day {s.day}, {s.time_of_day.value}. "
        f"The party rests at {location}. "
        f"The {biome} stretches ahead."
    )


def _node_by_id(state, node_id):
    for node in state.map_nodes:
        if node.node_id == node_id:
            return node
    return None


def _build_supplies(s) -> dict[str, int]:
    """Build supplies dict grouped by category from resource catalog."""
    result: dict[str, int] = {}
    # Consumables first, then gear
    for cat in (ResourceCategory.CONSUMABLE, ResourceCategory.GEAR):
        for rdef in RESOURCE_CATALOG.values():
            if rdef.category == cat:
                result[rdef.display] = s.supplies.get(rdef.key)
    return result


def _build_warnings(s) -> list[str]:
    """Generate warnings from resource catalog thresholds + game state."""
    warnings: list[str] = []
    cliff_keys: set[str] = set()  # skip standard warning if cliff-edge fired
    alive = s.party.alive_count

    # ── Cliff-edge warnings (last-safe-moment) ──
    if alive > 0:
        if 0 < s.supplies.food <= alive * 2:
            warnings.append(
                "Food for one day. After that, the hunger starts."
            )
            cliff_keys.add("food")
        if 0 < s.supplies.water <= alive * 2:
            warnings.append(
                "Water for one day. The valley does not forgive thirst."
            )
            cliff_keys.add("water")

    if 0 < s.wagon.condition <= 15:
        if s.supplies.parts <= 0:
            warnings.append(
                "Wagon barely holds. One more break and you walk."
            )
        else:
            warnings.append("Wagon on its last legs.")
        cliff_keys.add("_wagon")

    if s.supplies.parts == 0 and s.wagon.condition < 50:
        warnings.append(
            "No spare parts. Next breakdown could end everything."
        )
        cliff_keys.add("parts")

    # ── Standard warnings (skipped in minimal mode) ──
    if getattr(s, "callout_level", "verbose") != "minimal":
        for rdef in RESOURCE_CATALOG.values():
            if rdef.key in cliff_keys:
                continue
            val = s.supplies.get(rdef.key)
            if rdef.warning_low > 0 and val <= rdef.warning_low:
                if val == 0:
                    warnings.append(f"No {rdef.display} remaining")
                else:
                    warnings.append(f"Low {rdef.display} ({val})")
        if "_wagon" not in cliff_keys and s.wagon.condition < 30:
            warnings.append(f"Wagon critical ({s.wagon.condition}%)")
        for m in s.party.members:
            if m.is_alive() and m.condition.value == "sick":
                warnings.append(f"{m.name} is sick")
    return warnings
