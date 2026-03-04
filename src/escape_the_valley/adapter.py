"""Adapter: RunState + StepEngine -> FrameState for the TUI.

The UI never sees RunState directly. This module pre-renders all
strings so the TUI just displays them.
"""

from __future__ import annotations

from .intent import GamePhase
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

    # Supplies
    supplies = {
        "FOOD": s.supplies.food,
        "WATR": s.supplies.water,
        "MEDS": s.supplies.meds,
        "AMMO": s.supplies.ammo,
        "PART": s.supplies.parts,
    }

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

    # Warnings
    warnings = []
    if s.supplies.food < 10:
        warnings.append(f"Low FOOD ({s.supplies.food})")
    if s.supplies.water < 10:
        warnings.append(f"Low WATR ({s.supplies.water})")
    if s.supplies.meds == 0:
        warnings.append("No MEDS remaining")
    if s.supplies.parts == 0:
        warnings.append("No PART remaining")
    if s.wagon.condition < 30:
        warnings.append(
            f"Wagon critical ({s.wagon.condition}%)"
        )
    for m in s.party.members:
        if m.is_alive() and m.condition.value == "sick":
            warnings.append(f"{m.name} is sick")

    # Choices — depends on phase
    prompt_title, prompt_text, choices = _build_prompt(engine)

    # Journal
    journal = []
    for j in s.journal[-20:]:
        journal.append(
            f"Day {j.day} \u2014 {j.scene_title}: {j.choice_made}"
        )

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
        if engine.state.victory:
            return ("Victory!", "You escaped the valley.", [])
        return (
            "Game Over",
            engine.state.cause_of_death or "The journey ends.",
            [],
        )

    # CAMP — standard actions
    return (
        "Camp",
        "What will you do?",
        [
            Choice("A", "Travel", "Consumes supplies", "Food/Water"),
            Choice("B", "Rest", "Heals party", "Food/Water"),
            Choice("C", "Hunt", "Needs ammo", "Ammo"),
            Choice("D", "Repair", "Needs parts", "Parts"),
        ],
    )


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
