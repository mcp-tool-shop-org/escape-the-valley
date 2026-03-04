"""Game engine — main loop, turn processing, event orchestration."""

from __future__ import annotations

from .events import (
    EventOutcome,
    apply_outcome,
    build_event_library,
    resolve_event,
    select_event,
)
from .gm import GMClient, GMConfig
from .models import (
    JournalEntry,
    RunState,
    SeededRNG,
    TimeOfDay,
)
from .physics import (
    apply_breakdown,
    attempt_hunt,
    attempt_repair,
    check_breakdown,
    check_game_over,
    check_health_effects,
    compute_daily_consumption,
    compute_travel_distance,
    rest_day,
    update_morale,
)
from .save import save_game
from .ui import (
    show_action_menu,
    show_event_scene,
    show_game_over,
    show_journal,
    show_message,
    show_outcome,
    show_pace_menu,
    show_route_choice,
    show_status,
)
from .worldgen import generate_weather


class GameEngine:
    """Main game engine that orchestrates turns, events, and GM interaction."""

    def __init__(self, state: RunState, gm_config: GMConfig | None = None):
        self.state = state
        self.rng = SeededRNG(state.seed, state.rng_counter)
        self.event_library = build_event_library()
        self.gm = GMClient(gm_config)

    def run(self) -> None:
        """Main game loop."""
        show_status(self.state)

        while not self.state.game_over:
            action = show_action_menu()

            if action == "1":
                self._do_travel()
            elif action == "2":
                self._do_rest()
            elif action == "3":
                self._do_hunt()
            elif action == "4":
                self._do_repair()
            elif action == "5":
                show_status(self.state)
            elif action == "6":
                self._do_change_pace()
            elif action == "7":
                show_journal(self.state.journal)
            elif action == "Q":
                self._save()
                show_message("Game saved.", "bold green")
                return

            # Check game over
            result = check_game_over(self.state)
            if result:
                if result == "VICTORY":
                    self.state.victory = True
                    self.state.game_over = True
                else:
                    self.state.cause_of_death = result
                    self.state.game_over = True

            # Autosave after each action
            self._save()

        show_game_over(self.state)
        self._save()

    def _do_travel(self) -> None:
        """Process a travel action — move, consume, events, advance time."""
        distance = compute_travel_distance(self.state)

        # Check for route choice at branching nodes
        self._check_route_choice()

        # Move
        self.state.distance_remaining -= distance
        self.state.distance_traveled += distance

        # Consume supplies
        consumption = compute_daily_consumption(self.state)
        self.state.supplies.apply_delta(consumption)

        # Check for arrival
        if self.state.distance_remaining <= 0:
            self._arrive_at_next_node()
        else:
            show_message(
                f"Traveled {distance} miles. {self.state.distance_remaining} miles remaining.",
                "bold",
            )

        # Advance time
        self._advance_time()

        # Check for breakdown
        breakdown = check_breakdown(self.state, self.rng)
        if breakdown:
            damage = breakdown["wagon_damage"]
            deltas = apply_breakdown(self.state, damage)
            if deltas:
                show_message(
                    f"Wagon breakdown! Repaired. Damage: {damage}",
                    "yellow",
                )
            else:
                show_message(f"Wagon breakdown! No parts for repair. Damage: {damage}", "red bold")

        # Health effects
        effects = check_health_effects(self.state, self.rng)
        for eff in effects:
            if eff["type"] == "died":
                show_message(f"{eff['member']} has died.", "red bold")
            elif eff["type"] == "fell_sick":
                show_message(f"{eff['member']} has fallen ill.", "yellow")
            elif eff["type"] == "healed":
                show_message(f"{eff['member']} was treated and is recovering.", "green")

        # Random event
        self._trigger_event()

        # Morale update
        update_morale(self.state)

        # Show status after travel
        show_status(self.state)

    def _do_rest(self) -> None:
        """Process a rest action."""
        consumption = compute_daily_consumption(self.state)
        self.state.supplies.apply_delta(consumption)

        effects = rest_day(self.state, self.rng)
        self._advance_time()

        show_message("The party rests for a day.", "bold")
        for eff in effects:
            if eff["type"] == "recovered":
                show_message(f"  {eff['member']} has recovered.", "green")

        update_morale(self.state)
        show_status(self.state)

    def _do_hunt(self) -> None:
        """Process a hunt action."""
        if self.state.supplies.ammo <= 0:
            show_message("No ammunition for hunting.", "red")
            return

        deltas = attempt_hunt(self.state, self.rng)
        self.state.supplies.apply_delta(deltas)

        if deltas.get("food", 0) > 0:
            show_message(f"Hunt successful! Gained {deltas['food']} food. Used 1 ammo.", "green")
        else:
            show_message("The hunt yielded nothing. 1 ammo spent.", "yellow")

        # Half-day action — partial consumption
        half_consumption = {k: v // 2 for k, v in compute_daily_consumption(self.state).items()}
        self.state.supplies.apply_delta(half_consumption)

        update_morale(self.state)
        show_status(self.state)

    def _do_repair(self) -> None:
        """Process a repair action."""
        if self.state.supplies.parts <= 0:
            show_message("No parts for repairs.", "red")
            return

        if self.state.wagon.condition >= 90:
            show_message("Wagon is in good condition. No repair needed.", "dim")
            return

        deltas = attempt_repair(self.state)
        self.state.supplies.apply_delta(deltas)
        show_message(
            f"Wagon repaired. Condition: {self.state.wagon.condition}/100. Used 1 part.",
            "green",
        )

        # Half-day action
        half_consumption = {k: v // 2 for k, v in compute_daily_consumption(self.state).items()}
        self.state.supplies.apply_delta(half_consumption)

        show_status(self.state)

    def _do_change_pace(self) -> None:
        """Change travel pace."""
        new_pace = show_pace_menu(self.state.wagon.pace)
        self.state.wagon.pace = new_pace
        show_message(f"Pace set to {new_pace.value}.", "bold")

    def _trigger_event(self) -> None:
        """Select and run a random event."""
        # ~60% chance of event per travel action
        if self.rng.random() > 0.6:
            return

        event = select_event(self.state, self.rng, self.event_library)
        node = _find_node(self.state)
        weather = generate_weather(self.rng, node.biome, self.state.day) if node else None

        # Try GM narration first
        scene = None
        if self.gm.config.enabled:
            weather_str = weather.value if weather else "unknown"
            scene = self.gm.generate_scene(self.state, event, weather_str)

        # Use GM scene or fallback
        if scene and scene.choices:
            title = scene.title or event.title
            narration = scene.narration or event.fallback_narration
            choices = scene.choices
        else:
            title = event.title
            narration = event.fallback_narration
            choices = [
                {
                    "id": c.choice_id,
                    "label": c.label,
                    "risk_hint": c.risk_hint,
                    "cost_hint": c.cost_hint,
                }
                for c in event.fallback_choices
            ]

        if not choices:
            return

        # Show scene and get player choice
        choice_id = show_event_scene(title, narration, choices)

        # Resolve outcome
        outcome = resolve_event(self.state, event, choice_id, self.rng)
        apply_outcome(self.state, outcome)

        # Get choice label
        choice_label = ""
        for c in choices:
            if c.get("id") == choice_id:
                choice_label = c.get("label", "")
                break

        # Try GM outcome narration
        outcome_narration = ""
        outcome_title = ""
        callout = ""

        if self.gm.config.enabled and scene:
            outcome_facts = {
                "Supplies delta": outcome.supplies_delta,
                "Health effects": outcome.health_delta,
                "Wagon effects": outcome.wagon_delta,
                "Morale effects": outcome.morale_delta,
                "Time cost": outcome.time_cost,
                "Special": outcome.special_flags,
            }
            gm_outcome = self.gm.generate_outcome(
                self.state, event, title, choice_id, choice_label, outcome_facts
            )
            if gm_outcome:
                outcome_narration = gm_outcome.outcome_narration
                outcome_title = gm_outcome.outcome_title
                callout = gm_outcome.callout

        if not outcome_narration:
            outcome_title = "Outcome"
            callout = _build_fallback_callout(outcome)
            outcome_narration = callout

        show_outcome(outcome_title, outcome_narration, callout, outcome.supplies_delta)

        # Journal entry
        self.state.journal.append(JournalEntry(
            day=self.state.day,
            location=node.name if node else "unknown",
            event_id=event.event_id,
            scene_title=title,
            narration=narration[:300],
            choice_made=f"{choice_id}: {choice_label}",
            outcome=outcome_narration[:300],
            deltas=outcome.supplies_delta,
            tags=event.tags,
        ))

        # Update RNG counter for determinism
        self.state.rng_counter = self.rng.counter

    def _arrive_at_next_node(self) -> None:
        """Handle arrival at the next node."""
        # Find the destination node
        dest_node = None
        for node in self.state.map_nodes:
            if node.node_id == self.state.destination_id:
                dest_node = node
                break

        if dest_node:
            self.state.location_id = dest_node.node_id
            self.state.distance_remaining = 0
            show_message(f"Arrived at {dest_node.name}!", "bold green")

            if dest_node.is_town:
                show_message("  This is a settlement. You may find supplies or trade.", "dim")

            # Set up next destination
            if dest_node.connections:
                if len(dest_node.connections) == 1:
                    next_id = dest_node.connections[0]
                    self.state.destination_id = next_id
                    self.state.distance_remaining = dest_node.distance_to.get(next_id, 15)
                else:
                    # Route choice handled next travel action
                    pass

    def _check_route_choice(self) -> None:
        """Check if the player needs to choose a route."""
        node = _find_node(self.state)
        if not node or len(node.connections) <= 1:
            return

        if self.state.distance_remaining > 0:
            return  # Not at a junction yet

        # Build connection info
        connections = []
        for conn_id in node.connections:
            for n in self.state.map_nodes:
                if n.node_id == conn_id:
                    dist = node.distance_to.get(conn_id, 15)
                    connections.append((conn_id, n.name, dist))
                    break

        if len(connections) > 1:
            chosen_id = show_route_choice(connections)
            self.state.destination_id = chosen_id
            self.state.distance_remaining = node.distance_to.get(chosen_id, 15)

    def _advance_time(self) -> None:
        """Advance time of day and day counter."""
        times = [TimeOfDay.MORNING, TimeOfDay.AFTERNOON, TimeOfDay.EVENING, TimeOfDay.NIGHT]
        current_idx = times.index(self.state.time_of_day)

        if current_idx < len(times) - 1:
            self.state.time_of_day = times[current_idx + 1]
        else:
            self.state.time_of_day = TimeOfDay.MORNING
            self.state.day += 1

    def _save(self) -> None:
        """Autosave current state."""
        self.state.rng_counter = self.rng.counter
        save_game(self.state)


def _build_fallback_callout(outcome: EventOutcome) -> str:
    """Build a simple callout from outcome data."""
    parts = []
    for key, val in outcome.supplies_delta.items():
        if val > 0:
            parts.append(f"+{val} {key}")
        elif val < 0:
            parts.append(f"{val} {key}")

    if outcome.health_delta:
        parts.append(f"health {'+'if outcome.health_delta > 0 else ''}{outcome.health_delta}")
    if outcome.wagon_delta:
        parts.append(f"wagon {'+'if outcome.wagon_delta > 0 else ''}{outcome.wagon_delta}")
    if outcome.morale_delta:
        parts.append(f"morale {'+'if outcome.morale_delta > 0 else ''}{outcome.morale_delta}")
    if outcome.time_cost:
        parts.append(f"{outcome.time_cost} time lost")

    return ", ".join(parts) if parts else "No significant effect."


def _find_node(state: RunState):
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
